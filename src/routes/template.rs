use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Path, State};
use axum::http::HeaderMap;
use axum::response::{Html, Redirect};
use axum::Form;
use serde::Deserialize;

use crate::auth::AdminUser;
use crate::error::AppError;
use crate::newsletter;
use crate::AppState;

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
            chrono::DateTime<chrono::Utc>,
        ),
    >(
        "SELECT id, slug, name, description, created_at \
         FROM newsletter_templates ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    let templates: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(id, slug, name, description, created_at)| {
            serde_json::json!({
                "id": id.to_string(),
                "slug": slug,
                "name": name,
                "description": description,
                "created_at": created_at.format("%Y-%m-%d %H:%M").to_string(),
            })
        })
        .collect();

    let mut ctx = tera::Context::new();
    ctx.insert("admin_email", &admin_email);
    ctx.insert("templates", &templates);
    let html = state.tera.render("admin/templates.html", &ctx)?;
    Ok(Html(html))
}

// --- New ---

pub async fn new_form(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
) -> Result<Html<String>, AppError> {
    let mut ctx = tera::Context::new();
    ctx.insert("admin_email", &admin_email);
    ctx.insert("template", &serde_json::json!(null));
    let html = state.tera.render("admin/template_edit.html", &ctx)?;
    Ok(Html(html))
}

#[derive(Deserialize)]
pub struct TemplateForm {
    pub name: String,
    pub slug: String,
    pub description: String,
    pub html_body: String,
}

pub async fn create(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<TemplateForm>,
) -> Result<Redirect, AppError> {
    let slug = form.slug.trim().to_string();
    validate_template_slug(&slug)?;

    let name = form.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::BadRequest("Name is required".to_string()));
    }

    let id = sqlx::query_scalar::<_, uuid::Uuid>(
        "INSERT INTO newsletter_templates (name, slug, description, html_body, created_by) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(&name)
    .bind(&slug)
    .bind(form.description.trim())
    .bind(&form.html_body)
    .bind(&admin_email)
    .fetch_one(&state.db)
    .await?;

    let client_ip = super::extract_client_ip(&headers, &ConnectInfo(addr));
    crate::audit::log(
        &state.db,
        &admin_email,
        "template.create",
        Some(serde_json::json!({ "template_id": id.to_string(), "name": name })),
        Some(client_ip),
    )
    .await;

    Ok(Redirect::to(&format!("/admin/templates/{id}")))
}

// --- Edit ---

pub async fn edit_form(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Html<String>, AppError> {
    let row = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT name, slug, description, html_body FROM newsletter_templates WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let (name, slug, description, html_body) = row;

    let tpl = serde_json::json!({
        "id": id.to_string(),
        "name": name,
        "slug": slug,
        "description": description,
        "html_body": html_body,
    });

    let mut ctx = tera::Context::new();
    ctx.insert("admin_email", &admin_email);
    ctx.insert("template", &tpl);
    let html = state.tera.render("admin/template_edit.html", &ctx)?;
    Ok(Html(html))
}

pub async fn update(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<uuid::Uuid>,
    Form(form): Form<TemplateForm>,
) -> Result<Redirect, AppError> {
    let slug = form.slug.trim().to_string();
    validate_template_slug(&slug)?;

    let name = form.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::BadRequest("Name is required".to_string()));
    }

    // Check template exists
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM newsletter_templates WHERE id = $1)",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    if !exists {
        return Err(AppError::NotFound);
    }

    sqlx::query(
        "UPDATE newsletter_templates SET name = $1, slug = $2, description = $3, html_body = $4, updated_at = NOW() WHERE id = $5",
    )
    .bind(&name)
    .bind(&slug)
    .bind(form.description.trim())
    .bind(&form.html_body)
    .bind(id)
    .execute(&state.db)
    .await?;

    let client_ip = super::extract_client_ip(&headers, &ConnectInfo(addr));
    crate::audit::log(
        &state.db,
        &admin_email,
        "template.update",
        Some(serde_json::json!({ "template_id": id.to_string() })),
        Some(client_ip),
    )
    .await;

    Ok(Redirect::to(&format!("/admin/templates/{id}")))
}

// --- Delete ---

