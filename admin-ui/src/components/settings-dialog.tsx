import { useState } from 'react'
import { Eye, EyeOff, Copy, Pencil, Cpu } from 'lucide-react'
import { toast } from 'sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import {
  useModels,
  useApiKey,
  useSetApiKey,
  useSystemSettings,
  useSetSystemSettings,
} from '@/hooks/use-credentials'
import { extractErrorMessage } from '@/lib/utils'
import { getMessages } from '@/lib/i18n'

interface SettingsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function SettingsDialog({ open, onOpenChange }: SettingsDialogProps) {
  const { data: modelsData } = useModels()
  const { data: apiKeyData } = useApiKey()
  const { mutate: setApiKeyMutation, isPending: isSettingApiKey } = useSetApiKey()
  const { data: systemSettings, isLoading: isLoadingSystemSettings } = useSystemSettings()
  const { mutate: setSystemSettings, isPending: isSettingSystemSettings } = useSetSystemSettings()

  const [showApiKey, setShowApiKey] = useState(false)
  const [editingApiKey, setEditingApiKey] = useState(false)
  const [newApiKeyValue, setNewApiKeyValue] = useState('')
  const messages = getMessages(typeof navigator === 'undefined' ? 'en' : navigator.language)
  const handleCopyApiKey = () => {
    if (apiKeyData?.apiKey) {
      navigator.clipboard.writeText(apiKeyData.apiKey)
      toast.success('API 密钥已复制到剪贴板')
    }
  }

  const handleSaveApiKey = () => {
    const trimmed = newApiKeyValue.trim()
    if (!trimmed) {
      toast.error('API 密钥不能为空')
      return
    }
    setApiKeyMutation(
      { apiKey: trimmed },
      {
        onSuccess: () => {
          toast.success('API 密钥已更新')
          setEditingApiKey(false)
          setNewApiKeyValue('')
        },
        onError: (error) => {
          toast.error(`更新失败: ${extractErrorMessage(error)}`)
        },
      }
    )
  }

  const handleToggleBillingHeader = (checked: boolean) => {
    setSystemSettings(
      { stripBillingHeader: checked },
      {
        onSuccess: (response) => {
          toast.success(
            response.stripBillingHeader
              ? messages.billingHeaderEnableSuccess
              : messages.billingHeaderDisableSuccess
          )
        },
        onError: (error) => {
          toast.error(`${messages.billingHeaderUpdateErrorPrefix}${extractErrorMessage(error)}`)
        },
      }
    )
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg max-h-[80vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>设置</DialogTitle>
        </DialogHeader>

        <div className="space-y-6">
          {/* 可用模型列表 */}
          <div>
            <div className="flex items-center gap-2 mb-3">
              <Cpu className="h-4 w-4" />
              <span className="text-sm font-medium">可用模型</span>
              {modelsData?.models && (
                <Badge variant="secondary">{modelsData.models.length}</Badge>
              )}
            </div>
            <div className="flex flex-wrap gap-2">
              {modelsData?.models.map((model) => (
                <Badge key={model.id} variant="outline" className="text-xs">
                  {model.displayName}
                </Badge>
              ))}
            </div>
          </div>

          <hr className="border-border" />

          {/* API 密钥 */}
          <div>
            <span className="text-sm font-medium mb-3 block">API 密钥</span>
            {editingApiKey ? (
              <div className="flex items-center gap-2">
                <Input
                  placeholder="输入新的 API 密钥"
                  value={newApiKeyValue}
                  onChange={(e) => setNewApiKeyValue(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') handleSaveApiKey()
                    if (e.key === 'Escape') {
                      setEditingApiKey(false)
                      setNewApiKeyValue('')
                    }
                  }}
                  autoFocus
                />
                <Button
                  size="sm"
                  onClick={handleSaveApiKey}
                  disabled={isSettingApiKey || !newApiKeyValue.trim()}
                >
                  {isSettingApiKey ? '保存中...' : '保存'}
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => {
                    setEditingApiKey(false)
                    setNewApiKeyValue('')
                  }}
                >
                  取消
                </Button>
              </div>
            ) : (
              <div className="flex items-center gap-2">
                <code className="flex-1 text-sm bg-muted px-3 py-2 rounded-md font-mono truncate">
                  {showApiKey
                    ? apiKeyData?.apiKey || '未设置'
                    : apiKeyData?.apiKey
                      ? '•'.repeat(Math.min(apiKeyData.apiKey.length, 32))
                      : '未设置'}
                </code>
                <Button variant="ghost" size="icon" onClick={() => setShowApiKey(!showApiKey)} title={showApiKey ? '隐藏' : '显示'}>
                  {showApiKey ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                </Button>
                <Button variant="ghost" size="icon" onClick={handleCopyApiKey} title="复制" disabled={!apiKeyData?.apiKey}>
                  <Copy className="h-4 w-4" />
                </Button>
                <Button variant="ghost" size="icon" onClick={() => { setNewApiKeyValue(''); setEditingApiKey(true) }} title="修改">
                  <Pencil className="h-4 w-4" />
                </Button>
              </div>
            )}
            <p className="text-xs text-muted-foreground mt-2">修改后旧密钥将立即失效</p>
          </div>

          <hr className="border-border" />

          {/* Billing Header 设置 */}
          <div className="flex items-center justify-between gap-4">
            <div>
              <span className="text-sm font-medium">{messages.billingHeaderSetting}</span>
              <p className="text-sm text-muted-foreground mt-1">{messages.billingHeaderSettingDesc}</p>
              <p className="text-xs text-muted-foreground mt-1">
                {systemSettings?.stripBillingHeader
                  ? messages.billingHeaderEnabled
                  : messages.billingHeaderDisabled}
              </p>
            </div>
            <Switch
              checked={Boolean(systemSettings?.stripBillingHeader)}
              onCheckedChange={handleToggleBillingHeader}
              disabled={isLoadingSystemSettings || isSettingSystemSettings}
            />
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
