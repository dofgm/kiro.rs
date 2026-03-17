//! Admin API 中间件

use std::sync::Arc;

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use parking_lot::Mutex;

use super::service::AdminService;
use super::types::AdminErrorResponse;
use crate::common::auth;
use crate::model::config::Config;

/// Admin API 共享状态
#[derive(Clone)]
pub struct AdminState {
    /// Admin API 密钥
    pub admin_api_key: String,
    /// Admin 服务
    pub service: Arc<AdminService>,
    /// 共享的应用配置（用于读写 api_key 并持久化）
    pub config: Arc<Mutex<Config>>,
    /// 共享的应用 API Key（运行时认证中间件使用）
    pub app_api_key: Arc<Mutex<String>>,
}

impl AdminState {
    pub fn new(
        admin_api_key: impl Into<String>,
        service: AdminService,
        config: Arc<Mutex<Config>>,
        app_api_key: Arc<Mutex<String>>,
    ) -> Self {
        Self {
            admin_api_key: admin_api_key.into(),
            service: Arc::new(service),
            config,
            app_api_key,
        }
    }
}

/// Admin API 认证中间件
pub async fn admin_auth_middleware(
    State(state): State<AdminState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let api_key = auth::extract_api_key(&request);

    match api_key {
        Some(key) if auth::constant_time_eq(&key, &state.admin_api_key) => next.run(request).await,
        _ => {
            let error = AdminErrorResponse::authentication_error();
            (StatusCode::UNAUTHORIZED, Json(error)).into_response()
        }
    }
}
