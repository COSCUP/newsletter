use async_trait::async_trait;

#[async_trait]
pub trait EmailService: Send + Sync {
    async fn send_email(&self, to: &str, subject: &str, html_body: &str) -> Result<(), EmailError>;
}

#[derive(Debug, thiserror::Error)]
pub enum EmailError {
    #[error("Failed to send email: {0}")]
    SendFailed(String),
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
}

#[async_trait]
impl EmailService for SmtpEmailService {
    async fn send_email(&self, to: &str, subject: &str, html_body: &str) -> Result<(), EmailError> {
        use lettre::message::header::ContentType;
        use lettre::{AsyncTransport, Message};

        let email = Message::builder()
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
            .header(ContentType::TEXT_HTML)
            .body(html_body.to_string())
            .map_err(|e| EmailError::SendFailed(e.to_string()))?;

        self.transport
            .send(email)
            .await
            .map_err(|e| EmailError::SendFailed(e.to_string()))?;

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
}
