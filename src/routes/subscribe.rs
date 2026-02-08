use axum::extract::State;
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

pub async fn subscribe_api(
    State(state): State<AppState>,
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

    // Check if already exists
    let existing =
        sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM subscribers WHERE email = $1")
            .bind(&email)
            .fetch_optional(&state.db)
            .await?;

    if existing.is_some() {
        // Don't reveal if email exists, just show success
        let mut ctx = tera::Context::new();
        ctx.insert("message", "請檢查您的信箱並點擊驗證連結。");
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
    let mut email_ctx = tera::Context::new();
    email_ctx.insert("verify_url", &verify_url);
    email_ctx.insert("name", &name);
    let email_html = state.tera.render("emails/verification.html", &email_ctx)?;

    if let Err(e) = state
        .email
        .send_email(&email, "COSCUP Newsletter - 驗證您的 Email", &email_html)
        .await
    {
        tracing::error!("Failed to send verification email: {e}");
    }

    let mut ctx = tera::Context::new();
    ctx.insert("message", "請檢查您的信箱並點擊驗證連結。");
    let html = state.tera.render("verify_success.html", &ctx)?;
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
        return Err(AppError::NotFound);
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
