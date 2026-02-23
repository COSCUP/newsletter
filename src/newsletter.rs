use regex::Regex;

use crate::security;
use crate::shorturl::ShortUrlService;
use crate::AppState;

/// Convert Markdown to HTML using comrak, absolutize relative image srcs,
/// and add inline styles on `<img>` tags so images display properly in email clients.
pub fn render_markdown(md: &str, base_url: &str) -> String {
    use comrak::{markdown_to_html, Options};
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.render.unsafe_ = true;
    let html = markdown_to_html(md, &options);
    let html = style_images_for_email(&html);
    absolutize_image_srcs(&html, base_url)
}

/// Rewrite relative `src` attributes (e.g. `/uploads/...`) to absolute URLs
/// so that images display correctly in email clients.
pub fn absolutize_image_srcs(html: &str, base_url: &str) -> String {
    let re = Regex::new(r#"src="(/[^"]+)""#).expect("valid regex");
    re.replace_all(html, |caps: &regex::Captures| {
        format!("src=\"{}{}\"", base_url, &caps[1])
    })
    .into_owned()
}

/// Add inline styles to `<img>` tags so images display properly in email clients
/// without breaking layout.
fn style_images_for_email(html: &str) -> String {
    let re = Regex::new(r"<img\b").expect("valid regex");
    re.replace_all(
        html,
        r#"<img style="max-width:100%;height:auto;display:block;""#,
    )
    .into_owned()
}

/// Sanitize HTML for public web display: strip `<script>`, event handlers,
/// and other dangerous elements while preserving formatting tags.
pub fn sanitize_html(html: &str) -> String {
    ammonia::clean(html)
}

/// Replace `%recipient_name%` placeholder with the subscriber's name.
pub fn replace_recipient_name(html: &str, name: &str) -> String {
    html.replace("%recipient_name%", name)
}

/// Find all `<a href="...">` links in HTML, shorten them via `ShortUrlService`,
/// and return (rewritten HTML, list of (original, short) pairs).
/// Skips mailto:, tel:, and anchor (#) links.
pub async fn shorten_links(
    html: &str,
    svc: &dyn ShortUrlService,
) -> (String, Vec<(String, String)>) {
    let re = Regex::new(r#"<a\s[^>]*href\s*=\s*"([^"]+)"#).expect("valid regex");

    let mut link_map: Vec<(String, String)> = Vec::new();
    let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    // Collect all unique URLs to shorten
    for cap in re.captures_iter(html) {
        let url = cap[1].to_string();
        if url.starts_with("mailto:")
            || url.starts_with("tel:")
            || url.starts_with('#')
            || url.starts_with("{{")
        {
            continue;
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            continue;
        }
        if seen.contains_key(&url) {
            continue;
        }
        match svc.shorten(&url).await {
            Ok(short) => {
                seen.insert(url.clone(), short.clone());
                link_map.push((url, short));
            }
            Err(e) => {
                tracing::warn!("Failed to shorten {url}: {e}, using original");
                seen.insert(url.clone(), url.clone());
            }
        }
    }

    // Replace all occurrences in HTML
    let mut result = html.to_string();
    for (original, short) in &link_map {
        result = result.replace(
            &format!("href=\"{original}\""),
            &format!("href=\"{short}\""),
        );
    }

    (result, link_map)
}

/// Personalize the email template for a specific subscriber.
/// Fills in `{{ content }}`, `{{ title }}`, `{{ tracking_pixel }}`, `{{ unsubscribe_url }}`.
pub fn personalize_email(
    template_html: &str,
    content_html: &str,
    title: &str,
    tracking_pixel_html: &str,
    unsubscribe_url: &str,
    base_url: &str,
    web_url: &str,
) -> Result<String, tera::Error> {
    let mut ctx = tera::Context::new();
    ctx.insert("content", content_html);
    ctx.insert("title", title);
    ctx.insert("tracking_pixel", tracking_pixel_html);
    ctx.insert("unsubscribe_url", unsubscribe_url);
    ctx.insert("base_url", base_url);
    ctx.insert("web_url", web_url);

    tera::Tera::one_off(template_html, &ctx, false)
}

/// Rewrite all http/https links in HTML to go through `/r/c` click tracking.
/// Each link becomes `/r/c?ucode=...&topic=...&hash=...&url=<original>`.
/// The hash is HMAC-SHA256 over (ucode, topic, url), so the URL is tamper-proof.
/// This is per-subscriber (each subscriber gets their own hash per link).
pub fn rewrite_links_for_tracking(
    html: &str,
    base_url: &str,
    ucode: &str,
    topic: &str,
    secret_code: &str,
) -> String {
    let re = Regex::new(r#"href="(https?://[^"]+)""#).expect("valid regex");
    re.replace_all(html, |caps: &regex::Captures| {
        let original_url = &caps[1];
        let hash = security::compute_openhash(secret_code, ucode, topic, original_url);
        let tracking_url = format!(
            "{}/r/c?ucode={}&topic={}&hash={}&url={}",
            base_url,
            urlencoding::encode(ucode),
            urlencoding::encode(topic),
            urlencoding::encode(&hash),
            urlencoding::encode(original_url),
        );
        format!("href=\"{tracking_url}\"")
    })
    .into_owned()
}

/// Build a tracking pixel `<img>` tag for a specific subscriber.
pub fn build_tracking_pixel(base_url: &str, ucode: &str, topic: &str, openhash: &str) -> String {
    let pixel_url = format!(
        "{}/r/o?ucode={}&topic={}&hash={}",
        base_url,
        urlencoding::encode(ucode),
        urlencoding::encode(topic),
        urlencoding::encode(openhash),
    );
    format!("<img src=\"{pixel_url}\" width=\"1\" height=\"1\" alt=\"\" style=\"border:0;width:1px;height:1px;\" />")
}

/// Send a newsletter to all active+verified subscribers.
/// This is meant to be called in a background task.
#[allow(clippy::too_many_lines)]
pub async fn send_newsletter(
    state: &AppState,
    newsletter_id: uuid::Uuid,
    shorturl_service: &dyn ShortUrlService,
    rate_limit_ms: u64,
) -> Result<(), String> {
    // Load newsletter
    let row = sqlx::query_as::<_, (String, String, String, Option<uuid::Uuid>)>(
        "SELECT title, markdown_content, slug, template_id FROM newsletters WHERE id = $1",
    )
    .bind(newsletter_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "Newsletter not found".to_string())?;

    let (title, markdown_content, slug, template_id) = row;

    // Load template (use selected template, or fall back to coscup-default)
    let template_html = if let Some(tid) = template_id {
        sqlx::query_scalar::<_, String>("SELECT html_body FROM newsletter_templates WHERE id = $1")
            .bind(tid)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| e.to_string())?
    } else {
        None
    };
    let template_html = match template_html {
        Some(html) => html,
        None => sqlx::query_scalar::<_, String>(
            "SELECT html_body FROM newsletter_templates WHERE slug = 'coscup-default'",
        )
        .fetch_one(&state.db)
        .await
        .map_err(|e| e.to_string())?,
    };

    // Render markdown → HTML (includes image src absolutization), then sanitize
    let content_html = render_markdown(&markdown_content, &state.config.base_url);
    let content_html = sanitize_html(&content_html);

    // Update rendered_html
    sqlx::query("UPDATE newsletters SET rendered_html = $1, updated_at = NOW() WHERE id = $2")
        .bind(&content_html)
        .bind(newsletter_id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    // Shorten links (once for all subscribers)
    let (shortened_html, link_pairs) = shorten_links(&content_html, shorturl_service).await;

    // Store link mappings
    for (original, short) in &link_pairs {
        let _ = sqlx::query(
            "INSERT INTO newsletter_links (newsletter_id, original_url, short_url) VALUES ($1, $2, $3)",
        )
        .bind(newsletter_id)
        .bind(original)
        .bind(short)
        .execute(&state.db)
        .await;
    }

    // Mark as sending
    sqlx::query(
        "UPDATE newsletters SET status = 'sending', sending_started_at = NOW(), updated_at = NOW() WHERE id = $1",
    )
    .bind(newsletter_id)
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    // Fetch all active+verified subscribers (excluding bounced)
    let subscribers = sqlx::query_as::<_, (uuid::Uuid, String, String, String, String)>(
        "SELECT id, email, name, ucode, secret_code FROM subscribers \
         WHERE status = true AND verified_email = true AND bounced_at IS NULL",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    let total = i32::try_from(subscribers.len()).unwrap_or(0);
    sqlx::query("UPDATE newsletters SET total_count = $1, updated_at = NOW() WHERE id = $2")
        .bind(total)
        .bind(newsletter_id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    // Create pending send records
    for (sub_id, _, _, _, _) in &subscribers {
        let _ = sqlx::query(
            "INSERT INTO newsletter_sends (newsletter_id, subscriber_id, status) VALUES ($1, $2, 'pending') ON CONFLICT DO NOTHING",
        )
        .bind(newsletter_id)
        .bind(sub_id)
        .execute(&state.db)
        .await;
    }

    let mut sent_count = 0i32;
    let mut failed_count = 0i32;

    for (sub_id, email, name, ucode, secret_code) in &subscribers {
        // Check if newsletter was paused
        let current_status =
            sqlx::query_scalar::<_, String>("SELECT status FROM newsletters WHERE id = $1")
                .bind(newsletter_id)
                .fetch_one(&state.db)
                .await
                .map_err(|e| e.to_string())?;

        if current_status == "paused" {
            tracing::info!("Newsletter {newsletter_id} was paused, stopping send");
            break;
        }

        // Skip subscribers already sent (important for resume after pause)
        let already_sent = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM newsletter_sends WHERE newsletter_id = $1 AND subscriber_id = $2 AND status = 'sent')",
        )
        .bind(newsletter_id)
        .bind(sub_id)
        .fetch_one(&state.db)
        .await
        .map_err(|e| e.to_string())?;

        if already_sent {
            sent_count += 1;
            continue;
        }

        // Compute per-subscriber open-tracking pixel hash (no URL)
        let openhash = security::compute_openhash(secret_code, ucode, &slug, "");
        let tracking_pixel = build_tracking_pixel(&state.config.base_url, ucode, &slug, &openhash);

        // Rewrite links for per-subscriber click tracking (each link gets its own HMAC)
        let tracked_html = rewrite_links_for_tracking(
            &shortened_html,
            &state.config.base_url,
            ucode,
            &slug,
            secret_code,
        );
        let tracked_html = replace_recipient_name(&tracked_html, name);

        let admin_link = security::compute_admin_link(secret_code, email);
        let unsubscribe_url = format!(
            "{}/manage/{}?from={}",
            state.config.base_url,
            admin_link,
            urlencoding::encode(&slug)
        );

        // Personalize template
        let web_url = format!("{}/newsletters/{}", state.config.base_url, slug);
        let final_html = match personalize_email(
            &template_html,
            &tracked_html,
            &title,
            &tracking_pixel,
            &unsubscribe_url,
            &state.config.base_url,
            &web_url,
        ) {
            Ok(html) => html,
            Err(e) => {
                tracing::error!("Template error for {email}: {e}");
                failed_count += 1;
                let _ = sqlx::query(
                    "UPDATE newsletter_sends SET status = 'failed', error_message = $1 WHERE newsletter_id = $2 AND subscriber_id = $3",
                )
                .bind(e.to_string())
                .bind(newsletter_id)
                .bind(sub_id)
                .execute(&state.db)
                .await;
                continue;
            }
        };

        // Build List-Unsubscribe headers (RFC 2369 + RFC 8058)
        let one_click_url = format!(
            "{}/unsubscribe/{}?from={}",
            state.config.base_url,
            admin_link,
            urlencoding::encode(&slug)
        );
        let list_unsubscribe_headers: Vec<crate::email::EmailHeader> = vec![
            (
                "List-Unsubscribe".to_string(),
                format!("<{one_click_url}>, <{unsubscribe_url}>"),
            ),
            (
                "List-Unsubscribe-Post".to_string(),
                "List-Unsubscribe=One-Click".to_string(),
            ),
        ];

        // Send email
        match state
            .email
            .send_email_with_headers(email, &title, &final_html, &list_unsubscribe_headers)
            .await
        {
            Ok(()) => {
                sent_count += 1;
                let _ = sqlx::query(
                    "UPDATE newsletter_sends SET status = 'sent', sent_at = NOW() WHERE newsletter_id = $1 AND subscriber_id = $2",
                )
                .bind(newsletter_id)
                .bind(sub_id)
                .execute(&state.db)
                .await;
            }
            Err(e) => {
                tracing::error!("Failed to send to {email}: {e}");
                failed_count += 1;
                let _ = sqlx::query(
                    "UPDATE newsletter_sends SET status = 'failed', error_message = $1 WHERE newsletter_id = $2 AND subscriber_id = $3",
                )
                .bind(e.to_string())
                .bind(newsletter_id)
                .bind(sub_id)
                .execute(&state.db)
                .await;

                // On hard bounce (5xx), mark subscriber so we never send again
                if e.is_hard_bounce() {
                    tracing::warn!("Hard bounce for {email}, marking as bounced");
                    let _ = sqlx::query("UPDATE subscribers SET bounced_at = NOW() WHERE id = $1")
                        .bind(sub_id)
                        .execute(&state.db)
                        .await;
                }
            }
        }

        // Update progress
        let _ = sqlx::query(
            "UPDATE newsletters SET sent_count = $1, failed_count = $2, updated_at = NOW() WHERE id = $3",
        )
        .bind(sent_count)
        .bind(failed_count)
        .bind(newsletter_id)
        .execute(&state.db)
        .await;

        // Rate limit
        if rate_limit_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(rate_limit_ms)).await;
        }
    }

    // Check if we stopped because of a pause
    let current_status =
        sqlx::query_scalar::<_, String>("SELECT status FROM newsletters WHERE id = $1")
            .bind(newsletter_id)
            .fetch_one(&state.db)
            .await
            .map_err(|e| e.to_string())?;

    if current_status == "paused" {
        // Only update counts, keep paused status
        sqlx::query(
            "UPDATE newsletters SET sent_count = $1, failed_count = $2, updated_at = NOW() WHERE id = $3",
        )
        .bind(sent_count)
        .bind(failed_count)
        .bind(newsletter_id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

        tracing::info!(
            "Newsletter {newsletter_id} paused: {sent_count} sent, {failed_count} failed so far"
        );
    } else {
        // Mark as completed
        let final_status = if failed_count > 0 && sent_count == 0 {
            "failed"
        } else {
            "sent"
        };

        sqlx::query(
            "UPDATE newsletters SET status = $1, sending_completed_at = NOW(), sent_count = $2, failed_count = $3, updated_at = NOW() WHERE id = $4",
        )
        .bind(final_status)
        .bind(sent_count)
        .bind(failed_count)
        .bind(newsletter_id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

        tracing::info!(
            "Newsletter {newsletter_id} send complete: {sent_count} sent, {failed_count} failed"
        );
    }

    Ok(())
}

/// Background scheduler loop: checks for scheduled newsletters every `interval_secs`.
pub async fn newsletter_scheduler(
    state: AppState,
    shorturl_service: std::sync::Arc<dyn ShortUrlService>,
    interval_secs: u64,
    rate_limit_ms: u64,
) {
    let interval = std::time::Duration::from_secs(interval_secs);
    loop {
        tokio::time::sleep(interval).await;

        let due = sqlx::query_as::<_, (uuid::Uuid,)>(
            "SELECT id FROM newsletters WHERE status = 'scheduled' AND scheduled_at <= NOW()",
        )
        .fetch_all(&state.db)
        .await;

        match due {
            Ok(rows) => {
                for (newsletter_id,) in rows {
                    tracing::info!("Scheduler triggering newsletter {newsletter_id}");
                    let state_clone = state.clone();
                    let svc = shorturl_service.clone();
                    tokio::spawn(async move {
                        if let Err(e) = send_newsletter(
                            &state_clone,
                            newsletter_id,
                            svc.as_ref(),
                            rate_limit_ms,
                        )
                        .await
                        {
                            tracing::error!("Scheduled send failed for {newsletter_id}: {e}");
                        }
                    });
                }
            }
            Err(e) => {
                tracing::error!("Scheduler query failed: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_markdown_basic() {
        let html = render_markdown("# Hello\n\nWorld", "");
        assert!(html.contains("<h1>Hello</h1>"));
        assert!(html.contains("<p>World</p>"));
    }

    #[test]
    fn test_render_markdown_links() {
        let html = render_markdown("[COSCUP](https://coscup.org)", "");
        assert!(html.contains("href=\"https://coscup.org\""));
        assert!(html.contains("COSCUP"));
    }

    #[test]
    fn test_render_markdown_table() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let html = render_markdown(md, "");
        assert!(html.contains("<table>"));
    }

    #[test]
    fn test_render_markdown_strikethrough() {
        let html = render_markdown("~~deleted~~", "");
        assert!(html.contains("<del>deleted</del>"));
    }

    #[test]
    fn test_absolutize_image_srcs() {
        let html = r#"<img src="/uploads/abc.png" alt="test">"#;
        let result = absolutize_image_srcs(html, "https://example.com");
        assert_eq!(
            result,
            r#"<img src="https://example.com/uploads/abc.png" alt="test">"#
        );
    }

    #[test]
    fn test_absolutize_image_srcs_preserves_absolute() {
        let html = r#"<img src="https://cdn.example.com/img.png">"#;
        let result = absolutize_image_srcs(html, "https://example.com");
        assert_eq!(result, html);
    }

    #[test]
    fn test_absolutize_image_srcs_multiple() {
        let html = r#"<img src="/uploads/a.png"><img src="/static/logo.svg">"#;
        let result = absolutize_image_srcs(html, "https://example.com");
        assert!(result.contains(r#"src="https://example.com/uploads/a.png""#));
        assert!(result.contains(r#"src="https://example.com/static/logo.svg""#));
    }

    #[test]
    fn test_style_images_for_email() {
        let html = r#"<img src="https://example.com/uploads/abc.png" alt="test">"#;
        let result = style_images_for_email(html);
        assert!(result.contains(r#"style="max-width:100%;height:auto;display:block;""#));
        assert!(result.contains(r#"src="https://example.com/uploads/abc.png""#));
    }

    #[test]
    fn test_style_images_for_email_multiple() {
        let html = r#"<p>text</p><img src="a.png"><p>more</p><img src="b.png">"#;
        let result = style_images_for_email(html);
        assert_eq!(result.matches("max-width:100%").count(), 2);
    }

    #[test]
    fn test_sanitize_html_strips_script() {
        let html = r#"<p>Hello</p><script>alert('xss')</script><p>World</p>"#;
        let result = sanitize_html(html);
        assert!(!result.contains("<script>"));
        assert!(!result.contains("alert"));
        assert!(result.contains("<p>Hello</p>"));
        assert!(result.contains("<p>World</p>"));
    }

    #[test]
    fn test_sanitize_html_strips_event_handlers() {
        let html = r#"<img src="x.png" onload="alert(1)"><a href="https://coscup.org" onclick="evil()">Link</a>"#;
        let result = sanitize_html(html);
        assert!(!result.contains("onload"));
        assert!(!result.contains("onclick"));
        assert!(result.contains("https://coscup.org"));
    }

    #[test]
    fn test_sanitize_html_preserves_formatting() {
        let html = r#"<h1>Title</h1><p><strong>Bold</strong> and <em>italic</em></p><ul><li>Item</li></ul>"#;
        let result = sanitize_html(html);
        assert!(result.contains("<h1>Title</h1>"));
        assert!(result.contains("<strong>Bold</strong>"));
        assert!(result.contains("<em>italic</em>"));
        assert!(result.contains("<li>Item</li>"));
    }

    #[test]
    fn test_replace_recipient_name() {
        let html = "<p>Hello %recipient_name%, welcome!</p>";
        let result = replace_recipient_name(html, "Alice");
        assert_eq!(result, "<p>Hello Alice, welcome!</p>");
    }

    #[test]
    fn test_replace_recipient_name_multiple() {
        let html = "<p>Hi %recipient_name%</p><p>Dear %recipient_name%</p>";
        let result = replace_recipient_name(html, "Bob");
        assert_eq!(result.matches("Bob").count(), 2);
        assert!(!result.contains("%recipient_name%"));
    }

    #[test]
    fn test_replace_recipient_name_no_placeholder() {
        let html = "<p>Hello world</p>";
        let result = replace_recipient_name(html, "Alice");
        assert_eq!(result, html);
    }

    #[tokio::test]
    async fn test_shorten_links_basic() {
        use crate::shorturl::tests::MockShortUrlService;
        let svc = MockShortUrlService::default();
        let html = r#"<a href="https://coscup.org">COSCUP</a> and <a href="https://example.com">Example</a>"#;

        let (result, pairs) = shorten_links(html, &svc).await;
        assert_eq!(pairs.len(), 2);
        assert!(!result.contains("href=\"https://coscup.org\""));
        assert!(!result.contains("href=\"https://example.com\""));
        assert!(result.contains("href=\"https://s.coscup.org/test_"));
    }

    #[tokio::test]
    async fn test_shorten_links_skips_mailto() {
        use crate::shorturl::tests::MockShortUrlService;
        let svc = MockShortUrlService::default();
        let html = r#"<a href="mailto:test@example.com">Email</a>"#;

        let (result, pairs) = shorten_links(html, &svc).await;
        assert_eq!(pairs.len(), 0);
        assert!(result.contains("mailto:test@example.com"));
    }

    #[tokio::test]
    async fn test_shorten_links_skips_anchor() {
        use crate::shorturl::tests::MockShortUrlService;
        let svc = MockShortUrlService::default();
        let html = r##"<a href="#section">Jump</a>"##;

        let (result, pairs) = shorten_links(html, &svc).await;
        assert_eq!(pairs.len(), 0);
        assert!(result.contains("#section"));
    }

    #[tokio::test]
    async fn test_shorten_links_skips_template_vars() {
        use crate::shorturl::tests::MockShortUrlService;
        let svc = MockShortUrlService::default();
        let html = r#"<a href="{{ unsubscribe_url }}">Unsub</a>"#;

        let (result, pairs) = shorten_links(html, &svc).await;
        assert_eq!(pairs.len(), 0);
        assert!(result.contains("{{ unsubscribe_url }}"));
    }

    #[tokio::test]
    async fn test_shorten_links_fallback_on_failure() {
        use crate::shorturl::tests::MockShortUrlService;
        let svc = MockShortUrlService {
            should_fail: true,
            ..Default::default()
        };
        let html = r#"<a href="https://coscup.org">COSCUP</a>"#;

        let (result, pairs) = shorten_links(html, &svc).await;
        // On failure, link_map is empty (original URL kept via seen map)
        assert_eq!(pairs.len(), 0);
        assert!(result.contains("https://coscup.org"));
    }

    #[tokio::test]
    async fn test_shorten_links_dedup() {
        use crate::shorturl::tests::MockShortUrlService;
        let svc = MockShortUrlService::default();
        let html =
            r#"<a href="https://coscup.org">Link1</a> <a href="https://coscup.org">Link2</a>"#;

        let (_result, pairs) = shorten_links(html, &svc).await;
        // Same URL should only appear once
        assert_eq!(pairs.len(), 1);

        // Mock should only be called once
        let calls = svc.shorten_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn test_personalize_email() {
        let template = "<h1>{{ title }}</h1><div>{{ content }}</div><p>{{ tracking_pixel }}</p><a href=\"{{ unsubscribe_url }}\">Unsub</a><a href=\"{{ web_url }}\">Web</a>";
        let result = personalize_email(
            template,
            "<p>Hello world</p>",
            "Test Newsletter",
            "<img src=\"pixel.png\" />",
            "https://example.com/unsub",
            "https://example.com",
            "https://example.com/newsletters/test",
        )
        .unwrap();

        assert!(result.contains("Test Newsletter"));
        assert!(result.contains("<p>Hello world</p>"));
        assert!(result.contains("pixel.png"));
        assert!(result.contains("https://example.com/unsub"));
        assert!(result.contains("https://example.com/newsletters/test"));
    }

    #[test]
    fn test_build_tracking_pixel() {
        let pixel = build_tracking_pixel(
            "https://newsletter.coscup.org",
            "abc123",
            "newsletter-01",
            "hashvalue",
        );
        assert!(pixel.contains("r/o"));
        assert!(pixel.contains("ucode=abc123"));
        assert!(pixel.contains("topic=newsletter-01"));
        assert!(pixel.contains("hash=hashvalue"));
        assert!(pixel.contains("width=\"1\""));
        assert!(pixel.contains("height=\"1\""));
    }

    #[test]
    fn test_rewrite_links_for_tracking() {
        let secret = "mysecret";
        let ucode = "abc123";
        let topic = "nl-01";
        let url1 = "https://coscup.org";
        let url2 = "https://example.com/page";

        let html = format!(r#"<a href="{url1}">COSCUP</a> and <a href="{url2}">Example</a>"#);
        let result = rewrite_links_for_tracking(
            &html,
            "https://newsletter.coscup.org",
            ucode,
            topic,
            secret,
        );

        assert!(result.contains("/r/c?"));
        assert!(result.contains("ucode=abc123"));
        assert!(result.contains("topic=nl-01"));
        assert!(result.contains("url=https%3A%2F%2Fcoscup.org"));
        assert!(result.contains("url=https%3A%2F%2Fexample.com%2Fpage"));

        // Each link has its own per-URL hash
        let hash1 = security::compute_openhash(secret, ucode, topic, url1);
        let hash2 = security::compute_openhash(secret, ucode, topic, url2);
        assert_ne!(hash1, hash2);
        assert!(result.contains(&urlencoding::encode(&hash1).to_string()));
        assert!(result.contains(&urlencoding::encode(&hash2).to_string()));
    }

    #[test]
    fn test_rewrite_links_skips_non_http() {
        let html = r##"<a href="mailto:hi@coscup.org">Mail</a> <a href="#top">Top</a>"##;
        let result = rewrite_links_for_tracking(html, "https://x.com", "u", "t", "secret");
        // Non-http links should be unchanged
        assert!(result.contains("mailto:hi@coscup.org"));
        assert!(result.contains("#top"));
        assert!(!result.contains("/r/c"));
    }
}
