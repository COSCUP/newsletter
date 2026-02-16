use std::net::IpAddr;

use serde_json::Value as JsonValue;
use sqlx::PgPool;

pub async fn log(
    pool: &PgPool,
    admin_email: &str,
    action: &str,
    details: Option<JsonValue>,
    ip: Option<IpAddr>,
) {
    let ip_str = ip.map(|i| i.to_string());
    let result = sqlx::query(
        "INSERT INTO audit_log (admin_email, action, details, ip_address) VALUES ($1, $2, $3, $4)",
    )
    .bind(admin_email)
    .bind(action)
    .bind(&details)
    .bind(&ip_str)
    .execute(pool)
    .await;

    if let Err(e) = result {
        tracing::error!("Failed to write audit log: {e}");
    }
}
