use std::net::SocketAddr;

use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::response::Html;
use axum::{extract::Path, Form};
use chrono::Utc;
use serde::Deserialize;

use crate::error::AppError;
use crate::security;
use crate::AppState;

#[derive(Deserialize)]
pub struct SubscribeForm {
    pub email: String,
    pub name: String,
    #[serde(rename = "cf-turnstile-response")]
    pub captcha_response: String,
}

pub async fn subscribe_page(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let mut ctx = tera::Context::new();
    ctx.insert("turnstile_sitekey", &state.config.turnstile_sitekey);
    let html = state.tera.render("subscribe.html", &ctx)?;
    Ok(Html(html))
}

#[allow(clippy::too_many_lines)]
pub async fn subscribe_api(
    State(state): State<AppState>,
    connect_info: ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<SubscribeForm>,
) -> Result<Html<String>, AppError> {
    let email = form.email.trim().to_lowercase();
    let name = form.name.trim().to_string();

    if email.is_empty() {
        return Err(AppError::BadRequest("Email is required".to_string()));
    }

    // Verify captcha
    let captcha_ok = state
        .captcha
        .verify(&form.captcha_response)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    if !captcha_ok {
        return Err(AppError::BadRequest(
            "Captcha verification failed".to_string(),
        ));
    }

    // Rate limiting
    let client_ip = super::extract_client_ip(&headers, &connect_info);
    let ip_str = client_ip.to_string();

    let email_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM subscribe_email_log WHERE email = $1 AND created_at > NOW() - INTERVAL '24 hours'",
    )
    .bind(&email)
    .fetch_one(&state.db)
    .await?;

    if email_count >= 5 {
        return Err(AppError::RateLimitExceeded);
    }

    let ip_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM subscribe_email_log WHERE ip_address = $1::inet AND created_at > NOW() - INTERVAL '24 hours'",
    )
    .bind(&ip_str)
    .fetch_one(&state.db)
    .await?;

    if ip_count >= 10 {
        return Err(AppError::RateLimitExceeded);
    }

    // Check if already exists
    let existing =
        sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM subscribers WHERE email = $1")
            .bind(&email)
            .fetch_optional(&state.db)
            .await?;

    if existing.is_some() {
        // Send management URL to the existing subscriber
        let row = sqlx::query_as::<_, (String, String)>(
            "SELECT secret_code, email FROM subscribers WHERE email = $1",
        )
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;

        if let Some((secret_code, subscriber_email)) = row {
            let admin_link = security::compute_admin_link(&secret_code, &subscriber_email);
            let manage_url = format!("{}/manage/{}", state.config.base_url, admin_link);

            let logo_url = format!("{}/static/coscup-logo.svg", state.config.base_url);
            let mut email_ctx = tera::Context::new();
            email_ctx.insert("manage_url", &manage_url);
            email_ctx.insert("logo_url", &logo_url);
            let email_html = state
                .tera
                .render("emails/already_subscribed.html", &email_ctx)?;

            if let Err(e) = state
                .email
                .send_email(
                    &subscriber_email,
                    "COSCUP Newsletter - 您的訂閱管理連結",
                    &email_html,
                )
                .await
            {
                tracing::error!("Failed to send manage URL email: {e}");
            }
        }

        // Log the email sending event
        sqlx::query("INSERT INTO subscribe_email_log (email, ip_address) VALUES ($1, $2::inet)")
            .bind(&email)
            .bind(&ip_str)
            .execute(&state.db)
            .await?;

        let mut ctx = tera::Context::new();
        ctx.insert("message", "請檢查您的信箱以完成訂閱流程。");
        let html = state.tera.render("verify_success.html", &ctx)?;
        return Ok(Html(html));
    }

    // Create subscriber
    let secret_code = security::generate_secret_code();
    let ucode = security::generate_ucode();

    sqlx::query(
        "INSERT INTO subscribers (email, name, secret_code, ucode, subscription_source) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(&email)
    .bind(&name)
    .bind(&secret_code)
    .bind(&ucode)
    .bind("web")
    .execute(&state.db)
    .await?;

    let subscriber_id =
        sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM subscribers WHERE email = $1")
            .bind(&email)
            .fetch_one(&state.db)
            .await?;

    // Create verification token
    let token = security::generate_token();
    let expires_at = Utc::now() + chrono::Duration::hours(24);

    sqlx::query(
        "INSERT INTO verification_tokens (subscriber_id, token, token_type, expires_at) VALUES ($1, $2, 'email_verify', $3)",
    )
    .bind(subscriber_id)
    .bind(&token)
    .bind(expires_at)
    .execute(&state.db)
    .await?;

    // Send verification email
    let verify_url = format!("{}/verify/{}", state.config.base_url, token);
    let logo_url = format!("{}/static/coscup-logo.svg", state.config.base_url);
    let mut email_ctx = tera::Context::new();
    email_ctx.insert("verify_url", &verify_url);
    email_ctx.insert("name", &name);
    email_ctx.insert("logo_url", &logo_url);
    let email_html = state.tera.render("emails/verification.html", &email_ctx)?;

    if let Err(e) = state
        .email
        .send_email(&email, "COSCUP Newsletter - 驗證您的 Email", &email_html)
        .await
    {
        tracing::error!("Failed to send verification email: {e}");
    }

    // Log the email sending event
    sqlx::query("INSERT INTO subscribe_email_log (email, ip_address) VALUES ($1, $2::inet)")
        .bind(&email)
        .bind(&ip_str)
        .execute(&state.db)
        .await?;

    let mut ctx = tera::Context::new();
    ctx.insert("message", "請檢查您的信箱以完成訂閱流程。");
    let html = state.tera.render("verify_success.html", &ctx)?;
    Ok(Html(html))
}

