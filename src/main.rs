mod core;
mod module;
mod engine;
mod handler;
mod route;
mod tasks;
mod utils;

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
use crate::tasks::{spawn_cleanup_task, spawn_config_reloader, build_http_client, ensure_geoip_db, test_connectivity};

/// 程序主入口
#[tokio::main]
async fn main() {
    // 1. 加载初始配置
    let initial_config = match load_config().await {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Failed to load configuration: {}", e);
            std::process::exit(1);
        }
    };

    // 2. 初始化日志系统
    let filter = if initial_config.server.debug {
        tracing_subscriber::EnvFilter::new("debug")
    } else {
        tracing_subscriber::EnvFilter::new("info")
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Log level initialized. Debug mode: {}", initial_config.server.debug);

    // 3. 初始化全局状态
    let http_client = build_http_client(&initial_config.access.proxy);

    // 4. 启动连通性测试 (不阻塞主流程)
    {
        let client_clone = http_client.clone();
        tokio::spawn(async move {
            test_connectivity(&client_clone).await;
        });
    }
    
    // 5. 初始化全局状态
    let state = Arc::new(AppState {
        config: RwLock::new(initial_config.clone()),
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

    // 6. 异步确保 GeoIP 数据库就绪 (如果启用)
    {
        let config = state.config.read().await;
        if config.access.geoip.enabled {
            let state_clone = state.clone();
            tokio::spawn(async move {
                ensure_geoip_db(state_clone).await;
            });
        }
    }

    // 7. 启动后台调度任务
    spawn_config_reloader(state.clone());
    spawn_cleanup_task(state.clone());

    // 8. 构建路由
    let app = create_router(state);

    // 9. 启动 HTTP 服务
    let addr: SocketAddr = format!("{}:{}", initial_config.server.host, initial_config.server.port)
        .parse()
        .expect("Invalid server host or port in config.yaml");
    
    info!("start HTTP server @ {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
}
