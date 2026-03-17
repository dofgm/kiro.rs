import { useRef, useState, type ChangeEvent } from 'react'
import { toast } from 'sonner'
import { CheckCircle2, XCircle, AlertCircle, Loader2, Upload } from 'lucide-react'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { useAddCredential, useDeleteCredential } from '@/hooks/use-credentials'
import { getCredentialBalance, setCredentialDisabled } from '@/api/credentials'
import { extractErrorMessage } from '@/lib/utils'

interface BatchImportDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

interface CredentialInput {
  refreshToken: string
  authMethod?: string
  clientId?: string
  clientSecret?: string
  region?: string
  authRegion?: string
  apiRegion?: string
  priority?: number
  machineId?: string
  proxyUrl?: string
  proxyUsername?: string
  proxyPassword?: string
}

interface VerificationResult {
  index: number
  status: 'pending' | 'checking' | 'verifying' | 'verified' | 'duplicate' | 'failed'
  error?: string
  usage?: string
  email?: string
  credentialId?: number
  rollbackStatus?: 'success' | 'failed' | 'skipped'
  rollbackError?: string
}

function asObject(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('凭据项必须是 JSON 对象')
  }
  return value as Record<string, unknown>
}

function pickString(obj: Record<string, unknown>, ...keys: string[]): string | undefined {
  for (const key of keys) {
    const value = obj[key]
    if (typeof value === 'string') {
      const trimmed = value.trim()
      if (trimmed) {
        return trimmed
      }
    }
  }
  return undefined
}

function pickPriority(obj: Record<string, unknown>): number | undefined {
  const value = obj.priority
  if (typeof value === 'number' && Number.isFinite(value)) {
    return Math.max(0, Math.floor(value))
  }

  if (typeof value === 'string') {
    const parsed = Number.parseInt(value, 10)
    if (Number.isFinite(parsed)) {
      return Math.max(0, parsed)
    }
  }

  return undefined
}

function normalizeCredential(raw: unknown, index: number): CredentialInput {
  const obj = asObject(raw)
  const refreshToken = pickString(obj, 'refreshToken', 'refresh_token')

  if (!refreshToken) {
    throw new Error(`第 ${index + 1} 项缺少 refreshToken`)
  }

  return {
    refreshToken,
    authMethod: pickString(obj, 'authMethod', 'auth_method'),
    clientId: pickString(obj, 'clientId', 'client_id'),
    clientSecret: pickString(obj, 'clientSecret', 'client_secret'),
    region: pickString(obj, 'region'),
    authRegion: pickString(obj, 'authRegion', 'auth_region'),
    apiRegion: pickString(obj, 'apiRegion', 'api_region'),
    priority: pickPriority(obj),
    machineId: pickString(obj, 'machineId', 'machine_id'),
    proxyUrl: pickString(obj, 'proxyUrl', 'proxy_url'),
    proxyUsername: pickString(obj, 'proxyUsername', 'proxy_username'),
    proxyPassword: pickString(obj, 'proxyPassword', 'proxy_password'),
  }
}

function parseCredentialList(jsonText: string): CredentialInput[] {
  const parsed: unknown = JSON.parse(jsonText)
  let source: unknown = parsed

  if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
    const maybeCredentials = (parsed as Record<string, unknown>).credentials
    if (Array.isArray(maybeCredentials)) {
      source = maybeCredentials
    }
  }

  const list: unknown[] = Array.isArray(source) ? source : [source]

  if (list.length === 0) {
    throw new Error('没有可导入的凭据')
  }

  return list.map((item, index) => normalizeCredential(item, index))
}

function normalizeAuthMethod(value?: string): 'social' | 'idc' | undefined {
  if (!value) {
    return undefined
  }

  const normalized = value.trim().toLowerCase()
  if (normalized === 'idc' || normalized === 'builder-id' || normalized === 'iam') {
    return 'idc'
  }

  if (normalized === 'social') {
    return 'social'
  }

  return undefined
}

