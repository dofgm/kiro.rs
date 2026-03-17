//! Admin API 类型定义

use serde::{Deserialize, Serialize};

// ============ 凭据状态 ============

/// 所有凭据状态响应
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsStatusResponse {
    /// 凭据总数
    pub total: usize,
    /// 可用凭据数量（未禁用）
    pub available: usize,
    /// 当前活跃凭据 ID
    pub current_id: u64,
    /// 各凭据状态列表
    pub credentials: Vec<CredentialStatusItem>,
}

/// 单个凭据的状态信息
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialStatusItem {
    /// 凭据唯一 ID
    pub id: u64,
    /// 优先级（数字越小优先级越高）
    pub priority: u32,
    /// 是否被禁用
    pub disabled: bool,
    /// 连续失败次数
    pub failure_count: u32,
    /// 是否为当前活跃凭据
    pub is_current: bool,
    /// Token 过期时间（RFC3339 格式）
    pub expires_at: Option<String>,
    /// 认证方式
    pub auth_method: Option<String>,
    /// 是否有 Profile ARN
    pub has_profile_arn: bool,
    /// refreshToken 的 SHA-256 哈希（用于前端重复检测）
    pub refresh_token_hash: Option<String>,
    /// 用户邮箱（用于前端显示）
    pub email: Option<String>,
    /// API 调用成功次数
    pub success_count: u64,
    /// 最后一次 API 调用时间（RFC3339 格式）
    pub last_used_at: Option<String>,
    /// 最近一次请求消耗的 credits
    pub last_request_credits: f64,
    /// 累计消耗的 credits
    pub total_credits: f64,
    /// 是否配置了凭据级代理
    pub has_proxy: bool,
    /// 代理 URL（用于前端展示）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_url: Option<String>,
    /// 订阅类型（如 KIRO FREE, KIRO PRO, KIRO PRO+, KIRO POWER）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_title: Option<String>,
}

// ============ 操作请求 ============

/// 启用/禁用凭据请求
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetDisabledRequest {
    /// 是否禁用
    pub disabled: bool,
}

/// 修改优先级请求
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPriorityRequest {
    /// 新优先级值
    pub priority: u32,
}

/// 添加凭据请求
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddCredentialRequest {
    /// 刷新令牌（必填）
    pub refresh_token: String,

    /// 认证方式（可选，默认 social）
    #[serde(default = "default_auth_method")]
    pub auth_method: String,

    /// OIDC Client ID（IdC 认证需要）
    pub client_id: Option<String>,

    /// OIDC Client Secret（IdC 认证需要）
    pub client_secret: Option<String>,

    /// 优先级（可选，默认 0）
    #[serde(default)]
    pub priority: u32,

    /// 凭据级 Region 配置（用于 OIDC token 刷新）
    /// 未配置时回退到 config.json 的全局 region
    pub region: Option<String>,

    /// 凭据级 Auth Region（用于 Token 刷新）
    pub auth_region: Option<String>,

    /// 凭据级 API Region（用于 API 请求）
    pub api_region: Option<String>,

    /// 凭据级 Machine ID（可选，64 位字符串）
    /// 未配置时回退到 config.json 的 machineId
    pub machine_id: Option<String>,

    /// 用户邮箱（可选，用于前端显示）
    pub email: Option<String>,

    /// 凭据级代理 URL（可选，特殊值 "direct" 表示不使用代理）
    pub proxy_url: Option<String>,

    /// 凭据级代理认证用户名（可选）
    pub proxy_username: Option<String>,

    /// 凭据级代理认证密码（可选）
    pub proxy_password: Option<String>,
}

fn default_auth_method() -> String {
    "social".to_string()
}

