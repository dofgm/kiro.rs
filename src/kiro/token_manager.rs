//! Token 管理模块
//!
//! 负责 Token 过期检测和刷新，支持 Social 和 IdC 认证方式
//! 支持单凭据 (TokenManager) 和多凭据 (MultiTokenManager) 管理

use anyhow::bail;
use chrono::{DateTime, Duration, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex as TokioMutex;

use std::collections::{HashMap, HashSet};
use std::net::Ipv6Addr;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration as StdDuration, Instant};

use crate::http_client::{ProxyConfig, build_client};
use crate::kiro::machine_id;
use crate::kiro::model::credentials::KiroCredentials;
use crate::kiro::model::token_refresh::{
    IdcRefreshRequest, IdcRefreshResponse, RefreshRequest, RefreshResponse,
};
use crate::kiro::model::usage_limits::UsageLimitsResponse;
use crate::model::config::Config;

/// Token 管理器
///
/// 负责管理凭据和 Token 的自动刷新
pub struct TokenManager {
    config: Config,
    credentials: KiroCredentials,
    proxy: Option<ProxyConfig>,
}

impl TokenManager {
    /// 创建新的 TokenManager 实例
    pub fn new(config: Config, credentials: KiroCredentials, proxy: Option<ProxyConfig>) -> Self {
        Self {
            config,
            credentials,
            proxy,
        }
    }

    /// 获取凭据的引用
    pub fn credentials(&self) -> &KiroCredentials {
        &self.credentials
    }

    /// 获取配置的引用
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// 确保获取有效的访问 Token
    ///
    /// 如果 Token 过期或即将过期，会自动刷新
    pub async fn ensure_valid_token(&mut self) -> anyhow::Result<String> {
        if is_token_expired(&self.credentials) || is_token_expiring_soon(&self.credentials) {
            self.credentials =
                refresh_token(&self.credentials, &self.config, self.proxy.as_ref()).await?;

            // 刷新后再次检查 token 时间有效性
            if is_token_expired(&self.credentials) {
                anyhow::bail!("刷新后的 Token 仍然无效或已过期");
            }
        }

        self.credentials
            .access_token
            .clone()
            .ok_or_else(|| anyhow::anyhow!("没有可用的 accessToken"))
    }

    /// 获取使用额度信息
    ///
    /// 调用 getUsageLimits API 查询当前账户的使用额度
    pub async fn get_usage_limits(&mut self) -> anyhow::Result<UsageLimitsResponse> {
        let token = self.ensure_valid_token().await?;
        get_usage_limits(&self.credentials, &self.config, &token, self.proxy.as_ref()).await
    }
}

/// 检查 Token 是否在指定时间内过期
pub(crate) fn is_token_expiring_within(
    credentials: &KiroCredentials,
    minutes: i64,
) -> Option<bool> {
    credentials
        .expires_at
        .as_ref()
        .and_then(|expires_at| DateTime::parse_from_rfc3339(expires_at).ok())
        .map(|expires| expires <= Utc::now() + Duration::minutes(minutes))
}

/// 检查 Token 是否已过期（提前 5 分钟判断）
pub(crate) fn is_token_expired(credentials: &KiroCredentials) -> bool {
    is_token_expiring_within(credentials, 5).unwrap_or(true)
}

/// 检查 Token 是否即将过期（10分钟内）
pub(crate) fn is_token_expiring_soon(credentials: &KiroCredentials) -> bool {
    is_token_expiring_within(credentials, 10).unwrap_or(false)
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)
}

/// 验证 refreshToken 的基本有效性
pub(crate) fn validate_refresh_token(credentials: &KiroCredentials) -> anyhow::Result<()> {
    let refresh_token = credentials
        .refresh_token
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("缺少 refreshToken"))?;

    if refresh_token.is_empty() {
        bail!("refreshToken 为空");
    }

    if refresh_token.len() < 100 || refresh_token.ends_with("...") || refresh_token.contains("...")
    {
        bail!(
            "refreshToken 已被截断（长度: {} 字符）。\n\
             这通常是 Kiro IDE 为了防止凭证被第三方工具使用而故意截断的。",
            refresh_token.len()
        );
    }

    Ok(())
}

/// 刷新 Token
pub(crate) async fn refresh_token(
    credentials: &KiroCredentials,
    config: &Config,
    proxy: Option<&ProxyConfig>,
) -> anyhow::Result<KiroCredentials> {
    validate_refresh_token(credentials)?;

    // 根据 auth_method 选择刷新方式
    // 如果未指定 auth_method，根据是否有 clientId/clientSecret 自动判断
    let auth_method = credentials.auth_method.as_deref().unwrap_or_else(|| {
        if credentials.client_id.is_some() && credentials.client_secret.is_some() {
            "idc"
        } else {
            "social"
        }
    });

    if auth_method.eq_ignore_ascii_case("idc")
        || auth_method.eq_ignore_ascii_case("builder-id")
        || auth_method.eq_ignore_ascii_case("iam")
    {
        refresh_idc_token(credentials, config, proxy).await
    } else {
        refresh_social_token(credentials, config, proxy).await
    }
}

/// 刷新 Social Token
async fn refresh_social_token(
    credentials: &KiroCredentials,
    config: &Config,
    proxy: Option<&ProxyConfig>,
) -> anyhow::Result<KiroCredentials> {
    tracing::info!("正在刷新 Social Token...");

    let refresh_token = credentials.refresh_token.as_ref().unwrap();
    // 优先级：凭据.auth_region > 凭据.region > config.auth_region > config.region
    let region = credentials.effective_auth_region(config);

    let refresh_url = format!("https://prod.{}.auth.desktop.kiro.dev/refreshToken", region);
    let refresh_domain = format!("prod.{}.auth.desktop.kiro.dev", region);
    let machine_id = machine_id::generate_from_credentials(credentials, config)
        .ok_or_else(|| anyhow::anyhow!("无法生成 machineId"))?;
    let kiro_version = &config.kiro_version;

    let client = build_client(proxy, 60, config.tls_backend)?;
    let body = RefreshRequest {
        refresh_token: refresh_token.to_string(),
    };

    let response = client
        .post(&refresh_url)
        .header("Accept", "application/json, text/plain, */*")
        .header("Content-Type", "application/json")
        .header(
            "User-Agent",
            format!("KiroIDE-{}-{}", kiro_version, machine_id),
        )
        .header("Accept-Encoding", "gzip, compress, deflate, br")
        .header("host", &refresh_domain)
        .header("Connection", "close")
        .json(&body)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        let error_msg = match status.as_u16() {
            401 => "OAuth 凭证已过期或无效，需要重新认证",
            403 => "权限不足，无法刷新 Token",
            429 => "请求过于频繁，已被限流",
            500..=599 => "服务器错误，AWS OAuth 服务暂时不可用",
            _ => "Token 刷新失败",
        };
        bail!("{}: {} {}", error_msg, status, body_text);
    }

    let data: RefreshResponse = response.json().await?;

    let mut new_credentials = credentials.clone();
    new_credentials.access_token = Some(data.access_token);

    if let Some(new_refresh_token) = data.refresh_token {
        new_credentials.refresh_token = Some(new_refresh_token);
    }

    if let Some(profile_arn) = data.profile_arn {
        new_credentials.profile_arn = Some(profile_arn);
    }

    if let Some(expires_in) = data.expires_in {
        let expires_at = Utc::now() + Duration::seconds(expires_in);
        new_credentials.expires_at = Some(expires_at.to_rfc3339());
    }

    Ok(new_credentials)
}

/// IdC Token 刷新所需的 x-amz-user-agent header
const IDC_AMZ_USER_AGENT: &str = "aws-sdk-js/3.738.0 ua/2.1 os/other lang/js md/browser#unknown_unknown api/sso-oidc#3.738.0 m/E KiroIDE";

/// 刷新 IdC Token (AWS SSO OIDC)
async fn refresh_idc_token(
    credentials: &KiroCredentials,
    config: &Config,
    proxy: Option<&ProxyConfig>,
) -> anyhow::Result<KiroCredentials> {
    tracing::info!("正在刷新 IdC Token...");

    let refresh_token = credentials.refresh_token.as_ref().unwrap();
    let client_id = credentials
        .client_id
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("IdC 刷新需要 clientId"))?;
    let client_secret = credentials
        .client_secret
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("IdC 刷新需要 clientSecret"))?;

    // 优先级：凭据.auth_region > 凭据.region > config.auth_region > config.region
    let region = credentials.effective_auth_region(config);
    let refresh_url = format!("https://oidc.{}.amazonaws.com/token", region);

    let client = build_client(proxy, 60, config.tls_backend)?;
    let body = IdcRefreshRequest {
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
        refresh_token: refresh_token.to_string(),
        grant_type: "refresh_token".to_string(),
    };

    let response = client
        .post(&refresh_url)
        .header("Content-Type", "application/json")
        .header("Host", format!("oidc.{}.amazonaws.com", region))
        .header("Connection", "keep-alive")
        .header("x-amz-user-agent", IDC_AMZ_USER_AGENT)
        .header("Accept", "*/*")
        .header("Accept-Language", "*")
        .header("sec-fetch-mode", "cors")
        .header("User-Agent", "node")
        .header("Accept-Encoding", "br, gzip, deflate")
        .json(&body)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        let error_msg = match status.as_u16() {
            401 => "IdC 凭证已过期或无效，需要重新认证",
            403 => "权限不足，无法刷新 Token",
            429 => "请求过于频繁，已被限流",
            500..=599 => "服务器错误，AWS OIDC 服务暂时不可用",
            _ => "IdC Token 刷新失败",
        };
        bail!("{}: {} {}", error_msg, status, body_text);
    }

    let data: IdcRefreshResponse = response.json().await?;

    let mut new_credentials = credentials.clone();
    new_credentials.access_token = Some(data.access_token);

    if let Some(new_refresh_token) = data.refresh_token {
        new_credentials.refresh_token = Some(new_refresh_token);
    }

    if let Some(expires_in) = data.expires_in {
        let expires_at = Utc::now() + Duration::seconds(expires_in);
        new_credentials.expires_at = Some(expires_at.to_rfc3339());
    }

    Ok(new_credentials)
}

/// getUsageLimits API 所需的 x-amz-user-agent header 前缀
const USAGE_LIMITS_AMZ_USER_AGENT_PREFIX: &str = "aws-sdk-js/1.0.0";

/// 获取使用额度信息
pub(crate) async fn get_usage_limits(
    credentials: &KiroCredentials,
    config: &Config,
    token: &str,
    proxy: Option<&ProxyConfig>,
) -> anyhow::Result<UsageLimitsResponse> {
    tracing::debug!("正在获取使用额度信息...");

    // 优先级：凭据.api_region > config.api_region > config.region
    let region = credentials.effective_api_region(config);
    let host = format!("q.{}.amazonaws.com", region);
    let machine_id = machine_id::generate_from_credentials(credentials, config)
        .ok_or_else(|| anyhow::anyhow!("无法生成 machineId"))?;
    let kiro_version = &config.kiro_version;

    // 构建 URL
    let mut url = format!(
        "https://{}/getUsageLimits?origin=AI_EDITOR&resourceType=AGENTIC_REQUEST",
        host
    );

    // profileArn 是可选的
    if let Some(profile_arn) = &credentials.profile_arn {
        url.push_str(&format!("&profileArn={}", urlencoding::encode(profile_arn)));
    }

    // 构建 User-Agent headers
    let user_agent = format!(
        "aws-sdk-js/1.0.0 ua/2.1 os/darwin#24.6.0 lang/js md/nodejs#22.21.1 \
         api/codewhispererruntime#1.0.0 m/N,E KiroIDE-{}-{}",
        kiro_version, machine_id
    );
    let amz_user_agent = format!(
        "{} KiroIDE-{}-{}",
        USAGE_LIMITS_AMZ_USER_AGENT_PREFIX, kiro_version, machine_id
    );

    let client = build_client(proxy, 60, config.tls_backend)?;

    let response = client
        .get(&url)
        .header("x-amz-user-agent", &amz_user_agent)
        .header("User-Agent", &user_agent)
        .header("host", &host)
        .header("amz-sdk-invocation-id", uuid::Uuid::new_v4().to_string())
        .header("amz-sdk-request", "attempt=1; max=1")
        .header("Authorization", format!("Bearer {}", token))
        .header("Connection", "close")
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        let error_msg = match status.as_u16() {
            401 => "认证失败，Token 无效或已过期",
            403 => "权限不足，无法获取使用额度",
            429 => "请求过于频繁，已被限流",
            500..=599 => "服务器错误，AWS 服务暂时不可用",
            _ => "获取使用额度失败",
        };
        bail!("{}: {} {}", error_msg, status, body_text);
    }

    let data: UsageLimitsResponse = response.json().await?;
    Ok(data)
}

