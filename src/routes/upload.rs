use axum::extract::{Multipart, State};
use axum::response::Json;

use crate::error::AppError;
use crate::AppState;

const ALLOWED_CONTENT_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/svg+xml",
];

fn extension_from_content_type(ct: &str) -> Option<&'static str> {
    match ct {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/svg+xml" => Some("svg"),
        _ => None,
    }
}

pub async fn upload_image(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, AppError> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?
    {
        if field.name() != Some("image") {
            continue;
        }

        let content_type = field.content_type().unwrap_or("").to_string();

        if !ALLOWED_CONTENT_TYPES.contains(&content_type.as_str()) {
            return Err(AppError::BadRequest(format!(
                "Unsupported image type: {content_type}. Allowed: png, jpg, gif, webp, svg"
            )));
        }

        let ext = extension_from_content_type(&content_type)
            .ok_or_else(|| AppError::BadRequest("Unknown content type".to_string()))?;

        let data = field
            .bytes()
            .await
            .map_err(|e| AppError::BadRequest(e.to_string()))?;

        if data.len() > state.config.max_upload_size_bytes {
            return Err(AppError::BadRequest(format!(
                "File too large. Max size: {} bytes",
                state.config.max_upload_size_bytes
            )));
        }

        let filename = format!("{}.{}", uuid::Uuid::new_v4(), ext);
        let filepath = std::path::Path::new(&state.config.upload_dir).join(&filename);

        tokio::fs::write(&filepath, &data)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to write file: {e}")))?;

        return Ok(Json(serde_json::json!({
            "url": format!("/uploads/{filename}")
        })));
    }

    Err(AppError::BadRequest(
        "No image field found in upload".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_from_content_type() {
        assert_eq!(extension_from_content_type("image/png"), Some("png"));
        assert_eq!(extension_from_content_type("image/jpeg"), Some("jpg"));
        assert_eq!(extension_from_content_type("image/gif"), Some("gif"));
        assert_eq!(extension_from_content_type("image/webp"), Some("webp"));
        assert_eq!(extension_from_content_type("image/svg+xml"), Some("svg"));
        assert_eq!(extension_from_content_type("text/html"), None);
        assert_eq!(extension_from_content_type("application/pdf"), None);
    }

    #[test]
    fn test_allowed_content_types() {
        assert!(ALLOWED_CONTENT_TYPES.contains(&"image/png"));
        assert!(ALLOWED_CONTENT_TYPES.contains(&"image/jpeg"));
        assert!(ALLOWED_CONTENT_TYPES.contains(&"image/gif"));
        assert!(ALLOWED_CONTENT_TYPES.contains(&"image/webp"));
        assert!(ALLOWED_CONTENT_TYPES.contains(&"image/svg+xml"));
        assert!(!ALLOWED_CONTENT_TYPES.contains(&"text/html"));
    }

    #[test]
    fn test_filename_generation() {
        let ext = "png";
        let filename = format!("{}.{}", uuid::Uuid::new_v4(), ext);
        assert!(filename.ends_with(".png"));
        assert!(filename.len() > 4);
        // UUID v4 format check
        let parts: Vec<&str> = filename.trim_end_matches(".png").split('-').collect();
        assert_eq!(parts.len(), 5);
    }
}
