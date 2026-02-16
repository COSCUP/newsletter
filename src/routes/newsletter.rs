use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Path, State};
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse, Json, Redirect};
use axum::Form;
use chrono::{FixedOffset, NaiveDateTime, Utc};
use serde::Deserialize;

use crate::auth::AdminUser;
use crate::error::AppError;
use crate::newsletter;
use crate::AppState;

fn taiwan_offset() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).expect("valid offset")
}

fn generate_slug(title: &str) -> String {
    let timestamp = Utc::now().timestamp();
    let sanitized: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = sanitized.trim_matches('-').to_lowercase();
    let short = if trimmed.len() > 50 {
        &trimmed[..50]
    } else {
        &trimmed
    };
    format!("{short}-{timestamp}")
}

// --- List ---

pub async fn list(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
) -> Result<Html<String>, AppError> {
    let rows = sqlx::query_as::<
        _,
        (
            uuid::Uuid,
            String,
            String,
            String,
            i32,
            i32,
            i32,
            chrono::DateTime<Utc>,
        ),
    >(
        "SELECT id, title, slug, status, sent_count, failed_count, total_count, created_at \
         FROM newsletters ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    let newsletters: Vec<serde_json::Value> = rows
        .into_iter()
        .map(
            |(id, title, slug, status, sent_count, failed_count, total_count, created_at)| {
                serde_json::json!({
                    "id": id.to_string(),
                    "title": title,
                    "slug": slug,
                    "status": status,
                    "sent_count": sent_count,
                    "failed_count": failed_count,
                    "total_count": total_count,
                    "created_at": created_at.with_timezone(&taiwan_offset()).format("%Y-%m-%d %H:%M").to_string(),
                })
            },
        )
        .collect();

    let mut ctx = tera::Context::new();
    ctx.insert("admin_email", &admin_email);
    ctx.insert("newsletters", &newsletters);
    let html = state.tera.render("admin/newsletters.html", &ctx)?;
    Ok(Html(html))
}

// --- New ---

pub async fn new_form(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
) -> Result<Html<String>, AppError> {
    let templates = sqlx::query_as::<_, (uuid::Uuid, String, String)>(
        "SELECT id, slug, name FROM newsletter_templates ORDER BY name",
    )
    .fetch_all(&state.db)
    .await?;

    let template_list: Vec<serde_json::Value> = templates
        .into_iter()
        .map(|(id, slug, name)| {
            serde_json::json!({ "id": id.to_string(), "slug": slug, "name": name })
        })
        .collect();

    let mut ctx = tera::Context::new();
    ctx.insert("admin_email", &admin_email);
    ctx.insert("templates", &template_list);
    ctx.insert("newsletter", &serde_json::json!(null));
    let html = state.tera.render("admin/newsletter_edit.html", &ctx)?;
    Ok(Html(html))
}

#[derive(Deserialize)]
pub struct NewsletterForm {
    pub title: String,
    pub markdown_content: String,
    pub template_id: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<NewsletterForm>,
) -> Result<Redirect, AppError> {
    let title = form.title.trim().to_string();
    if title.is_empty() {
        return Err(AppError::BadRequest("Title is required".to_string()));
    }

    let slug = generate_slug(&title);
    let template_id: Option<uuid::Uuid> = form
        .template_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse().ok());

    let id = sqlx::query_scalar::<_, uuid::Uuid>(
        "INSERT INTO newsletters (title, slug, markdown_content, template_id, created_by) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(&title)
    .bind(&slug)
    .bind(&form.markdown_content)
    .bind(template_id)
    .bind(&admin_email)
    .fetch_one(&state.db)
    .await?;

    let client_ip = super::extract_client_ip(&headers, &ConnectInfo(addr));
    crate::audit::log(
        &state.db,
        &admin_email,
        "newsletter.create",
        Some(serde_json::json!({ "newsletter_id": id.to_string(), "title": title })),
        Some(client_ip),
    )
    .await;

    Ok(Redirect::to(&format!("/admin/newsletters/{id}")))
}

// --- Edit ---

pub async fn edit_form(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Html<String>, AppError> {
    let row = sqlx::query_as::<_, (String, String, String, Option<uuid::Uuid>, String, i32, i32, i32)>(
        "SELECT title, slug, markdown_content, template_id, status, sent_count, failed_count, total_count FROM newsletters WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let (title, slug, markdown_content, template_id, status, sent_count, failed_count, total_count) =
        row;

    let templates = sqlx::query_as::<_, (uuid::Uuid, String, String)>(
        "SELECT id, slug, name FROM newsletter_templates ORDER BY name",
    )
    .fetch_all(&state.db)
    .await?;

    let template_list: Vec<serde_json::Value> = templates
        .into_iter()
        .map(|(tid, tslug, name)| {
            serde_json::json!({ "id": tid.to_string(), "slug": tslug, "name": name })
        })
        .collect();

    let nl = serde_json::json!({
        "id": id.to_string(),
        "title": title,
        "slug": slug,
        "markdown_content": markdown_content,
        "template_id": template_id.map(|t| t.to_string()).unwrap_or_default(),
        "status": status,
        "sent_count": sent_count,
        "failed_count": failed_count,
        "total_count": total_count,
    });

    let mut ctx = tera::Context::new();
    ctx.insert("admin_email", &admin_email);
    ctx.insert("templates", &template_list);
    ctx.insert("newsletter", &nl);
    let html = state.tera.render("admin/newsletter_edit.html", &ctx)?;
    Ok(Html(html))
}

pub async fn update(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<uuid::Uuid>,
    Form(form): Form<NewsletterForm>,
) -> Result<Redirect, AppError> {
    // Only allow editing drafts
    let status = sqlx::query_scalar::<_, String>("SELECT status FROM newsletters WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    if status != "draft" {
        return Err(AppError::BadRequest(
            "Only draft newsletters can be edited".to_string(),
        ));
    }

    let template_id: Option<uuid::Uuid> = form
        .template_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse().ok());

    sqlx::query(
        "UPDATE newsletters SET title = $1, markdown_content = $2, template_id = $3, updated_at = NOW() WHERE id = $4",
    )
    .bind(form.title.trim())
    .bind(&form.markdown_content)
    .bind(template_id)
    .bind(id)
    .execute(&state.db)
    .await?;

    let client_ip = super::extract_client_ip(&headers, &ConnectInfo(addr));
    crate::audit::log(
        &state.db,
        &admin_email,
        "newsletter.update",
        Some(serde_json::json!({ "newsletter_id": id.to_string() })),
        Some(client_ip),
    )
    .await;

    Ok(Redirect::to(&format!("/admin/newsletters/{id}")))
}

// --- Preview ---

pub async fn preview(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Html<String>, AppError> {
    let row = sqlx::query_as::<_, (String, String, Option<uuid::Uuid>)>(
        "SELECT title, markdown_content, template_id FROM newsletters WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let (title, markdown_content, template_id) = row;

    // Load template (use selected template, or fall back to coscup-default)
    let template_html = if let Some(tid) = template_id {
        sqlx::query_scalar::<_, String>("SELECT html_body FROM newsletter_templates WHERE id = $1")
            .bind(tid)
            .fetch_optional(&state.db)
            .await?
    } else {
        None
    };
    let template_html = match template_html {
        Some(html) => html,
        None => {
            sqlx::query_scalar::<_, String>(
                "SELECT html_body FROM newsletter_templates WHERE slug = 'coscup-default'",
            )
            .fetch_one(&state.db)
            .await?
        }
    };

    let content_html = newsletter::render_markdown(&markdown_content);
    let content_html = newsletter::absolutize_image_srcs(&content_html, &state.config.base_url);
    let content_html = newsletter::replace_recipient_name(&content_html, "王小明");

    // Use dummy values for preview
    let tracking_pixel = "<!-- tracking pixel placeholder -->";
    let unsubscribe_url = "#";
    let web_url = "#";

    let rendered = newsletter::personalize_email(
        &template_html,
        &content_html,
        &title,
        tracking_pixel,
        unsubscribe_url,
        &state.config.base_url,
        web_url,
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let mut ctx = tera::Context::new();
    ctx.insert("admin_email", &admin_email);
    ctx.insert("newsletter_id", &id.to_string());
    ctx.insert("title", &title);
    ctx.insert("rendered_html", &rendered);
    let html = state.tera.render("admin/newsletter_preview.html", &ctx)?;
    Ok(Html(html))
}

// --- Send ---

pub async fn send_now(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<uuid::Uuid>,
) -> Result<Redirect, AppError> {
    let status = sqlx::query_scalar::<_, String>("SELECT status FROM newsletters WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    if status != "draft" && status != "scheduled" && status != "paused" {
        return Err(AppError::BadRequest(
            "Newsletter must be in draft, scheduled, or paused status to send".to_string(),
        ));
    }

    let rate_limit_ms = state.config.smtp_rate_limit_ms;
    let state_clone = state.clone();
    let svc = state.shorturl.clone();

    tokio::spawn(async move {
        if let Err(e) =
            newsletter::send_newsletter(&state_clone, id, svc.as_ref(), rate_limit_ms).await
        {
            tracing::error!("Newsletter send failed: {e}");
        }
    });

    let client_ip = super::extract_client_ip(&headers, &ConnectInfo(addr));
    crate::audit::log(
        &state.db,
        &admin_email,
        "newsletter.send",
        Some(serde_json::json!({ "newsletter_id": id.to_string() })),
        Some(client_ip),
    )
    .await;

    Ok(Redirect::to(&format!("/admin/newsletters/{id}")))
}

// --- Schedule ---

#[derive(Deserialize)]
pub struct ScheduleForm {
    pub scheduled_at: String,
}

pub async fn schedule(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<uuid::Uuid>,
    Form(form): Form<ScheduleForm>,
) -> Result<Redirect, AppError> {
    let status = sqlx::query_scalar::<_, String>("SELECT status FROM newsletters WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    if status != "draft" {
        return Err(AppError::BadRequest(
            "Only draft newsletters can be scheduled".to_string(),
        ));
    }

    let naive = NaiveDateTime::parse_from_str(&form.scheduled_at, "%Y-%m-%dT%H:%M")
        .map_err(|e| AppError::BadRequest(format!("Invalid datetime: {e}")))?;
    let scheduled_at = naive
        .and_local_timezone(taiwan_offset())
        .single()
        .ok_or_else(|| AppError::BadRequest("Invalid timezone conversion".to_string()))?
        .with_timezone(&Utc);

    sqlx::query(
        "UPDATE newsletters SET status = 'scheduled', scheduled_at = $1, updated_at = NOW() WHERE id = $2",
    )
    .bind(scheduled_at)
    .bind(id)
    .execute(&state.db)
    .await?;

    let client_ip = super::extract_client_ip(&headers, &ConnectInfo(addr));
    crate::audit::log(
        &state.db,
        &admin_email,
        "newsletter.schedule",
        Some(serde_json::json!({ "newsletter_id": id.to_string(), "scheduled_at": form.scheduled_at })),
        Some(client_ip),
    )
    .await;

    Ok(Redirect::to(&format!("/admin/newsletters/{id}")))
}

// --- Cancel ---

pub async fn cancel(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<uuid::Uuid>,
) -> Result<Redirect, AppError> {
    let status = sqlx::query_scalar::<_, String>("SELECT status FROM newsletters WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    match status.as_str() {
        "scheduled" => {
            sqlx::query(
                "UPDATE newsletters SET status = 'draft', scheduled_at = NULL, updated_at = NOW() WHERE id = $1",
            )
            .bind(id)
            .execute(&state.db)
            .await?;
        }
        "sending" => {
            sqlx::query(
                "UPDATE newsletters SET status = 'paused', updated_at = NOW() WHERE id = $1",
            )
            .bind(id)
            .execute(&state.db)
            .await?;
        }
        "paused" => {
            sqlx::query(
                "UPDATE newsletters SET status = 'sent', sending_completed_at = NOW(), updated_at = NOW() WHERE id = $1",
            )
            .bind(id)
            .execute(&state.db)
            .await?;
        }
        _ => {
            return Err(AppError::BadRequest(
                "Newsletter is not in a cancellable state".to_string(),
            ));
        }
    }

    let client_ip = super::extract_client_ip(&headers, &ConnectInfo(addr));
    crate::audit::log(
        &state.db,
        &admin_email,
        "newsletter.cancel",
        Some(serde_json::json!({ "newsletter_id": id.to_string() })),
        Some(client_ip),
    )
    .await;

    Ok(Redirect::to(&format!("/admin/newsletters/{id}")))
}

// --- Status (JSON for polling) ---

pub async fn status_json(
    State(state): State<AppState>,
    Path(id): Path<uuid::Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let row = sqlx::query_as::<_, (String, i32, i32, i32)>(
        "SELECT status, sent_count, failed_count, total_count FROM newsletters WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let (status, sent_count, failed_count, total_count) = row;

    Ok(Json(serde_json::json!({
        "status": status,
        "sent_count": sent_count,
        "failed_count": failed_count,
        "total_count": total_count,
    })))
}

// --- Stats ---

pub async fn stats(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Html<String>, AppError> {
    let row = sqlx::query_as::<_, (String, String, i32, i32, i32, Option<String>)>(
        "SELECT title, status, sent_count, failed_count, total_count, rendered_html FROM newsletters WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let (title, status, sent_count, failed_count, total_count, rendered_html) = row;

    // Extract link text from rendered HTML: URL → anchor text
    let link_text_map: std::collections::HashMap<String, String> = {
        let mut map = std::collections::HashMap::new();
        if let Some(ref html) = rendered_html {
            let re = regex::Regex::new(r#"<a\s[^>]*href="(https?://[^"]+)"[^>]*>(.*?)</a>"#)
                .expect("valid regex");
            let strip_tags = regex::Regex::new(r"<[^>]+>").expect("valid regex");
            for caps in re.captures_iter(html) {
                let url = caps[1].to_string();
                // Strip HTML tags from link text (e.g. <img> inside <a>)
                let text = strip_tags.replace_all(&caps[2], "").trim().to_string();
                if !text.is_empty() {
                    map.entry(url).or_insert(text);
                }
            }
        }
        map
    };

    // Get unique opens from email_events
    let slug = sqlx::query_scalar::<_, String>("SELECT slug FROM newsletters WHERE id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;

    let unique_opens: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT ucode) FROM email_events WHERE topic = $1 AND event_type = 'open'",
    )
    .bind(&slug)
    .fetch_one(&state.db)
    .await?;

    // Get per-URL click counts from email_events
    let url_clicks = sqlx::query_as::<_, (String, i64)>(
        "SELECT clicked_url, COUNT(*) as clicks FROM email_events \
         WHERE topic = $1 AND event_type = 'click' AND clicked_url IS NOT NULL \
         GROUP BY clicked_url ORDER BY clicks DESC",
    )
    .bind(&slug)
    .fetch_all(&state.db)
    .await?;

    let link_list: Vec<serde_json::Value> = url_clicks
        .into_iter()
        .map(|(url, clicks)| {
            let text = link_text_map.get(&url).cloned().unwrap_or_default();
            serde_json::json!({
                "url": url,
                "text": text,
                "clicks": clicks,
            })
        })
        .collect();

    let total_clicks: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM email_events WHERE topic = $1 AND event_type = 'click'",
    )
    .bind(&slug)
    .fetch_one(&state.db)
    .await?;

    let unique_clicks: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT ucode) FROM email_events WHERE topic = $1 AND event_type = 'click'",
    )
    .bind(&slug)
    .fetch_one(&state.db)
    .await?;

    let open_rate = if sent_count > 0 {
        #[allow(clippy::cast_precision_loss)]
        let rate = (unique_opens as f64 / f64::from(sent_count)) * 100.0;
        format!("{rate:.1}%")
    } else {
        "—".to_string()
    };

    let unsubscribe_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM unsubscribe_events WHERE newsletter_id = $1")
            .bind(id)
            .fetch_one(&state.db)
            .await?;

    let mut ctx = tera::Context::new();
    ctx.insert("admin_email", &admin_email);
    ctx.insert("newsletter_id", &id.to_string());
    ctx.insert("title", &title);
    ctx.insert("status", &status);
    ctx.insert("sent_count", &sent_count);
    ctx.insert("failed_count", &failed_count);
    ctx.insert("total_count", &total_count);
    ctx.insert("unique_opens", &unique_opens);
    ctx.insert("open_rate", &open_rate);
    ctx.insert("total_clicks", &total_clicks);
    ctx.insert("unique_clicks", &unique_clicks);
    ctx.insert("unsubscribe_count", &unsubscribe_count);
    ctx.insert("links", &link_list);
    let html = state.tera.render("admin/newsletter_stats.html", &ctx)?;
    Ok(Html(html))
}

// --- Delete ---

pub async fn delete(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<uuid::Uuid>,
) -> Result<Redirect, AppError> {
    let status = sqlx::query_scalar::<_, String>("SELECT status FROM newsletters WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    if status != "draft" {
        return Err(AppError::BadRequest(
            "Only draft newsletters can be deleted".to_string(),
        ));
    }

    sqlx::query("DELETE FROM newsletters WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    let client_ip = super::extract_client_ip(&headers, &ConnectInfo(addr));
    crate::audit::log(
        &state.db,
        &admin_email,
        "newsletter.delete",
        Some(serde_json::json!({ "newsletter_id": id.to_string() })),
        Some(client_ip),
    )
    .await;

    Ok(Redirect::to("/admin/newsletters"))
}
