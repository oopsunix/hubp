use crate::core::{Config, ProxyProvider};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

/// GitHub Web 页面代理实现
pub struct GithubWebProvider;

impl GithubWebProvider {
    pub fn new() -> Self {
        Self
    }

    fn is_sensitive(&self, path: &str) -> bool {
        let p = path.to_lowercase();
        let base_path = p.split('?').next().unwrap_or("");
        base_path.starts_with("login") || 
        base_path.starts_with("signup") || 
        base_path.starts_with("settings") || 
        base_path.starts_with("join") || 
        base_path.starts_with("sessions") || 
        base_path.starts_with("auth") ||
        base_path.starts_with("logout") ||
        base_path.starts_with("password_reset") ||
        base_path.starts_with("account")
    }

    fn is_static_extension(&self, path: &str) -> bool {
        let exts = [
            ".js", ".css", ".png", ".jpg", ".jpeg", ".gif", ".svg", 
            ".woff", ".woff2", ".ttf", ".eot", ".map"
        ];
        exts.iter().any(|&ext| path.ends_with(ext))
    }
}

impl ProxyProvider for GithubWebProvider {
    fn kind(&self) -> crate::core::ProviderKind {
        crate::core::ProviderKind::Web
    }

    fn matches(&self, _path: &str) -> bool {
        true
    }

    fn upstream_host(&self, path: &str, _config: &Config) -> Option<String> {
        if path.starts_with("avatars/") {
            return Some("avatars.githubusercontent.com".to_string());
        }
        let asset_prefixes = ["assets/", "favicons/", "images/", "fonts/"];
        if asset_prefixes.iter().any(|&p| path.starts_with(p)) || self.is_static_extension(path) {
            return Some("github.githubassets.com".to_string());
        }
        Some("github.com".to_string())
    }

    fn pre_check(&self, path: &str, _config: &Config) -> Option<Response> {
        if self.is_sensitive(path) {
            let display_path = path.split('?').next().unwrap_or(path);
            let display_path = if !display_path.starts_with('/') {
                format!("/{}", display_path)
            } else {
                display_path.to_string()
            };
            return Some((StatusCode::FORBIDDEN, format!("Path {} is blocked.", display_path)).into_response());
        }
        None
    }

    fn transform(&self, path: String, _config: &Config) -> String {
        if path.starts_with("avatars/") {
            return format!("https://avatars.githubusercontent.com/{}", &path[8..]);
        }
        let asset_prefixes = ["assets/", "favicons/", "images/", "fonts/"];
        if asset_prefixes.iter().any(|&p| path.starts_with(p)) || (self.is_static_extension(&path) && path.contains('/')) {
            return format!("https://github.githubassets.com/{}", path);
        }
        let root_files = ["manifest.json", "robots.txt", "favicon.ico", "favicon.svg"];
        if root_files.iter().any(|&f| path == f) {
            return format!("https://github.com/{}", path);
        }
        format!("https://github.com/{}", path)
    }

    fn extract_keywords(&self, path: &str) -> Option<Vec<String>> {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 1 && !parts[0].is_empty() {
            return Some(vec![parts[0].to_string()]);
        }
        None
    }

    fn handle_response(&self, headers: &mut HeaderMap, _config: &Config) {
        if let Some(location) = headers.get(axum::http::header::LOCATION) {
            if let Ok(loc_str) = location.to_str() {
                if loc_str.starts_with("https://github.com/") {
                    let new_loc = format!("/{}", &loc_str[19..]);
                    if let Ok(v) = axum::http::HeaderValue::from_str(&new_loc) {
                        headers.insert(axum::http::header::LOCATION, v);
                    }
                } else if loc_str.starts_with("https://github.githubassets.com/") {
                    let new_loc = format!("/{}", &loc_str[32..]);
                    if let Ok(v) = axum::http::HeaderValue::from_str(&new_loc) {
                        headers.insert(axum::http::header::LOCATION, v);
                    }
                }
            }
        }
    }

    fn rewrite_body(&self, _path: &str, body: String, _config: &Config) -> Option<String> {
        let new_body = body
            .replace("https://github.com/", "/")
            .replace("https://github.githubassets.com/", "/")
            .replace("https://avatars.githubusercontent.com/", "/avatars/")
            .replace("https://raw.githubusercontent.com/", "/https://raw.githubusercontent.com/");
        Some(new_body)
    }
}
