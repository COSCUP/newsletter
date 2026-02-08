use axum::extract::{Path, State};
use axum::response::Html;
use axum::Form;
use chrono::Utc;
use serde::Deserialize;

use crate::error::AppError;
use crate::security;
use crate::AppState;

struct SubscriberRow {
    id: uuid::Uuid,
    email: String,
    name: String,
    status: bool,
}

async fn find_subscriber_by_admin_link(
    state: &AppState,
    admin_link: &str,
) -> Result<Option<SubscriberRow>, AppError> {
    // First try legacy_admin_link
    let row = sqlx::query_as::<_, (uuid::Uuid, String, String, bool)>(
        "SELECT id, email, name, status FROM subscribers WHERE legacy_admin_link = $1",
    )
    .bind(admin_link)
    .fetch_optional(&state.db)
    .await?;

    if let Some((id, email, name, status)) = row {
        return Ok(Some(SubscriberRow {
            id,
            email,
            name,
            status,
        }));
    }

    // Try computing admin_link for all subscribers
    let rows = sqlx::query_as::<_, (uuid::Uuid, String, String, bool, String)>(
        "SELECT id, email, name, status, secret_code FROM subscribers",
    )
    .fetch_all(&state.db)
    .await?;

    for (id, email, name, status, secret_code) in rows {
        let computed = security::compute_admin_link(&secret_code, &email);
        if security::verify_admin_link(admin_link, &computed) {
            return Ok(Some(SubscriberRow {
                id,
                email,
                name,
                status,
            }));
        }
    }

    Ok(None)
}

pub async fn manage_page(
    State(state): State<AppState>,
    Path(admin_link): Path<String>,
) -> Result<Html<String>, AppError> {
    let subscriber = find_subscriber_by_admin_link(&state, &admin_link)
        .await?
        .ok_or(AppError::NotFound)?;

    let mut ctx = tera::Context::new();
    ctx.insert("name", &subscriber.name);
    ctx.insert("email", &subscriber.email);
    ctx.insert("status", &subscriber.status);
    ctx.insert("admin_link", &admin_link);
    let html = state.tera.render("manage.html", &ctx)?;
    Ok(Html(html))
}

#[derive(Deserialize)]
pub struct UpdateNameForm {
    pub name: String,
}

pub async fn update_name(
    State(state): State<AppState>,
    Path(admin_link): Path<String>,
    Form(form): Form<UpdateNameForm>,
) -> Result<Html<String>, AppError> {
    let subscriber = find_subscriber_by_admin_link(&state, &admin_link)
        .await?
        .ok_or(AppError::NotFound)?;

    let name = form.name.trim().to_string();
    let now = Utc::now();

    sqlx::query("UPDATE subscribers SET name = $1, updated_at = $2 WHERE id = $3")
        .bind(&name)
        .bind(now)
        .bind(subscriber.id)
        .execute(&state.db)
        .await?;

    let mut ctx = tera::Context::new();
    ctx.insert("name", &name);
    ctx.insert("email", &subscriber.email);
    ctx.insert("status", &subscriber.status);
    ctx.insert("admin_link", &admin_link);
    ctx.insert("message", "名稱已更新！");
    let html = state.tera.render("manage.html", &ctx)?;
    Ok(Html(html))
}

pub async fn unsubscribe(
    State(state): State<AppState>,
    Path(admin_link): Path<String>,
) -> Result<Html<String>, AppError> {
    let subscriber = find_subscriber_by_admin_link(&state, &admin_link)
        .await?
        .ok_or(AppError::NotFound)?;

    let now = Utc::now();
    sqlx::query("UPDATE subscribers SET status = false, updated_at = $1 WHERE id = $2")
        .bind(now)
        .bind(subscriber.id)
        .execute(&state.db)
        .await?;

    let mut ctx = tera::Context::new();
    ctx.insert("name", &subscriber.name);
    ctx.insert("email", &subscriber.email);
    ctx.insert("status", &false);
    ctx.insert("admin_link", &admin_link);
    ctx.insert("message", "您已成功取消訂閱。");
    let html = state.tera.render("manage.html", &ctx)?;
    Ok(Html(html))
}
