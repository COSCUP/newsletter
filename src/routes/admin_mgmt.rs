use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{Html, Redirect};
use axum::Form;
use chrono::{FixedOffset, Utc};
use serde::Deserialize;

use crate::auth::AdminUser;
use crate::error::AppError;
use crate::AppState;

fn taiwan_offset() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).expect("valid offset")
}

// --- Admins list ---

pub async fn admins_list(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
) -> Result<Html<String>, AppError> {
    let rows = sqlx::query_as::<_, (uuid::Uuid, String, Option<String>, chrono::DateTime<Utc>)>(
        "SELECT id, email, added_by, created_at FROM admins ORDER BY created_at ASC",
    )
    .fetch_all(&state.db)
    .await?;

    let admins: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(id, email, added_by, created_at)| {
            serde_json::json!({
                "id": id.to_string(),
                "email": email,
                "added_by": added_by.unwrap_or_default(),
                "created_at": created_at.with_timezone(&taiwan_offset()).format("%Y-%m-%d %H:%M").to_string(),
            })
        })
        .collect();

    let admin_count = admins.len();

    let mut ctx = tera::Context::new();
    ctx.insert("admin_email", &admin_email);
    ctx.insert("admins", &admins);
    ctx.insert("admin_count", &admin_count);
    let html = state.tera.render("admin/admins.html", &ctx)?;
    Ok(Html(html))
}

// --- Add admin ---

#[derive(Deserialize)]
pub struct AddAdminForm {
    pub email: String,
}

pub async fn add_admin(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<AddAdminForm>,
) -> Result<Redirect, AppError> {
    let email = form.email.trim().to_lowercase();
    if email.is_empty() {
        return Err(AppError::BadRequest("Email is required".to_string()));
    }

    sqlx::query(
        "INSERT INTO admins (email, added_by) VALUES ($1, $2) ON CONFLICT (email) DO NOTHING",
    )
    .bind(&email)
    .bind(&admin_email)
    .execute(&state.db)
    .await?;

    let client_ip = super::extract_client_ip(&headers, &ConnectInfo(addr));
    crate::audit::log(
        &state.db,
        &admin_email,
        "admin.add",
        Some(serde_json::json!({ "added_email": email })),
        Some(client_ip),
    )
    .await;

    Ok(Redirect::to("/admin/admins"))
}

// --- Remove admin ---

pub async fn remove_admin(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<uuid::Uuid>,
) -> Result<Redirect, AppError> {
    // Get the email of the admin to remove
    let target_email = sqlx::query_scalar::<_, String>("SELECT email FROM admins WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    // Prevent removing self
    if target_email == admin_email {
        return Err(AppError::BadRequest("無法移除自己的管理員帳號".to_string()));
    }

    // Prevent removing the last admin
    let admin_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM admins")
        .fetch_one(&state.db)
        .await?;

    if admin_count <= 1 {
        return Err(AppError::BadRequest("無法移除最後一位管理員".to_string()));
    }

    sqlx::query("DELETE FROM admins WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    // Delete active sessions for this admin
    let _ = sqlx::query("DELETE FROM admin_sessions WHERE admin_email = $1")
        .bind(&target_email)
        .execute(&state.db)
        .await;

    let client_ip = super::extract_client_ip(&headers, &ConnectInfo(addr));
    crate::audit::log(
        &state.db,
        &admin_email,
        "admin.remove",
        Some(serde_json::json!({ "removed_email": target_email })),
        Some(client_ip),
    )
    .await;

    Ok(Redirect::to("/admin/admins"))
}

// --- Audit log page ---

#[derive(Deserialize)]
pub struct AuditLogQuery {
    pub page: Option<i64>,
    pub action: Option<String>,
}

#[allow(clippy::too_many_lines)]
pub async fn audit_log_page(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    Query(query): Query<AuditLogQuery>,
) -> Result<Html<String>, AppError> {
    let page = query.page.unwrap_or(1).max(1);
    let per_page: i64 = 50;
    let offset = (page - 1) * per_page;

    let action_filter = query
        .action
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(String::from);

    let (rows, total): (Vec<_>, i64) = if let Some(ref action) = action_filter {
        let rows = sqlx::query_as::<
            _,
            (
                String,
                String,
                Option<serde_json::Value>,
                Option<String>,
                chrono::DateTime<Utc>,
            ),
        >(
            "SELECT admin_email, action, details, ip_address, created_at \
             FROM audit_log WHERE action = $1 \
             ORDER BY created_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(action)
        .bind(per_page)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;

        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE action = $1")
            .bind(action)
            .fetch_one(&state.db)
            .await?;

        (rows, total)
    } else {
        let rows = sqlx::query_as::<
            _,
            (
                String,
                String,
                Option<serde_json::Value>,
                Option<String>,
                chrono::DateTime<Utc>,
            ),
        >(
            "SELECT admin_email, action, details, ip_address, created_at \
             FROM audit_log ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(per_page)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;

        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_log")
            .fetch_one(&state.db)
            .await?;

        (rows, total)
    };

    let total_pages = (total + per_page - 1) / per_page;

    let logs: Vec<serde_json::Value> = rows
        .into_iter()
        .map(
            |(log_admin_email, action, details, ip_address, created_at)| {
                serde_json::json!({
                    "admin_email": log_admin_email,
                    "action": action,
                    "details": details.map(|d| d.to_string()).unwrap_or_default(),
                    "ip_address": ip_address.unwrap_or_default(),
                    "created_at": created_at.with_timezone(&taiwan_offset()).format("%Y-%m-%d %H:%M:%S").to_string(),
                })
            },
        )
        .collect();

    let mut ctx = tera::Context::new();
    ctx.insert("admin_email", &admin_email);
    ctx.insert("logs", &logs);
    ctx.insert("page", &page);
    ctx.insert("total_pages", &total_pages);
    ctx.insert("total", &total);
    ctx.insert("action_filter", &action_filter.unwrap_or_default());
    let html = state.tera.render("admin/audit_log.html", &ctx)?;
    Ok(Html(html))
}
