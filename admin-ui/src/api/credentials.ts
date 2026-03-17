import axios from 'axios'
import { storage } from '@/lib/storage'
import type {
  CredentialsStatusResponse,
  BalanceResponse,
  RequestDetailsResponse,
  SuccessResponse,
  SetDisabledRequest,
  SetPriorityRequest,
  AddCredentialRequest,
  AddCredentialResponse,
  LoadBalancingMode,
  LoadBalancingModeResponse,
  SetSystemSettingsRequest,
  SystemSettingsResponse,
  ModelsListResponse,
  ApiKeyResponse,
  SetApiKeyRequest,
} from '@/types/api'

// 创建 axios 实例
const api = axios.create({
  baseURL: '/api/admin',
  headers: {
    'Content-Type': 'application/json',
  },
})

// 请求拦截器添加 API Key
api.interceptors.request.use((config) => {
  const apiKey = storage.getApiKey()
  if (apiKey) {
    config.headers['x-api-key'] = apiKey
  }
  return config
})

// 获取所有凭据状态
export async function getCredentials(): Promise<CredentialsStatusResponse> {
  const { data } = await api.get<CredentialsStatusResponse>('/credentials')
  return data
}

// 设置凭据禁用状态
export async function setCredentialDisabled(
  id: number,
  disabled: boolean
): Promise<SuccessResponse> {
  const { data } = await api.post<SuccessResponse>(
    `/credentials/${id}/disabled`,
    { disabled } as SetDisabledRequest
  )
  return data
}

// 设置凭据优先级
export async function setCredentialPriority(
  id: number,
  priority: number
): Promise<SuccessResponse> {
  const { data } = await api.post<SuccessResponse>(
    `/credentials/${id}/priority`,
    { priority } as SetPriorityRequest
  )
  return data
}

// 重置失败计数
export async function resetCredentialFailure(
  id: number
): Promise<SuccessResponse> {
  const { data } = await api.post<SuccessResponse>(`/credentials/${id}/reset`)
  return data
}

// 获取凭据余额
export async function getCredentialBalance(id: number): Promise<BalanceResponse> {
  const { data } = await api.get<BalanceResponse>(`/credentials/${id}/balance`)
  return data
}

// 获取请求明细
export async function getRequestDetails(limit = 100): Promise<RequestDetailsResponse> {
  const { data } = await api.get<RequestDetailsResponse>('/details', {
    params: { limit },
  })
  return data
}

// 清空请求明细
export async function clearRequestDetails(): Promise<SuccessResponse> {
  const { data } = await api.delete<SuccessResponse>('/details')
  return data
}

// 添加新凭据
export async function addCredential(
  req: AddCredentialRequest
): Promise<AddCredentialResponse> {
  const { data } = await api.post<AddCredentialResponse>('/credentials', req)
  return data
}

// 删除凭据
export async function deleteCredential(id: number): Promise<SuccessResponse> {
  const { data } = await api.delete<SuccessResponse>(`/credentials/${id}`)
  return data
}

// 获取负载均衡模式
export async function getLoadBalancingMode(): Promise<LoadBalancingModeResponse> {
  const { data } = await api.get<LoadBalancingModeResponse>('/config/load-balancing')
  return data
}

// 设置负载均衡模式
export async function setLoadBalancingMode(mode: LoadBalancingMode): Promise<LoadBalancingModeResponse> {
  const { data } = await api.put<LoadBalancingModeResponse>('/config/load-balancing', { mode })
  return data
}

// 获取系统设置
export async function getSystemSettings(): Promise<SystemSettingsResponse> {
  const { data } = await api.get<SystemSettingsResponse>('/config/system-settings')
  return data
}

// 设置系统设置
export async function setSystemSettings(
  req: SetSystemSettingsRequest
): Promise<SystemSettingsResponse> {
  const { data } = await api.put<SystemSettingsResponse>('/config/system-settings', req)
  return data
}

// 获取可用模型列表
export async function getModels(): Promise<ModelsListResponse> {
  const { data } = await api.get<ModelsListResponse>('/config/models')
  return data
}

// 获取 API 密钥
export async function getApiKey(): Promise<ApiKeyResponse> {
  const { data } = await api.get<ApiKeyResponse>('/config/api-key')
  return data
}

// 设置 API 密钥
export async function setApiKey(req: SetApiKeyRequest): Promise<SuccessResponse> {
  const { data } = await api.put<SuccessResponse>('/config/api-key', req)
  return data
}
