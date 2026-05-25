use serde::Deserialize;
use std::collections::HashMap;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use reqwest::Client;
use tokio::sync::RwLock;
use std::sync::Arc;
use axum::http::HeaderMap;
use bytes::Bytes;
use maxminddb::Reader;

/// --- 配置模型 ---
#[derive(Debug, Deserialize, Clone, Default)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub access: AccessConfig,
    #[serde(default)]
    pub request_limit: RequestLimitConfig,
    #[serde(default)]
    pub docker: DockerConfig,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct AccessConfig {
    #[serde(default)]
    pub proxy: String,
    #[serde(default)]
    pub white_list: Vec<String>,
    #[serde(default)]
    pub black_list: Vec<String>,
    /// GeoIP 国家限制配置
    #[serde(default)]
    pub geoip: GeoIPConfig,
}

/// GeoIP 详细配置
#[derive(Debug, Deserialize, Clone, Default)]
pub struct GeoIPConfig {
    /// 是否启用国家限制
    #[serde(default)]
    pub enabled: bool,
    /// 允许访问的国家代码列表 (如: ["CN"])
    #[serde(default = "default_countries", rename = "allowedCountries")]
    pub allowed_countries: Vec<String>,
    /// GeoIP 数据库保存路径
    #[serde(default = "default_geoip_path", rename = "databasePath")]
    pub database_path: String,
    /// GeoIP 数据库下载地址
    #[serde(default = "default_geoip_url", rename = "databaseUrl")]
    pub database_url: String,
    /// 数据库自动更新周期 (天)
    #[serde(default = "default_update_days", rename = "databaseUpdateDays")]
    pub database_update_days: u64,
}

fn default_countries() -> Vec<String> { vec!["CN".to_string()] }
fn default_geoip_path() -> String { "GeoLite2-Country.mmdb".to_string() }
fn default_geoip_url() -> String { "https://github.com/P3TERX/GeoLite.mmdb/raw/download/GeoLite2-Country.mmdb".to_string() }
fn default_update_days() -> u64 { 30 }

#[derive(Debug, Deserialize, Clone, Default)]
pub struct DockerConfig {
    #[serde(default)]
    pub registries: HashMap<String, RegistryMapping>,
    #[serde(default, rename = "tokenCache")]
    pub cache: TokenCacheConfig,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub debug: bool,
}

fn default_host() -> String { "0.0.0.0".to_string() }
fn default_port() -> u16 { 45000 }

#[derive(Debug, Deserialize, Clone, Default)]
pub struct TokenCacheConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ttl")]
    pub default_ttl: String,
}

fn default_ttl() -> String { "20m".to_string() }

#[derive(Debug, Deserialize, Clone, Default)]
pub struct RegistryMapping {
    pub upstream: String,
    #[serde(rename = "authHost")]
    pub auth_host: String,
    #[serde(rename = "authType")]
    pub _auth_type: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool { true }

#[derive(Debug, Deserialize, Clone, Default)]
pub struct RequestLimitConfig {
    pub limit_rate: i64,
    pub limit_size: i64,
    #[serde(rename = "periodHours", default = "default_period_hours")]
    pub period_hours: f64,
    #[serde(default)]
    pub white_list: Vec<String>,
    #[serde(default)]
    pub black_list: Vec<String>,
}

fn default_period_hours() -> f64 { 3.0 }

pub const CONFIG_TEMPLATE: &str = r#"server:
  host: "0.0.0.0"
  port: 45000
  debug: false

# --- 访问控制配置 ---
access:
  # 上游代理地址 (可选)，支持 http, https, socks5
  # 格式示例:
  #   - HTTP 无认证: "http://127.0.0.1:7890"
  #   - HTTP 带认证: "http://user:pass@127.0.0.1:7890"
  #   - SOCKS5 无认证: "socks5://127.0.0.1:1080"
  #   - SOCKS5 带认证: "socks5://user:pass@127.0.0.1:1080"
  proxy: ""
  white_list: []
  black_list:
    - "baduser/badrepo"
    - "*/badrepo"
    - "baduser/*"
  