fn render_link_error(
    state: &AppState,
    title: &str,
    message: &str,
    hint: Option<&str>,
) -> Result<Html<String>, AppError> {
    let mut ctx = tera::Context::new();
    ctx.insert("title", title);
    ctx.insert("message", message);
    if let Some(h) = hint {
        ctx.insert("hint", h);
    }
    let html = state.tera.render("error.html", &ctx)?;
    Ok(Html(html))
}

pub async fn verify_email(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Html<String>, AppError> {
    let now = Utc::now();

    // Find valid token
    let row = sqlx::query_as::<_, (uuid::Uuid, uuid::Uuid)>(
        "SELECT vt.id, vt.subscriber_id FROM verification_tokens vt \
         WHERE vt.token = $1 AND vt.token_type = 'email_verify' \
         AND vt.expires_at > $2 AND vt.used_at IS NULL",
    )
    .bind(&token)
    .bind(now)
    .fetch_optional(&state.db)
    .await?;

    let Some((token_id, subscriber_id)) = row else {
        return render_link_error(
            &state,
            "驗證連結已失效",
            "此驗證連結已過期或已被使用，無法再次驗證。",
            Some("如需重新驗證，請重新訂閱電子報，系統將會寄送新的驗證信。"),
        );
    };

    // Mark token as used
    sqlx::query("UPDATE verification_tokens SET used_at = $1 WHERE id = $2")
        .bind(now)
        .bind(token_id)
        .execute(&state.db)
        .await?;

    // Activate subscriber
    sqlx::query(
        "UPDATE subscribers SET verified_email = true, status = true, updated_at = $1 WHERE id = $2",
    )
    .bind(now)
    .bind(subscriber_id)
    .execute(&state.db)
    .await?;

    // Get admin_link for the user
    let (secret_code, email) = sqlx::query_as::<_, (String, String)>(
        "SELECT secret_code, email FROM subscribers WHERE id = $1",
    )
    .bind(subscriber_id)
    .fetch_one(&state.db)
    .await?;

    let admin_link = security::compute_admin_link(&secret_code, &email);
    let manage_url = format!("{}/manage/{}", state.config.base_url, admin_link);

    let mut ctx = tera::Context::new();
    ctx.insert("manage_url", &manage_url);
    ctx.insert("message", "您的 Email 已成功驗證！");
    let html = state.tera.render("verify_success.html", &ctx)?;
    Ok(Html(html))
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn test_extract_client_ip_from_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("1.2.3.4, 5.6.7.8"),
        );
        let connect_info = ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12345)));

        let ip = super::super::extract_client_ip(&headers, &connect_info);
        assert_eq!(ip, "1.2.3.4".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn test_extract_client_ip_single_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("10.0.0.1"));
        let connect_info = ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12345)));

        let ip = super::super::extract_client_ip(&headers, &connect_info);
        assert_eq!(ip, "10.0.0.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn test_extract_client_ip_fallback_to_connect_info() {
        let headers = HeaderMap::new();
        let connect_info = ConnectInfo(SocketAddr::from(([192, 168, 1, 1], 54321)));

        let ip = super::super::extract_client_ip(&headers, &connect_info);
        assert_eq!(ip, "192.168.1.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn test_extract_client_ip_invalid_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("not-an-ip"));
        let connect_info = ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12345)));

        let ip = super::super::extract_client_ip(&headers, &connect_info);
        assert_eq!(ip, "127.0.0.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn test_extract_client_ip_ipv6_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("::1"));
        let connect_info = ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12345)));

        let ip = super::super::extract_client_ip(&headers, &connect_info);
        assert_eq!(ip, "::1".parse::<IpAddr>().unwrap());
    }
}
