use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum ShortUrlError {
    #[error("Failed to shorten URL: {0}")]
    ShortenFailed(String),

    #[error("Failed to get click stats: {0}")]
    StatsFailed(String),
}

#[async_trait]
pub trait ShortUrlService: Send + Sync {
    async fn shorten(&self, url: &str) -> Result<String, ShortUrlError>;
    async fn get_clicks(&self, short_url: &str) -> Result<u64, ShortUrlError>;
}

// --- YOURLS implementation ---

pub struct YourlsService {
    api_url: String,
    signature: String,
    client: reqwest::Client,
    cache: Mutex<HashMap<String, String>>,
}

impl YourlsService {
    pub fn new(api_url: String, signature: String) -> Self {
        Self {
            api_url,
            signature,
            client: reqwest::Client::new(),
            cache: Mutex::new(HashMap::new()),
        }
    }
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct YourlsShortenResponse {
    shorturl: Option<String>,
    status: Option<String>,
    message: Option<String>,
    url: Option<YourlsUrlInfo>,
}

#[derive(Deserialize)]
struct YourlsUrlInfo {
    #[serde(default)]
    shorturl: Option<String>,
}

#[derive(Deserialize)]
#[allow(dead_code, non_snake_case)]
struct YourlsStatsResponse {
    link: Option<YourlsLinkStats>,
    statusCode: Option<u16>,
    message: Option<String>,
}

#[derive(Deserialize)]
struct YourlsLinkStats {
    clicks: Option<String>,
}

#[async_trait]
impl ShortUrlService for YourlsService {
    async fn shorten(&self, url: &str) -> Result<String, ShortUrlError> {
        // Check cache first
        if let Ok(cache) = self.cache.lock() {
            if let Some(cached) = cache.get(url) {
                return Ok(cached.clone());
            }
        }

        let resp = self
            .client
            .post(&self.api_url)
            .form(&[
                ("action", "shorturl"),
                ("url", url),
                ("format", "json"),
                ("signature", &self.signature),
            ])
            .send()
            .await
            .map_err(|e| ShortUrlError::ShortenFailed(e.to_string()))?;

        let body = resp
            .json::<YourlsShortenResponse>()
            .await
            .map_err(|e| ShortUrlError::ShortenFailed(e.to_string()))?;

        let short_url = body
            .shorturl
            .or_else(|| body.url.and_then(|u| u.shorturl))
            .ok_or_else(|| {
                ShortUrlError::ShortenFailed(
                    body.message
                        .unwrap_or_else(|| "No short URL returned".to_string()),
                )
            })?;

        // Update cache
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(url.to_string(), short_url.clone());
        }

        Ok(short_url)
    }

    async fn get_clicks(&self, short_url: &str) -> Result<u64, ShortUrlError> {
        let resp = self
            .client
            .get(&self.api_url)
            .query(&[
                ("action", "url-stats"),
                ("shorturl", short_url),
                ("format", "json"),
                ("signature", &self.signature),
            ])
            .send()
            .await
            .map_err(|e| ShortUrlError::StatsFailed(e.to_string()))?;

        let body = resp
            .json::<YourlsStatsResponse>()
            .await
            .map_err(|e| ShortUrlError::StatsFailed(e.to_string()))?;

        let clicks = body
            .link
            .and_then(|l| l.clicks)
            .and_then(|c| c.parse::<u64>().ok())
            .unwrap_or(0);

        Ok(clicks)
    }
}

// --- Mock implementation for testing ---

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::Arc;

    #[derive(Default)]
    pub struct MockShortUrlService {
        pub shorten_calls: Arc<Mutex<Vec<String>>>,
        pub should_fail: bool,
    }

    #[async_trait]
    impl ShortUrlService for MockShortUrlService {
        async fn shorten(&self, url: &str) -> Result<String, ShortUrlError> {
            if self.should_fail {
                return Err(ShortUrlError::ShortenFailed("mock failure".to_string()));
            }
            if let Ok(mut calls) = self.shorten_calls.lock() {
                calls.push(url.to_string());
            }
            // Return a fake short URL
            Ok(format!("https://s.coscup.org/test_{}", url.len()))
        }

        async fn get_clicks(&self, _short_url: &str) -> Result<u64, ShortUrlError> {
            if self.should_fail {
                return Err(ShortUrlError::StatsFailed("mock failure".to_string()));
            }
            Ok(42)
        }
    }

    #[tokio::test]
    async fn test_mock_shorten() {
        let svc = MockShortUrlService::default();
        let result = svc.shorten("https://example.com").await.unwrap();
        assert!(result.starts_with("https://s.coscup.org/test_"));

        let calls = svc.shorten_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], "https://example.com");
    }

    #[tokio::test]
    async fn test_mock_get_clicks() {
        let svc = MockShortUrlService::default();
        let clicks = svc.get_clicks("https://s.coscup.org/abc").await.unwrap();
        assert_eq!(clicks, 42);
    }

    #[tokio::test]
    async fn test_mock_shorten_failure() {
        let svc = MockShortUrlService {
            should_fail: true,
            ..Default::default()
        };
        let result = svc.shorten("https://example.com").await;
        assert!(result.is_err());
    }
}
