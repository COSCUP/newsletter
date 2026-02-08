use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use tower_http::trace::TraceLayer;

mod captcha;
mod config;
mod csv_handler;
mod db;
mod email;
mod error;
mod routes;
mod security;

use captcha::CaptchaVerifier;
use email::EmailService;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
    pub config: config::AppConfig,
    pub tera: tera::Tera,
    pub email: Arc<dyn EmailService>,
    pub captcha: Arc<dyn CaptchaVerifier>,
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

fn build_router(state: AppState) -> Router {
    Router::new()
        // Public routes
        .route("/health", get(health))
        .route("/", get(routes::subscribe::subscribe_page))
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
        .route("/track/open", get(routes::tracking::track_open))
        .route("/track/click", get(routes::tracking::track_click))
        // Admin routes
        .route("/admin/login", get(routes::admin::login_page))
        .route("/admin/login", post(routes::admin::login_submit))
        .route("/admin/auth/{token}", get(routes::admin::auth_magic_link))
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
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    dotenvy::dotenv().ok();

    let config = config::AppConfig::from_env().expect("Failed to load config");
    let pool = db::create_pool(&config.database_url)
        .await
        .expect("Failed to create DB pool");

    db::run_migrations(&pool)
        .await
        .expect("Failed to run migrations");

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

    let state = AppState {
        db: pool,
        config: config.clone(),
        tera,
        email: email_service,
        captcha: captcha_verifier,
    };

    let app = build_router(state);

    let addr = format!("{}:{}", config.host, config.port);
    tracing::info!("Starting server on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Server error");
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install signal handler");
    tracing::info!("Shutting down...");
}
