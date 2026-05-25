use std::net::IpAddr;
use std::str::FromStr;
use ipnet::IpNet;

/// URL 规范化：将 https:/ 或 http:/ 修复为标准的双斜杠格式
/// 主要场景：用户直接拼接域名访问时可能产生的单斜杠畸形格式
pub fn normalize_url(url: &str) -> String {
    if url.contains("://") {
        return url.to_string();
    }

    let mut s = url.to_string();
    if s.starts_with("https:/") {
        s = s.replacen("https:/", "https://", 1);
    } else if s.starts_with("http:/") {
        s = s.replacen("http:/", "http://", 1);
    }
    s
}

/// 校验 IP 是否匹配列表（支持 CIDR 和精确 IP）
pub fn is_ip_match(ip: IpAddr, list: &[String]) -> bool {
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

/// 检查是否为局域网/私有 IP
pub fn is_internal_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback() || 
            (v6.segments()[0] & 0xfe00) == 0xfc00 ||
            (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}
