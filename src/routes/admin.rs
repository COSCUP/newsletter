use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Multipart, Path, Query, State};
use axum::http::{header, HeaderMap};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::SameSite;
use axum_extra::extract::CookieJar;
use chrono::Utc;
use serde::Deserialize;

use crate::auth::{AdminUser, SESSION_COOKIE};
use crate::csv_handler::{self, ExportCsvRecord};
use crate::error::AppError;
use crate::security;
use crate::AppState;

// --- Login ---

pub async fn login_page(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let ctx = tera::Context::new();
    let html = state.tera.render("admin/login.html", &ctx)?;
    Ok(Html(html))
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub email: String,
}

pub async fn login_submit(
    State(state): State<AppState>,
    connect_info: ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::Form(form): axum::Form<LoginForm>,
) -> Result<Html<String>, AppError> {
    let email = form.email.trim().to_lowercase();
    let client_ip = super::extract_client_ip(&headers, &connect_info);
    let ip_str = client_ip.to_string();

    // Rate limiting: same limits as subscribe (email: 5/24h, IP: 10/24h)
    let email_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM admin_login_log WHERE email = $1 AND created_at > NOW() - INTERVAL '24 hours'",
    )
    .bind(&email)
    .fetch_one(&state.db)
    .await?;

    if email_count >= 5 {
        return Err(AppError::RateLimitExceeded);
    }

    let ip_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM admin_login_log WHERE ip_address = $1::inet AND created_at > NOW() - INTERVAL '24 hours'",
    )
    .bind(&ip_str)
    .fetch_one(&state.db)
    .await?;

    if ip_count >= 10 {
        return Err(AppError::RateLimitExceeded);
    }

    // Always show success to prevent email enumeration
    let mut ctx = tera::Context::new();
    ctx.insert("message", "如果此 Email 有管理權限，您將收到一封登入連結。");

    let is_admin: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM admins WHERE email = $1)")
        .bind(&email)
        .fetch_one(&state.db)
        .await?;

    // Log unconditionally (before checking is_admin) so rate limit applies to all attempts
    sqlx::query("INSERT INTO admin_login_log (email, ip_address) VALUES ($1, $2::inet)")
        .bind(&email)
        .bind(&ip_str)
        .execute(&state.db)
        .await?;

    if is_admin {
        let token = security::generate_token();
        let expires_at = Utc::now() + chrono::Duration::minutes(15);

        sqlx::query(
            "INSERT INTO verification_tokens (admin_email, token, token_type, expires_at) VALUES ($1, $2, 'magic_link', $3)",
        )
        .bind(&email)
        .bind(&token)
        .bind(expires_at)
        .execute(&state.db)
        .await?;

        let link = format!("{}/admin/auth/{}", state.config.base_url, token);
        let logo_url = format!("{}/static/coscup-logo.svg", state.config.base_url);
        let mut email_ctx = tera::Context::new();
        email_ctx.insert("magic_link", &link);
        email_ctx.insert("logo_url", &logo_url);
        let email_html = state.tera.render("emails/magic_link.html", &email_ctx)?;

        if let Err(e) = state
            .email
            .send_email(&email, "COSCUP Newsletter Admin - 登入連結", &email_html)
            .await
        {
            tracing::error!("Failed to send magic link: {e}");
        }
    }

    let html = state.tera.render("admin/login.html", &ctx)?;
    Ok(Html(html))
}

pub async fn auth_magic_link(
    State(state): State<AppState>,
    jar: CookieJar,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(token): Path<String>,
) -> Result<(CookieJar, Redirect), AppError> {
    let now = Utc::now();

    let row = sqlx::query_as::<_, (uuid::Uuid, String)>(
        "SELECT id, admin_email FROM verification_tokens \
         WHERE token = $1 AND token_type = 'magic_link' \
         AND expires_at > $2 AND used_at IS NULL",
    )
    .bind(&token)
    .bind(now)
    .fetch_optional(&state.db)
    .await?;

    let Some((token_id, admin_email)) = row else {
        return Err(AppError::NotFound);
    };

    // Mark token as used
    sqlx::query("UPDATE verification_tokens SET used_at = $1 WHERE id = $2")
        .bind(now)
        .bind(token_id)
        .execute(&state.db)
        .await?;

    // Create session
    let session_token = security::generate_token();
    let session_expires = now + chrono::Duration::hours(24);

    sqlx::query(
        "INSERT INTO admin_sessions (admin_email, session_token, expires_at) VALUES ($1, $2, $3)",
    )
    .bind(&admin_email)
    .bind(&session_token)
    .bind(session_expires)
    .execute(&state.db)
    .await?;

    let client_ip = super::extract_client_ip(&headers, &ConnectInfo(addr));
    crate::audit::log(
        &state.db,
        &admin_email,
        "admin.login",
        None,
        Some(client_ip),
    )
    .await;

    let is_https = state.config.base_url.starts_with("https://");
    let cookie = axum_extra::extract::cookie::Cookie::build((SESSION_COOKIE, session_token))
        .path("/admin")
        .http_only(true)
        .secure(is_https)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::hours(24))
        .build();

    Ok((jar.add(cookie), Redirect::to("/admin")))
}

