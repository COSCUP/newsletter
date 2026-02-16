use axum::extract::State;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use axum_extra::extract::CookieJar;
use chrono::Utc;

use crate::error::AppError;
use crate::AppState;

pub const SESSION_COOKIE: &str = "admin_session";

#[derive(Clone)]
struct AdminEmail(String);

/// Extractor for authenticated admin users.
///
/// Reads the email from request extensions (set by `admin_auth_middleware`).
/// If used outside the middleware-protected route group, returns `Unauthorized`.
#[derive(Debug)]
pub struct AdminUser(pub String);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for AdminUser {
    type Rejection = AppError;

    fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        let result = parts
            .extensions
            .get::<AdminEmail>()
            .map(|e| Self(e.0.clone()))
            .ok_or(AppError::Unauthorized);
        std::future::ready(result)
    }
}

/// Middleware that verifies admin session from cookie and stores the email
/// in request extensions for downstream extractors.
pub async fn admin_auth_middleware(
    State(state): State<AppState>,
    jar: CookieJar,
    mut req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, AppError> {
    let email = get_admin_email_from_jar(&state, &jar).await?;
    req.extensions_mut().insert(AdminEmail(email));
    Ok(next.run(req).await)
}

async fn get_admin_email_from_jar(state: &AppState, jar: &CookieJar) -> Result<String, AppError> {
    let token = jar
        .get(SESSION_COOKIE)
        .map(|c| c.value().to_string())
        .ok_or(AppError::Unauthorized)?;

    let now = Utc::now();
    let email = sqlx::query_scalar::<_, String>(
        "SELECT admin_email FROM admin_sessions WHERE session_token = $1 AND expires_at > $2",
    )
    .bind(&token)
    .bind(now)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Unauthorized)?;

    Ok(email)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::FromRequestParts;
    use axum::http::request::Parts;

    fn make_parts() -> Parts {
        let (parts, _body) = Request::builder().body(()).unwrap().into_parts();
        parts
    }

    #[tokio::test]
    async fn admin_user_from_extensions_success() {
        let mut parts = make_parts();
        parts
            .extensions
            .insert(AdminEmail("admin@example.com".to_string()));

        let result = AdminUser::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "admin@example.com");
    }

    #[tokio::test]
    async fn admin_user_from_extensions_missing() {
        let mut parts = make_parts();

        let result = AdminUser::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::Unauthorized));
    }
}
