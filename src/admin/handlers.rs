//! Admin API HTTP 处理器

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};

use super::{
    middleware::AdminState,
    types::{
        AddCredentialRequest, RequestDetailsQuery, SetDisabledRequest, SetLoadBalancingModeRequest,
        SetPriorityRequest, SetSystemSettingsRequest, SuccessResponse, SetApiKeyRequest,
        ApiKeyResponse, AdminErrorResponse,
    },
};

/// GET /api/admin/credentials
/// 获取所有凭据状态
pub async fn get_all_credentials(State(state): State<AdminState>) -> impl IntoResponse {
    let response = state.service.get_all_credentials();
    Json(response)
}

/// POST /api/admin/credentials/:id/disabled
/// 设置凭据禁用状态
pub async fn set_credential_disabled(
    State(state): State<AdminState>,
    Path(id): Path<u64>,
    Json(payload): Json<SetDisabledRequest>,
) -> impl IntoResponse {
    match state.service.set_disabled(id, payload.disabled) {
        Ok(_) => {
            let action = if payload.disabled { "禁用" } else { "启用" };
            Json(SuccessResponse::new(format!("凭据 #{} 已{}", id, action))).into_response()
        }
        Err(e) => (e.status_code(), Json(e.into_response())).into_response(),
    }
}

/// POST /api/admin/credentials/:id/priority
/// 设置凭据优先级
pub async fn set_credential_priority(
    State(state): State<AdminState>,
    Path(id): Path<u64>,
    Json(payload): Json<SetPriorityRequest>,
) -> impl IntoResponse {
    match state.service.set_priority(id, payload.priority) {
        Ok(_) => Json(SuccessResponse::new(format!(
            "凭据 #{} 优先级已设置为 {}",
            id, payload.priority
        )))
        .into_response(),
        Err(e) => (e.status_code(), Json(e.into_response())).into_response(),
    }
}

/// POST /api/admin/credentials/:id/reset
/// 重置失败计数并重新启用
pub async fn reset_failure_count(
    State(state): State<AdminState>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    match state.service.reset_and_enable(id) {
        Ok(_) => Json(SuccessResponse::new(format!(
            "凭据 #{} 失败计数已重置并重新启用",
            id
        )))
        .into_response(),
        Err(e) => (e.status_code(), Json(e.into_response())).into_response(),
    }
}

/// GET /api/admin/credentials/:id/balance
/// 获取指定凭据的余额
pub async fn get_credential_balance(
    State(state): State<AdminState>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    match state.service.get_balance(id).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => (e.status_code(), Json(e.into_response())).into_response(),
    }
}

/// GET /api/admin/details
/// 获取请求明细（模拟 KV 缓存统计）
pub async fn get_request_details(
    State(state): State<AdminState>,
    Query(query): Query<RequestDetailsQuery>,
) -> impl IntoResponse {
    match state.service.get_request_details(query.limit) {
        Ok(response) => Json(response).into_response(),
        Err(e) => (e.status_code(), Json(e.into_response())).into_response(),
    }
}

/// DELETE /api/admin/details
/// 清空请求明细
pub async fn clear_request_details(State(state): State<AdminState>) -> impl IntoResponse {
    match state.service.clear_request_details() {
        Ok(_) => Json(SuccessResponse::new("请求明细已清空".to_string())).into_response(),
        Err(e) => (e.status_code(), Json(e.into_response())).into_response(),
    }
}

/// POST /api/admin/credentials
/// 添加新凭据
pub async fn add_credential(
    State(state): State<AdminState>,
    Json(payload): Json<AddCredentialRequest>,
) -> impl IntoResponse {
    match state.service.add_credential(payload).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => (e.status_code(), Json(e.into_response())).into_response(),
    }
}

/// DELETE /api/admin/credentials/:id
/// 删除凭据
pub async fn delete_credential(
    State(state): State<AdminState>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    match state.service.delete_credential(id) {
        Ok(_) => Json(SuccessResponse::new(format!("凭据 #{} 已删除", id))).into_response(),
        Err(e) => (e.status_code(), Json(e.into_response())).into_response(),
    }
}

/// GET /api/admin/config/load-balancing
/// 获取负载均衡模式
pub async fn get_load_balancing_mode(State(state): State<AdminState>) -> impl IntoResponse {
    let response = state.service.get_load_balancing_mode();
    Json(response)
}

/// PUT /api/admin/config/load-balancing
/// 设置负载均衡模式
pub async fn set_load_balancing_mode(
    State(state): State<AdminState>,
    Json(payload): Json<SetLoadBalancingModeRequest>,
) -> impl IntoResponse {
    match state.service.set_load_balancing_mode(payload) {
        Ok(response) => Json(response).into_response(),
        Err(e) => (e.status_code(), Json(e.into_response())).into_response(),
    }
}

/// GET /api/admin/config/system-settings
/// 获取系统设置
pub async fn get_system_settings(State(state): State<AdminState>) -> impl IntoResponse {
    let response = state.service.get_system_settings();
    Json(response)
}

/// PUT /api/admin/config/system-settings
/// 设置系统设置
pub async fn set_system_settings(
    State(state): State<AdminState>,
    Json(payload): Json<SetSystemSettingsRequest>,
) -> impl IntoResponse {
    match state.service.set_system_settings(payload) {
        Ok(response) => Json(response).into_response(),
        Err(e) => (e.status_code(), Json(e.into_response())).into_response(),
    }
}

/// GET /api/admin/config/models
/// 获取可用模型列表
pub async fn get_models(State(state): State<AdminState>) -> impl IntoResponse {
    Json(state.service.get_models())
}

/// GET /api/admin/config/api-key
/// 获取当前 API 密钥
pub async fn get_api_key(State(state): State<AdminState>) -> impl IntoResponse {
    let key = state.app_api_key.lock().clone();
    Json(ApiKeyResponse { api_key: key })
}

/// PUT /api/admin/config/api-key
/// 设置 API 密钥
pub async fn set_api_key(
    State(state): State<AdminState>,
    Json(payload): Json<SetApiKeyRequest>,
) -> impl IntoResponse {
    let new_key = payload.api_key.trim().to_string();
    if new_key.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(AdminErrorResponse::invalid_request("API 密钥不能为空")),
        )
            .into_response();
    }

    // 更新运行时 API Key
    *state.app_api_key.lock() = new_key.clone();

    // 持久化到 config.json
    {
        let mut config = state.config.lock();
        config.api_key = Some(new_key);
        if let Err(e) = config.save() {
            tracing::error!("保存配置文件失败: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AdminErrorResponse::internal_error(format!(
                    "API 密钥已更新但保存配置失败: {}",
                    e
                ))),
            )
                .into_response();
        }
    }

    Json(SuccessResponse::new("API 密钥已更新")).into_response()
}