// ============================================================================
// 多凭据 Token 管理器
// ============================================================================

/// 单个凭据条目的状态
struct CredentialEntry {
    /// 凭据唯一 ID
    id: u64,
    /// 凭据信息
    credentials: KiroCredentials,
    /// API 调用连续失败次数
    failure_count: u32,
    /// 是否已禁用
    disabled: bool,
    /// 禁用原因（用于区分手动禁用 vs 自动禁用，便于自愈）
    disabled_reason: Option<DisabledReason>,
    /// API 调用成功次数
    success_count: u64,
    /// 最后一次 API 调用时间（RFC3339 格式）
    last_used_at: Option<String>,
    /// 最近一次请求消耗的 credits
    last_request_credits: f64,
    /// 累计消耗的 credits
    total_credits: f64,
}

/// 禁用原因
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisabledReason {
    /// Admin API 手动禁用
    Manual,
    /// 连续失败达到阈值后自动禁用
    TooManyFailures,
    /// 额度已用尽（如 MONTHLY_REQUEST_COUNT）
    QuotaExceeded,
}

/// 统计数据持久化条目
#[derive(Serialize, Deserialize)]
struct StatsEntry {
    success_count: u64,
    last_used_at: Option<String>,
    #[serde(default)]
    last_request_credits: f64,
    #[serde(default)]
    total_credits: f64,
}

/// 路由亲和条目（用于会话到凭据的短期粘性路由）
#[derive(Clone, Debug)]
struct RouteAffinityEntry {
    credential_id: u64,
    last_seen_at: Instant,
}

// ============================================================================
// Admin API 公开结构
// ============================================================================

/// 凭据条目快照（用于 Admin API 读取）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialEntrySnapshot {
    /// 凭据唯一 ID
    pub id: u64,
    /// 优先级
    pub priority: u32,
    /// 是否被禁用
    pub disabled: bool,
    /// 连续失败次数
    pub failure_count: u32,
    /// 认证方式
    pub auth_method: Option<String>,
    /// 是否有 Profile ARN
    pub has_profile_arn: bool,
    /// Token 过期时间
    pub expires_at: Option<String>,
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
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerSnapshot {
    /// 凭据条目列表
    pub entries: Vec<CredentialEntrySnapshot>,
    /// 当前活跃凭据 ID
    pub current_id: u64,
    /// 总凭据数量
    pub total: usize,
    /// 可用凭据数量
    pub available: usize,
}

/// 多凭据 Token 管理器
///
/// 支持多个凭据的管理，实现固定优先级 + 故障转移策略
/// 故障统计基于 API 调用结果，而非 Token 刷新结果
pub struct MultiTokenManager {
    config: Config,
    proxy: Option<ProxyConfig>,
    /// 凭据条目列表
    entries: Mutex<Vec<CredentialEntry>>,
    /// 当前活动凭据 ID
    current_id: Mutex<u64>,
    /// Token 刷新锁，确保同一时间只有一个刷新操作
    refresh_lock: TokioMutex<()>,
    /// 凭据文件路径（用于回写）
    credentials_path: Option<PathBuf>,
    /// 是否为多凭据格式（数组格式才回写）
    is_multiple_format: bool,
    /// 负载均衡模式（运行时可修改）
    load_balancing_mode: Mutex<String>,
    /// 是否在转发前移除 system 中的 billing header（运行时可修改）
    strip_billing_header: Mutex<bool>,
    /// 最近一次统计持久化时间（用于 debounce）
    last_stats_save_at: Mutex<Option<Instant>>,
    /// 统计数据是否有未落盘更新
    stats_dirty: AtomicBool,
    /// 路由亲和缓存（route_key -> 最近使用的凭据）
    route_affinity: Mutex<HashMap<String, RouteAffinityEntry>>,
    /// weighted_round_robin 平滑权重状态（credential_id -> current_weight）
    weighted_round_robin_state: Mutex<HashMap<u64, i64>>,
}

/// 每个凭据最大 API 调用失败次数
const MAX_FAILURES_PER_CREDENTIAL: u32 = 3;
/// 统计数据持久化防抖间隔
const STATS_SAVE_DEBOUNCE: StdDuration = StdDuration::from_secs(30);
/// 路由亲和生存时间（1小时）
const ROUTE_AFFINITY_TTL: StdDuration = StdDuration::from_secs(3600);
/// 路由亲和最大条目数（防止高并发下无界增长）
const ROUTE_AFFINITY_MAX_ENTRIES: usize = 20_000;
/// Opus 模型优先使用的凭据 ID（用户指定）
/// Opus 模型优先使用的凭据 ID（从 config 读取，None 表示不做定向路由）
fn opus_preferred_credential_id(config: &Config) -> Option<u64> {
    config.opus_preferred_credential_id
}
/// /64 代理地址生成失败时的最大重试次数
const AUTO_PROXY_MAX_GENERATE_ATTEMPTS: usize = 1024;
/// 自动绑定的 SOCKS5H 代理用户名
const AUTO_PROXY_USERNAME: &str = "proxyuser";
/// 自动绑定的 SOCKS5H 代理密码
const AUTO_PROXY_PASSWORD: &str = "INTqEpmpOcK6q2VvKTgRnantFo5HJjcsF6jCYQFa";
/// 自动绑定的 SOCKS5H 代理端口
const AUTO_PROXY_PORT: u16 = 1080;
/// 自动绑定的 /64 IPv6 前缀（前 64 位）
const AUTO_PROXY_PREFIX: [u16; 4] = [0x2001, 0x0470, 0x1f06, 0x0396];

fn is_in_auto_proxy_prefix(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    segments[0] == AUTO_PROXY_PREFIX[0]
        && segments[1] == AUTO_PROXY_PREFIX[1]
        && segments[2] == AUTO_PROXY_PREFIX[2]
        && segments[3] == AUTO_PROXY_PREFIX[3]
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
        return false;
    }

    let seg0 = ip.segments()[0];
    let is_unique_local = (seg0 & 0xfe00) == 0xfc00;
    let is_link_local = (seg0 & 0xffc0) == 0xfe80;
    let is_documentation = seg0 == 0x2001 && ip.segments()[1] == 0x0db8;

    !is_unique_local && !is_link_local && !is_documentation
}

fn build_auto_proxy_url(ip: Ipv6Addr) -> String {
    format!("socks5h://[{ip}]:{AUTO_PROXY_PORT}")
}

fn parse_ip_cidr_to_ipv6(token: &str) -> Option<Ipv6Addr> {
    let ip = token.split_once('/').map(|(addr, _)| addr).unwrap_or(token);
    ip.parse().ok()
}

fn discover_local_public_ipv6() -> Vec<Ipv6Addr> {
    let mut found = HashSet::new();

    if let Ok(output) = Command::new("ip")
        .args(["-6", "-o", "addr", "show", "scope", "global"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let mut parts = line.split_whitespace();
                while let Some(part) = parts.next() {
                    if part == "inet6" {
                        if let Some(cidr) = parts.next() {
                            if let Some(ip) = parse_ip_cidr_to_ipv6(cidr) {
                                if is_public_ipv6(ip) {
                                    found.insert(ip);
                                }
                            }
                        }
                        break;
                    }
                }
            }
        }
    }

    if found.is_empty() {
        if let Ok(output) = Command::new("hostname").arg("-I").output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for token in stdout.split_whitespace() {
                    if let Ok(ip) = token.parse::<Ipv6Addr>() {
                        if is_public_ipv6(ip) {
                            found.insert(ip);
                        }
                    }
                }
            }
        }
    }

    let mut ips: Vec<Ipv6Addr> = found.into_iter().collect();
    ips.sort_unstable();
    ips
}

fn parse_socks5h_ipv6(url: &str) -> Option<Ipv6Addr> {
    let tail = url.strip_prefix("socks5h://")?;
    let host_port = if let Some((_, host_part)) = tail.rsplit_once("@[") {
        host_part
    } else {
        tail.strip_prefix("[")?
    };
    let host = host_port.strip_suffix(&format!("]:{AUTO_PROXY_PORT}"))?;
    host.parse().ok()
}

fn parse_existing_auto_proxy_ip(cred: &KiroCredentials) -> Option<Ipv6Addr> {
    let url = cred.proxy_url.as_deref()?;
    let ip = parse_socks5h_ipv6(url)?;
    if !is_public_ipv6(ip) && !is_in_auto_proxy_prefix(ip) {
        return None;
    }

    let split_auth = cred.proxy_username.as_deref() == Some(AUTO_PROXY_USERNAME)
        && cred.proxy_password.as_deref() == Some(AUTO_PROXY_PASSWORD);
    let inline_auth = url.starts_with(&format!(
        "socks5h://{}:{}@[",
        AUTO_PROXY_USERNAME, AUTO_PROXY_PASSWORD
    ));

    if split_auth || inline_auth {
        Some(ip)
    } else {
        None
    }
}

fn apply_auto_proxy_fields(cred: &mut KiroCredentials, ip: Ipv6Addr) -> bool {
    let new_url = build_auto_proxy_url(ip);
    let changed = cred.proxy_url.as_deref() != Some(new_url.as_str())
        || cred.proxy_username.as_deref() != Some(AUTO_PROXY_USERNAME)
        || cred.proxy_password.as_deref() != Some(AUTO_PROXY_PASSWORD);

    if changed {
        cred.proxy_url = Some(new_url);
        cred.proxy_username = Some(AUTO_PROXY_USERNAME.to_string());
        cred.proxy_password = Some(AUTO_PROXY_PASSWORD.to_string());
    }

    changed
}

fn generate_unique_auto_proxy_ip(
    used_ips: &HashSet<Ipv6Addr>,
    local_public_ipv6: &[Ipv6Addr],
) -> anyhow::Result<Ipv6Addr> {
    let mut available_local_ips: Vec<Ipv6Addr> = local_public_ipv6
        .iter()
        .copied()
        .filter(|ip| !used_ips.contains(ip))
        .collect();

    if !available_local_ips.is_empty() {
        let idx = fastrand::usize(..available_local_ips.len());
        return Ok(available_local_ips.swap_remove(idx));
    }

    for _ in 0..AUTO_PROXY_MAX_GENERATE_ATTEMPTS {
        let ip = Ipv6Addr::new(
            AUTO_PROXY_PREFIX[0],
            AUTO_PROXY_PREFIX[1],
            AUTO_PROXY_PREFIX[2],
            AUTO_PROXY_PREFIX[3],
            fastrand::u16(..),
            fastrand::u16(..),
            fastrand::u16(..),
            fastrand::u16(..),
        );
        if !used_ips.contains(&ip) {
            return Ok(ip);
        }
    }

    anyhow::bail!(
        "自动分配 /64 代理地址失败：已重试 {} 次",
        AUTO_PROXY_MAX_GENERATE_ATTEMPTS
    );
}

fn assign_unique_auto_proxy(
    cred: &mut KiroCredentials,
    used_ips: &mut HashSet<Ipv6Addr>,
    local_public_ipv6: &[Ipv6Addr],
) -> anyhow::Result<bool> {
    if let Some(existing_ip) = parse_existing_auto_proxy_ip(cred) {
        if used_ips.insert(existing_ip) {
            return Ok(apply_auto_proxy_fields(cred, existing_ip));
        }
    }

    let new_ip = generate_unique_auto_proxy_ip(used_ips, local_public_ipv6)?;
    used_ips.insert(new_ip);
    Ok(apply_auto_proxy_fields(cred, new_ip))
}

fn should_auto_assign_proxy(cred: &KiroCredentials) -> bool {
    match cred.proxy_url.as_deref() {
        // 默认不再自动分配代理；仅对历史自动代理配置做兼容性修复。
        None => false,
        Some(url) if url.eq_ignore_ascii_case(KiroCredentials::PROXY_DIRECT) => false,
        Some(_) => parse_existing_auto_proxy_ip(cred).is_some(),
    }
}

fn is_opus_model(model: Option<&str>) -> bool {
    model
        .map(|m| m.to_lowercase().contains("opus"))
        .unwrap_or(false)
}