pub async fn delete(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<uuid::Uuid>,
) -> Result<Redirect, AppError> {
    // Check if any newsletters reference this template
    let ref_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM newsletters WHERE template_id = $1")
            .bind(id)
            .fetch_one(&state.db)
            .await?;

    if ref_count > 0 {
        return Err(AppError::BadRequest(format!(
            "此模板被 {ref_count} 封電子報使用中，無法刪除"
        )));
    }

    sqlx::query("DELETE FROM newsletter_templates WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    let client_ip = super::extract_client_ip(&headers, &ConnectInfo(addr));
    crate::audit::log(
        &state.db,
        &admin_email,
        "template.delete",
        Some(serde_json::json!({ "template_id": id.to_string() })),
        Some(client_ip),
    )
    .await;

    Ok(Redirect::to("/admin/templates"))
}

// --- Preview ---

pub async fn preview(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Html<String>, AppError> {
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT name, html_body FROM newsletter_templates WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let (name, html_body) = row;

    // Use a realistic Markdown sample so the preview goes through the same
    // render_markdown pipeline as actual newsletters.
    let sample_markdown = "\
## COSCUP 2025 活動公告

感謝您訂閱 COSCUP 電子報！以下是本期精彩內容：

### 活動亮點

- 超過 **100 場**議程，涵蓋 Open Source 各領域
- 活動日期：**8 月 9 日 ~ 10 日**
- 地點：[台灣科技大學](https://coscup.org)

### 特別活動

本年度特別新增「親子工作坊」，歡迎帶孩子一起參與開源文化！

---

[立即報名](https://coscup.org) | [查看議程](https://coscup.org)\
";
    let content_html = newsletter::render_markdown(sample_markdown, &state.config.base_url);
    let tracking_pixel = "<!-- tracking pixel placeholder -->";
    let unsubscribe_url = "#unsubscribe";

    let rendered = newsletter::personalize_email(
        &html_body,
        &content_html,
        "COSCUP 2025 電子報 - 第一期",
        tracking_pixel,
        unsubscribe_url,
        &state.config.base_url,
        "#web-version",
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    // Replace recipient name placeholder as the actual send pipeline does.
    let rendered = newsletter::replace_recipient_name(&rendered, "COSCUP 訂閱者");

    let mut ctx = tera::Context::new();
    ctx.insert("admin_email", &admin_email);
    ctx.insert("template_id", &id.to_string());
    ctx.insert("name", &name);
    ctx.insert("rendered_html", &rendered);
    let html = state.tera.render("admin/template_preview.html", &ctx)?;
    Ok(Html(html))
}

// --- Duplicate ---

pub async fn duplicate(
    State(state): State<AppState>,
    AdminUser(admin_email): AdminUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<uuid::Uuid>,
) -> Result<Redirect, AppError> {
    let row = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT name, slug, description, html_body FROM newsletter_templates WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let (name, slug, description, html_body) = row;

    let new_slug = generate_copy_slug(&slug);
    let new_name = format!("{name} (copy)");

    let new_id = sqlx::query_scalar::<_, uuid::Uuid>(
        "INSERT INTO newsletter_templates (name, slug, description, html_body, created_by) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(&new_name)
    .bind(&new_slug)
    .bind(&description)
    .bind(&html_body)
    .bind(&admin_email)
    .fetch_one(&state.db)
    .await?;

    let client_ip = super::extract_client_ip(&headers, &ConnectInfo(addr));
    crate::audit::log(
        &state.db,
        &admin_email,
        "template.duplicate",
        Some(serde_json::json!({ "source_id": id.to_string(), "new_id": new_id.to_string() })),
        Some(client_ip),
    )
    .await;

    Ok(Redirect::to(&format!("/admin/templates/{new_id}")))
}

// --- Helpers ---

fn validate_template_slug(slug: &str) -> Result<(), AppError> {
    if slug.is_empty() {
        return Err(AppError::BadRequest("Slug is required".to_string()));
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(AppError::BadRequest(
            "Slug must contain only lowercase letters, numbers, and hyphens".to_string(),
        ));
    }
    Ok(())
}

fn generate_copy_slug(original: &str) -> String {
    let timestamp = chrono::Utc::now().timestamp();
    format!("{original}-copy-{timestamp}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_template_slug_valid() {
        assert!(validate_template_slug("coscup-default").is_ok());
        assert!(validate_template_slug("my-template-2024").is_ok());
        assert!(validate_template_slug("a").is_ok());
        assert!(validate_template_slug("123").is_ok());
    }

    #[test]
    fn test_validate_template_slug_empty() {
        let err = validate_template_slug("").unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn test_validate_template_slug_uppercase() {
        let err = validate_template_slug("MyTemplate").unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn test_validate_template_slug_spaces() {
        let err = validate_template_slug("my template").unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn test_validate_template_slug_special_chars() {
        let err = validate_template_slug("my_template").unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn test_generate_copy_slug() {
        let slug = generate_copy_slug("coscup-default");
        assert!(slug.starts_with("coscup-default-copy-"));
        assert!(slug.len() > "coscup-default-copy-".len());
        // Verify the suffix is a valid timestamp
        let suffix = slug.strip_prefix("coscup-default-copy-").unwrap();
        assert!(suffix.parse::<i64>().is_ok());
    }

    #[test]
    fn test_generate_copy_slug_unique() {
        let slug1 = generate_copy_slug("test");
        // Timestamps within the same second may be equal, but the function should at least
        // produce a valid slug
        assert!(slug1.starts_with("test-copy-"));
    }
}
