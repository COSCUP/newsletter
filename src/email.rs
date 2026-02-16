use async_trait::async_trait;

/// Extra header to include in an email (name, value).
pub type EmailHeader = (String, String);

#[async_trait]
pub trait EmailService: Send + Sync {
    async fn send_email(&self, to: &str, subject: &str, html_body: &str) -> Result<(), EmailError>;

    async fn send_email_with_headers(
        &self,
        to: &str,
        subject: &str,
        html_body: &str,
        headers: &[EmailHeader],
    ) -> Result<(), EmailError> {
        // Default: ignore headers, just send
        let _ = headers;
        self.send_email(to, subject, html_body).await
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EmailError {
    #[error("Failed to send email: {0}")]
    SendFailed(String),

    #[error("Hard bounce (permanent SMTP error): {0}")]
    HardBounce(String),
}

impl EmailError {
    /// Returns true if this is a permanent delivery failure (5xx).
    pub fn is_hard_bounce(&self) -> bool {
        matches!(self, Self::HardBounce(_))
    }
}

pub struct SmtpEmailService {
    transport: lettre::AsyncSmtpTransport<lettre::Tokio1Executor>,
    from_email: String,
}

impl SmtpEmailService {
    pub fn new(
        host: &str,
        port: u16,
        username: Option<&str>,
        password: Option<&str>,
        use_tls: bool,
        from_email: String,
    ) -> Result<Self, EmailError> {
        use lettre::transport::smtp::authentication::Credentials;
        use lettre::AsyncSmtpTransport;

        let mut builder = if use_tls {
            AsyncSmtpTransport::<lettre::Tokio1Executor>::relay(host)
                .map_err(|e| EmailError::SendFailed(e.to_string()))?
                .port(port)
        } else {
            AsyncSmtpTransport::<lettre::Tokio1Executor>::builder_dangerous(host).port(port)
        };

        if let (Some(user), Some(pass)) = (username, password) {
            builder = builder.credentials(Credentials::new(user.to_string(), pass.to_string()));
        }

        let transport = builder.build();
        Ok(Self {
            transport,
            from_email,
        })
    }

    fn build_message(
        &self,
        to: &str,
        subject: &str,
        html_body: &str,
        headers: &[EmailHeader],
    ) -> Result<lettre::Message, EmailError> {
        use lettre::message::header::{ContentType, HeaderName, HeaderValue};
        use lettre::Message;

        let mut builder = Message::builder()
            .from(
                self.from_email
                    .parse()
                    .map_err(|e: lettre::address::AddressError| {
                        EmailError::SendFailed(e.to_string())
                    })?,
            )
            .to(to.parse().map_err(|e: lettre::address::AddressError| {
                EmailError::SendFailed(e.to_string())
            })?)
            .subject(subject)
            .header(ContentType::TEXT_HTML);

        for (name, value) in headers {
            let header_name = HeaderName::new_from_ascii(name.clone())
                .map_err(|e| EmailError::SendFailed(format!("Invalid header name: {e}")))?;
            builder = builder.raw_header(HeaderValue::new(header_name, value.clone()));
        }

        builder
            .body(html_body.to_string())
            .map_err(|e| EmailError::SendFailed(e.to_string()))
    }
}

#[async_trait]
impl EmailService for SmtpEmailService {
    async fn send_email(&self, to: &str, subject: &str, html_body: &str) -> Result<(), EmailError> {
        let email = self.build_message(to, subject, html_body, &[])?;
        self.send_message(email).await
    }

    async fn send_email_with_headers(
        &self,
        to: &str,
        subject: &str,
        html_body: &str,
        headers: &[EmailHeader],
    ) -> Result<(), EmailError> {
        let email = self.build_message(to, subject, html_body, headers)?;
        self.send_message(email).await
    }
}

impl SmtpEmailService {
    async fn send_message(&self, email: lettre::Message) -> Result<(), EmailError> {
        use lettre::AsyncTransport;

        self.transport.send(email).await.map_err(|e| {
            if e.is_permanent() {
                EmailError::HardBounce(e.to_string())
            } else {
                EmailError::SendFailed(e.to_string())
            }
        })?;

        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    pub struct MockEmailService {
        pub sent_emails: Arc<Mutex<Vec<(String, String, String)>>>,
    }

    #[async_trait]
    impl EmailService for MockEmailService {
        async fn send_email(
            &self,
            to: &str,
            subject: &str,
            html_body: &str,
        ) -> Result<(), EmailError> {
            self.sent_emails.lock().unwrap().push((
                to.to_string(),
                subject.to_string(),
                html_body.to_string(),
            ));
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_mock_email_service() {
        let svc = MockEmailService::default();
        svc.send_email("test@example.com", "Subject", "<p>Body</p>")
            .await
            .unwrap();

        let sent = svc.sent_emails.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, "test@example.com");
    }

    #[test]
    fn test_hard_bounce_detection() {
        let hard = EmailError::HardBounce("550 User not found".to_string());
        assert!(hard.is_hard_bounce());

        let soft = EmailError::SendFailed("connection timeout".to_string());
        assert!(!soft.is_hard_bounce());
    }
}
