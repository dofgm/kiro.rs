import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import {
  getCredentials,
  setCredentialDisabled,
  setCredentialPriority,
  resetCredentialFailure,
  getCredentialBalance,
  getRequestDetails,
  clearRequestDetails,
  addCredential,
  deleteCredential,
  getLoadBalancingMode,
  getSystemSettings,
  setLoadBalancingMode,
  setSystemSettings,
  getModels,
  getApiKey,
  setApiKey,
} from '@/api/credentials'
import type { AddCredentialRequest, SetSystemSettingsRequest, SetApiKeyRequest } from '@/types/api'

// 查询凭据列表
export function useCredentials() {
  return useQuery({
    queryKey: ['credentials'],
    queryFn: getCredentials,
    refetchInterval: 30000, // 每 30 秒刷新一次
  })
}

// 查询凭据余额
export function useCredentialBalance(id: number | null) {
  return useQuery({
    queryKey: ['credential-balance', id],
    queryFn: () => getCredentialBalance(id!),
    enabled: id !== null,
    retry: false, // 余额查询失败时不重试（避免重复请求被封禁的账号）
  })
}

// 查询请求明细
export function useRequestDetails(limit = 100) {
  return useQuery({
    queryKey: ['request-details', limit],
    queryFn: () => getRequestDetails(limit),
    refetchInterval: 15000,
  })
}

// 清空请求明细
export function useClearRequestDetails() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: clearRequestDetails,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['request-details'] })
    },
  })
}

// 设置禁用状态
export function useSetDisabled() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, disabled }: { id: number; disabled: boolean }) =>
      setCredentialDisabled(id, disabled),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['credentials'] })
    },
  })
}

// 设置优先级
export function useSetPriority() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, priority }: { id: number; priority: number }) =>
      setCredentialPriority(id, priority),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['credentials'] })
    },
  })
}

// 重置失败计数
export function useResetFailure() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: number) => resetCredentialFailure(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['credentials'] })
    },
  })
}

// 添加新凭据
export function useAddCredential() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (req: AddCredentialRequest) => addCredential(req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['credentials'] })
    },
  })
}

// 删除凭据
export function useDeleteCredential() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: number) => deleteCredential(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['credentials'] })
    },
  })
}

// 获取负载均衡模式
export function useLoadBalancingMode() {
  return useQuery({
    queryKey: ['loadBalancingMode'],
    queryFn: getLoadBalancingMode,
  })
}

// 设置负载均衡模式
export function useSetLoadBalancingMode() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: setLoadBalancingMode,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['loadBalancingMode'] })
    },
  })
}

// 获取系统设置
export function useSystemSettings() {
  return useQuery({
    queryKey: ['systemSettings'],
    queryFn: getSystemSettings,
  })
}

// 设置系统设置
export function useSetSystemSettings() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (req: SetSystemSettingsRequest) => setSystemSettings(req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['systemSettings'] })
    },
  })
}

// 获取可用模型列表
export function useModels() {
  return useQuery({
    queryKey: ['models'],
    queryFn: getModels,
  })
}

// 获取 API 密钥
export function useApiKey() {
  return useQuery({
    queryKey: ['apiKey'],
    queryFn: getApiKey,
  })
}

// 设置 API 密钥
export function useSetApiKey() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (req: SetApiKeyRequest) => setApiKey(req),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['apiKey'] })
    },
  })
}
