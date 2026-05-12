use crate::core::{Config, ProxyProvider};
use regex::Regex;

/// GitHub 代理服务模块的实现
pub struct GithubProvider {
    exps: Vec<Regex>,
}

impl GithubProvider {
    pub fn new() -> Self {
        Self {
            exps: vec![
                // 0. 匹配 Release/Archive
                Regex::new(r"^(?:https?://)?github\.com/([^/]+)/([^/]+)/(?:releases|archive)/.*$").unwrap(),
                // 1. 匹配 Blob/Raw 文件 (index 1)
                Regex::new(r"^(?:https?://)?github\.com/([^/]+)/([^/]+)/(?:blob|raw)/.*$").unwrap(),
                // 2. 匹配 Git 仓库链接
                Regex::new(r"^(?:https?://)?github\.com/([^/]+)/([^/]+)/(?:info|git-).*$").unwrap(),
                // 3. 匹配 Raw 域名
                Regex::new(r"^(?:https?://)?raw\.github(?:usercontent|)\.com/([^/]+)/([^/]+)/.+?/.+$").unwrap(),
                // 4. 匹配 Gist
                Regex::new(r"^(?:https?://)?gist\.github(?:usercontent|)\.com/([^/]+)/.+?/.+$").unwrap(),
                
                // --- GitHub API 专用匹配规则 ---
                // 5. 匹配 API Repos 路径 (提取 user, repo)
                Regex::new(r"^(?:https?://)?api\.github\.com/repos/([^/]+)/([^/]+)(?:/.*)?$").unwrap(),
                // 6. 匹配 API Users 路径 (提取 user)
                Regex::new(r"^(?:https?://)?api\.github\.com/users/([^/]+)(?:/.*)?$").unwrap(),
                // 7. 匹配 API Gists 路径
                Regex::new(r"^(?:https?://)?api\.github\.com/gists(?:/.*)?$").unwrap(),
                // 8. 匹配 API 通用路径 (兜底)
                Regex::new(r"^(?:https?://)?api\.github\.com/(.*)$").unwrap(),
                
                // 9. 通用主站匹配 (兜底网页浏览)
                Regex::new(r"^(?:https?://)?github\.com/.*$").unwrap(),
            ],
        }
    }
}

impl ProxyProvider for GithubProvider {
    fn matches(&self, url: &str) -> bool {
        self.exps.iter().any(|exp| exp.is_match(url))
    }

    fn transform(&self, mut url: String, _config: &Config) -> String {
        if self.exps[1].is_match(&url) {
            url = url.replacen("/blob/", "/raw/", 1);
        }
        if !url.starts_with("http") {
            let domains = ["github.com", "raw.github", "gist.github", "api.github"];
            if domains.iter().any(|d| url.starts_with(d)) {
                url = format!("https://{}", url);
            }
        }
        url
    }

    fn upstream_host(&self, path: &str, _config: &Config) -> Option<String> {
        if self.exps[3].is_match(path) {
            return Some("raw.githubusercontent.com".to_string());
        }
        if self.exps[4].is_match(path) {
            return Some("gist.githubusercontent.com".to_string());
        }
        if self.exps[5..9].iter().any(|e| e.is_match(path)) {
            return Some("api.github.com".to_string());
        }
        Some("github.com".to_string())
    }

    fn extract_keywords(&self, url: &str) -> Option<Vec<String>> {
        for exp in &self.exps {
            if let Some(caps) = exp.captures(url) {
                let mut matches = Vec::new();
                for i in 1..caps.len() {
                    let m = caps.get(i).map_or("", |m| m.as_str());
                    if !m.is_empty() {
                        matches.push(m.to_string());
                    }
                }
                if !matches.is_empty() {
                    return Some(matches);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Config;

    #[test]
    fn test_github_matches() {
        let provider = GithubProvider::new();
        assert!(provider.matches("github.com/torvalds/linux"));
        assert!(provider.matches("https://github.com/torvalds/linux/releases"));
        assert!(provider.matches("raw.githubusercontent.com/user/repo/main/file"));
    }

    #[test]
    fn test_github_transform() {
        let provider = GithubProvider::new();
        let config = Config::default();

        let url = "github.com/user/repo/blob/main/README.md".to_string();
        let result = provider.transform(url, &config);
        assert_eq!(result, "https://github.com/user/repo/raw/main/README.md");
    }

    #[test]
    fn test_github_keywords() {
        let provider = GithubProvider::new();
        let keywords = provider.extract_keywords("https://github.com/oopsunix/ghproxy").unwrap();
        assert!(keywords.contains(&"oopsunix".to_string()));
    }
}
