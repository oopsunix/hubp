use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use crate::core::{AppState, ProxyProvider};

/// 核心代理转发引擎
pub async fn do_proxy(
    state: Arc<AppState>, 
    req: Request, 
    target_url: String, 
    depth: u8,
    provider: Option<Arc<dyn ProxyProvider>>
) -> Response {
    if depth >= 10 {
        return (StatusCode::LOOP_DETECTED, "Too many redirects.").into_response();
    }
    
    let path = req.uri().path().to_string(); 
    let method = req.method().clone();
    let headers = req.headers().clone();
    let body = req.into_body();

    // 1. 构造转发请求
    let proxy_req_builder = {
        let client = state.http_client.read().await;
        client.request(method, &target_url)
    };
    
    let mut proxy_req_builder = proxy_req_builder;
    
    // 2. 处理请求头
    for (key, value) in headers {
        if let Some(k) = key {
            if k == axum::http::header::HOST {
                continue;
            }
            
            if k == axum::http::header::REFERER {
                if let (Some(ref p), Ok(_val_str)) = (&provider, value.to_str()) {
                    let config = state.config.read().await;
                    if let Some(upstream_host) = p.upstream_host(&path, &config) {
                        let new_referer = format!("https://{}/", upstream_host);
                        if let Ok(v) = HeaderValue::from_str(&new_referer) {
                            proxy_req_builder = proxy_req_builder.header(k, v);
                            continue;
                        }
                    }
                }
            }
            
            proxy_req_builder = proxy_req_builder.header(k, value);
        }
    }

    // 3. 发起请求
    let proxy_req = match proxy_req_builder.body(reqwest::Body::wrap_stream(body.into_data_stream())).send().await {
        Ok(res) => res,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("server error {}", e)).into_response(),
    };

    let status = proxy_req.status();
    let headers = proxy_req.headers().clone();

    // 4. 文件大小检查
    if let Some(content_length) = headers.get(axum::http::header::CONTENT_LENGTH) {
        if let Ok(size) = content_length.to_str().unwrap_or("0").parse::<i64>() {
            let config = state.config.read().await;
            let limit_size = config.request_limit.limit_size * 1024 * 1024;
            if size > limit_size {
                return (StatusCode::PAYLOAD_TOO_LARGE, "File too large.").into_response();
            }
        }
    }

    // 5. 清洗并重写响应头
    let mut res_headers = HeaderMap::new();
    for (key, value) in headers {
        if let Some(k) = key {
            let k_str = k.as_str().to_lowercase();
            if k_str != "content-security-policy" && k_str != "referrer-policy" && k_str != "strict-transport-security" {
                res_headers.insert(k, value);
            }
        }
    }

    if let Some(ref p) = provider {
        let config = state.config.read().await;
        p.handle_response(&mut res_headers, &config);
    }

    // 6. 处理重定向
    if let Some(location) = res_headers.get(axum::http::header::LOCATION) {
        if let Ok(loc_str) = location.to_str() {
            let next_provider = state.find_provider(loc_str);
            if next_provider.is_some() {
                let new_loc = format!("/{}", loc_str);
                res_headers.insert(axum::http::header::LOCATION, HeaderValue::from_str(&new_loc).unwrap());
            } else {
                return Box::pin(do_proxy(state.clone(), Request::new(Body::empty()), loc_str.to_string(), depth + 1, next_provider)).await;
            }
        }
    }

    // 7. 处理体改写
    let is_html = res_headers.get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains("text/html"))
        .unwrap_or(false);

    if is_html {
        if let Some(ref p) = provider {
            let config = state.config.read().await;
            let body_bytes = match proxy_req.bytes().await {
                Ok(b) => b,
                Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("read error {}", e)).into_response(),
            };
            
            if let Ok(body_str) = String::from_utf8(body_bytes.to_vec()) {
                if let Some(rewritten) = p.rewrite_body(&path, body_str, &config) {
                    let mut builder = Response::builder().status(status);
                    *builder.headers_mut().unwrap() = res_headers;
                    builder = builder.header(axum::http::header::CONTENT_LENGTH, rewritten.len());
                    return builder.body(Body::from(rewritten)).unwrap();
                }
            }
            
            let mut builder = Response::builder().status(status);
            *builder.headers_mut().unwrap() = res_headers;
            return builder.body(Body::from(body_bytes)).unwrap();
        }
    }

    let mut builder = Response::builder().status(status);
    *builder.headers_mut().unwrap() = res_headers;
    let body = Body::from_stream(proxy_req.bytes_stream());
    builder.body(body).unwrap()
}

/// 自定义日志中间件引擎
pub async fn custom_logger_engine(
    State(_state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    if path == "/" {
        return next.run(req).await;
    }

    let start = std::time::Instant::now();
    let method = req.method().clone();
    let user_agent = req.headers().get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or("unknown").to_string();
    
    let response = next.run(req).await;
    
    let latency = start.elapsed();
    let status = response.status();

    tracing::info!(
        "{} | {:?} | {:?} | {} | {}",
        status.as_u16(),
        latency,
        method,
        path,
        user_agent
    );

    response
}

/// 列表匹配辅助工具
pub fn check_list(keywords: &[String], list: &[String]) -> bool {
    if keywords.is_empty() || list.is_empty() {
        return false;
    }

    let target = keywords.join("/");

    for pattern in list {
        if pattern == "*" {
            return true;
        }
        
        if pattern.contains('*') {
            let regex_pattern = format!("^{}$", pattern.replace(".", "\\.").replace("*", ".*"));
            if let Ok(re) = regex::Regex::new(&regex_pattern) {
                if re.is_match(&target) {
                    return true;
                }
            }
        } else {
            if target.starts_with(pattern) || pattern == &target {
                return true;
            }
        }
    }
    false
}
