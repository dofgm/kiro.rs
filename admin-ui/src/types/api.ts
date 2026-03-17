// 凭据状态响应
export interface CredentialsStatusResponse {
  total: number
  available: number
  currentId: number
  credentials: CredentialStatusItem[]
}

// 单个凭据状态
export interface CredentialStatusItem {
  id: number
  priority: number
  disabled: boolean
  failureCount: number
  isCurrent: boolean
  expiresAt: string | null
  authMethod: string | null
  hasProfileArn: boolean
  email?: string
  refreshTokenHash?: string
  successCount: number
  lastUsedAt: string | null
  lastRequestCredits: number
  totalCredits: number
  hasProxy: boolean
  proxyUrl?: string
  subscriptionTitle?: string
}

// 余额响应
export interface BalanceResponse {
  id: number
  subscriptionTitle: string | null
  currentUsage: number
  usageLimit: number
  remaining: number
  usagePercentage: number
  nextResetAt: number | null
}

// 请求明细条目
export interface RequestDetailItem {
  recordedAt: string
  requestId: string
  endpoint: string
  model: string
  credentialId: number
  stream: boolean
  cacheHit: boolean
  inputTokens: number
  cachedTokens: number
  outputTokens: number
  cacheRatio: number
  costUsd: number
  creditsUsed: number
  specialSettings: string[]
}

// 请求明细响应
export interface RequestDetailsResponse {
  total: number
  records: RequestDetailItem[]
}

// 成功响应
export interface SuccessResponse {
  success: boolean
  message: string
}

// 错误响应
export interface AdminErrorResponse {
  error: {
    type: string
    message: string
  }
}

// 请求类型
export interface SetDisabledRequest {
  disabled: boolean
}

export interface SetPriorityRequest {
  priority: number
}

// 添加凭据请求
export interface AddCredentialRequest {
  refreshToken: string
  authMethod?: 'social' | 'idc'
  clientId?: string
  clientSecret?: string
  priority?: number
  authRegion?: string
  apiRegion?: string
  machineId?: string
  proxyUrl?: string
  proxyUsername?: string
  proxyPassword?: string
}

// 添加凭据响应
export interface AddCredentialResponse {
  success: boolean
  message: string
  credentialId: number
  email?: string
}

export type LoadBalancingMode = 'priority' | 'balanced' | 'weighted_round_robin'

export interface LoadBalancingModeResponse {
  mode: LoadBalancingMode
}

export interface SystemSettingsResponse {
  stripBillingHeader: boolean
}

export interface SetSystemSettingsRequest {
  stripBillingHeader: boolean
}

// 可用模型列表
export interface AdminModelItem {
  id: string
  displayName: string
}

export interface ModelsListResponse {
  models: AdminModelItem[]
}

// API 密钥管理
export interface ApiKeyResponse {
  apiKey: string
}

export interface SetApiKeyRequest {
  apiKey: string
}