/// API 调用上下文
///
/// 绑定特定凭据的调用上下文，确保 token、credentials 和 id 的一致性
/// 用于解决并发调用时 current_id 竞态问题
#[derive(Clone)]
pub struct CallContext {
    /// 凭据 ID（用于 report_success/report_failure）
    pub id: u64,
    /// 凭据信息（用于构建请求头）
    pub credentials: KiroCredentials,
    /// 访问 Token
    pub token: String,
}

impl MultiTokenManager {
    /// 创建多凭据 Token 管理器
    ///
    /// # Arguments
    /// * `config` - 应用配置
    /// * `credentials` - 凭据列表
    /// * `proxy` - 可选的代理配置
    /// * `credentials_path` - 凭据文件路径（用于回写）
    /// * `is_multiple_format` - 是否为多凭据格式（数组格式才回写）
    pub fn new(
        config: Config,
        credentials: Vec<KiroCredentials>,
        proxy: Option<ProxyConfig>,
        credentials_path: Option<PathBuf>,
        is_multiple_format: bool,
    ) -> anyhow::Result<Self> {
        // 计算当前最大 ID，为没有 ID 的凭据分配新 ID
        let max_existing_id = credentials.iter().filter_map(|c| c.id).max().unwrap_or(0);
        let mut next_id = max_existing_id + 1;
        let mut has_new_ids = false;
        let mut has_new_machine_ids = false;
        let mut has_new_auto_proxies = false;
        let config_ref = &config;
        let local_public_ipv6 = discover_local_public_ipv6();
        let mut used_auto_proxy_ips = HashSet::new();

        let mut entries: Vec<CredentialEntry> = Vec::with_capacity(credentials.len());
        for mut cred in credentials {
            cred.canonicalize_auth_method();
            if should_auto_assign_proxy(&cred) {
                if assign_unique_auto_proxy(
                    &mut cred,
                    &mut used_auto_proxy_ips,
                    &local_public_ipv6,
                )? {
                    has_new_auto_proxies = true;
                }
            }
            let id = cred.id.unwrap_or_else(|| {
                let id = next_id;
                next_id += 1;
                cred.id = Some(id);
                has_new_ids = true;
                id
            });
            if cred.machine_id.is_none() {
                if let Some(machine_id) = machine_id::generate_from_credentials(&cred, config_ref) {
                    cred.machine_id = Some(machine_id);
                    has_new_machine_ids = true;
                }
            }
            let disabled = cred.disabled;
            entries.push(CredentialEntry {
                id,
                credentials: cred,
                failure_count: 0,
                // 启动时保留配置文件中的手动禁用状态
                disabled,
                disabled_reason: if disabled {
                    Some(DisabledReason::Manual)
                } else {
                    None
                },
                success_count: 0,
                last_used_at: None,
                last_request_credits: 0.0,
                total_credits: 0.0,
            });
        }

        // 检测重复 ID
        let mut seen_ids = std::collections::HashSet::new();
        let mut duplicate_ids = Vec::new();
        for entry in &entries {
            if !seen_ids.insert(entry.id) {
                duplicate_ids.push(entry.id);
            }
        }
        if !duplicate_ids.is_empty() {
            anyhow::bail!("检测到重复的凭据 ID: {:?}", duplicate_ids);
        }

        // 选择初始凭据：优先级最高（priority 最小）的凭据，无凭据时为 0
        let initial_id = entries
            .iter()
            .min_by_key(|e| e.credentials.priority)
            .map(|e| e.id)
            .unwrap_or(0);

        let load_balancing_mode = config.load_balancing_mode.clone();
        let strip_billing_header = config.strip_billing_header;
        let manager = Self {
            config,
            proxy,
            entries: Mutex::new(entries),
            current_id: Mutex::new(initial_id),
            refresh_lock: TokioMutex::new(()),
            credentials_path,
            is_multiple_format,
            load_balancing_mode: Mutex::new(load_balancing_mode),
            strip_billing_header: Mutex::new(strip_billing_header),
            last_stats_save_at: Mutex::new(None),
            stats_dirty: AtomicBool::new(false),
            route_affinity: Mutex::new(HashMap::new()),
            weighted_round_robin_state: Mutex::new(HashMap::new()),
        };

        // 如果有新分配的 ID/machineId/代理，立即持久化到配置文件
        if has_new_ids || has_new_machine_ids || has_new_auto_proxies {
            if let Err(e) = manager.persist_credentials() {
                tracing::warn!("补全凭据 ID/machineId/代理 后持久化失败: {}", e);
            } else {
                tracing::info!("已补全凭据 ID/machineId/代理 并写回配置文件");
            }
        }

        // 加载持久化的统计数据（success_count / credits / last_used_at）
        manager.load_stats();

        Ok(manager)
    }

    /// 获取配置的引用
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// 获取当前活动凭据的克隆
    pub fn credentials(&self) -> KiroCredentials {
        let entries = self.entries.lock();
        let current_id = *self.current_id.lock();
        entries
            .iter()
            .find(|e| e.id == current_id)
            .map(|e| e.credentials.clone())
            .unwrap_or_default()
    }

    /// 获取凭据总数
    pub fn total_count(&self) -> usize {
        self.entries.lock().len()
    }

    /// 获取可用凭据数量
    pub fn available_count(&self) -> usize {
        self.entries.lock().iter().filter(|e| !e.disabled).count()
    }

    fn prune_route_affinity_locked(route_affinity: &mut HashMap<String, RouteAffinityEntry>) {
        route_affinity.retain(|_, entry| entry.last_seen_at.elapsed() < ROUTE_AFFINITY_TTL);
    }

    fn get_preferred_credential_for_route(
        &self,
        route_key: &str,
        model: Option<&str>,
    ) -> Option<(u64, KiroCredentials)> {
        let mapped_id = {
            let mut route_affinity = self.route_affinity.lock();
            Self::prune_route_affinity_locked(&mut route_affinity);
            route_affinity
                .get(route_key)
                .map(|entry| entry.credential_id)
        }?;

        let is_opus = is_opus_model(model);
        let preferred = {
            let entries = self.entries.lock();
            entries
                .iter()
                .find(|e| {
                    e.id == mapped_id && !e.disabled && (!is_opus || e.credentials.supports_opus())
                })
                .map(|e| (e.id, e.credentials.clone()))
        };

        if preferred.is_none() {
            self.clear_route_affinity_if_matches(route_key, mapped_id);
        }

        preferred
    }

    fn remember_route_affinity(&self, route_key: &str, credential_id: u64) {
        let mut route_affinity = self.route_affinity.lock();
        Self::prune_route_affinity_locked(&mut route_affinity);

        if !route_affinity.contains_key(route_key)
            && route_affinity.len() >= ROUTE_AFFINITY_MAX_ENTRIES
        {
            if let Some(oldest_key) = route_affinity
                .iter()
                .max_by_key(|(_, entry)| entry.last_seen_at.elapsed())
                .map(|(key, _)| key.clone())
            {
                route_affinity.remove(&oldest_key);
            }
        }

        route_affinity.insert(
            route_key.to_string(),
            RouteAffinityEntry {
                credential_id,
                last_seen_at: Instant::now(),
            },
        );
    }

    /// 清除指定 route_key 的亲和绑定（仅当当前映射命中该凭据）
    pub fn clear_route_affinity_if_matches(&self, route_key: &str, credential_id: u64) -> bool {
        let mut route_affinity = self.route_affinity.lock();
        match route_affinity.get(route_key) {
            Some(entry) if entry.credential_id == credential_id => {
                route_affinity.remove(route_key);
                true
            }
            _ => false,
        }
    }

    fn clear_route_affinity_for_credential(&self, credential_id: u64) -> usize {
        let mut route_affinity = self.route_affinity.lock();
        let before = route_affinity.len();
        route_affinity.retain(|_, entry| entry.credential_id != credential_id);
        before.saturating_sub(route_affinity.len())
    }

    /// 根据负载均衡模式选择下一个凭据
    ///
    /// - priority 模式：选择优先级最高（priority 最小）的可用凭据
    /// - balanced 模式：轮询选择可用凭据
    /// - weighted_round_robin 模式：基于优先级权重做平滑轮询
    ///
    /// # 参数
    /// - `model`: 可选的模型名称，用于过滤支持该模型的凭据（如 opus 模型需要付费订阅）
    /// - `excluded_ids`: 本次选择中需要跳过的凭据 ID（用于同一次请求内失败后重试）
    fn select_next_credential(
        &self,
        model: Option<&str>,
        excluded_ids: Option<&HashSet<u64>>,
    ) -> Option<(u64, KiroCredentials)> {
        let entries = self.entries.lock();

        // 检查是否是 opus 模型
        let is_opus = is_opus_model(model);
        let is_excluded = |id: u64| excluded_ids.map(|ids| ids.contains(&id)).unwrap_or(false);

        // Opus 请求优先走配置指定的凭据（如果配置了 opus_preferred_credential_id）
        if is_opus {
            if let Some(preferred_id) = opus_preferred_credential_id(&self.config) {
                if let Some(entry) = entries.iter().find(|e| {
                    e.id == preferred_id
                        && !is_excluded(e.id)
                        && !e.disabled
                        && e.credentials.supports_opus()
                }) {
                    return Some((entry.id, entry.credentials.clone()));
                }
            }
        }

        // 过滤可用凭据
        let available: Vec<_> = entries
            .iter()
            .filter(|e| {
                if e.disabled {
                    return false;
                }
                if is_excluded(e.id) {
                    return false;
                }
                // 如果是 opus 模型，需要检查订阅等级
                if is_opus && !e.credentials.supports_opus() {
                    return false;
                }
                true
            })
            .collect();

        if available.is_empty() {
            return None;
        }

        let mode = self.load_balancing_mode.lock().clone();
        let mode = mode.as_str();

        match mode {
            "balanced" => {
                // Least-Used 策略：选择成功次数最少的凭据
                // 优先避开正在失败的凭据（failure_count > 0），防止重试时反复选到同一个失败凭据
                // 平局时按优先级排序（数字越小优先级越高）
                let entry = available
                    .iter()
                    .min_by_key(|e| (e.failure_count, e.success_count, e.credentials.priority))?;

                Some((entry.id, entry.credentials.clone()))
            }
            "weighted_round_robin" => {
                let all_ids: HashSet<u64> = entries.iter().map(|e| e.id).collect();
                self.select_weighted_round_robin(&available, &all_ids)
            }
            _ => {
                // priority 模式（默认）：选择优先级最高的
                let entry = available.iter().min_by_key(|e| e.credentials.priority)?;
                Some((entry.id, entry.credentials.clone()))
            }
        }
    }

    fn priority_to_weight(priority: u32, max_priority: u32) -> i64 {
        i64::from(max_priority.saturating_sub(priority).saturating_add(1))
    }

    fn select_weighted_round_robin(
        &self,
        available: &[&CredentialEntry],
        all_ids: &HashSet<u64>,
    ) -> Option<(u64, KiroCredentials)> {
        if available.is_empty() {
            return None;
        }

        let max_priority = available
            .iter()
            .map(|entry| entry.credentials.priority)
            .max()
            .unwrap_or(0);

        let mut state = self.weighted_round_robin_state.lock();
        // 只清理已不存在于全部凭据列表中的条目（真正被删除/禁用的凭据）
        // 不要按 available_ids 清理，因为 available 可能已被 excluded_ids 过滤，
        // 临时排除的凭据不应丢失权重状态
        // 注意：all_ids 由调用方在持有 entries 锁时收集，避免此处重复加锁导致死锁
        state.retain(|credential_id, _| all_ids.contains(credential_id));

        let mut total_weight = 0i64;
        let mut selected_id: Option<u64> = None;
        let mut selected_priority = u32::MAX;
        let mut selected_current_weight = i64::MIN;

        for entry in available {
            let weight = Self::priority_to_weight(entry.credentials.priority, max_priority);
            total_weight += weight;

            let current_weight = state.entry(entry.id).or_insert(0);
            *current_weight += weight;

            if *current_weight > selected_current_weight
                || (*current_weight == selected_current_weight
                    && (entry.credentials.priority < selected_priority
                        || (entry.credentials.priority == selected_priority
                            && selected_id.map(|id| entry.id < id).unwrap_or(true))))
            {
                selected_id = Some(entry.id);
                selected_priority = entry.credentials.priority;
                selected_current_weight = *current_weight;
            }
        }

        let selected_id = selected_id?;
        if let Some(current_weight) = state.get_mut(&selected_id) {
            *current_weight -= total_weight;
        }

        available
            .iter()
            .find(|entry| entry.id == selected_id)
            .map(|entry| (entry.id, entry.credentials.clone()))
    }

