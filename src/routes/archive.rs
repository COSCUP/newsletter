use axum::extract::{Path, State};
use axum::response::Html;

use crate::error::AppError;
use crate::newsletter;
use crate::AppState;

/// Public page: list all sent newsletters.
pub async fn list(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let rows = sqlx::query_as::<_, (String, String, chrono::DateTime<chrono::Utc>)>(
        "SELECT slug, title, sending_completed_at \
         FROM newsletters \
         WHERE status = 'sent' AND sending_completed_at IS NOT NULL \
         ORDER BY sending_completed_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    let newsletters: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(slug, title, sent_at)| {
            serde_json::json!({
                "slug": slug,
                "title": title,
                "sent_at": sent_at.format("%Y-%m-%d").to_string(),
            })
        })
        .collect();

    let mut ctx = tera::Context::new();
    ctx.insert("newsletters", &newsletters);
    let html = state.tera.render("newsletters.html", &ctx)?;
    Ok(Html(html))
}

/// Public page: view a single sent newsletter.
pub async fn view(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Html<String>, AppError> {
    let row = sqlx::query_as::<_, (String, String, Option<uuid::Uuid>)>(
        "SELECT title, markdown_content, template_id \
         FROM newsletters \
         WHERE slug = $1 AND status = 'sent'",
    )
    .bind(&slug)
    .fetch_optional(&state.db)
    .await?;

    let Some(row) = row else {
        let mut ctx = tera::Context::new();
        ctx.insert("title", "找不到此電子報");
        ctx.insert("message", "此電子報不存在或尚未寄送。");
        let html = state.tera.render("error.html", &ctx)?;
        return Ok(Html(html));
    };

    let (title, markdown_content, template_id) = row;

    // Render markdown to HTML (includes image src absolutization), then sanitize
    // (strips <script>, event handlers, and other dangerous elements)
    let content_html = newsletter::render_markdown(&markdown_content, &state.config.base_url);
    let content_html = newsletter::replace_recipient_name(&content_html, "訂閱者");
    let content_html = newsletter::sanitize_html(&content_html);

    // Load template
    let template_html = if let Some(tid) = template_id {
        sqlx::query_scalar::<_, String>("SELECT html_body FROM newsletter_templates WHERE id = $1")
            .bind(tid)
            .fetch_optional(&state.db)
            .await?
    } else {
        None
    };

    let template_html = match template_html {
        Some(t) => t,
        None => {
            // Fallback: load coscup-default template
            sqlx::query_scalar::<_, String>(
                "SELECT html_body FROM newsletter_templates WHERE slug = 'coscup-default'",
            )
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(|| AppError::Internal("No default template found".to_string()))?
        }
    };

    // Personalize with empty tracking/unsubscribe (public view)
    let web_url = format!("{}/newsletters/{}", state.config.base_url, slug);
    let rendered = newsletter::personalize_email(
        &template_html,
        &content_html,
        &title,
        "",
        "#",
        &state.config.base_url,
        &web_url,
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let mut ctx = tera::Context::new();
    ctx.insert("subject", &title);
    ctx.insert("rendered_html", &rendered);
    let html = state.tera.render("newsletter_view.html", &ctx)?;
    Ok(Html(html))
}