export function BatchImportDialog({ open, onOpenChange }: BatchImportDialogProps) {
  const [jsonInput, setJsonInput] = useState('')
  const [importing, setImporting] = useState(false)
  const [progress, setProgress] = useState({ current: 0, total: 0 })
  const [currentProcessing, setCurrentProcessing] = useState<string>('')
  const [results, setResults] = useState<VerificationResult[]>([])
  const [selectedFileName, setSelectedFileName] = useState('')
  const fileInputRef = useRef<HTMLInputElement | null>(null)

  const { mutateAsync: addCredential } = useAddCredential()
  const { mutateAsync: deleteCredential } = useDeleteCredential()

  const rollbackCredential = async (id: number): Promise<{ success: boolean; error?: string }> => {
    try {
      await setCredentialDisabled(id, true)
    } catch (error) {
      return {
        success: false,
        error: `禁用失败: ${extractErrorMessage(error)}`,
      }
    }

    try {
      await deleteCredential(id)
      return { success: true }
    } catch (error) {
      return {
        success: false,
        error: `删除失败: ${extractErrorMessage(error)}`,
      }
    }
  }

  const resetForm = () => {
    setJsonInput('')
    setProgress({ current: 0, total: 0 })
    setCurrentProcessing('')
    setResults([])
    setSelectedFileName('')
  }

  const handleSelectFile = () => {
    if (!importing) {
      fileInputRef.current?.click()
    }
  }

  const handleFileChange = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0]
    event.target.value = ''

    if (!file) {
      return
    }

    try {
      const text = await file.text()
      const credentials = parseCredentialList(text)
      setJsonInput(JSON.stringify(credentials, null, 2))
      setSelectedFileName(file.name)
      toast.success(`已载入 ${credentials.length} 个凭据`)
    } catch (error) {
      setSelectedFileName('')
      toast.error(`文件解析失败: ${extractErrorMessage(error)}`)
    }
  }

  const handleBatchImport = async () => {
    // 先单独解析 JSON，给出精准的错误提示
    let credentials: CredentialInput[]
    try {
      credentials = parseCredentialList(jsonInput)
    } catch (error) {
      toast.error(`JSON 格式错误: ${extractErrorMessage(error)}`)
      return
    }

    try {
      setImporting(true)
      setProgress({ current: 0, total: credentials.length })

      const initialResults: VerificationResult[] = credentials.map((_, i) => ({
        index: i + 1,
        status: 'pending'
      }))
      setResults(initialResults)

      let successCount = 0
      let duplicateCount = 0
      let failCount = 0
      let rollbackSuccessCount = 0
      let rollbackFailedCount = 0
      let rollbackSkippedCount = 0

      for (let i = 0; i < credentials.length; i++) {
        const cred = credentials[i]
        const token = cred.refreshToken

        setCurrentProcessing(`正在处理凭据 ${i + 1}/${credentials.length}`)
        setResults(prev => {
          const newResults = [...prev]
          newResults[i] = { ...newResults[i], status: 'checking' }
          return newResults
        })

        setResults(prev => {
          const newResults = [...prev]
          newResults[i] = { ...newResults[i], status: 'verifying' }
          return newResults
        })

        let addedCredId: number | null = null

        try {
          const clientId = cred.clientId
          const clientSecret = cred.clientSecret
          const explicitAuthMethod = normalizeAuthMethod(cred.authMethod)
          const inferredAuthMethod: 'social' | 'idc' = clientId && clientSecret ? 'idc' : 'social'
          const authMethod = explicitAuthMethod || inferredAuthMethod

          if (authMethod === 'idc' && (!clientId || !clientSecret)) {
            throw new Error('第 ' + (i + 1) + ' 项选择了 idc，但缺少 clientId 或 clientSecret')
          }

          if (authMethod === 'social' && (clientId || clientSecret)) {
            throw new Error('第 ' + (i + 1) + ' 项 social 模式下不应只填写 clientId 或 clientSecret')
          }

          const addedCred = await addCredential({
            refreshToken: token,
            authMethod,
            authRegion: cred.authRegion || cred.region || undefined,
            apiRegion: cred.apiRegion || undefined,
            clientId,
            clientSecret,
            priority: cred.priority || 0,
            machineId: cred.machineId || undefined,
            proxyUrl: cred.proxyUrl || undefined,
            proxyUsername: cred.proxyUsername || undefined,
            proxyPassword: cred.proxyPassword || undefined,
          })

          addedCredId = addedCred.credentialId

          await new Promise(resolve => setTimeout(resolve, 1000))

          const balance = await getCredentialBalance(addedCred.credentialId)

          successCount++
          setCurrentProcessing(addedCred.email ? `验活成功: ${addedCred.email}` : `验活成功: 凭据 ${i + 1}`)
          setResults(prev => {
            const newResults = [...prev]
            newResults[i] = {
              ...newResults[i],
              status: 'verified',
              usage: `${balance.currentUsage}/${balance.usageLimit}`,
              email: addedCred.email || undefined,
              credentialId: addedCred.credentialId
            }
            return newResults
          })
        } catch (error) {
          const errorMessage = extractErrorMessage(error)
          const isDuplicate =
            errorMessage.includes('凭据已存在') ||
            errorMessage.includes('refreshToken 重复') ||
            errorMessage.toLowerCase().includes('duplicate')

          if (isDuplicate) {
            duplicateCount++
            setResults(prev => {
              const newResults = [...prev]
              newResults[i] = {
                ...newResults[i],
                status: 'duplicate',
                error: '该凭据已存在',
                email: undefined,
              }
              return newResults
            })
            setProgress({ current: i + 1, total: credentials.length })
            continue
          }

          let rollbackStatus: VerificationResult['rollbackStatus'] = 'skipped'
          let rollbackError: string | undefined

          if (addedCredId) {
            const rollbackResult = await rollbackCredential(addedCredId)
            if (rollbackResult.success) {
              rollbackStatus = 'success'
              rollbackSuccessCount++
            } else {
              rollbackStatus = 'failed'
              rollbackFailedCount++
              rollbackError = rollbackResult.error
            }
          } else {
            rollbackSkippedCount++
          }

          failCount++
          setResults(prev => {
            const newResults = [...prev]
            newResults[i] = {
              ...newResults[i],
              status: 'failed',
              error: errorMessage,
              email: undefined,
              rollbackStatus,
              rollbackError,
            }
            return newResults
          })
        }

        setProgress({ current: i + 1, total: credentials.length })
      }

      if (failCount === 0 && duplicateCount === 0) {
        toast.success(`成功导入并验活 ${successCount} 个凭据`)
      } else {
        const failureSummary = failCount > 0
          ? `，失败 ${failCount} 个（已排除 ${rollbackSuccessCount}，未排除 ${rollbackFailedCount}，无需排除 ${rollbackSkippedCount}）`
          : ''
        toast.info(`验活完成：成功 ${successCount} 个，重复 ${duplicateCount} 个${failureSummary}`)

        if (rollbackFailedCount > 0) {
          toast.warning(`有 ${rollbackFailedCount} 个失败凭据回滚未完成，请手动禁用并删除`)
        }
      }
    } catch (error) {
      toast.error(`导入失败: ${extractErrorMessage(error)}`)
    } finally {
      setImporting(false)
    }
  }

  const getStatusIcon = (status: VerificationResult['status']) => {
    switch (status) {
      case 'pending':
        return <div className="w-5 h-5 rounded-full border-2 border-gray-300" />
      case 'checking':
      case 'verifying':
        return <Loader2 className="w-5 h-5 animate-spin text-blue-500" />
      case 'verified':
        return <CheckCircle2 className="w-5 h-5 text-green-500" />
      case 'duplicate':
        return <AlertCircle className="w-5 h-5 text-yellow-500" />
      case 'failed':
        return <XCircle className="w-5 h-5 text-red-500" />
    }
  }

  const getStatusText = (result: VerificationResult) => {
    switch (result.status) {
      case 'pending':
        return '等待中'
      case 'checking':
        return '检查重复...'
      case 'verifying':
        return '验活中...'
      case 'verified':
        return '验活成功'
      case 'duplicate':
        return '重复凭据'
      case 'failed':
        if (result.rollbackStatus === 'success') return '验活失败（已排除）'
        if (result.rollbackStatus === 'failed') return '验活失败（未排除）'
        return '验活失败（未创建）'
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(newOpen) => {
        if (!newOpen && !importing) {
          resetForm()
        }
        onOpenChange(newOpen)
      }}
    >
      <DialogContent className="sm:max-w-2xl max-h-[80vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>批量导入凭据（自动验活）</DialogTitle>
        </DialogHeader>

        <div className="flex-1 overflow-y-auto space-y-4 py-4">
          <div className="space-y-2">
            <label className="text-sm font-medium">
              JSON 格式凭据
            </label>

            <input
              ref={fileInputRef}
              type="file"
              accept=".json,application/json"
              onChange={handleFileChange}
              disabled={importing}
              className="hidden"
            />

            <div className="flex items-center gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handleSelectFile}
                disabled={importing}
              >
                <Upload className="w-4 h-4 mr-2" />
                导入 JSON 文件
              </Button>
              {selectedFileName && (
                <span className="text-xs text-muted-foreground truncate" title={selectedFileName}>
                  已选择: {selectedFileName}
                </span>
              )}
            </div>

            <textarea
              placeholder={'可粘贴 JSON，或先点“导入 JSON 文件”\n支持单个对象、数组、或 {"credentials":[...]}\n例如: [{"refreshToken":"...","clientId":"...","clientSecret":"...","authRegion":"us-east-1","apiRegion":"us-west-2"}]'}
              value={jsonInput}
              onChange={(e) => setJsonInput(e.target.value)}
              disabled={importing}
              className="flex min-h-[220px] w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50 font-mono"
            />
            <p className="text-xs text-muted-foreground">
              导入时会自动验活；失败项会自动回滚（禁用并删除）
            </p>
          </div>

          {(importing || results.length > 0) && (
            <>
              <div className="space-y-2">
                <div className="flex justify-between text-sm">
                  <span>{importing ? '验活进度' : '验活完成'}</span>
                  <span>{progress.current} / {progress.total}</span>
                </div>
                <div className="w-full bg-secondary rounded-full h-2">
                  <div
                    className="bg-primary h-2 rounded-full transition-all"
                    style={{ width: `${(progress.current / progress.total) * 100}%` }}
                  />
                </div>
                {importing && currentProcessing && (
                  <div className="text-xs text-muted-foreground">
                    {currentProcessing}
                  </div>
                )}
              </div>

              <div className="flex gap-4 text-sm">
                <span className="text-green-600 dark:text-green-400">
                  ✓ 成功: {results.filter(r => r.status === 'verified').length}
                </span>
                <span className="text-yellow-600 dark:text-yellow-400">
                  ⚠ 重复: {results.filter(r => r.status === 'duplicate').length}
                </span>
                <span className="text-red-600 dark:text-red-400">
                  ✗ 失败: {results.filter(r => r.status === 'failed').length}
                </span>
              </div>

              <div className="border rounded-md divide-y max-h-[300px] overflow-y-auto">
                {results.map((result) => (
                  <div key={result.index} className="p-3">
                    <div className="flex items-start gap-3">
                      {getStatusIcon(result.status)}
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <span className="text-sm font-medium">
                            {result.email || `凭据 #${result.index}`}
                          </span>
                          <span className="text-xs text-muted-foreground">
                            {getStatusText(result)}
                          </span>
                        </div>
                        {result.usage && (
                          <div className="text-xs text-muted-foreground mt-1">
                            用量: {result.usage}
                          </div>
                        )}
                        {result.error && (
                          <div className="text-xs text-red-600 dark:text-red-400 mt-1">
                            {result.error}
                          </div>
                        )}
                        {result.rollbackError && (
                          <div className="text-xs text-red-600 dark:text-red-400 mt-1">
                            回滚失败: {result.rollbackError}
                          </div>
                        )}
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            </>
          )}
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            onClick={() => {
              onOpenChange(false)
              resetForm()
            }}
            disabled={importing}
          >
            {importing ? '验活中...' : results.length > 0 ? '关闭' : '取消'}
          </Button>
          {results.length === 0 && (
            <Button
              type="button"
              onClick={handleBatchImport}
              disabled={importing || !jsonInput.trim()}
            >
              开始导入并验活
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
