use axum::{
    middleware,
    routing::get,
    Router,
};
use std::sync::Arc;
use crate::core::AppState;
use crate::handler::{index_handler, proxy_handler};
use crate::engine::custom_logger_engine;

/// 构建 Axum 路由
pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(index_handler)) // 首页
        .fallback(proxy_handler)       // 所有非首页请求都进入代理逻辑
        .layer(middleware::from_fn_with_state(state.clone(), custom_logger_engine)) // 日志中间件
        .with_state(state)             // 注入全局状态
}
