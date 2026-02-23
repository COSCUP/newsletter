use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::Router;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

mod audit;
mod auth;
mod captcha;
mod config;
mod csv_handler;
mod db;
mod email;
mod error;
mod newsletter;
mod routes;
mod security;
mod shorturl;

use captcha::CaptchaVerifier;
use email::EmailService;
use shorturl::ShortUrlService;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
    pub config: config::AppConfig,
    pub tera: tera::Tera,
    pub email: Arc<dyn EmailService>,
    pub captcha: Arc<dyn CaptchaVerifier>,
    pub shorturl: Arc<dyn ShortUrlService>,
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

#[allow(clippy::too_many_lines)]
fn build_router(state: AppState) -> Router {
    // Public routes (no auth required)
    let public_routes = Router::new()
        .route("/health", get(health))
        .route("/", get(routes::subscribe::subscribe_page))
        .route("/subscribe/coscup", get(|| async { Redirect::to("/") }))
        .route("/api/subscribe", post(routes::subscribe::subscribe_api))
        .route("/verify/{token}", get(routes::subscribe::verify_email))
        .route("/manage/{admin_link}", get(routes::manage::manage_page))
        .route(
            "/manage/{admin_link}/update",
            post(routes::manage::update_name),
        )
        .route(
            "/manage/{admin_link}/unsubscribe",
            post(routes::manage::unsubscribe),
        )
        .route(
            "/manage/{admin_link}/resubscribe",
            post(routes::manage::resubscribe),
        )
        .route(
            "/unsubscribe/{admin_link}",
            post(routes::manage::one_click_unsubscribe),
        )
        .route("/newsletters", get(routes::archive::list))
        .route("/newsletters/{slug}", get(routes::archive::view))
        .route("/r/o", get(routes::tracking::track_open))
        .route("/r/c", get(routes::tracking::track_click))
        // Admin login/auth (must be accessible without session)
        .route("/admin/login", get(routes::admin::login_page))
        .route("/admin/login", post(routes::admin::login_submit))
        .route("/admin/auth/{token}", get(routes::admin::auth_magic_link));

    // Admin routes (protected by auth middleware)
    let admin_routes = Router::new()
        .route("/admin", get(routes::admin::dashboard))
        .route("/admin/subscribers", get(routes::admin::subscribers_list))
        .route("/admin/subscribers/import", post(routes::admin::import_csv))
        .route("/admin/subscribers/export", get(routes::admin::export_csv))
        .route(
            "/admin/subscribers/{id}/toggle",
            post(routes::admin::toggle_status),
        )
        .route(
            "/admin/subscribers/{id}/resend",
            post(routes::admin::resend_verification),
        )
        .route("/admin/stats", get(routes::admin::stats_page))
        .route("/admin/logout", post(routes::admin::logout))
        // Newsletter admin routes
        .route("/admin/newsletters", get(routes::newsletter::list))
        .route(
            "/admin/newsletters/new",
            get(routes::newsletter::new_form).post(routes::newsletter::create),
        )
        .route(
            "/admin/newsletters/{id}",
            get(routes::newsletter::edit_form).post(routes::newsletter::update),
        )
        .route(
            "/admin/newsletters/{id}/preview",
            get(routes::newsletter::preview),
        )
        .route(
            "/admin/newsletters/{id}/send",
            post(routes::newsletter::send_now),
        )
        .route(
            "/admin/newsletters/{id}/schedule",
            post(routes::newsletter::schedule),
        )
        .route(
            "/admin/newsletters/{id}/cancel",
            post(routes::newsletter::cancel),
        )
        .route(
            "/admin/newsletters/{id}/status",
            get(routes::newsletter::status_json),
        )
        .route(
            "/admin/newsletters/{id}/stats",
            get(routes::newsletter::stats),
        )
        .route(
            "/admin/newsletters/{id}/delete",
            post(routes::newsletter::delete),
        )
        // Image upload (increased body limit for large images)
        .route(
            "/admin/upload/image",
            post(routes::upload::upload_image)
                .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024)),
        )
        // Template management routes
        .route("/admin/templates", get(routes::template::list))
        .route(
            "/admin/templates/new",
            get(routes::template::new_form).post(routes::template::create),
        )
        .route(
            "/admin/templates/{id}",
            get(routes::template::edit_form).post(routes::template::update),
        )
        .route(
            "/admin/templates/{id}/delete",
            post(routes::template::delete),
        )
        .route(
            "/admin/templates/{id}/preview",
            get(routes::template::preview),
        )
        .route(
            "/admin/templates/{id}/duplicate",
            post(routes::template::duplicate),
        )
        // Admin management routes
        .route("/admin/admins", get(routes::admin_mgmt::admins_list))
        .route("/admin/admins/add", post(routes::admin_mgmt::add_admin))
        .route(
            "/admin/admins/{id}/remove",
            post(routes::admin_mgmt::remove_admin),
        )
        .route("/admin/audit-log", get(routes::admin_mgmt::audit_log_page))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::admin_auth_middleware,
        ));

    public_routes
        .merge(admin_routes)
        .nest_service("/uploads", ServeDir::new(&state.config.upload_dir))
        .nest_service("/static", ServeDir::new("static"))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    dotenvy::dotenv().ok();

    let config = config::AppConfig::from_env().expect("Failed to load config");

    // Ensure upload directory exists
    std::fs::create_dir_all(&config.upload_dir).expect("Failed to create upload directory");

    let pool = db::create_pool(&config.database_url)
        .await
        .expect("Failed to create DB pool");

    db::run_migrations(&pool)
        .await
        .expect("Failed to run migrations");

    db::sync_seed_admins(&pool, &config.admin_emails)
        .await
        .expect("Failed to sync seed admins");

    let tera = tera::Tera::new("src/templates/**/*.html").expect("Failed to load templates");

    let email_service: Arc<dyn EmailService> = Arc::new(
        email::SmtpEmailService::new(
            &config.smtp_host,
            config.smtp_port,
            config.smtp_username.as_deref(),
            config.smtp_password.as_deref(),
            config.smtp_tls,
            config.smtp_from_email.clone(),
        )
        .expect("Failed to create SMTP email service"),
    );

    let captcha_verifier: Arc<dyn CaptchaVerifier> = Arc::new(captcha::TurnstileVerifier::new(
        config.turnstile_secret.clone(),
    ));

    // Create YOURLS short URL service (or a passthrough if not configured)
    let shorturl_service: Arc<dyn ShortUrlService> = if let (Some(api_url), Some(signature)) =
        (&config.yourls_api_url, &config.yourls_signature)
    {
        Arc::new(shorturl::YourlsService::new(
            api_url.clone(),
            signature.clone(),
        ))
    } else {
        tracing::warn!(
                "YOURLS not configured (YOURLS_API_URL / YOURLS_SIGNATURE missing), short URLs disabled"
            );
        Arc::new(PassthroughShortUrlService)
    };

    let state = AppState {
        db: pool,
        config: config.clone(),
        tera,
        email: email_service,
        captcha: captcha_verifier,
        shorturl: shorturl_service,
    };

    // Spawn newsletter scheduler
    let scheduler_state = state.clone();
    let scheduler_interval = config.newsletter_scheduler_interval_secs;
    let rate_limit = config.smtp_rate_limit_ms;
    tokio::spawn(async move {
        newsletter::newsletter_scheduler(
            scheduler_state.clone(),
            scheduler_state.shorturl.clone(),
            scheduler_interval,
            rate_limit,
        )
        .await;
    });

    let app = build_router(state);

    let addr = format!("{}:{}", config.host, config.port);
    tracing::info!("Starting server on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .expect("Server error");
}

/// Passthrough service that returns original URLs when YOURLS is not configured.
struct PassthroughShortUrlService;

#[async_trait::async_trait]
impl ShortUrlService for PassthroughShortUrlService {
    async fn shorten(&self, url: &str) -> Result<String, shorturl::ShortUrlError> {
        Ok(url.to_string())
    }

    async fn get_clicks(&self, _short_url: &str) -> Result<u64, shorturl::ShortUrlError> {
        Ok(0)
    }
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install signal handler");
    tracing::info!("Shutting down...");
}