// --- Dashboard ---

pub async fn dashboard(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
) -> Result<Html<String>, AppError> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM subscribers")
        .fetch_one(&state.db)
        .await?;
    let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM subscribers WHERE status = true")
        .fetch_one(&state.db)
        .await?;
    let verified: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM subscribers WHERE verified_email = true")
            .fetch_one(&state.db)
            .await?;

    let mut ctx = tera::Context::new();
    ctx.insert("admin_email", &admin_email);
    ctx.insert("total", &total);
    ctx.insert("active", &active);
    ctx.insert("verified", &verified);
    let html = state.tera.render("admin/dashboard.html", &ctx)?;
    Ok(Html(html))
}

// --- Subscribers list ---

#[derive(Deserialize)]
pub struct PaginationQuery {
    pub page: Option<i64>,
    pub search: Option<String>,
}

pub async fn subscribers_list(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    Query(query): Query<PaginationQuery>,
) -> Result<Html<String>, AppError> {
    let page = query.page.unwrap_or(1).max(1);
    let per_page: i64 = 50;
    let offset = (page - 1) * per_page;

    let search_pattern = query
        .search
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{s}%"));

    let (rows, total): (Vec<_>, i64) = if let Some(ref pattern) = search_pattern {
        let rows = sqlx::query_as::<_, (uuid::Uuid, String, String, bool, bool, String)>(
            "SELECT id, email, name, status, verified_email, ucode FROM subscribers \
             WHERE email ILIKE $1 OR name ILIKE $1 \
             ORDER BY created_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(pattern)
        .bind(per_page)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM subscribers WHERE email ILIKE $1 OR name ILIKE $1",
        )
        .bind(pattern)
        .fetch_one(&state.db)
        .await?;

        (rows, total)
    } else {
        let rows = sqlx::query_as::<_, (uuid::Uuid, String, String, bool, bool, String)>(
            "SELECT id, email, name, status, verified_email, ucode FROM subscribers \
             ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(per_page)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;

        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM subscribers")
            .fetch_one(&state.db)
            .await?;

        (rows, total)
    };

    let total_pages = (total + per_page - 1) / per_page;

    let subscribers: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(id, email, name, status, verified_email, ucode)| {
            serde_json::json!({
                "id": id.to_string(),
                "email": email,
                "name": name,
                "status": status,
                "verified_email": verified_email,
                "ucode": ucode,
            })
        })
        .collect();

    let mut ctx = tera::Context::new();
    ctx.insert("admin_email", &admin_email);
    ctx.insert("subscribers", &subscribers);
    ctx.insert("page", &page);
    ctx.insert("total_pages", &total_pages);
    ctx.insert("total", &total);
    ctx.insert("search", &query.search.unwrap_or_default());
    let html = state.tera.render("admin/subscribers.html", &ctx)?;
    Ok(Html(html))
}

// --- Toggle status ---

pub async fn toggle_status(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<uuid::Uuid>,
) -> Result<Redirect, AppError> {
    let now = Utc::now();

    sqlx::query("UPDATE subscribers SET status = NOT status, updated_at = $1 WHERE id = $2")
        .bind(now)
        .bind(id)
        .execute(&state.db)
        .await?;

    let client_ip = super::extract_client_ip(&headers, &ConnectInfo(addr));
    crate::audit::log(
        &state.db,
        &admin_email,
        "subscriber.toggle",
        Some(serde_json::json!({ "subscriber_id": id.to_string() })),
        Some(client_ip),
    )
    .await;

    Ok(Redirect::to("/admin/subscribers"))
}

// --- Resend verification ---