    /// 获取 API 调用上下文
    ///
    /// 返回绑定了 id、credentials 和 token 的调用上下文
    /// 确保整个 API 调用过程中使用一致的凭据信息
    ///
    /// 如果 Token 过期或即将过期，会自动刷新
    /// Token 刷新失败时会尝试下一个可用凭据（不计入失败次数）
    ///
    /// # 参数
    /// - `model`: 可选的模型名称，用于过滤支持该模型的凭据（如 opus 模型需要付费订阅）
    /// - `route_key`: 可选路由键（命中后 1 小时内优先复用同一凭据）
    pub async fn acquire_context(
        &self,
        model: Option<&str>,
        route_key: Option<&str>,
    ) -> anyhow::Result<CallContext> {
        let total = self.total_count();
        let mut tried_count = 0;
        let mut tried_ids = HashSet::new();

        loop {
            if tried_count >= total {
                anyhow::bail!(
                    "所有凭据均无法获取有效 Token（可用: {}/{}）",
                    self.available_count(),
                    total
                );
            }

            let (id, credentials) = {
                let is_opus = is_opus_model(model);
                let load_balancing_mode = self.load_balancing_mode.lock().clone();
                let is_dynamic_mode = matches!(
                    load_balancing_mode.as_str(),
                    "balanced" | "weighted_round_robin"
                );
                let route_hit = route_key
                    .and_then(|key| self.get_preferred_credential_for_route(key, model))
                    .filter(|(candidate_id, _)| !tried_ids.contains(candidate_id));

                // 1) 先命中 route_key 的亲和凭据
                // 2) 未命中再走现有负载均衡策略
                let current_hit = if route_hit.is_some() {
                    route_hit
                } else if is_dynamic_mode || is_opus {
                    // balanced / weighted_round_robin 模式：每次请求都动态选择，不固定 current_id
                    // Opus 请求不直接命中 current_id，必须走 select_next_credential 以应用 Opus 过滤规则
                    None
                } else {
                    // priority 模式：优先使用 current_id 指向的凭据
                    let entries = self.entries.lock();
                    let current_id = *self.current_id.lock();
                    entries
                        .iter()
                        .find(|e| e.id == current_id && !e.disabled && !tried_ids.contains(&e.id))
                        .map(|e| (e.id, e.credentials.clone()))
                };

                if let Some(hit) = current_hit {
                    hit
                } else {
                    // 当前凭据不可用或动态模式，根据负载均衡策略选择
                    let mut best = self.select_next_credential(model, Some(&tried_ids));

                    // 没有可用凭据：如果是"自动禁用导致全灭"，做一次类似重启的自愈
                    if best.is_none() {
                        let mut entries = self.entries.lock();
                        if entries.iter().any(|e| {
                            e.disabled && e.disabled_reason == Some(DisabledReason::TooManyFailures)
                        }) {
                            tracing::warn!(
                                "所有凭据均已被自动禁用，执行自愈：重置失败计数并重新启用（等价于重启）"
                            );
                            for e in entries.iter_mut() {
                                if e.disabled_reason == Some(DisabledReason::TooManyFailures) {
                                    e.disabled = false;
                                    e.disabled_reason = None;
                                    e.failure_count = 0;
                                }
                            }
                            drop(entries);
                            best = self.select_next_credential(model, Some(&tried_ids));
                        }
                    }

                    if let Some((new_id, new_creds)) = best {
                        // 更新 current_id
                        let mut current_id = self.current_id.lock();
                        *current_id = new_id;
                        (new_id, new_creds)
                    } else {
                        let entries = self.entries.lock();
                        // 注意：必须在 bail! 之前计算 available_count，
                        // 因为 available_count() 会尝试获取 entries 锁，
                        // 而此时我们已经持有该锁，会导致死锁
                        let available = entries.iter().filter(|e| !e.disabled).count();
                        anyhow::bail!("所有凭据均已禁用（{}/{}）", available, total);
                    }
                }
            };

            // 尝试获取/刷新 Token
            match self.try_ensure_token(id, &credentials).await {
                Ok(ctx) => {
                    if let Some(route_key) = route_key {
                        self.remember_route_affinity(route_key, id);
                    }
                    return Ok(ctx);
                }
                Err(e) => {
                    tracing::warn!("凭据 #{} Token 刷新失败，尝试下一个凭据: {}", id, e);

                    if let Some(route_key) = route_key {
                        self.clear_route_affinity_if_matches(route_key, id);
                    }
                    tried_ids.insert(id);
                    // Token 刷新失败，切换到下一个优先级的凭据（不计入失败次数）
                    self.switch_to_next_by_priority();
                    tried_count += 1;
                }
            }
        }
    }

    /// 切换到下一个优先级最高的可用凭据（内部方法）
    fn switch_to_next_by_priority(&self) {
        let entries = self.entries.lock();
        let mut current_id = self.current_id.lock();

        // 选择优先级最高的未禁用凭据（排除当前凭据）
        if let Some(entry) = entries
            .iter()
            .filter(|e| !e.disabled && e.id != *current_id)
            .min_by_key(|e| e.credentials.priority)
        {
            *current_id = entry.id;
            tracing::info!(
                "已切换到凭据 #{}（优先级 {}）",
                entry.id,
                entry.credentials.priority
            );
        }
    }

    /// 选择优先级最高的未禁用凭据作为当前凭据（内部方法）
    ///
    /// 与 `switch_to_next_by_priority` 不同，此方法不排除当前凭据，
    /// 纯粹按优先级选择，用于优先级变更后立即生效
    fn select_highest_priority(&self) {
        let entries = self.entries.lock();
        let mut current_id = self.current_id.lock();

        // 选择优先级最高的未禁用凭据（不排除当前凭据）
        if let Some(best) = entries
            .iter()
            .filter(|e| !e.disabled)
            .min_by_key(|e| e.credentials.priority)
        {
            if best.id != *current_id {
                tracing::info!(
                    "优先级变更后切换凭据: #{} -> #{}（优先级 {}）",
                    *current_id,
                    best.id,
                    best.credentials.priority
                );
                *current_id = best.id;
            }
        }
    }

    /// 尝试使用指定凭据获取有效 Token
    ///
    /// 使用双重检查锁定模式，确保同一时间只有一个刷新操作
    ///
    /// # Arguments
    /// * `id` - 凭据 ID，用于更新正确的条目
    /// * `credentials` - 凭据信息
    async fn try_ensure_token(
        &self,
        id: u64,
        credentials: &KiroCredentials,
    ) -> anyhow::Result<CallContext> {
        // 第一次检查（无锁）：快速判断是否需要刷新
        let needs_refresh = is_token_expired(credentials) || is_token_expiring_soon(credentials);

        let creds = if needs_refresh {
            // 获取刷新锁，确保同一时间只有一个刷新操作
            let _guard = self.refresh_lock.lock().await;

            // 第二次检查：获取锁后重新读取凭据，因为其他请求可能已经完成刷新
            let current_creds = {
                let entries = self.entries.lock();
                entries
                    .iter()
                    .find(|e| e.id == id)
                    .map(|e| e.credentials.clone())
                    .ok_or_else(|| anyhow::anyhow!("凭据 #{} 不存在", id))?
            };

            if is_token_expired(&current_creds) || is_token_expiring_soon(&current_creds) {
                // 确实需要刷新
                let effective_proxy = current_creds.effective_proxy(self.proxy.as_ref());
                let new_creds =
                    refresh_token(&current_creds, &self.config, effective_proxy.as_ref()).await?;

                if is_token_expired(&new_creds) {
                    anyhow::bail!("刷新后的 Token 仍然无效或已过期");
                }

                // 更新凭据
                {
                    let mut entries = self.entries.lock();
                    if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
                        entry.credentials = new_creds.clone();
                    }
                }

                // 回写凭据到文件（仅多凭据格式），失败只记录警告
                if let Err(e) = self.persist_credentials() {
                    tracing::warn!("Token 刷新后持久化失败（不影响本次请求）: {}", e);
                }

                new_creds
            } else {
                // 其他请求已经完成刷新，直接使用新凭据
                tracing::debug!("Token 已被其他请求刷新，跳过刷新");
                current_creds
            }
        } else {
            credentials.clone()
        };

        let token = creds
            .access_token
            .clone()
            .ok_or_else(|| anyhow::anyhow!("没有可用的 accessToken"))?;

