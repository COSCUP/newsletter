use async_trait::async_trait;
use serde::Deserialize;

#[async_trait]
pub trait CaptchaVerifier: Send + Sync {
    async fn verify(&self, token: &str) -> Result<bool, CaptchaError>;
}

#[derive(Debug, thiserror::Error)]
pub enum CaptchaError {
    #[error("Captcha verification request failed: {0}")]
    RequestFailed(String),
}

pub struct TurnstileVerifier {
    secret: String,
    client: reqwest::Client,
}

impl TurnstileVerifier {
    pub fn new(secret: String) -> Self {
        Self {
            secret,
            client: reqwest::Client::new(),
        }
    }
}

#[derive(Deserialize)]
struct TurnstileResponse {
    success: bool,
}

#[async_trait]
impl CaptchaVerifier for TurnstileVerifier {
    async fn verify(&self, token: &str) -> Result<bool, CaptchaError> {
        let resp = self
            .client
            .post("https://challenges.cloudflare.com/turnstile/v0/siteverify")
            .form(&[("response", token), ("secret", &self.secret)])
            .send()
            .await
            .map_err(|e| CaptchaError::RequestFailed(e.to_string()))?
            .json::<TurnstileResponse>()
            .await
            .map_err(|e| CaptchaError::RequestFailed(e.to_string()))?;

        Ok(resp.success)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub struct MockCaptchaVerifier {
        pub should_pass: bool,
    }

    #[async_trait]
    impl CaptchaVerifier for MockCaptchaVerifier {
        async fn verify(&self, _token: &str) -> Result<bool, CaptchaError> {
            Ok(self.should_pass)
        }
    }

    #[tokio::test]
    async fn test_mock_captcha_pass() {
        let v = MockCaptchaVerifier { should_pass: true };
        assert!(v.verify("any").await.unwrap());
    }

    #[tokio::test]
    async fn test_mock_captcha_fail() {
        let v = MockCaptchaVerifier { should_pass: false };
        assert!(!v.verify("any").await.unwrap());
    }
}
