import { RefreshCw, Trash2 } from 'lucide-react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useRequestDetails, useClearRequestDetails } from '@/hooks/use-credentials'
import { extractErrorMessage } from '@/lib/utils'
import { getMessages } from '@/lib/i18n'

interface RequestDetailsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

const formatTokenCount = (value: number) => value.toLocaleString('en-US')
const formatRatio = (value: number) => `${(value * 100).toFixed(1)}%`
const formatCost = (value: number) => `$${value.toFixed(6)}`
const formatTimestamp = (value: string) => {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return date.toLocaleString()
}

const DETAILS_LIMIT = 100

export function RequestDetailsDialog({ open, onOpenChange }: RequestDetailsDialogProps) {
  const { data: requestDetails, isLoading, refetch } = useRequestDetails(DETAILS_LIMIT)
  const { mutate: clearDetails, isPending: isClearing } = useClearRequestDetails()
  const messages = getMessages(typeof navigator === 'undefined' ? 'en' : navigator.language)

  const handleClear = () => {
    if (!confirm('确定要清空所有请求明细吗？此操作无法撤销。')) return
    clearDetails(undefined, {
      onSuccess: () => toast.success('请求明细已清空'),
      onError: (err) => toast.error(`清空失败: ${extractErrorMessage(err)}`),
    })
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-6xl max-h-[80vh] flex flex-col">
        <DialogHeader>
          <div className="flex items-center justify-between pr-6">
            <DialogTitle>请求明细（最近 {DETAILS_LIMIT} 条）</DialogTitle>
            <div className="flex gap-2">
              <Button variant="outline" size="sm" onClick={() => refetch()}>
                <RefreshCw className="h-4 w-4 mr-2" />
                刷新
              </Button>
              <Button
                variant="outline"
                size="sm"
                className="text-destructive hover:text-destructive"
                onClick={handleClear}
                disabled={isClearing || !requestDetails?.records.length}
              >
                <Trash2 className="h-4 w-4 mr-2" />
                清空
              </Button>
            </div>
          </div>
        </DialogHeader>
        <div className="flex-1 overflow-auto">
          {isLoading ? (
            <div className="text-sm text-muted-foreground py-8 text-center">加载明细中...</div>
          ) : requestDetails?.records.length ? (
            <table className="w-full text-sm">
              <thead className="sticky top-0 bg-background">
                <tr className="border-b text-left">
                  <th className="py-2 pr-4 font-medium">时间</th>
                  <th className="py-2 pr-4 font-medium">凭据</th>
                  <th className="py-2 pr-4 font-medium">模型</th>
                  <th className="py-2 pr-4 font-medium">输入</th>
                  <th className="py-2 pr-4 font-medium">缓存</th>
                  <th className="py-2 pr-4 font-medium">输出</th>
                  <th className="py-2 pr-4 font-medium">缓存占比</th>
                  <th className="py-2 pr-4 font-medium">{messages.specialSettingsColumn}</th>
                  <th className="py-2 pr-4 font-medium">花费(USD)</th>
                  <th className="py-2 pr-0 font-medium">credits</th>
                </tr>
              </thead>
              <tbody>
                {requestDetails.records.map((record) => (
                  <tr key={record.requestId} className="border-b last:border-b-0">
                    <td className="py-2 pr-4 whitespace-nowrap">{formatTimestamp(record.recordedAt)}</td>
                    <td className="py-2 pr-4">#{record.credentialId}</td>
                    <td className="py-2 pr-4 font-mono text-xs">{record.model}</td>
                    <td className="py-2 pr-4">{formatTokenCount(record.inputTokens)}</td>
                    <td className="py-2 pr-4">{formatTokenCount(record.cachedTokens)}</td>
                    <td className="py-2 pr-4">{formatTokenCount(record.outputTokens)}</td>
                    <td className="py-2 pr-4">{formatRatio(record.cacheRatio)}</td>
                    <td className="py-2 pr-4 text-xs">
                      {record.specialSettings.length > 0 ? record.specialSettings.join(', ') : '-'}
                    </td>
                    <td className="py-2 pr-4">{formatCost(record.costUsd)}</td>
                    <td className="py-2 pr-0">{record.creditsUsed.toFixed(6)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          ) : (
            <div className="text-sm text-muted-foreground py-8 text-center">
              暂无请求明细
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}
