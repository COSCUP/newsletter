use axum::extract::{Path, Query, State};
use axum::response::Html;
use axum::Form;
use chrono::Utc;
use serde::Deserialize;

use crate::error::AppError;
use crate::security;
use crate::AppState;

#[derive(Deserialize, Default)]
pub struct FromQuery {
    #[serde(default)]
    pub from: Option<String>,
}

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

const INVALID_LINK_TITLE: &str = "管理連結已失效";
const INVALID_LINK_MSG: &str = "此連結無效或找不到對應的訂閱記錄。";
const INVALID_LINK_HINT: &str =
    "如需管理訂閱，請使用信箱中最新的管理連結，或前往首頁重新訂閱以取得新的連結。";

pub async fn manage_page(
    State(state): State<AppState>,
    Path(admin_link): Path<String>,
    Query(query): Query<FromQuery>,
) -> Result<Html<String>, AppError> {
    let Some(subscriber) = find_subscriber_by_admin_link(&state, &admin_link).await? else {
        return render_link_error(
            &state,
            INVALID_LINK_TITLE,
            INVALID_LINK_MSG,
            Some(INVALID_LINK_HINT),
        );
    };

    let mut ctx = tera::Context::new();
    ctx.insert("name", &subscriber.name);
    ctx.insert("email", &subscriber.email);
    ctx.insert("status", &subscriber.status);
    ctx.insert("admin_link", &admin_link);
    ctx.insert("from_newsletter", &query.from.unwrap_or_default());
    let html = state.tera.render("manage.html", &ctx)?;
    Ok(Html(html))
}

#[derive(Deserialize)]
pub struct UpdateNameForm {
    pub name: String,
}

#[derive(Deserialize)]
pub struct UnsubscribeForm {
    #[serde(default)]
    pub from: Option<String>,
}

pub async fn update_name(
    State(state): State<AppState>,
    Path(admin_link): Path<String>,
    Form(form): Form<UpdateNameForm>,
) -> Result<Html<String>, AppError> {
    let Some(subscriber) = find_subscriber_by_admin_link(&state, &admin_link).await? else {
        return render_link_error(
            &state,
            INVALID_LINK_TITLE,
            INVALID_LINK_MSG,
            Some(INVALID_LINK_HINT),
        );
    };

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
    ctx.insert("from_newsletter", "");
    ctx.insert("message", "名稱已更新！");
    let html = state.tera.render("manage.html", &ctx)?;
    Ok(Html(html))
}

/// Look up a newsletter ID by its slug.
async fn lookup_newsletter_id(
    state: &AppState,
    slug: Option<&str>,
) -> Result<Option<uuid::Uuid>, AppError> {
    if let Some(slug) = slug {
        let id = sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM newsletters WHERE slug = $1")
            .bind(slug)
            .fetch_optional(&state.db)
            .await?;
        Ok(id)
    } else {
        Ok(None)
    }
}

/// Record an unsubscribe event linking the subscriber to the newsletter that triggered it.
async fn record_unsubscribe_event(
    state: &AppState,
    subscriber_id: uuid::Uuid,
    newsletter_id: Option<uuid::Uuid>,
) -> Result<(), AppError> {
    sqlx::query("INSERT INTO unsubscribe_events (subscriber_id, newsletter_id) VALUES ($1, $2)")
        .bind(subscriber_id)
        .bind(newsletter_id)
        .execute(&state.db)
        .await?;
    Ok(())
}

/// RFC 8058 one-click unsubscribe endpoint.
/// Email clients POST `List-Unsubscribe=One-Click` to this URL.
pub async fn one_click_unsubscribe(
    State(state): State<AppState>,
    Path(admin_link): Path<String>,
    Query(query): Query<FromQuery>,
) -> Result<axum::http::StatusCode, AppError> {
    let Some(subscriber) = find_subscriber_by_admin_link(&state, &admin_link).await? else {
        return Err(AppError::NotFound);
    };

    let now = Utc::now();
    sqlx::query("UPDATE subscribers SET status = false, updated_at = $1 WHERE id = $2")
        .bind(now)
        .bind(subscriber.id)
        .execute(&state.db)
        .await?;

    let newsletter_id = lookup_newsletter_id(&state, query.from.as_deref()).await?;
    record_unsubscribe_event(&state, subscriber.id, newsletter_id).await?;

    Ok(axum::http::StatusCode::OK)
}

pub async fn resubscribe(
    State(state): State<AppState>,
    Path(admin_link): Path<String>,
) -> Result<Html<String>, AppError> {
    let Some(subscriber) = find_subscriber_by_admin_link(&state, &admin_link).await? else {
        return render_link_error(
            &state,
            INVALID_LINK_TITLE,
            INVALID_LINK_MSG,
            Some(INVALID_LINK_HINT),
        );
    };

    let now = Utc::now();
    sqlx::query(
        "UPDATE subscribers SET status = true, bounced_at = NULL, updated_at = $1 WHERE id = $2",
    )
    .bind(now)
    .bind(subscriber.id)
    .execute(&state.db)
    .await?;

    let mut ctx = tera::Context::new();
    ctx.insert("name", &subscriber.name);
    ctx.insert("email", &subscriber.email);
    ctx.insert("status", &true);
    ctx.insert("admin_link", &admin_link);
    ctx.insert("from_newsletter", "");
    ctx.insert("message", "您已成功重新訂閱！");
    let html = state.tera.render("manage.html", &ctx)?;
    Ok(Html(html))
}

pub async fn unsubscribe(
    State(state): State<AppState>,
    Path(admin_link): Path<String>,
    Form(form): Form<UnsubscribeForm>,
) -> Result<Html<String>, AppError> {
    let Some(subscriber) = find_subscriber_by_admin_link(&state, &admin_link).await? else {
        return render_link_error(
            &state,
            INVALID_LINK_TITLE,
            INVALID_LINK_MSG,
            Some(INVALID_LINK_HINT),
        );
    };

    let now = Utc::now();
    sqlx::query("UPDATE subscribers SET status = false, updated_at = $1 WHERE id = $2")
        .bind(now)
        .bind(subscriber.id)
        .execute(&state.db)
        .await?;

    let newsletter_id = lookup_newsletter_id(&state, form.from.as_deref()).await?;
    record_unsubscribe_event(&state, subscriber.id, newsletter_id).await?;

    let mut ctx = tera::Context::new();
    ctx.insert("name", &subscriber.name);
    ctx.insert("email", &subscriber.email);
    ctx.insert("status", &false);
    ctx.insert("admin_link", &admin_link);
    ctx.insert("from_newsletter", "");
    ctx.insert("message", "您已成功取消訂閱。");
    let html = state.tera.render("manage.html", &ctx)?;
    Ok(Html(html))
}
