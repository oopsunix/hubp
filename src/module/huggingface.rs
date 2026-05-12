use crate::core::{Config, ProxyProvider};
use regex::Regex;

/// Hugging Face 模型加速服务实现
pub struct HuggingfaceProvider {
    exps: Vec<Regex>,
}

impl HuggingfaceProvider {
    pub fn new() -> Self {
        Self {
            exps: vec![
                Regex::new(r"^(?:https?://)?huggingface\.co/([^/]+)/([^/]+)/resolve/([^/]+)/(.*)$").unwrap(),
                Regex::new(r"^(?:https?://)?huggingface\.co/api/models/([^/]+)/([^/]+)/.*$").unwrap(),
                Regex::new(r"^(?:https?://)?cdn-lfs\.huggingface\.co/.*$").unwrap(),
                Regex::new(r"^(?:https?://)?huggingface\.co/(.*)$").unwrap(),
            ],
        }
    }
}

impl ProxyProvider for HuggingfaceProvider {
    fn matches(&self, path: &str) -> bool {
        self.exps.iter().any(|exp| exp.is_match(path)) || path.contains("huggingface.co")
    }

    fn upstream_host(&self, _path: &str, _config: &Config) -> Option<String> {
        Some("huggingface.co".to_string())
    }

    fn transform(&self, mut path: String, _config: &Config) -> String {
        let upstream_domain = "huggingface.co";
        if path.contains("huggingface.co") {
            path = path.replace("huggingface.co", upstream_domain);
        }
        if !path.starts_with("http") {
            format!("https://{}/{}", upstream_domain, path.trim_start_matches('/'))
        } else {
            path
        }
    }

    fn extract_keywords(&self, path: &str) -> Option<Vec<String>> {
        for exp in &self.exps[..2] {
            if let Some(caps) = exp.captures(path) {
                let mut matches = Vec::new();
                if caps.len() >= 3 {
                    matches.push(caps.get(1).map_or("", |m| m.as_str()).to_string());
                    matches.push(caps.get(2).map_or("", |m| m.as_str()).to_string());
                    return Some(matches);
                }
            }
        }
        None
    }

    fn handle_response(&self, headers: &mut axum::http::HeaderMap, _config: &Config) {
        if let Some(location) = headers.get(axum::http::header::LOCATION) {
            if let Ok(loc_str) = location.to_str() {
                if loc_str.contains("cdn-lfs.huggingface.co") {
                    let new_loc = format!("/{}", loc_str);
                    if let Ok(new_val) = axum::http::HeaderValue::from_str(&new_loc) {
                        headers.insert(axum::http::header::LOCATION, new_val);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Config;

    #[test]
    fn test_hf_matches() {
        let provider = HuggingfaceProvider::new();
        assert!(provider.matches("huggingface.co/gpt2/resolve/main/config.json"));
        assert!(provider.matches("cdn-lfs.huggingface.co/LFS_FILE"));
    }

    #[test]
    fn test_hf_transform_direct() {
        let provider = HuggingfaceProvider::new();
        let config = Config::default();

        let path = "gpt2/resolve/main/config.json".to_string();
        let result = provider.transform(path, &config);
        assert_eq!(result, "https://huggingface.co/gpt2/resolve/main/config.json");
    }
}
