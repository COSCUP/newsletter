use std::env;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub database_url: String,
    pub host: String,
    pub port: u16,
    pub base_url: String,
    pub admin_emails: Vec<String>,
    pub turnstile_secret: String,
    pub turnstile_sitekey: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: Option<String>,
    pub smtp_password: Option<String>,
    pub smtp_tls: bool,
    pub smtp_from_email: String,
    pub smtp_rate_limit_ms: u64,
    pub newsletter_scheduler_interval_secs: u64,
    pub yourls_api_url: Option<String>,
    pub yourls_signature: Option<String>,
    pub upload_dir: String,
    pub max_upload_size_bytes: usize,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, env::VarError> {
        let admin_emails_str = env::var("ADMIN_EMAILS")?;
        let admin_emails = admin_emails_str
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(Self {
            database_url: env::var("DATABASE_URL")?,
            host: env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env::var("PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()
                .unwrap_or(8080),
            base_url: env::var("BASE_URL")?,
            admin_emails,
            turnstile_secret: env::var("TURNSTILE_SECRET")?,
            turnstile_sitekey: env::var("TURNSTILE_SITEKEY")?,
            smtp_host: env::var("SMTP_HOST").unwrap_or_else(|_| "localhost".to_string()),
            smtp_port: env::var("SMTP_PORT")
                .unwrap_or_else(|_| "1025".to_string())
                .parse()
                .unwrap_or(1025),
            smtp_username: env::var("SMTP_USERNAME").ok().filter(|s| !s.is_empty()),
            smtp_password: env::var("SMTP_PASSWORD").ok().filter(|s| !s.is_empty()),
            smtp_tls: env::var("SMTP_TLS")
                .unwrap_or_else(|_| "false".to_string())
                .parse()
                .unwrap_or(false),
            smtp_from_email: env::var("SMTP_FROM_EMAIL")
                .unwrap_or_else(|_| "newsletter@coscup.org".to_string()),
            smtp_rate_limit_ms: env::var("SMTP_RATE_LIMIT_MS")
                .unwrap_or_else(|_| "100".to_string())
                .parse()
                .unwrap_or(100),
            newsletter_scheduler_interval_secs: env::var("NEWSLETTER_SCHEDULER_INTERVAL_SECS")
                .unwrap_or_else(|_| "30".to_string())
                .parse()
                .unwrap_or(30),
            yourls_api_url: env::var("YOURLS_API_URL").ok().filter(|s| !s.is_empty()),
            yourls_signature: env::var("YOURLS_SIGNATURE").ok().filter(|s| !s.is_empty()),
            upload_dir: env::var("UPLOAD_DIR").unwrap_or_else(|_| "uploads".to_string()),
            max_upload_size_bytes: env::var("MAX_UPLOAD_SIZE_BYTES")
                .unwrap_or_else(|_| "5242880".to_string())
                .parse()
                .unwrap_or(5_242_880),
        })
    }

    pub fn is_admin_email(&self, email: &str) -> bool {
        self.admin_emails.contains(&email.to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_admin_email() {
        let config = AppConfig {
            database_url: String::new(),
            host: "0.0.0.0".to_string(),
            port: 8080,
            base_url: "http://localhost:8080".to_string(),
            admin_emails: vec!["admin@coscup.org".to_string()],
            turnstile_secret: String::new(),
            turnstile_sitekey: String::new(),
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: None,
            smtp_password: None,
            smtp_tls: false,
            smtp_from_email: "test@example.com".to_string(),
            smtp_rate_limit_ms: 100,
            newsletter_scheduler_interval_secs: 30,
            yourls_api_url: None,
            yourls_signature: None,
            upload_dir: "uploads".to_string(),
            max_upload_size_bytes: 5_242_880,
        };

        assert!(config.is_admin_email("admin@coscup.org"));
        assert!(config.is_admin_email("ADMIN@COSCUP.ORG"));
        assert!(!config.is_admin_email("other@coscup.org"));
    }
}
