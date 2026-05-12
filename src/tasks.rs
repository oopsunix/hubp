use std::sync::Arc;
use std::time::Duration;
use chrono::Utc;
use reqwest::{Client, Proxy};
use tracing::{error, info};
use maxminddb::Reader;
use crate::core::{load_config, AppState};

/// 创建 HTTP 客户端的统一辅助函数
pub fn build_http_client(proxy_url: &str) -> Client {
    let mut builder = Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(64)
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .tcp_nodelay(true)
        .http2_initial_connection_window_size(4 * 1024 * 1024)
        .http2_initial_stream_window_size(2 * 1024 * 1024)
        .http2_max_frame_size(16384)
        .use_rustls_tls() 
        .brotli(true)
        .gzip(true);

    if !proxy_url.is_empty() {
        match Proxy::all(proxy_url) {
            Ok(proxy) => {
                info!("Using upstream proxy: {}", proxy_url);
                builder = builder.proxy(proxy);
            }
            Err(e) => {
                error!("Invalid proxy URL '{}': {}", proxy_url, e);
            }
        }
    }

    builder.build().expect("Failed to create http client")
}

/// 检查并下载 GeoIP 数据库
pub async fn ensure_geoip_db(state: Arc<AppState>) {
    let (enabled, db_path, db_url) = {
        let config = state.config.read().await;
        (
            config.access.geoip.enabled,
            config.access.geoip.database_path.clone(),
            config.access.geoip.database_url.clone(),
        )
    };

    if !enabled {
        return;
    }

    if tokio::fs::metadata(&db_path).await.is_ok() {
        info!("GeoIP database found at {}.", db_path);
        load_geoip_db(state.clone(), &db_path).await;
    } else {
        info!("GeoIP database not found, downloading from {}...", db_url);
        download_geoip_db(state.clone(), &db_path, &db_url).await;
    }

    // 启动定期更新任务
    spawn_geoip_updater(state);
}

/// 下载 GeoIP 数据库
async fn download_geoip_db(state: Arc<AppState>, db_path: &str, db_url: &str) {
    let client = Client::new();
    match client.get(db_url).send().await {
        Ok(resp) => {
            if resp.status().is_success() {
                let bytes = resp.bytes().await.unwrap_or_default();
                if let Err(e) = tokio::fs::write(db_path, bytes).await {
                    error!("Failed to write GeoIP database: {}", e);
                } else {
                    info!("GeoIP database downloaded successfully.");
                    load_geoip_db(state, db_path).await;
                }
            } else {
                error!("Failed to download GeoIP database: HTTP {}", resp.status());
            }
        }
        Err(e) => error!("Failed to download GeoIP database: {}", e),
    }
}

/// 加载 GeoIP 数据库到内存
async fn load_geoip_db(state: Arc<AppState>, db_path: &str) {
    match Reader::open_readfile(db_path) {
        Ok(reader) => {
            let mut db_lock = state.geoip_reader.write().await;
            *db_lock = Some(reader);
            info!("GeoIP database loaded into memory.");
        }
        Err(e) => error!("Failed to open GeoIP database: {}", e),
    }
}

/// 启动 GeoIP 数据库自动更新任务
fn spawn_geoip_updater(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let update_days = {
                let config = state.config.read().await;
                config.access.geoip.database_update_days
            };
            
            // 等待指定的更新周期
            tokio::time::sleep(Duration::from_secs(update_days * 24 * 3600)).await;

            let (enabled, db_path, db_url) = {
                let config = state.config.read().await;
                (
                    config.access.geoip.enabled,
                    config.access.geoip.database_path.clone(),
                    config.access.geoip.database_url.clone(),
                )
            };

            if enabled {
                info!("Starting periodic GeoIP database update...");
                download_geoip_db(state.clone(), &db_path, &db_url).await;
            }
        }
    });
}

/// 启动配置自动重载任务
pub fn spawn_config_reloader(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(600));
        loop {
            interval.tick().await;
            match load_config().await {
                Ok(new_config) => {
                    info!("Reloading config...");
                    let mut config_lock = state.config.write().await;
                    if config_lock.access.proxy != new_config.access.proxy {
                        info!("Proxy setting changed, rebuilding http client...");
                        let new_client = build_http_client(&new_config.access.proxy);
                        let mut client_lock = state.http_client.write().await;
                        *client_lock = new_client;
                    }
                    *config_lock = new_config;
                }
                Err(e) => error!("failed to reload config: {}", e),
            }
        }
    });
}

/// 启动内存清理任务
pub fn spawn_cleanup_task(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            let now = Utc::now();
            state.ip_requests.retain(|_, times| {
                times.retain(|t| now.signed_duration_since(*t) <= chrono::Duration::minutes(1));
                !times.is_empty()
            });
        }
    });
}