        Ok(CallContext {
            id,
            credentials: creds,
            token,
        })
    }

    /// 将凭据列表回写到源文件
    ///
    /// 仅在以下条件满足时回写：
    /// - 源文件是多凭据格式（数组）
    /// - credentials_path 已设置
    ///
    /// # Returns
    /// - `Ok(true)` - 成功写入文件
    /// - `Ok(false)` - 跳过写入（非多凭据格式或无路径配置）
    /// - `Err(_)` - 写入失败
    fn persist_credentials(&self) -> anyhow::Result<bool> {
        use anyhow::Context;

        // 仅多凭据格式才回写
        if !self.is_multiple_format {
            return Ok(false);
        }

        let path = match &self.credentials_path {
            Some(p) => p,
            None => return Ok(false),
        };

        // 收集所有凭据
        let credentials: Vec<KiroCredentials> = {
            let entries = self.entries.lock();
            entries
                .iter()
                .map(|e| {
                    let mut cred = e.credentials.clone();
                    cred.canonicalize_auth_method();
                    // 同步 disabled 状态到凭据对象
                    cred.disabled = e.disabled;
                    cred
                })
                .collect()
        };

        // 序列化为 pretty JSON
        let json = serde_json::to_string_pretty(&credentials).context("序列化凭据失败")?;

        // 写入文件（在 Tokio runtime 内使用 block_in_place 避免阻塞 worker）
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::task::block_in_place(|| std::fs::write(path, &json))
                .with_context(|| format!("回写凭据文件失败: {:?}", path))?;
        } else {
            std::fs::write(path, &json).with_context(|| format!("回写凭据文件失败: {:?}", path))?;
        }

        tracing::debug!("已回写凭据到文件: {:?}", path);
        Ok(true)
    }

    /// 获取缓存目录（凭据文件所在目录）
    pub fn cache_dir(&self) -> Option<PathBuf> {
        self.credentials_path
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    }

    /// 统计数据文件路径
    fn stats_path(&self) -> Option<PathBuf> {
        self.cache_dir().map(|d| d.join("kiro_stats.json"))
    }

    /// 从磁盘加载统计数据并应用到当前条目
    fn load_stats(&self) {
        let path = match self.stats_path() {
            Some(p) => p,
            None => return,
        };

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return, // 首次运行时文件不存在
        };

        let stats: HashMap<String, StatsEntry> = match serde_json::from_str(&content) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("解析统计缓存失败，将忽略: {}", e);
                return;
            }
        };

        let mut entries = self.entries.lock();
        for entry in entries.iter_mut() {
            if let Some(s) = stats.get(&entry.id.to_string()) {
                entry.success_count = s.success_count;
                entry.last_used_at = s.last_used_at.clone();
                entry.last_request_credits = s.last_request_credits.max(0.0);
                entry.total_credits = s.total_credits.max(0.0);
            }
        }
        *self.last_stats_save_at.lock() = Some(Instant::now());
        self.stats_dirty.store(false, Ordering::Relaxed);
        tracing::info!("已从缓存加载 {} 条统计数据", stats.len());
    }

    /// 将当前统计数据持久化到磁盘
    fn save_stats(&self) {
        let path = match self.stats_path() {
            Some(p) => p,
            None => return,
        };

        let stats: HashMap<String, StatsEntry> = {
            let entries = self.entries.lock();
            entries
                .iter()
                .map(|e| {
                    (
                        e.id.to_string(),
                        StatsEntry {
                            success_count: e.success_count,
                            last_used_at: e.last_used_at.clone(),
                            last_request_credits: e.last_request_credits,
                            total_credits: e.total_credits,
                        },
                    )
                })
                .collect()
        };

        match serde_json::to_string_pretty(&stats) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    tracing::warn!("保存统计缓存失败: {}", e);
                } else {
                    *self.last_stats_save_at.lock() = Some(Instant::now());
                    self.stats_dirty.store(false, Ordering::Relaxed);
                }
            }
            Err(e) => tracing::warn!("序列化统计数据失败: {}", e),
        }
    }

    /// 标记统计数据已更新，并按 debounce 策略决定是否立即落盘
    fn save_stats_debounced(&self) {
        self.stats_dirty.store(true, Ordering::Relaxed);

        let should_flush = {
            let last = *self.last_stats_save_at.lock();
            match last {
                Some(last_saved_at) => last_saved_at.elapsed() >= STATS_SAVE_DEBOUNCE,
                None => true,
            }
        };

        if should_flush {
            self.save_stats();
        }
    }

    /// 报告指定凭据 API 调用成功
    ///
    /// 重置该凭据的失败计数
    ///
    /// # Arguments
    /// * `id` - 凭据 ID（来自 CallContext）
    pub fn report_success(&self, id: u64) {
        {
            let mut entries = self.entries.lock();
            if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
                entry.failure_count = 0;
                entry.success_count += 1;
                entry.last_used_at = Some(Utc::now().to_rfc3339());
                tracing::debug!(
                    "凭据 #{} API 调用成功（累计 {} 次）",
                    id,
                    entry.success_count
                );
            }
        }
        self.save_stats_debounced();
    }

    /// 报告指定凭据本次请求消耗的 credits
    ///
    /// - `credits_used` 小于等于 0 时只更新最近值为 0，不累计
    /// - 会将本次消耗累加到 `total_credits`
    pub fn report_credits(&self, id: u64, credits_used: f64) {
        let credits_used = if credits_used.is_finite() {
            credits_used.max(0.0)
        } else {
            0.0
        };

        {
            let mut entries = self.entries.lock();
            if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
                entry.last_request_credits = credits_used;
                entry.total_credits += credits_used;
                tracing::debug!(
                    "凭据 #{} 本次 credits: {:.6}, 累计 credits: {:.6}",
                    id,
                    entry.last_request_credits,
                    entry.total_credits
                );
            }
        }
        self.save_stats_debounced();
    }

    /// 报告指定凭据 API 调用失败
    ///
    /// 增加失败计数，达到阈值时禁用凭据并切换到优先级最高的可用凭据
    /// 返回是否还有可用凭据可以重试
    ///
    /// # Arguments
    /// * `id` - 凭据 ID（来自 CallContext）
    pub fn report_failure(&self, id: u64) -> bool {
        let mut affinity_cleanup_id: Option<u64> = None;
        let result = {
            let mut entries = self.entries.lock();
            let mut current_id = self.current_id.lock();

            let entry = match entries.iter_mut().find(|e| e.id == id) {
                Some(e) => e,
                None => return entries.iter().any(|e| !e.disabled),
            };

            entry.failure_count += 1;
            entry.last_used_at = Some(Utc::now().to_rfc3339());
            let failure_count = entry.failure_count;

            tracing::warn!(
                "凭据 #{} API 调用失败（{}/{}）",
                id,
                failure_count,
                MAX_FAILURES_PER_CREDENTIAL
            );

            if failure_count >= MAX_FAILURES_PER_CREDENTIAL {
                entry.disabled = true;
                entry.disabled_reason = Some(DisabledReason::TooManyFailures);
                affinity_cleanup_id = Some(id);
                tracing::error!("凭据 #{} 已连续失败 {} 次，已被禁用", id, failure_count);

                // 切换到优先级最高的可用凭据
                if let Some(next) = entries
                    .iter()
                    .filter(|e| !e.disabled)
                    .min_by_key(|e| e.credentials.priority)
                {
                    *current_id = next.id;
                    tracing::info!(
                        "已切换到凭据 #{}（优先级 {}）",
                        next.id,
                        next.credentials.priority
                    );
                } else {
                    tracing::error!("所有凭据均已禁用！");
                }
            }

            entries.iter().any(|e| !e.disabled)
        };
        if let Some(credential_id) = affinity_cleanup_id {
            self.clear_route_affinity_for_credential(credential_id);
        }
        self.save_stats_debounced();
        result
    }

    /// 报告指定凭据额度已用尽
    ///
    /// 用于处理 402 Payment Required 且 reason 为 `MONTHLY_REQUEST_COUNT` 的场景：
    /// - 立即禁用该凭据（不等待连续失败阈值）
    /// - 切换到下一个可用凭据继续重试
    /// - 返回是否还有可用凭据
    pub fn report_quota_exhausted(&self, id: u64) -> bool {
        let should_clear_route_affinity: bool;
        let result = {
            let mut entries = self.entries.lock();
            let mut current_id = self.current_id.lock();

            let entry = match entries.iter_mut().find(|e| e.id == id) {
                Some(e) => e,
                None => return entries.iter().any(|e| !e.disabled),
            };

            if entry.disabled {
                return entries.iter().any(|e| !e.disabled);
            }

            entry.disabled = true;
            entry.disabled_reason = Some(DisabledReason::QuotaExceeded);
            should_clear_route_affinity = true;
            entry.last_used_at = Some(Utc::now().to_rfc3339());
            // 设为阈值，便于在管理面板中直观看到该凭据已不可用
            entry.failure_count = MAX_FAILURES_PER_CREDENTIAL;

            tracing::error!("凭据 #{} 额度已用尽（MONTHLY_REQUEST_COUNT），已被禁用", id);

            // 切换到优先级最高的可用凭据
            if let Some(next) = entries
                .iter()
                .filter(|e| !e.disabled)
                .min_by_key(|e| e.credentials.priority)
            {
                *current_id = next.id;
                tracing::info!(
                    "已切换到凭据 #{}（优先级 {}）",
                    next.id,
                    next.credentials.priority
                );
                true
            } else {
                tracing::error!("所有凭据均已禁用！");
                false
            }
        };
        if should_clear_route_affinity {
            self.clear_route_affinity_for_credential(id);
        }
        self.save_stats_debounced();
        result
    }

    /// 切换到优先级最高的可用凭据
    ///
    /// 返回是否成功切换
    pub fn switch_to_next(&self) -> bool {
        let entries = self.entries.lock();
        let mut current_id = self.current_id.lock();

        // 选择优先级最高的未禁用凭据（排除当前凭据）
        if let Some(next) = entries
            .iter()
            .filter(|e| !e.disabled && e.id != *current_id)
            .min_by_key(|e| e.credentials.priority)
        {
            *current_id = next.id;
            tracing::info!(
                "已切换到凭据 #{}（优先级 {}）",
                next.id,
                next.credentials.priority
            );
            true
        } else {
            // 没有其他可用凭据，检查当前凭据是否可用
            entries.iter().any(|e| e.id == *current_id && !e.disabled)
        }
    }

    /// 获取使用额度信息
    pub async fn get_usage_limits(&self) -> anyhow::Result<UsageLimitsResponse> {
        let ctx = self.acquire_context(None, None).await?;
        let effective_proxy = ctx.credentials.effective_proxy(self.proxy.as_ref());
        get_usage_limits(
            &ctx.credentials,
            &self.config,
            &ctx.token,
            effective_proxy.as_ref(),
        )
        .await
    }

    // ========================================================================
    // Admin API 方法
    // ========================================================================

    /// 获取管理器状态快照（用于 Admin API）
    pub fn snapshot(&self) -> ManagerSnapshot {
        let entries = self.entries.lock();
        let current_id = *self.current_id.lock();
        let available = entries.iter().filter(|e| !e.disabled).count();

        ManagerSnapshot {
            entries: entries
                .iter()
                .map(|e| CredentialEntrySnapshot {
                    id: e.id,
                    priority: e.credentials.priority,
                    disabled: e.disabled,
                    failure_count: e.failure_count,
                    auth_method: e.credentials.auth_method.as_deref().map(|m| {
                        if m.eq_ignore_ascii_case("builder-id") || m.eq_ignore_ascii_case("iam") {
                            "idc".to_string()
                        } else {
                            m.to_string()
                        }
                    }),
                    has_profile_arn: e.credentials.profile_arn.is_some(),
                    expires_at: e.credentials.expires_at.clone(),
                    refresh_token_hash: e.credentials.refresh_token.as_deref().map(sha256_hex),
                    email: e.credentials.email.clone(),
                    success_count: e.success_count,
                    last_used_at: e.last_used_at.clone(),
                    last_request_credits: e.last_request_credits,
                    total_credits: e.total_credits,
                    has_proxy: e.credentials.proxy_url.is_some(),
                    proxy_url: e.credentials.proxy_url.clone(),
                    subscription_title: e.credentials.subscription_title.clone(),
                })
                .collect(),
            current_id,
            total: entries.len(),
            available,
        }
    }

    /// 设置凭据禁用状态（Admin API）
    pub fn set_disabled(&self, id: u64, disabled: bool) -> anyhow::Result<()> {
        {
            let mut entries = self.entries.lock();
            let entry = entries
                .iter_mut()
                .find(|e| e.id == id)
                .ok_or_else(|| anyhow::anyhow!("凭据不存在: {}", id))?;
            entry.disabled = disabled;
            if !disabled {
                // 启用时重置失败计数
                entry.failure_count = 0;
                entry.disabled_reason = None;
            } else {
                entry.disabled_reason = Some(DisabledReason::Manual);
            }
        }
        if disabled {
            self.clear_route_affinity_for_credential(id);
        }
        // 持久化更改
        self.persist_credentials()?;
        Ok(())
    }

    /// 设置凭据优先级（Admin API）
    ///
    /// 修改优先级后会立即按新优先级重新选择当前凭据。
    /// 即使持久化失败，内存中的优先级和当前凭据选择也会生效。
    pub fn set_priority(&self, id: u64, priority: u32) -> anyhow::Result<()> {
        {
            let mut entries = self.entries.lock();
            let entry = entries
                .iter_mut()
                .find(|e| e.id == id)
                .ok_or_else(|| anyhow::anyhow!("凭据不存在: {}", id))?;
            entry.credentials.priority = priority;
        }
        // 立即按新优先级重新选择当前凭据（无论持久化是否成功）
        self.select_highest_priority();
        // 持久化更改
        self.persist_credentials()?;
        Ok(())
    }

    /// 重置凭据失败计数并重新启用（Admin API）
    pub fn reset_and_enable(&self, id: u64) -> anyhow::Result<()> {
        {
            let mut entries = self.entries.lock();
            let entry = entries
                .iter_mut()
                .find(|e| e.id == id)
                .ok_or_else(|| anyhow::anyhow!("凭据不存在: {}", id))?;
            entry.failure_count = 0;
            entry.disabled = false;
            entry.disabled_reason = None;
        }
        // 持久化更改
        self.persist_credentials()?;
        Ok(())
    }

    /// 获取指定凭据的使用额度（Admin API）
    pub async fn get_usage_limits_for(&self, id: u64) -> anyhow::Result<UsageLimitsResponse> {
        let credentials = {
            let entries = self.entries.lock();
            entries
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.credentials.clone())
                .ok_or_else(|| anyhow::anyhow!("凭据不存在: {}", id))?
        };

        // 检查是否需要刷新 token
        let needs_refresh = is_token_expired(&credentials) || is_token_expiring_soon(&credentials);

        let token = if needs_refresh {
            let _guard = self.refresh_lock.lock().await;
            let current_creds = {
                let entries = self.entries.lock();
                entries
                    .iter()
                    .find(|e| e.id == id)
                    .map(|e| e.credentials.clone())
                    .ok_or_else(|| anyhow::anyhow!("凭据不存在: {}", id))?
            };

            if is_token_expired(&current_creds) || is_token_expiring_soon(&current_creds) {
                let effective_proxy = current_creds.effective_proxy(self.proxy.as_ref());
                let new_creds =
                    refresh_token(&current_creds, &self.config, effective_proxy.as_ref()).await?;
                {
                    let mut entries = self.entries.lock();
                    if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
                        entry.credentials = new_creds.clone();
                    }
                }
                // 持久化失败只记录警告，不影响本次请求
                if let Err(e) = self.persist_credentials() {
                    tracing::warn!("Token 刷新后持久化失败（不影响本次请求）: {}", e);
                }
                new_creds
                    .access_token
                    .ok_or_else(|| anyhow::anyhow!("刷新后无 access_token"))?
            } else {
                current_creds
                    .access_token
                    .ok_or_else(|| anyhow::anyhow!("凭据无 access_token"))?
            }
        } else {
            credentials
                .access_token
                .ok_or_else(|| anyhow::anyhow!("凭据无 access_token"))?
        };

        let credentials = {
            let entries = self.entries.lock();
            entries
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.credentials.clone())
                .ok_or_else(|| anyhow::anyhow!("凭据不存在: {}", id))?
        };

        let effective_proxy = credentials.effective_proxy(self.proxy.as_ref());
        let usage_limits =
            get_usage_limits(&credentials, &self.config, &token, effective_proxy.as_ref()).await?;

        // 更新订阅等级到凭据（仅在发生变化时持久化）
        if let Some(subscription_title) = usage_limits.subscription_title() {
            let changed = {
                let mut entries = self.entries.lock();
                if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
                    let old_title = entry.credentials.subscription_title.clone();
                    if old_title.as_deref() != Some(subscription_title) {
                        entry.credentials.subscription_title = Some(subscription_title.to_string());
                        tracing::info!(
                            "凭据 #{} 订阅等级已更新: {:?} -> {}",
                            id,
                            old_title,
                            subscription_title
                        );
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            };

            if changed {
                if let Err(e) = self.persist_credentials() {
                    tracing::warn!("订阅等级更新后持久化失败（不影响本次请求）: {}", e);
                }
            }
        }

        Ok(usage_limits)
    }

    /// 添加新凭据（Admin API）
    ///
    /// # 流程
    /// 1. 验证凭据基本字段（refresh_token 不为空）
    /// 2. 基于 refreshToken 的 SHA-256 哈希检测重复
    /// 3. 尝试刷新 Token 验证凭据有效性
    /// 4. 分配新 ID（当前最大 ID + 1）
    /// 5. 添加到 entries 列表
    /// 6. 持久化到配置文件
    ///
    /// # 返回
    /// - `Ok(u64)` - 新凭据 ID
    /// - `Err(_)` - 验证失败或添加失败
    pub async fn add_credential(&self, new_cred: KiroCredentials) -> anyhow::Result<u64> {
        let mut new_cred = new_cred;
        if should_auto_assign_proxy(&new_cred) {
            let local_public_ipv6 = discover_local_public_ipv6();
            let mut used_auto_proxy_ips = {
                let entries = self.entries.lock();
                entries
                    .iter()
                    .filter_map(|e| parse_existing_auto_proxy_ip(&e.credentials))
                    .collect::<HashSet<Ipv6Addr>>()
            };
            assign_unique_auto_proxy(&mut new_cred, &mut used_auto_proxy_ips, &local_public_ipv6)?;
        }

        // 1. 基本验证
        validate_refresh_token(&new_cred)?;

        // 2. 基于 refreshToken 的 SHA-256 哈希检测重复
        let new_refresh_token = new_cred
            .refresh_token
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("缺少 refreshToken"))?;
        let new_refresh_token_hash = sha256_hex(new_refresh_token);
        let duplicate_exists = {
            let entries = self.entries.lock();
            entries.iter().any(|entry| {
                entry
                    .credentials
                    .refresh_token
                    .as_deref()
                    .map(sha256_hex)
                    .as_deref()
                    == Some(new_refresh_token_hash.as_str())
            })
        };
        if duplicate_exists {
            anyhow::bail!("凭据已存在（refreshToken 重复）");
        }

        // 3. 尝试刷新 Token 验证凭据有效性
        let effective_proxy = new_cred.effective_proxy(self.proxy.as_ref());
        let mut validated_cred =
            refresh_token(&new_cred, &self.config, effective_proxy.as_ref()).await?;

        // 4. 分配新 ID
        let new_id = {
            let entries = self.entries.lock();
            entries.iter().map(|e| e.id).max().unwrap_or(0) + 1
        };

        // 5. 设置 ID 并保留用户输入的元数据
        validated_cred.id = Some(new_id);
        validated_cred.priority = new_cred.priority;
        validated_cred.auth_method = new_cred.auth_method.map(|m| {
            if m.eq_ignore_ascii_case("builder-id") || m.eq_ignore_ascii_case("iam") {
                "idc".to_string()
            } else {
                m
            }
        });
        validated_cred.client_id = new_cred.client_id;
        validated_cred.client_secret = new_cred.client_secret;
        validated_cred.region = new_cred.region;
        validated_cred.auth_region = new_cred.auth_region;
        validated_cred.api_region = new_cred.api_region;
        validated_cred.machine_id = new_cred.machine_id;
        validated_cred.email = new_cred.email;
        validated_cred.proxy_url = new_cred.proxy_url;
        validated_cred.proxy_username = new_cred.proxy_username;
        validated_cred.proxy_password = new_cred.proxy_password;

        {
            let mut entries = self.entries.lock();
            entries.push(CredentialEntry {
                id: new_id,
                credentials: validated_cred,
                failure_count: 0,
                disabled: false,
                disabled_reason: None,
                success_count: 0,
                last_used_at: None,
                last_request_credits: 0.0,
                total_credits: 0.0,
            });
        }

        // 6. 持久化
        self.persist_credentials()?;

        tracing::info!("成功添加凭据 #{}", new_id);
        Ok(new_id)
    }

    /// 删除凭据（Admin API）
    ///
    /// # 前置条件
    /// - 凭据必须已禁用（disabled = true）
    ///
    /// # 行为
    /// 1. 验证凭据存在
    /// 2. 验证凭据已禁用
    /// 3. 从 entries 移除
    /// 4. 如果删除的是当前凭据，切换到优先级最高的可用凭据
    /// 5. 如果删除后没有凭据，将 current_id 重置为 0
    /// 6. 持久化到文件
    ///
    /// # 返回
    /// - `Ok(())` - 删除成功
    /// - `Err(_)` - 凭据不存在、未禁用或持久化失败
    pub fn delete_credential(&self, id: u64) -> anyhow::Result<()> {
        let was_current = {
            let mut entries = self.entries.lock();

            // 查找凭据
            let entry = entries
                .iter()
                .find(|e| e.id == id)
                .ok_or_else(|| anyhow::anyhow!("凭据不存在: {}", id))?;

            // 检查是否已禁用
            if !entry.disabled {
                anyhow::bail!("只能删除已禁用的凭据（请先禁用凭据 #{}）", id);
            }

            // 记录是否是当前凭据
            let current_id = *self.current_id.lock();
            let was_current = current_id == id;

            // 删除凭据
            entries.retain(|e| e.id != id);

            was_current
        };

        // 如果删除的是当前凭据，切换到优先级最高的可用凭据
        if was_current {
            self.select_highest_priority();
        }

        // 如果删除后没有任何凭据，将 current_id 重置为 0（与初始化行为保持一致）
        {
            let entries = self.entries.lock();
            if entries.is_empty() {
                let mut current_id = self.current_id.lock();
                *current_id = 0;
                tracing::info!("所有凭据已删除，current_id 已重置为 0");
            }
        }

        // 持久化更改
        self.persist_credentials()?;
        self.clear_route_affinity_for_credential(id);

        // 立即回写统计数据，清除已删除凭据的残留条目
        self.save_stats();

        tracing::info!("已删除凭据 #{}", id);
        Ok(())
    }

    /// 获取负载均衡模式（Admin API）
    pub fn get_load_balancing_mode(&self) -> String {
        self.load_balancing_mode.lock().clone()
    }

    /// 获取是否启用 billing header 预清洗（Admin API）
    pub fn get_strip_billing_header(&self) -> bool {
        *self.strip_billing_header.lock()
    }

    fn persist_load_balancing_mode(&self, mode: &str) -> anyhow::Result<()> {
        use anyhow::Context;

        let config_path = match self.config.config_path() {
            Some(path) => path.to_path_buf(),
            None => {
                tracing::warn!("配置文件路径未知，负载均衡模式仅在当前进程生效: {}", mode);
                return Ok(());
            }
        };

        let mut config = Config::load(&config_path)
            .with_context(|| format!("重新加载配置失败: {}", config_path.display()))?;
        config.load_balancing_mode = mode.to_string();
        config
            .save()
            .with_context(|| format!("持久化负载均衡模式失败: {}", config_path.display()))?;

        Ok(())
    }

    fn persist_strip_billing_header(&self, enabled: bool) -> anyhow::Result<()> {
        use anyhow::Context;

        let config_path = match self.config.config_path() {
            Some(path) => path.to_path_buf(),
            None => {
                tracing::warn!(
                    "配置文件路径未知，billing header 预清洗设置仅在当前进程生效: {}",
                    enabled
                );
                return Ok(());
            }
        };

        let mut config = Config::load(&config_path)
            .with_context(|| format!("重新加载配置失败: {}", config_path.display()))?;
        config.strip_billing_header = enabled;
        config.save().with_context(|| {
            format!(
                "持久化 billing header 预清洗设置失败: {}",
                config_path.display()
            )
        })?;

        Ok(())
    }

    /// 设置负载均衡模式（Admin API）
    pub fn set_load_balancing_mode(&self, mode: String) -> anyhow::Result<()> {
        // 验证模式值
        if mode != "priority" && mode != "balanced" && mode != "weighted_round_robin" {
            anyhow::bail!("无效的负载均衡模式: {}", mode);
        }

        let previous_mode = self.get_load_balancing_mode();
        if previous_mode == mode {
            return Ok(());
        }

        *self.load_balancing_mode.lock() = mode.clone();

        if let Err(err) = self.persist_load_balancing_mode(&mode) {
            *self.load_balancing_mode.lock() = previous_mode;
            return Err(err);
        }

        tracing::info!("负载均衡模式已设置为: {}", mode);
        Ok(())
    }

    /// 设置是否启用 billing header 预清洗（Admin API）
    pub fn set_strip_billing_header(&self, enabled: bool) -> anyhow::Result<()> {
        let previous = self.get_strip_billing_header();
        if previous == enabled {
            return Ok(());
        }

        *self.strip_billing_header.lock() = enabled;

        if let Err(err) = self.persist_strip_billing_header(enabled) {
            *self.strip_billing_header.lock() = previous;
            return Err(err);
        }

        tracing::info!("billing header 预清洗已设置为: {}", enabled);
        Ok(())
    }
}

