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
    #[serde(default)]
    pub upstream: String,
    #[serde(rename = "authHost", default)]
    pub auth_host: String,
    #[serde(rename = "authType", default)]
    pub _auth_type: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool { true }

#[derive(Debug, Deserialize, Clone, Default)]
pub struct RequestLimitConfig {
    #[serde(default)]
    pub limit_rate: i64,
    #[serde(default)]
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
  proxy: ""              # 留空表示直连，不走上游代理

  # 访问白名单/黑名单，关键词支持通配符 *，匹配路径中的 user/repo 等信息
  white_list: []         # 白名单（空表示不限制）
  black_list:            # 黑名单
    - "baduser/badrepo"  # 禁止访问 baduser/badrepo
    - "*/badrepo"        # 禁止访问所有用户下的 badrepo
    - "baduser/*"        # 禁止访问 baduser 的所有仓库

  # GeoIP 国家限制
  geoip:
    enabled: false                          # 是否启用 GeoIP 国家限制
    databasePath: "GeoLite2-Country.mmdb"   # GeoIP 数据库文件路径
    databaseUrl: "https://github.com/P3TERX/GeoLite.mmdb/raw/download/GeoLite2-Country.mmdb"  # 数据库下载地址
    databaseUpdateDays: 30                  # 数据库自动更新周期（天）
    allowedCountries:                       # 允许访问的国家代码列表
      - "CN"                                # CN = 中国

# --- 请求频率与大小限制 ---
request_limit:
  limit_rate: 1000       # 周期内单 IP 最大请求数（0 表示不限制）
  limit_size: 10240      # 单次响应最大体积（单位：MB，0 表示不限制）
  periodHours: 3.0       # 统计周期（小时）
  white_list:            # IP 白名单（白名单中的 IP 不受限流影响）
    - "127.0.0.1"          # 本机地址
    - "172.17.0.0/24"      # Docker 默认网段
    - "192.168.1.0/24"     # 内网网段（按需调整）
  black_list:            # IP 黑名单（黑名单中的 IP 直接拒绝访问）
    - "192.168.167.1"

# --- Docker 镜像代理专项配置 ---
docker:
  tokenCache:            # Docker Token 缓存配置
    enabled: true        # 是否启用 Token/Manifest 缓存
    defaultTTL: "20m"    # 缓存有效期，单位：m（分钟）、h（小时）

  registries:
    "docker.io":
      enabled: true                      # 是否启用该 Registry 代理
      upstream: "registry-1.docker.io"   # 上游 Registry 地址
      authHost: "auth.docker.io/token"   # 认证服务地址
      authType: "docker"                 # 认证类型
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
    // 搜索优先级：当前目录 → config/ 子目录
    let paths = [
        "config.yaml", "config.yml",
        "config/config.yaml", "config/config.yml",
    ];
    let mut existing_path = None;

    for path in &paths {
        let p = std::path::Path::new(path);
        if p.is_file() {
            existing_path = Some(path.to_string());
            break;
        }
        // 检测到目录时（如 Docker 挂载空目录为文件），尝试写入模板
        if p.exists() && p.is_dir() {
            let template_path = format!("{}/config.yaml", path.trim_end_matches(".yaml").trim_end_matches(".yml"));
            tokio::fs::write(&template_path, CONFIG_TEMPLATE)
                .await
                .map_err(|e| format!("Failed to write config template at {}: {}", template_path, e))?;
            existing_path = Some(template_path);
            break;
        }
    }

    let config_path = match existing_path {
        Some(path) => path,
        None => {
            // 若 config/ 目录已存在（Docker 挂载），则在该目录内生成模板
            let config_dir = std::path::Path::new("config");
            let default_path = if config_dir.is_dir() {
                "config/config.yaml"
            } else {
                "config.yaml"
            };
            tokio::fs::write(default_path, CONFIG_TEMPLATE)
                .await
                .map_err(|e| format!("Failed to write config template: {}", e))?;
            return Err(format!("NOT_FOUND:{}", default_path));
        }
    };

    let content = tokio::fs::read_to_string(&config_path)
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
