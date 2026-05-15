use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    body::Body,
};
use chrono::Utc;
use std::{net::{SocketAddr, IpAddr}, sync::Arc, str::FromStr};
use tracing::info;
use ipnet::IpNet;

use crate::core::AppState;
use crate::engine::{do_proxy, check_list};

/// 首页处理器
pub async fn index_handler() -> impl IntoResponse {
    (StatusCode::OK, "Have fun!").into_response()
}

/// 辅助函数：校验 IP 是否匹配列表
fn is_ip_match(ip: IpAddr, list: &[String]) -> bool {
    for item in list {
        if let Ok(net) = IpNet::from_str(item) {
            if net.contains(&ip) {
                return true;
            }
        }
        if let Ok(target_ip) = IpAddr::from_str(item) {
            if target_ip == ip {
                return true;
            }
        }
    }
    false
}

/// 辅助函数：检查是否为局域网/私有 IP
fn is_internal_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback() || 
            (v6.segments()[0] & 0xfe00) == 0xfc00 || // Unique Local Address (fc00::/7)
            (v6.segments()[0] & 0xffc0) == 0xfe80    // Link-Local Address (fe80::/10)
        }
    }
}

/// 代理处理器
pub async fn proxy_handler(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Response {
    let mut raw_path = req.uri().path().to_string();
    let query = req.uri().query().unwrap_or("").to_string();
    if !query.is_empty() {
        raw_path.push('?');
        raw_path.push_str(&query);
    }
    let raw_path = raw_path.trim_start_matches('/');
    let target_path = raw_path.to_string();

    // 1. 识别请求意图并匹配 Provider
    // 策略：优先匹配 Explicit 类型（支持带 http 和不带 http 的域名路径），
    //       若无匹配，则视为 Web 相对路径。
    let (provider, _kind) = if let Some(p) = state.find_provider(&target_path, crate::core::ProviderKind::Explicit) {
        (p, crate::core::ProviderKind::Explicit)
    } else {
        match state.find_provider(&target_path, crate::core::ProviderKind::Web) {
            Some(p) => (p, crate::core::ProviderKind::Web),
            None => {
                return (StatusCode::NOT_FOUND, "No provider found for this path.").into_response();
            }
        }
    };

    // 2. 国家地理限制校验 (仅针对启用该功能的请求)
    {
        let config = state.config.read().await;
        if config.access.geoip.enabled {
            let client_ip = addr.ip();
            
            // 如果是局域网 IP 或在白名单中，跳过地理位置校验
            if is_internal_ip(client_ip) || is_ip_match(client_ip, &config.request_limit.white_list) {
                // 跳过 GeoIP 校验
            } else {
                let reader_lock = state.geoip_reader.read().await;
                if let Some(ref reader) = *reader_lock {
                    // 查询 IP 所属国家
                    let country_iso = match reader.lookup::<maxminddb::geoip2::Country>(client_ip) {
                        Ok(country) => country.country.and_then(|c| c.iso_code).unwrap_or(""),
                        Err(_) => "",
                    };

                    // 如果不在允许名单内，拦截
                    if !config.access.geoip.allowed_countries.iter().any(|c| c == country_iso) {
                        info!("Blocked request from {} (IP: {})", country_iso, client_ip);
                        return (StatusCode::FORBIDDEN, format!("Access from your country ({}) is restricted.", country_iso)).into_response();
                    }
                }
            }
        }
    }

    // 3. 前置校验 (例如拦截敏感路径)
    {
        let config = state.config.read().await;
        if let Some(intercept_res) = provider.pre_check(&target_path, &config) {
            return intercept_res;
        }
    }

    // 4. 缓存拦截 (仅针对 Docker Token 和 Manifest)
    {
        let config = state.config.read().await;
        if config.docker.cache.enabled && provider.matches(&target_path) {
            if target_path.starts_with("token") {
                if let Some(entry) = state.token_cache.get(&query) {
                    let (body, expiry) = entry.value();
                    if *expiry > Utc::now() {
                        info!("Token cache hit for query: {}", query);
                        return Response::builder()
                            .status(StatusCode::OK)
                            .header("Content-Type", "application/json")
                            .body(Body::from(body.clone()))
                            .unwrap();
                    }
                }
            }
            
            if target_path.contains("/manifests/") && req.method() == axum::http::Method::GET {
                if let Some(entry) = state.manifest_cache.get(&target_path) {
                    let (body, headers, expiry) = entry.value();
                    if *expiry > Utc::now() {
                        info!("Manifest cache hit for path: {}", target_path);
                        let mut builder = Response::builder().status(StatusCode::OK);
                        for (k, v) in headers {
                            builder = builder.header(k, v);
                        }
                        return builder.body(Body::from(body.clone())).unwrap();
                    }
                }
            }
        }
    }

    // 5. 黑白名单校验
    if let Some(keywords) = provider.extract_keywords(&target_path) {
        let config = state.config.read().await;
        if !config.access.white_list.is_empty() && !check_list(&keywords, &config.access.white_list) {
            return (StatusCode::FORBIDDEN, "Forbidden by proxy white list.").into_response();
        }
        if !config.access.black_list.is_empty() && check_list(&keywords, &config.access.black_list) {
            return (StatusCode::FORBIDDEN, "Forbidden by proxy black list.").into_response();
        }
    }

    // 6. 执行转换
    let target_url = {
        let config = state.config.read().await;
        provider.transform(target_path.clone(), &config)
    };

    // 7. IP 过滤与限流
    let client_ip = addr.ip();
    {
        let config = state.config.read().await;
        if is_ip_match(client_ip, &config.request_limit.black_list) {
            return (StatusCode::FORBIDDEN, "IP address is blacklisted.").into_response();
        }

        if !is_ip_match(client_ip, &config.request_limit.white_list) {
            let limit_rate = config.request_limit.limit_rate;
            let period_hours = config.request_limit.period_hours;
            let now = Utc::now();
            let ip_str = client_ip.to_string();
            
            let mut entry = state.ip_requests.entry(ip_str).or_insert(Vec::new());
            entry.retain(|t| now.signed_duration_since(*t).num_seconds() as f64 <= period_hours * 3600.0);

            if entry.len() as i64 >= limit_rate {
                return (StatusCode::TOO_MANY_REQUESTS, "Too Many Requests.").into_response();
            }
            entry.push(now);
        }
    }

    // 8. 发起请求并处理缓存写入
    let response = do_proxy(state.clone(), req, target_url, 0, Some(provider.clone())).await;
    
    if response.status() == StatusCode::OK {
        let config = state.config.read().await;
        if config.docker.cache.enabled {
            let ttl_minutes = config.docker.cache.default_ttl.trim_end_matches('m').parse::<i64>().unwrap_or(20);
            let expires_at = Utc::now() + chrono::Duration::minutes(ttl_minutes);

            if target_path.starts_with("token") {
                let (parts, body) = response.into_parts();
                let body_bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap_or_default();
                if let Ok(body_str) = String::from_utf8(body_bytes.to_vec()) {
                    state.token_cache.insert(query, (body_str, expires_at));
                }
                return Response::from_parts(parts, Body::from(body_bytes));
            }

            if target_path.contains("/manifests/") {
                let (parts, body) = response.into_parts();
                let headers = parts.headers.clone();
                let body_bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap_or_default();
                state.manifest_cache.insert(target_path, (body_bytes.clone(), headers, expires_at));
                return Response::from_parts(parts, Body::from(body_bytes));
            }
        }
    }

    response
}
