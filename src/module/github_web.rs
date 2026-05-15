use crate::core::{Config, ProxyProvider};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use regex::Regex;

/// GitHub Web 页面代理实现
pub struct GithubWebProvider {
    download_re: Regex,
    clone_url_re: Regex,
}

impl GithubWebProvider {
    pub fn new() -> Self {
        Self {
            download_re: Regex::new(r#"href="/([^/]+)/([^/]+)/(releases/download|archive)/"#).unwrap(),
            clone_url_re: Regex::new(r#"https://github\.com/([^"'\s<>]+\.git)"#).unwrap(),
        }
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

    fn matches(&self, path: &str) -> bool {
        // 只有不包含协议头且不是指向 codeload 的路径才由 Web 处理
        !path.starts_with("http") && !path.contains("codeload.github.com")
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

    fn rewrite_body(&self, _path: &str, body: String, _config: &Config, req_headers: &HeaderMap) -> Option<String> {
        let mut new_body = body;

        // 获取代理访问的 Scheme (http 或 https)
        let scheme = req_headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_else(|| {
                if let Some(host) = req_headers.get(axum::http::header::HOST).and_then(|v| v.to_str().ok()) {
                    if host.starts_with("localhost") || host.starts_with("127.0.0.1") {
                        return "http";
                    }
                }
                "https"
            });
            
        // 获取代理访问的 Host
        let host = req_headers
            .get(axum::http::header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        // 1. 识别 .git 结尾的 HTTPS clone 链接，并用特殊占位符保护起来
        // 这样可以防止它被下一步的全局 replace("https://github.com/", "/") 破坏
        if !host.is_empty() {
            new_body = self.clone_url_re.replace_all(&new_body, "__PROXY_GIT_CLONE_HACK__$1").to_string();
        }

        // 2. 执行常规的网页相对路径替换
        new_body = new_body
            .replace("https://github.com/", "/")
            .replace("https://github.githubassets.com/", "/")
            .replace("https://avatars.githubusercontent.com/", "/avatars/")
            .replace("https://raw.githubusercontent.com/", "/https://raw.githubusercontent.com/");
            
        // 3. 补全 Release/Archive 下载链接
        new_body = self.download_re.replace_all(&new_body, r#"href="/https://github.com/$1/$2/$3/"#).to_string();

        // 4. 最后：将之前保护的占位符恢复为完整的、带有代理前缀的绝对路径
        if !host.is_empty() {
            let proxy_prefix = format!("{}://{}", scheme, host);
            new_body = new_body.replace("__PROXY_GIT_CLONE_HACK__", &format!("{}/https://github.com/", proxy_prefix));
        }

        Some(new_body)
    }
}
