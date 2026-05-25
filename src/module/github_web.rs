use crate::core::{Config, ProxyProvider};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use regex::Regex;

/// GitHub Web 页面代理实现
pub struct GithubWebProvider {
    download_re: Regex,
    clone_url_re: Regex,
}

/// 敏感路径前缀（身份验证/账户相关），禁止通过代理访问
const SENSITIVE_PATHS: &[&str] = &[
    "login", "signup", "settings", "join", "sessions",
    "auth", "logout", "password_reset", "account",
];

/// 代理路径到上游源域名的路由映射表
/// (代理路径前缀, 上游域名, 上游路径前缀)
/// 上游路径前缀为空表示该上游的资源路径不包含代理子目录（如 avatars），
/// 不为空表示上游 URL 中已包含该子目录（如 githubassets 的资源路径中已含 assets/）
const ROUTE_TABLE: &[(&str, &str, &str)] = &[
    ("avatars/",  "avatars.githubusercontent.com",   ""),
    ("assets/",   "github.githubassets.com",         "assets/"),
    ("favicons/", "github.githubassets.com",         "favicons/"),
    ("images/",   "github.githubassets.com",         "images/"),
    ("fonts/",    "github.githubassets.com",         "fonts/"),
    ("camo/",     "camo.githubusercontent.com",      ""),
];

/// 查询路径匹配的资源文件上游域名
fn resource_upstream(path: &str) -> Option<&'static str> {
    ROUTE_TABLE.iter().find(|(prefix, _, _)| path.starts_with(prefix)).map(|(_, upstream, _)| *upstream)
}

/// 判断路径是否为资源文件路径前缀
pub fn is_resource_prefix(path: &str) -> bool {
    ROUTE_TABLE.iter().any(|(prefix, _, _)| path.starts_with(prefix))
}

impl GithubWebProvider {
    pub fn new() -> Self {
        Self {
            download_re: Regex::new(r#"href="/([^/]+)/([^/]+)/(releases/download|archive)/"#).unwrap(),
            clone_url_re: Regex::new(r#"https://github\.com/([^"'\s<>]+\.git)"#).unwrap(),
        }
    }

    fn is_sensitive(&self, path: &str) -> bool {
        let base_path = path.split('?').next().unwrap_or(path).to_lowercase();
        SENSITIVE_PATHS.iter().any(|&prefix| base_path.starts_with(prefix))
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
        if let Some(upstream) = resource_upstream(path) {
            return Some(upstream.to_string());
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
        if let Some((proxy_prefix, upstream, upstream_prefix)) = ROUTE_TABLE.iter().find(|(prefix, _, _)| path.starts_with(prefix)) {
            let upstream_path = if upstream_prefix.is_empty() {
                // upstream 路径不包含子目录（如 avatars），需要去除代理前缀
                &path[proxy_prefix.len()..]
            } else {
                // upstream 路径已包含子目录（如 githubassets/assets/），保持原样
                &path
            };
            return format!("https://{}/{}", upstream, upstream_path);
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
                let new_loc = if let Some(rest) = loc_str.strip_prefix("https://github.com/") {
                    Some(format!("/{}", rest))
                } else {
                    ROUTE_TABLE.iter().find_map(|(proxy_prefix, upstream, upstream_prefix)| {
                        let domain = if upstream_prefix.is_empty() {
                            format!("https://{}/", upstream)
                        } else {
                            format!("https://{}/{}", upstream, upstream_prefix)
                        };
                        loc_str.strip_prefix(&domain).map(|rest| format!("/{}{}", proxy_prefix, rest))
                    })
                };
                if let Some(new_loc) = new_loc {
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
        //    基于路由映射表将上游 URL 替换为代理相对路径
        for &(proxy_prefix, upstream, upstream_prefix) in ROUTE_TABLE {
            let domain_url = if upstream_prefix.is_empty() {
                format!("https://{}/", upstream)
            } else {
                format!("https://{}/{}", upstream, upstream_prefix)
            };
            let proxy_path = format!("/{}", proxy_prefix);
            new_body = new_body.replace(&domain_url, &proxy_path);
        }
        new_body = new_body.replace("https://github.com/", "/");
        // raw.githubusercontent.com 需要带协议头的特殊路径，确保浏览器能解析为绝对 URL
        new_body = new_body.replace("https://raw.githubusercontent.com/", "/https://raw.githubusercontent.com/");
            
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
