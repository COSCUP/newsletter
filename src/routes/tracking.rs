use axum::extract::{Query, State};
use axum::http::header;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;

use crate::error::AppError;
use crate::security;
use crate::AppState;

// 1x1 transparent PNG
const TRANSPARENT_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x62, 0x00, 0x00, 0x00, 0x02,
    0x00, 0x01, 0xE2, 0x21, 0xBC, 0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42,
    0x60, 0x82,
];

#[derive(Deserialize)]
pub struct TrackingQuery {
    pub ucode: String,
    pub topic: String,
    pub hash: String,
    pub url: Option<String>,
}

pub async fn track_open(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<TrackingQuery>,
) -> Result<Response, AppError> {
    // Verify openhash
    let subscriber =
        sqlx::query_as::<_, (String,)>("SELECT secret_code FROM subscribers WHERE ucode = $1")
            .bind(&query.ucode)
            .fetch_optional(&state.db)
            .await?;

    if let Some((secret_code,)) = subscriber {
        if security::verify_openhash(&secret_code, &query.ucode, &query.topic, "", &query.hash) {
            let user_agent = headers
                .get(header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            // Record event (best-effort, don't fail on error)
            let _ = sqlx::query(
                "INSERT INTO email_events (ucode, event_type, topic, user_agent) VALUES ($1, 'open', $2, $3)",
            )
            .bind(&query.ucode)
            .bind(&query.topic)
            .bind(&user_agent)
            .execute(&state.db)
            .await;
        }
    }

    // Always return the transparent PNG regardless of verification result
    Ok((
        [(header::CONTENT_TYPE, "image/png")],
        TRANSPARENT_PNG.to_vec(),
    )
        .into_response())
}

pub async fn track_click(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<TrackingQuery>,
) -> Result<Response, AppError> {
    let redirect_url = query
        .url
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("Missing url parameter".to_string()))?;

    // Validate redirect URL to prevent open redirect attacks
    if !redirect_url.starts_with("https://") && !redirect_url.starts_with("http://") {
        return Err(AppError::BadRequest("Invalid redirect URL".to_string()));
    }

    // Verify openhash
    let subscriber =
        sqlx::query_as::<_, (String,)>("SELECT secret_code FROM subscribers WHERE ucode = $1")
            .bind(&query.ucode)
            .fetch_optional(&state.db)
            .await?;

    if let Some((secret_code,)) = subscriber {
        if security::verify_openhash(
            &secret_code,
            &query.ucode,
            &query.topic,
            redirect_url,
            &query.hash,
        ) {
            let user_agent = headers
                .get(header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            let _ = sqlx::query(
                "INSERT INTO email_events (ucode, event_type, topic, user_agent, clicked_url) VALUES ($1, 'click', $2, $3, $4)",
            )
            .bind(&query.ucode)
            .bind(&query.topic)
            .bind(&user_agent)
            .bind(redirect_url)
            .execute(&state.db)
            .await;
        }
    }

    Ok(Redirect::temporary(redirect_url).into_response())
}
