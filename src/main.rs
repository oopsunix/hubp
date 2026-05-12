mod core;
mod module;
mod engine;
mod handler;
mod route;
mod tasks;

use dashmap::DashMap;
use std::{
    net::SocketAddr,
    sync::Arc,
};
use tokio::sync::RwLock;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::core::{load_config, AppState};
use crate::module::github::GithubProvider;
use crate::module::github_web::GithubWebProvider;
use crate::module::docker::DockerProvider;
use crate::module::huggingface::HuggingfaceProvider;
use crate::route::create_router;
use crate::tasks::{spawn_cleanup_task, spawn_config_reloader, build_http_client, ensure_geoip_db};

/// 程序主入口
#[tokio::main]
async fn main() {
    // 1. 加载初始配置
    let initial_config = load_config().await.unwrap_or_default();

    // 2. 初始化日志系统
    let filter = if initial_config.debug {
        tracing_subscriber::EnvFilter::new("debug")
    } else {
        tracing_subscriber::EnvFilter::new("info")
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Log level initialized. Debug mode: {}", initial_config.debug);

    // 3. 初始化全局状态
    let http_client = build_http_client(&initial_config.access.proxy);
    
    let state = Arc::new(AppState {
        config: RwLock::new(initial_config),
        ip_requests: DashMap::new(),
        http_client: RwLock::new(http_client),
        providers: vec![
            Arc::new(GithubProvider::new()),
            Arc::new(DockerProvider::new()),
            Arc::new(HuggingfaceProvider::new()),
            Arc::new(GithubWebProvider::new()),
        ],
        token_cache: DashMap::new(),
        manifest_cache: DashMap::new(),
        geoip_reader: RwLock::new(None),
    });

    // 4. 异步确保 GeoIP 数据库就绪 (如果启用)
    {
        let config = state.config.read().await;
        if config.access.geoip.enabled {
            let state_clone = state.clone();
            tokio::spawn(async move {
                ensure_geoip_db(state_clone).await;
            });
        }
    }

    // 5. 启动后台调度任务
    spawn_config_reloader(state.clone());
    spawn_cleanup_task(state.clone());

    // 6. 构建路由
    let app = create_router(state);

    // 7. 启动 HTTP 服务
    let addr = SocketAddr::from(([0, 0, 0, 0], 45000));
    info!("start HTTP server @ {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
}