pub async fn resend_verification(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<uuid::Uuid>,
) -> Result<Redirect, AppError> {
    let row =
        sqlx::query_as::<_, (String, String)>("SELECT email, name FROM subscribers WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::NotFound)?;

    let (email, name) = row;
    let token = security::generate_token();
    let expires_at = Utc::now() + chrono::Duration::hours(24);

    sqlx::query(
        "INSERT INTO verification_tokens (subscriber_id, token, token_type, expires_at) VALUES ($1, $2, 'email_verify', $3)",
    )
    .bind(id)
    .bind(&token)
    .bind(expires_at)
    .execute(&state.db)
    .await?;

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

    let client_ip = super::extract_client_ip(&headers, &ConnectInfo(addr));
    crate::audit::log(
        &state.db,
        &admin_email,
        "subscriber.resend",
        Some(serde_json::json!({ "subscriber_id": id.to_string() })),
        Some(client_ip),
    )
    .await;

    Ok(Redirect::to("/admin/subscribers"))
}

// --- CSV Import ---

pub async fn import_csv(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Redirect, AppError> {
    let mut csv_data = String::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?
    {
        if field.name() == Some("file") {
            csv_data = field
                .text()
                .await
                .map_err(|e| AppError::BadRequest(e.to_string()))?;
        }
    }

    if csv_data.is_empty() {
        return Err(AppError::BadRequest("No CSV data provided".to_string()));
    }

    let records = csv_handler::parse_legacy_csv(&csv_data)
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    for record in &records {
        let secret_code = security::generate_secret_code();
        let status = record.status == "1";
        let verified_email = record.verified_email == "1";

        let result = sqlx::query(
            "INSERT INTO subscribers (email, name, secret_code, ucode, legacy_admin_link, status, verified_email, subscription_source) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, 'import') \
             ON CONFLICT (email) DO NOTHING",
        )
        .bind(&record.clean_mail)
        .bind(&record.name)
        .bind(&secret_code)
        .bind(&record.ucode)
        .bind(&record.admin_link)
        .bind(status)
        .bind(verified_email)
        .execute(&state.db)
        .await;

        if let Err(e) = result {
            tracing::warn!("Failed to import record {}: {e}", record.clean_mail);
        }
    }

    let client_ip = super::extract_client_ip(&headers, &ConnectInfo(addr));
    crate::audit::log(
        &state.db,
        &admin_email,
        "subscriber.import",
        Some(serde_json::json!({ "count": records.len() })),
        Some(client_ip),
    )
    .await;

    Ok(Redirect::to("/admin/subscribers"))
}

// --- CSV Export ---

pub async fn export_csv(State(state): State<AppState>) -> Result<Response, AppError> {
    let rows = sqlx::query_as::<_, (String, String, String, bool, String)>(
        "SELECT email, name, ucode, status, secret_code FROM subscribers ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    let records: Vec<ExportCsvRecord> = rows
        .into_iter()
        .map(|(email, name, ucode, status, secret_code)| {
            let admin_link = security::compute_admin_link(&secret_code, &email);
            let openhash = security::compute_openhash(&secret_code, &ucode, "", "");
            ExportCsvRecord {
                email,
                name,
                ucode,
                status,
                admin_link,
                openhash,
            }
        })
        .collect();

    let csv_data =
        csv_handler::write_export_csv(&records).map_err(|e| AppError::Internal(e.to_string()))?;

    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"subscribers.csv\"",
            ),
        ],
        csv_data,
    )
        .into_response())
}

// --- Stats ---

pub async fn stats_page(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
) -> Result<Html<String>, AppError> {
    // Per-newsletter aggregated stats
    let newsletter_stats = sqlx::query_as::<_, (uuid::Uuid, String, String, i32, i32)>(
        "SELECT id, title, slug, sent_count, total_count FROM newsletters \
         WHERE status IN ('sent', 'sending') ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    let mut stats_rows: Vec<serde_json::Value> = Vec::new();
    for (id, title, slug, sent_count, _total_count) in &newsletter_stats {
        let unique_opens: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT ucode) FROM email_events WHERE topic = $1 AND event_type = 'open'",
        )
        .bind(slug)
        .fetch_one(&state.db)
        .await?;

        #[allow(clippy::cast_precision_loss)]
        let open_rate = if *sent_count > 0 {
            format!(
                "{:.1}%",
                (unique_opens as f64 / f64::from(*sent_count)) * 100.0
            )
        } else {
            "—".to_string()
        };

        stats_rows.push(serde_json::json!({
            "id": id.to_string(),
            "title": title,
            "sent_count": sent_count,
            "unique_opens": unique_opens,
            "open_rate": open_rate,
        }));
    }

    // Legacy topic-based stats (for events not linked to a newsletter)
    let topic_stats = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT topic, event_type, COUNT(*) as count FROM email_events \
         GROUP BY topic, event_type ORDER BY topic, event_type",
    )
    .fetch_all(&state.db)
    .await?;

    let legacy_stats: Vec<serde_json::Value> = topic_stats
        .into_iter()
        .map(|(topic, event_type, count)| {
            serde_json::json!({
                "topic": topic,
                "event_type": event_type,
                "count": count,
            })
        })
        .collect();

    let mut ctx = tera::Context::new();
    ctx.insert("admin_email", &admin_email);
    ctx.insert("newsletter_stats", &stats_rows);
    ctx.insert("stats", &legacy_stats);
    let html = state.tera.render("admin/stats.html", &ctx)?;
    Ok(Html(html))
}

// --- Logout ---

pub async fn logout(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<(CookieJar, Redirect), AppError> {
    let client_ip = super::extract_client_ip(&headers, &ConnectInfo(addr));
    crate::audit::log(
        &state.db,
        &admin_email,
        "admin.logout",
        None,
        Some(client_ip),
    )
    .await;

    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        let _ = sqlx::query("DELETE FROM admin_sessions WHERE session_token = $1")
            .bind(cookie.value())
            .execute(&state.db)
            .await;
    }

    let removal = axum_extra::extract::cookie::Cookie::build((SESSION_COOKIE, ""))
        .path("/admin")
        .max_age(time::Duration::ZERO)
        .build();

    Ok((jar.remove(removal), Redirect::to("/admin/login")))
}
