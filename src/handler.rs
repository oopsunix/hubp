use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    body::Body,
};
use chrono::Utc;
use std::{net::SocketAddr, sync::Arc};
use tracing::info;

use crate::core::AppState;
use crate::engine::{do_proxy, check_list};
use crate::module::github_web::is_resource_prefix;
use crate::utils::{normalize_url, is_internal_ip, is_ip_match};

/// 首页处理器
pub async fn index_handler() -> impl IntoResponse {
    (StatusCode::OK, "Have fun!").into_response()
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
    let target_path = normalize_url(raw_path);

    // 1. 识别请求意图并匹配 Provider
    // 策略：优先匹配 Explicit，失败则返回错误信息便于排障
    let (provider, kind) = {
        // 路径不含域名特征且非 Docker 专用前缀，直接跳过 Explicit 匹配
        let needs_explicit = target_path.starts_with("http")
            || target_path.starts_with("v2/")
            || target_path.starts_with("token")
            || target_path.split('/').next().map_or(false, |s| s.contains('.'));

        if !needs_explicit {
            match state.find_provider(&target_path, crate::core::ProviderKind::Web) {
                Some(p) => (p, crate::core::ProviderKind::Web),
                None => {
                    return (StatusCode::NOT_FOUND, format!("No provider found for path: /{}", target_path)).into_response();
                }
            }
        } else if let Some(p) = state.find_provider(&target_path, crate::core::ProviderKind::Explicit) {
            (p, crate::core::ProviderKind::Explicit)
        } else {
            return (StatusCode::NOT_FOUND, format!("No provider found for path: /{}", target_path)).into_response();
        }
    };

    // CDN 资源快速路径：由 GithubWebProvider 的 HTML 改写产生的 CDN 资源，
    // 直接从 CDN 分发，跳过校验链
    if kind == crate::core::ProviderKind::Web && is_resource_prefix(&target_path)
    {
        let target_url = {
            let config = state.config.read().await;
            provider.transform(target_path.clone(), &config)
        };
        return do_proxy(state.clone(), req, target_url, 0, Some(provider.clone())).await;
    }

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
            
            // limit_rate <= 0 表示不限制
            if limit_rate > 0 {
                let mut entry = state.ip_requests.entry(ip_str).or_insert(Vec::new());
                entry.retain(|t| now.signed_duration_since(*t).num_seconds() as f64 <= period_hours * 3600.0);

                if entry.len() as i64 >= limit_rate {
                    return (StatusCode::TOO_MANY_REQUESTS, "Too Many Requests.").into_response();
                }
                entry.push(now);
            }
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