impl Drop for MultiTokenManager {
    fn drop(&mut self) {
        if self.stats_dirty.load(Ordering::Relaxed) {
            self.save_stats();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_manager_new() {
        let config = Config::default();
        let credentials = KiroCredentials::default();
        let tm = TokenManager::new(config, credentials, None);
        assert!(tm.credentials().access_token.is_none());
    }

    #[test]
    fn test_is_token_expired_with_expired_token() {
        let mut credentials = KiroCredentials::default();
        credentials.expires_at = Some("2020-01-01T00:00:00Z".to_string());
        assert!(is_token_expired(&credentials));
    }

    #[test]
    fn test_is_token_expired_with_valid_token() {
        let mut credentials = KiroCredentials::default();
        let future = Utc::now() + Duration::hours(1);
        credentials.expires_at = Some(future.to_rfc3339());
        assert!(!is_token_expired(&credentials));
    }

    #[test]
    fn test_is_token_expired_within_5_minutes() {
        let mut credentials = KiroCredentials::default();
        let expires = Utc::now() + Duration::minutes(3);
        credentials.expires_at = Some(expires.to_rfc3339());
        assert!(is_token_expired(&credentials));
    }

    #[test]
    fn test_is_token_expired_no_expires_at() {
        let credentials = KiroCredentials::default();
        assert!(is_token_expired(&credentials));
    }

    #[test]
    fn test_is_token_expiring_soon_within_10_minutes() {
        let mut credentials = KiroCredentials::default();
        let expires = Utc::now() + Duration::minutes(8);
        credentials.expires_at = Some(expires.to_rfc3339());
        assert!(is_token_expiring_soon(&credentials));
    }

    #[test]
    fn test_is_token_expiring_soon_beyond_10_minutes() {
        let mut credentials = KiroCredentials::default();
        let expires = Utc::now() + Duration::minutes(15);
        credentials.expires_at = Some(expires.to_rfc3339());
        assert!(!is_token_expiring_soon(&credentials));
    }

    #[test]
    fn test_validate_refresh_token_missing() {
        let credentials = KiroCredentials::default();
        let result = validate_refresh_token(&credentials);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_refresh_token_valid() {
        let mut credentials = KiroCredentials::default();
        credentials.refresh_token = Some("a".repeat(150));
        let result = validate_refresh_token(&credentials);
        assert!(result.is_ok());
    }

    #[test]
    fn test_sha256_hex() {
        let result = sha256_hex("test");
        assert_eq!(
            result,
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
        );
    }

    #[test]
    fn test_parse_socks5h_ipv6_supports_plain_and_inline_auth() {
        let plain = "socks5h://[2001:470:1f06:396::91]:1080";
        let inline = format!(
            "socks5h://{}:{}@[2001:470:1f06:396::92]:1080",
            AUTO_PROXY_USERNAME, AUTO_PROXY_PASSWORD
        );

        assert_eq!(
            parse_socks5h_ipv6(plain),
            Some("2001:470:1f06:396::91".parse().unwrap())
        );
        assert_eq!(
            parse_socks5h_ipv6(&inline),
            Some("2001:470:1f06:396::92".parse().unwrap())
        );
    }

    #[test]
    fn test_assign_unique_auto_proxy_generates_different_proxy_per_credential() {
        let mut used_ips = HashSet::new();
        let mut cred1 = KiroCredentials::default();
        let mut cred2 = KiroCredentials::default();

        assert!(assign_unique_auto_proxy(&mut cred1, &mut used_ips, &[]).unwrap());
        assert!(assign_unique_auto_proxy(&mut cred2, &mut used_ips, &[]).unwrap());

        assert_ne!(cred1.proxy_url, cred2.proxy_url);
        assert_eq!(cred1.proxy_username.as_deref(), Some(AUTO_PROXY_USERNAME));
        assert_eq!(cred2.proxy_username.as_deref(), Some(AUTO_PROXY_USERNAME));
        assert_eq!(cred1.proxy_password.as_deref(), Some(AUTO_PROXY_PASSWORD));
        assert_eq!(cred2.proxy_password.as_deref(), Some(AUTO_PROXY_PASSWORD));
    }

    #[test]
    fn test_assign_unique_auto_proxy_keeps_existing_unique_auto_proxy() {
        let ip = Ipv6Addr::new(
            AUTO_PROXY_PREFIX[0],
            AUTO_PROXY_PREFIX[1],
            AUTO_PROXY_PREFIX[2],
            AUTO_PROXY_PREFIX[3],
            0,
            0,
            0,
            0x91,
        );
        let mut cred = KiroCredentials {
            proxy_url: Some(build_auto_proxy_url(ip)),
            proxy_username: Some(AUTO_PROXY_USERNAME.to_string()),
            proxy_password: Some(AUTO_PROXY_PASSWORD.to_string()),
            ..KiroCredentials::default()
        };
        let mut used_ips = HashSet::new();

        let changed = assign_unique_auto_proxy(&mut cred, &mut used_ips, &[]).unwrap();

        assert!(!changed);
        assert!(used_ips.contains(&ip));
        assert_eq!(cred.proxy_url, Some(build_auto_proxy_url(ip)));
    }

    #[tokio::test]
    async fn test_add_credential_reject_duplicate_refresh_token() {
        let config = Config::default();

        let mut existing = KiroCredentials::default();
        existing.refresh_token = Some("a".repeat(150));

        let manager = MultiTokenManager::new(config, vec![existing], None, None, false).unwrap();

        let mut duplicate = KiroCredentials::default();
        duplicate.refresh_token = Some("a".repeat(150));

        let result = manager.add_credential(duplicate).await;
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("凭据已存在"));
    }

    // MultiTokenManager 测试

    #[test]
    fn test_multi_token_manager_new() {
        let config = Config::default();
        let mut cred1 = KiroCredentials::default();
        cred1.priority = 0;
        let mut cred2 = KiroCredentials::default();
        cred2.priority = 1;

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred2], None, None, false).unwrap();
        assert_eq!(manager.total_count(), 2);
        assert_eq!(manager.available_count(), 2);
    }