  # GeoIP 国家限制 (默认只允许中国 IP 访问)
  geoip:
    enabled: false
    databasePath: "GeoLite2-Country.mmdb"
    databaseUrl: "https://github.com/P3TERX/GeoLite.mmdb/raw/download/GeoLite2-Country.mmdb"
    databaseUpdateDays: 30
    allowedCountries:
      - "CN"

# --- 路由与速率限制配置 ---
request_limit:
  limit_rate: 1000
  limit_size: 10240
  periodHours: 3.0
  white_list:
    - "127.0.0.1"
    - "172.17.0.0/24"
    - "192.168.1.0/24"
  black_list:
    - "192.168.167.1"

# --- Docker 镜像代理专项配置 ---
docker:
  tokenCache:
    enabled: true
    defaultTTL: "20m"

  registries:
    "docker.io":
      enabled: true
      upstream: "registry-1.docker.io"
      authHost: "auth.docker.io/token"
      authType: "docker"
    "ghcr.io":
      enabled: true
      upstream: "ghcr.io"
      authHost: "ghcr.io/token"
      authType: "github"
    "gcr.io":
      enabled: true
      upstream: "gcr.io"
      authHost: "gcr.io/v2/token"
      authType: "google"
    "quay.io":
      enabled: true
      upstream: "quay.io"
      authHost: "quay.io/v2/auth"
      authType: "quay"
    "registry.k8s.io":
      enabled: true
      upstream: "registry.k8s.io"
      authHost: "registry.k8s.io"
      authType: "anonymous"
"#;

pub async fn load_config() -> Result<Config, String> {
    let paths = ["config.yaml", "config.yml"];
    let mut existing_path = None;

    for path in &paths {
        if std::path::Path::new(path).exists() {
            existing_path = Some(*path);
            break;
        }
    }

    let config_path = match existing_path {
        Some(path) => path,
        None => {
            let default_path = "config.yaml";
            tokio::fs::write(default_path, CONFIG_TEMPLATE)
                .await
                .map_err(|e| format!("Failed to write config template: {}", e))?;
            return Err("NOT_FOUND".to_string());
        }
    };

    let content = tokio::fs::read_to_string(config_path)
        .await
        .map_err(|e| e.to_string())?;
    let config: Config = serde_yaml::from_str(&content).map_err(|e| e.to_string())?;
    Ok(config)
}

/// --- Provider 类型定义 ---
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ProviderKind {
    /// 显式代理 (处理 /https://... 绝对 URL)
    Explicit,
    /// Web 反向代理 (处理 /user/repo 等相对路径)
    Web,
}

/// --- Provider 接口定义 ---
pub trait ProxyProvider: Send + Sync {
    /// 声明 Provider 的类型 (默认为 Explicit)
    fn kind(&self) -> ProviderKind {
        ProviderKind::Explicit
    }

    fn matches(&self, path: &str) -> bool;
    fn transform(&self, path: String, config: &Config) -> String;
    fn extract_keywords(&self, path: &str) -> Option<Vec<String>>;
    
    fn upstream_host(&self, _path: &str, _config: &Config) -> Option<String> {
        None
    }

    fn pre_check(&self, _path: &str, _config: &Config) -> Option<axum::response::Response> {
        None
    }

    fn handle_response(&self, _headers: &mut HeaderMap, _config: &Config) {}
    
    fn rewrite_body(&self, _path: &str, _body: String, _config: &Config, _req_headers: &HeaderMap) -> Option<String> {
        None
    }
}

/// --- 全局应用状态 ---
pub struct AppState {
    pub config: RwLock<Config>,
    pub ip_requests: DashMap<String, Vec<DateTime<Utc>>>,
    pub http_client: RwLock<Client>,
    pub providers: Vec<Arc<dyn ProxyProvider>>,
    pub token_cache: DashMap<String, (String, DateTime<Utc>)>,
    pub manifest_cache: DashMap<String, (Bytes, HeaderMap, DateTime<Utc>)>,
    /// GeoIP 数据库读取器 (使用 RwLock 以便初始化)
    pub geoip_reader: RwLock<Option<Reader<Vec<u8>>>>,
}

impl AppState {
    pub fn find_provider(&self, path: &str, kind: ProviderKind) -> Option<Arc<dyn ProxyProvider>> {
        // 匹配前执行前导斜杠清理
        let clean_path = path.trim_start_matches('/');
        for provider in &self.providers {
            if provider.kind() == kind && provider.matches(clean_path) {
                return Some(provider.clone());
            }
        }
        None
    }
}
