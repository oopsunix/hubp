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
    pub debug: bool,
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

pub async fn load_config() -> Result<Config, String> {
    let content = tokio::fs::read_to_string("config.yaml")
        .await
        .map_err(|e| e.to_string())?;
    let config: Config = serde_yaml::from_str(&content).map_err(|e| e.to_string())?;
    Ok(config)
}

/// --- Provider 接口定义 ---
pub trait ProxyProvider: Send + Sync {
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
    
    fn rewrite_body(&self, _path: &str, _body: String, _config: &Config) -> Option<String> {
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
    pub fn find_provider(&self, path: &str) -> Option<Arc<dyn ProxyProvider>> {
        for provider in &self.providers {
            if provider.matches(path) {
                return Some(provider.clone());
            }
        }
        None
    }
}
