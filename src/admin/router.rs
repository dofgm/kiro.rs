//! Admin API 路由配置

use axum::{
    Router, middleware,
    routing::{delete, get, post},
};

use super::{
    handlers::{
        add_credential, clear_request_details, delete_credential, get_all_credentials,
        get_api_key, get_credential_balance, get_load_balancing_mode, get_models,
        get_request_details, get_system_settings, reset_failure_count, set_api_key,
        set_credential_disabled, set_credential_priority, set_load_balancing_mode,
        set_system_settings,
    },
    middleware::{AdminState, admin_auth_middleware},
};

/// 创建 Admin API 路由
///
/// # 端点
/// - `GET /credentials` - 获取所有凭据状态
/// - `POST /credentials` - 添加新凭据
/// - `DELETE /credentials/:id` - 删除凭据
/// - `POST /credentials/:id/disabled` - 设置凭据禁用状态
/// - `POST /credentials/:id/priority` - 设置凭据优先级
/// - `POST /credentials/:id/reset` - 重置失败计数
/// - `GET /credentials/:id/balance` - 获取凭据余额
/// - `GET /details` - 获取请求明细
/// - `DELETE /details` - 清空请求明细
/// - `GET /config/load-balancing` - 获取负载均衡模式
/// - `PUT /config/load-balancing` - 设置负载均衡模式
/// - `GET /config/system-settings` - 获取系统设置
/// - `PUT /config/system-settings` - 设置系统设置
///
/// # 认证
/// 需要 Admin API Key 认证，支持：
/// - `x-api-key` header
/// - `Authorization: Bearer <token>` header
pub fn create_admin_router(state: AdminState) -> Router {
    Router::new()
        .route(
            "/credentials",
            get(get_all_credentials).post(add_credential),
        )
        .route("/credentials/{id}", delete(delete_credential))
        .route("/credentials/{id}/disabled", post(set_credential_disabled))
        .route("/credentials/{id}/priority", post(set_credential_priority))
        .route("/credentials/{id}/reset", post(reset_failure_count))
        .route("/credentials/{id}/balance", get(get_credential_balance))
        .route("/details", get(get_request_details).delete(clear_request_details))
        .route(
            "/config/load-balancing",
            get(get_load_balancing_mode).put(set_load_balancing_mode),
        )
        .route(
            "/config/system-settings",
            get(get_system_settings).put(set_system_settings),
        )
        .route("/config/models", get(get_models))
        .route(
            "/config/api-key",
            get(get_api_key).put(set_api_key),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            admin_auth_middleware,
        ))
        .with_state(state)
}