/// 添加凭据成功响应
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddCredentialResponse {
    pub success: bool,
    pub message: String,
    /// 新添加的凭据 ID
    pub credential_id: u64,
    /// 用户邮箱（如果获取成功）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

// ============ 余额查询 ============

/// 余额查询响应
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceResponse {
    /// 凭据 ID
    pub id: u64,
    /// 订阅类型
    pub subscription_title: Option<String>,
    /// 当前使用量
    pub current_usage: f64,
    /// 使用限额
    pub usage_limit: f64,
    /// 剩余额度
    pub remaining: f64,
    /// 使用百分比
    pub usage_percentage: f64,
    /// 下次重置时间（Unix 时间戳）
    pub next_reset_at: Option<f64>,
}

// ============ 请求明细 ============

/// 请求明细查询参数
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestDetailsQuery {
    /// 返回条数（默认 100，最大 1000）
    pub limit: Option<usize>,
}

/// 请求明细响应
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestDetailsResponse {
    /// 解析成功的总记录数
    pub total: usize,
    /// 明细列表（按时间倒序）
    pub records: Vec<RequestDetailItem>,
}

/// 单次请求明细
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestDetailItem {
    /// 记录时间（RFC3339）
    pub recorded_at: String,
    /// 请求 ID
    pub request_id: String,
    /// 接口路径
    pub endpoint: String,
    /// 模型名
    pub model: String,
    /// 凭据 ID
    pub credential_id: u64,
    /// 是否流式
    pub stream: bool,
    /// 是否命中缓存
    pub cache_hit: bool,
    /// 输入 tokens（不包含缓存读取和缓存创建）
    pub input_tokens: i32,
    /// 缓存读取 tokens
    pub cached_tokens: i32,
    /// 输出 tokens
    pub output_tokens: i32,
    /// 缓存命中比例（cached / total_input）
    pub cache_ratio: f64,
    /// 按模型价格估算的费用（USD）
    pub cost_usd: f64,
    /// 上游返回的 credits（用于对照）
    pub credits_used: f64,
    /// 特殊设置命中记录（用于审计）
    pub special_settings: Vec<String>,
}

// ============ 负载均衡配置 ============

/// 负载均衡模式响应
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancingModeResponse {
    /// 当前模式（"priority"、"balanced" 或 "weighted_round_robin"）
    pub mode: String,
}

/// 设置负载均衡模式请求
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetLoadBalancingModeRequest {
    /// 模式（"priority"、"balanced" 或 "weighted_round_robin"）
    pub mode: String,
}

/// 系统设置响应
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSettingsResponse {
    /// 是否在转发前移除 system 中的 billing header
    pub strip_billing_header: bool,
}

/// 设置系统设置请求
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSystemSettingsRequest {
    /// 是否在转发前移除 system 中的 billing header
    pub strip_billing_header: bool,
}

// ============ 通用响应 ============

/// 操作成功响应
#[derive(Debug, Serialize)]
pub struct SuccessResponse {
    pub success: bool,
    pub message: String,
}

impl SuccessResponse {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
        }
    }
}

// ============ 可用模型列表 ============

/// 模型列表响应
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsListResponse {
    pub models: Vec<AdminModelItem>,
}

/// 单个模型信息
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminModelItem {
    pub id: String,
    pub display_name: String,
}

// ============ API 密钥管理 ============

/// API 密钥响应（返回遮罩后的 key）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyResponse {
    pub api_key: String,
}

/// 设置 API 密钥请求
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetApiKeyRequest {
    pub api_key: String,
}

// ============ 错误响应 ============
#[derive(Debug, Serialize)]
pub struct AdminErrorResponse {
    pub error: AdminError,
}

#[derive(Debug, Serialize)]
pub struct AdminError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

impl AdminErrorResponse {
    pub fn new(error_type: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: AdminError {
                error_type: error_type.into(),
                message: message.into(),
            },
        }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new("invalid_request", message)
    }

    pub fn authentication_error() -> Self {
        Self::new("authentication_error", "Invalid or missing admin API key")
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("not_found", message)
    }

    pub fn api_error(message: impl Into<String>) -> Self {
        Self::new("api_error", message)
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new("internal_error", message)
    }
}
