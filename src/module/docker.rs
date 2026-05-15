use crate::core::{Config, ProxyProvider};
use ax_http::HeaderMap;

/// Docker 镜像代理服务实现
pub struct DockerProvider;

use axum::http as ax_http;

impl DockerProvider {
    pub fn new() -> Self {
        Self
    }

    fn get_query_param<'a>(&self, query: &'a str, key: &str) -> Option<&'a str> {
        for pair in query.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                if k == key {
                    return Some(v);
                }
            }
        }
        None
    }
}

impl ProxyProvider for DockerProvider {
    fn matches(&self, path: &str) -> bool {
        path.starts_with("v2/") || path.starts_with("token")
    }

    fn upstream_host(&self, path: &str, config: &Config) -> Option<String> {
        if path.starts_with("v2/") {
            let inner_path = &path[3..];
            for (domain, mapping) in &config.docker.registries {
                if !mapping.enabled { continue; }
                if inner_path == *domain || (inner_path.starts_with(domain) && inner_path[domain.len()..].starts_with('/')) {
                    return Some(mapping.upstream.clone());
                }
            }
            return config.docker.registries.get("docker.io").map(|m| m.upstream.clone());
        }
        if path.starts_with("token") {
            let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
            if let Some(service) = self.get_query_param(query, "service") {
                for mapping in config.docker.registries.values() {
                    if mapping.enabled && (service == mapping.upstream || service.starts_with(&mapping.upstream)) {
                        return mapping.auth_host.split('/').next().map(|s| s.to_string());
                    }
                }
            }
        }
        None
    }

    fn transform(&self, path: String, config: &Config) -> String {
        if path.starts_with("v2/") {
            let inner_path = &path[3..];
            for (domain, mapping) in &config.docker.registries {
                if !mapping.enabled { continue; }
                if inner_path == *domain || (inner_path.starts_with(domain) && inner_path[domain.len()..].starts_with('/')) {
                    let remaining = if inner_path.len() <= domain.len() { "" } else { &inner_path[domain.len() + 1..] };
                    return format!("https://{}/v2/{}", mapping.upstream, remaining);
                }
            }
            let upstream = config.docker.registries.get("docker.io").map(|m| m.upstream.as_str()).unwrap_or("registry-1.docker.io");
            return format!("https://{}/v2/{}", upstream, inner_path);
        } else if path.starts_with("token") {
            let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
            let mut target_auth_host = "auth.docker.io/token".to_string();
            if let Some(service) = self.get_query_param(query, "service") {
                for mapping in config.docker.registries.values() {
                    if mapping.enabled && (service == mapping.upstream || service.starts_with(&mapping.upstream)) {
                        target_auth_host = mapping.auth_host.clone();
                        break;
                    }
                }
            }
            return if query.is_empty() { format!("https://{}", target_auth_host) } else { format!("https://{}?{}", target_auth_host, query) };
        }
        path
    }

    fn extract_keywords(&self, path: &str) -> Option<Vec<String>> {
        if path.starts_with("v2/") {
            let clean_path = path[3..].trim_start_matches('/');
            let parts: Vec<&str> = clean_path.split('/').collect();
            if parts.len() >= 2 {
                return Some(vec![parts[0].to_string(), parts[1].to_string()]);
            }
        }
        None
    }

    fn handle_response(&self, headers: &mut HeaderMap, config: &Config) {
        if let Some(auth_header) = headers.get_mut("www-authenticate") {
            if let Ok(auth_str) = auth_header.to_str() {
                let mut new_auth = auth_str.to_string();
                for mapping in config.docker.registries.values() {
                    if mapping.enabled {
                        let full_auth_url = format!("https://{}", mapping.auth_host);
                        if new_auth.contains(&full_auth_url) {
                            new_auth = new_auth.replace(&full_auth_url, "/token");
                        }
                    }
                }
                if !new_auth.contains("/token") && new_auth.contains("auth.docker.io/token") {
                    new_auth = new_auth.replace("https://auth.docker.io/token", "/token");
                }
                if let Ok(new_val) = ax_http::HeaderValue::from_str(&new_auth) {
                    *auth_header = new_val;
                }
            }
        }
    }
}