    #[test]
    fn test_multi_token_manager_new_without_proxy_keeps_proxy_empty() {
        let config = Config::default();
        let cred1 = KiroCredentials::default();
        let cred2 = KiroCredentials::default();

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred2], None, None, false).unwrap();
        let snapshot = manager.snapshot();

        for entry in snapshot.entries {
            assert!(!entry.has_proxy);
            assert!(entry.proxy_url.is_none());
        }
    }

    #[test]
    fn test_multi_token_manager_empty_credentials() {
        let config = Config::default();
        let result = MultiTokenManager::new(config, vec![], None, None, false);
        // 支持 0 个凭据启动（可通过管理面板添加）
        assert!(result.is_ok());
        let manager = result.unwrap();
        assert_eq!(manager.total_count(), 0);
        assert_eq!(manager.available_count(), 0);
    }

    #[test]
    fn test_multi_token_manager_duplicate_ids() {
        let config = Config::default();
        let mut cred1 = KiroCredentials::default();
        cred1.id = Some(1);
        let mut cred2 = KiroCredentials::default();
        cred2.id = Some(1); // 重复 ID

        let result = MultiTokenManager::new(config, vec![cred1, cred2], None, None, false);
        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(
            err_msg.contains("重复的凭据 ID"),
            "错误消息应包含 '重复的凭据 ID'，实际: {}",
            err_msg
        );
    }

    #[test]
    fn test_multi_token_manager_report_failure() {
        let config = Config::default();
        let cred1 = KiroCredentials::default();
        let cred2 = KiroCredentials::default();

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred2], None, None, false).unwrap();

        // 凭据会自动分配 ID（从 1 开始）
        // 前两次失败不会禁用（使用 ID 1）
        assert!(manager.report_failure(1));
        assert!(manager.report_failure(1));
        assert_eq!(manager.available_count(), 2);

        // 第三次失败会禁用第一个凭据
        assert!(manager.report_failure(1));
        assert_eq!(manager.available_count(), 1);

        // 继续失败第二个凭据（使用 ID 2）
        assert!(manager.report_failure(2));
        assert!(manager.report_failure(2));
        assert!(!manager.report_failure(2)); // 所有凭据都禁用了
        assert_eq!(manager.available_count(), 0);
    }

    #[test]
    fn test_multi_token_manager_report_success() {
        let config = Config::default();
        let cred = KiroCredentials::default();

        let manager = MultiTokenManager::new(config, vec![cred], None, None, false).unwrap();

        // 失败两次（使用 ID 1）
        manager.report_failure(1);
        manager.report_failure(1);

        // 成功后重置计数（使用 ID 1）
        manager.report_success(1);

        // 再失败两次不会禁用
        manager.report_failure(1);
        manager.report_failure(1);
        assert_eq!(manager.available_count(), 1);
    }

    #[test]
    fn test_multi_token_manager_report_credits_accumulates() {
        let config = Config::default();
        let cred = KiroCredentials::default();

        let manager = MultiTokenManager::new(config, vec![cred], None, None, false).unwrap();
        manager.report_credits(1, 0.25);
        manager.report_credits(1, 1.5);

        let snapshot = manager.snapshot();
        let entry = snapshot.entries.iter().find(|e| e.id == 1).unwrap();
        assert!((entry.last_request_credits - 1.5).abs() < 1e-9);
        assert!((entry.total_credits - 1.75).abs() < 1e-9);
    }

    #[test]
    fn test_multi_token_manager_switch_to_next() {
        let config = Config::default();
        let mut cred1 = KiroCredentials::default();
        cred1.refresh_token = Some("token1".to_string());
        let mut cred2 = KiroCredentials::default();
        cred2.refresh_token = Some("token2".to_string());

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred2], None, None, false).unwrap();

        // 初始是第一个凭据
        assert_eq!(
            manager.credentials().refresh_token,
            Some("token1".to_string())
        );

        // 切换到下一个
        assert!(manager.switch_to_next());
        assert_eq!(
            manager.credentials().refresh_token,
            Some("token2".to_string())
        );
    }

    #[tokio::test]
    async fn test_opus_request_prefers_configured_credential() {
        let mut config = Config::default();
        config.opus_preferred_credential_id = Some(7);

        let mut cred1 = KiroCredentials::default();
        cred1.id = Some(1);
        cred1.priority = 0;
        cred1.access_token = Some("token-1".to_string());
        cred1.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());
        cred1.subscription_title = Some("KIRO PRO+".to_string());

        let mut cred7 = KiroCredentials::default();
        cred7.id = Some(7);
        cred7.priority = 10;
        cred7.access_token = Some("token-7".to_string());
        cred7.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());
        cred7.subscription_title = Some("KIRO PRO+".to_string());

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred7], None, None, false).unwrap();

        // 即使当前凭据是 #1，Opus 也应被定向到 #7
        let ctx = manager
            .acquire_context(Some("claude-opus-4-1"), None)
            .await
            .unwrap();
        assert_eq!(ctx.id, 7);
        assert_eq!(ctx.token, "token-7");
    }

    #[tokio::test]
    async fn test_non_opus_request_keeps_priority_selection() {
        let config = Config::default();

        let mut cred1 = KiroCredentials::default();
        cred1.id = Some(1);
        cred1.priority = 0;
        cred1.access_token = Some("token-1".to_string());
        cred1.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());

        let mut cred7 = KiroCredentials::default();
        cred7.id = Some(7);
        cred7.priority = 10;
        cred7.access_token = Some("token-7".to_string());
        cred7.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred7], None, None, false).unwrap();

        // 非 Opus 仍按 priority/current_id 规则走 #1
        let ctx = manager
            .acquire_context(Some("claude-sonnet-4-5"), None)
            .await
            .unwrap();
        assert_eq!(ctx.id, 1);
        assert_eq!(ctx.token, "token-1");
    }

    #[tokio::test]
    async fn test_weighted_round_robin_prefers_higher_priority() {
        let config = Config::default();

        let mut high_weight_cred = KiroCredentials::default();
        high_weight_cred.id = Some(1);
        high_weight_cred.priority = 0;
        high_weight_cred.access_token = Some("token-1".to_string());
        high_weight_cred.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());

        let mut low_weight_cred = KiroCredentials::default();
        low_weight_cred.id = Some(2);
        low_weight_cred.priority = 4;
        low_weight_cred.access_token = Some("token-2".to_string());
        low_weight_cred.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());

        let manager = MultiTokenManager::new(
            config,
            vec![high_weight_cred, low_weight_cred],
            None,
            None,
            false,
        )
        .unwrap();
        manager
            .set_load_balancing_mode("weighted_round_robin".to_string())
            .unwrap();

        let mut picked_1 = 0;
        let mut picked_2 = 0;
        for _ in 0..6 {
            let ctx = manager
                .acquire_context(Some("claude-sonnet-4-5"), None)
                .await
                .unwrap();
            if ctx.id == 1 {
                picked_1 += 1;
            } else if ctx.id == 2 {
                picked_2 += 1;
            }
        }

        // priority=0 与 priority=4 会映射为 5:1 权重
        assert_eq!(picked_1, 5);
        assert_eq!(picked_2, 1);
    }

    #[tokio::test]
    async fn test_weighted_round_robin_fallbacks_when_selected_credential_unavailable() {
        let config = Config::default();

        let mut unavailable_cred = KiroCredentials::default();
        unavailable_cred.id = Some(1);
        unavailable_cred.priority = 0;
        unavailable_cred.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());

        let mut healthy_cred = KiroCredentials::default();
        healthy_cred.id = Some(2);
        healthy_cred.priority = 10;
        healthy_cred.access_token = Some("token-2".to_string());
        healthy_cred.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());

        let manager = MultiTokenManager::new(
            config,
            vec![unavailable_cred, healthy_cred],
            None,
            None,
            false,
        )
        .unwrap();
        manager
            .set_load_balancing_mode("weighted_round_robin".to_string())
            .unwrap();

        let ctx = manager
            .acquire_context(Some("claude-sonnet-4-5"), None)
            .await
            .unwrap();
        assert_eq!(ctx.id, 2);
    }

    #[tokio::test]
    async fn test_route_affinity_with_weighted_round_robin_keeps_same_credential() {
        let config = Config::default();

        let mut cred1 = KiroCredentials::default();
        cred1.id = Some(1);
        cred1.priority = 0;
        cred1.access_token = Some("token-1".to_string());
        cred1.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());

        let mut cred2 = KiroCredentials::default();
        cred2.id = Some(2);
        cred2.priority = 0;
        cred2.access_token = Some("token-2".to_string());
        cred2.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred2], None, None, false).unwrap();
        manager
            .set_load_balancing_mode("weighted_round_robin".to_string())
            .unwrap();

        let first = manager
            .acquire_context(Some("claude-sonnet-4-5"), Some("conversation:wrr-stick"))
            .await
            .unwrap();
        let second = manager
            .acquire_context(Some("claude-sonnet-4-5"), Some("conversation:wrr-stick"))
            .await
            .unwrap();
        let third = manager
            .acquire_context(Some("claude-sonnet-4-5"), None)
            .await
            .unwrap();

        assert_eq!(first.id, second.id);
        assert_ne!(first.id, third.id);
    }

    #[tokio::test]
    async fn test_route_affinity_prefers_previous_credential_within_ttl() {
        let config = Config::default();

        let mut cred1 = KiroCredentials::default();
        cred1.id = Some(1);
        cred1.priority = 0;
        cred1.access_token = Some("token-1".to_string());
        cred1.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());

        let mut cred2 = KiroCredentials::default();
        cred2.id = Some(2);
        cred2.priority = 10;
        cred2.access_token = Some("token-2".to_string());
        cred2.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred2], None, None, false).unwrap();

        // 首次请求按默认策略命中 #1，并建立 route_key 亲和
        let first = manager
            .acquire_context(Some("claude-sonnet-4-5"), Some("conversation:test-1"))
            .await
            .unwrap();
        assert_eq!(first.id, 1);

        // 人工切换 current_id 到 #2，验证 route_key 仍优先回到 #1
        assert!(manager.switch_to_next());
        let second = manager
            .acquire_context(Some("claude-sonnet-4-5"), Some("conversation:test-1"))
            .await
            .unwrap();
        assert_eq!(second.id, 1);
    }

    #[tokio::test]
    async fn test_route_affinity_falls_back_when_preferred_credential_disabled() {
        let config = Config::default();

        let mut cred1 = KiroCredentials::default();
        cred1.id = Some(1);
        cred1.priority = 0;
        cred1.access_token = Some("token-1".to_string());
        cred1.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());

        let mut cred2 = KiroCredentials::default();
        cred2.id = Some(2);
        cred2.priority = 10;
        cred2.access_token = Some("token-2".to_string());
        cred2.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred2], None, None, false).unwrap();

        let first = manager
            .acquire_context(Some("claude-sonnet-4-5"), Some("conversation:test-2"))
            .await
            .unwrap();
        assert_eq!(first.id, 1);

        manager.set_disabled(1, true).unwrap();

        let second = manager
            .acquire_context(Some("claude-sonnet-4-5"), Some("conversation:test-2"))
            .await
            .unwrap();
        assert_eq!(second.id, 2);
    }

    #[tokio::test]
    async fn test_route_affinity_rebinds_when_original_credential_token_unavailable() {
        let config = Config::default();

        let mut bad_cred = KiroCredentials::default();
        bad_cred.id = Some(1);
        bad_cred.priority = 0;
        // access_token 缺失会导致 try_ensure_token 失败，模拟凭据临时不可用
        bad_cred.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());

        let mut good_cred = KiroCredentials::default();
        good_cred.id = Some(2);
        good_cred.priority = 10;
        good_cred.access_token = Some("token-2".to_string());
        good_cred.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());

        let manager =
            MultiTokenManager::new(config, vec![bad_cred, good_cred], None, None, false).unwrap();

        let ctx = manager
            .acquire_context(Some("claude-sonnet-4-5"), Some("conversation:test-3"))
            .await
            .unwrap();
        assert_eq!(ctx.id, 2);

        // 第二次相同 route_key 应继续稳定命中 #2
        let ctx2 = manager
            .acquire_context(Some("claude-sonnet-4-5"), Some("conversation:test-3"))
            .await
            .unwrap();
        assert_eq!(ctx2.id, 2);
    }

    #[test]
    fn test_set_load_balancing_mode_persists_to_config_file() {
        let config_path =
            std::env::temp_dir().join(format!("kiro-load-balancing-{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&config_path, r#"{"loadBalancingMode":"priority"}"#).unwrap();

        let config = Config::load(&config_path).unwrap();
        let manager =
            MultiTokenManager::new(config, vec![KiroCredentials::default()], None, None, false)
                .unwrap();

        manager
            .set_load_balancing_mode("balanced".to_string())
            .unwrap();
        manager
            .set_load_balancing_mode("weighted_round_robin".to_string())
            .unwrap();

        let persisted = Config::load(&config_path).unwrap();
        assert_eq!(persisted.load_balancing_mode, "weighted_round_robin");
        assert_eq!(manager.get_load_balancing_mode(), "weighted_round_robin");

        std::fs::remove_file(&config_path).unwrap();
    }

    #[test]
    fn test_set_strip_billing_header_persists_to_config_file() {
        let config_path = std::env::temp_dir().join(format!(
            "kiro-strip-billing-header-{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&config_path, r#"{"stripBillingHeader":true}"#).unwrap();

        let config = Config::load(&config_path).unwrap();
        let manager =
            MultiTokenManager::new(config, vec![KiroCredentials::default()], None, None, false)
                .unwrap();

        manager.set_strip_billing_header(false).unwrap();

        let persisted = Config::load(&config_path).unwrap();
        assert!(!persisted.strip_billing_header);
        assert!(!manager.get_strip_billing_header());

        std::fs::remove_file(&config_path).unwrap();
    }

    #[tokio::test]
    async fn test_multi_token_manager_acquire_context_auto_recovers_all_disabled() {
        let config = Config::default();
        let mut cred1 = KiroCredentials::default();
        cred1.access_token = Some("t1".to_string());
        cred1.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());
        let mut cred2 = KiroCredentials::default();
        cred2.access_token = Some("t2".to_string());
        cred2.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred2], None, None, false).unwrap();

        // 凭据会自动分配 ID（从 1 开始）
        for _ in 0..MAX_FAILURES_PER_CREDENTIAL {
            manager.report_failure(1);
        }
        for _ in 0..MAX_FAILURES_PER_CREDENTIAL {
            manager.report_failure(2);
        }

        assert_eq!(manager.available_count(), 0);

        // 应触发自愈：重置失败计数并重新启用，避免必须重启进程
        let ctx = manager.acquire_context(None, None).await.unwrap();
        assert!(ctx.token == "t1" || ctx.token == "t2");
        assert_eq!(manager.available_count(), 2);
    }

    #[test]
    fn test_multi_token_manager_report_quota_exhausted() {
        let config = Config::default();
        let cred1 = KiroCredentials::default();
        let cred2 = KiroCredentials::default();

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred2], None, None, false).unwrap();

        // 凭据会自动分配 ID（从 1 开始）
        assert_eq!(manager.available_count(), 2);
        assert!(manager.report_quota_exhausted(1));
        assert_eq!(manager.available_count(), 1);

        // 再禁用第二个后，无可用凭据
        assert!(!manager.report_quota_exhausted(2));
        assert_eq!(manager.available_count(), 0);
    }

    #[tokio::test]
    async fn test_multi_token_manager_quota_disabled_is_not_auto_recovered() {
        let config = Config::default();
        let cred1 = KiroCredentials::default();
        let cred2 = KiroCredentials::default();

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred2], None, None, false).unwrap();

        manager.report_quota_exhausted(1);
        manager.report_quota_exhausted(2);
        assert_eq!(manager.available_count(), 0);

        let err = manager
            .acquire_context(None, None)
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(
            err.contains("所有凭据均已禁用"),
            "错误应提示所有凭据禁用，实际: {}",
            err
        );
        assert_eq!(manager.available_count(), 0);
    }

    // ============ 凭据级 Region 优先级测试 ============

    #[test]
    fn test_credential_region_priority_uses_credential_auth_region() {
        // 凭据配置了 auth_region 时，应使用凭据的 auth_region
        let mut config = Config::default();
        config.region = "us-west-2".to_string();

        let mut credentials = KiroCredentials::default();
        credentials.auth_region = Some("eu-west-1".to_string());

        let region = credentials.effective_auth_region(&config);
        assert_eq!(region, "eu-west-1");
    }

    #[test]
    fn test_credential_region_priority_fallback_to_credential_region() {
        // 凭据未配置 auth_region 但配置了 region 时，应回退到凭据.region
        let mut config = Config::default();
        config.region = "us-west-2".to_string();

        let mut credentials = KiroCredentials::default();
        credentials.region = Some("eu-central-1".to_string());

        let region = credentials.effective_auth_region(&config);
        assert_eq!(region, "eu-central-1");
    }

    #[test]
    fn test_credential_region_priority_fallback_to_config() {
        // 凭据未配置 auth_region 和 region 时，应回退到 config
        let mut config = Config::default();
        config.region = "us-west-2".to_string();

        let credentials = KiroCredentials::default();
        assert!(credentials.auth_region.is_none());
        assert!(credentials.region.is_none());

        let region = credentials.effective_auth_region(&config);
        assert_eq!(region, "us-west-2");
    }

    #[test]
    fn test_multiple_credentials_use_respective_regions() {
        // 多凭据场景下，不同凭据使用各自的 auth_region
        let mut config = Config::default();
        config.region = "ap-northeast-1".to_string();

        let mut cred1 = KiroCredentials::default();
        cred1.auth_region = Some("us-east-1".to_string());

        let mut cred2 = KiroCredentials::default();
        cred2.region = Some("eu-west-1".to_string());

        let cred3 = KiroCredentials::default(); // 无 region，使用 config

        assert_eq!(cred1.effective_auth_region(&config), "us-east-1");
        assert_eq!(cred2.effective_auth_region(&config), "eu-west-1");
        assert_eq!(cred3.effective_auth_region(&config), "ap-northeast-1");
    }

    #[test]
    fn test_idc_oidc_endpoint_uses_credential_auth_region() {
        // 验证 IdC OIDC endpoint URL 使用凭据 auth_region
        let mut config = Config::default();
        config.region = "us-west-2".to_string();

        let mut credentials = KiroCredentials::default();
        credentials.auth_region = Some("eu-central-1".to_string());

        let region = credentials.effective_auth_region(&config);
        let refresh_url = format!("https://oidc.{}.amazonaws.com/token", region);

        assert_eq!(refresh_url, "https://oidc.eu-central-1.amazonaws.com/token");
    }

    #[test]
    fn test_social_refresh_endpoint_uses_credential_auth_region() {
        // 验证 Social refresh endpoint URL 使用凭据 auth_region
        let mut config = Config::default();
        config.region = "us-west-2".to_string();

        let mut credentials = KiroCredentials::default();
        credentials.auth_region = Some("ap-southeast-1".to_string());

        let region = credentials.effective_auth_region(&config);
        let refresh_url = format!("https://prod.{}.auth.desktop.kiro.dev/refreshToken", region);

        assert_eq!(
            refresh_url,
            "https://prod.ap-southeast-1.auth.desktop.kiro.dev/refreshToken"
        );
    }

    #[test]
    fn test_api_call_uses_effective_api_region() {
        // 验证 API 调用使用 effective_api_region
        let mut config = Config::default();
        config.region = "us-west-2".to_string();

        let mut credentials = KiroCredentials::default();
        credentials.region = Some("eu-west-1".to_string());

        // 凭据.region 不参与 api_region 回退链
        let api_region = credentials.effective_api_region(&config);
        let api_host = format!("q.{}.amazonaws.com", api_region);

        assert_eq!(api_host, "q.us-west-2.amazonaws.com");
    }

    #[test]
    fn test_api_call_uses_credential_api_region() {
        // 凭据配置了 api_region 时，API 调用应使用凭据的 api_region
        let mut config = Config::default();
        config.region = "us-west-2".to_string();

        let mut credentials = KiroCredentials::default();
        credentials.api_region = Some("eu-central-1".to_string());

        let api_region = credentials.effective_api_region(&config);
        let api_host = format!("q.{}.amazonaws.com", api_region);

        assert_eq!(api_host, "q.eu-central-1.amazonaws.com");
    }

    #[test]
    fn test_credential_region_empty_string_treated_as_set() {
        // 空字符串 auth_region 被视为已设置（虽然不推荐，但行为应一致）
        let mut config = Config::default();
        config.region = "us-west-2".to_string();

        let mut credentials = KiroCredentials::default();
        credentials.auth_region = Some("".to_string());

        let region = credentials.effective_auth_region(&config);
        // 空字符串被视为已设置，不会回退到 config
        assert_eq!(region, "");
    }

    #[test]
    fn test_auth_and_api_region_independent() {
        // auth_region 和 api_region 互不影响
        let mut config = Config::default();
        config.region = "default".to_string();

        let mut credentials = KiroCredentials::default();
        credentials.auth_region = Some("auth-only".to_string());
        credentials.api_region = Some("api-only".to_string());

        assert_eq!(credentials.effective_auth_region(&config), "auth-only");
        assert_eq!(credentials.effective_api_region(&config), "api-only");
    }
}
