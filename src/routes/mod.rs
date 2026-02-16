use std::net::{IpAddr, SocketAddr};

use axum::extract::ConnectInfo;
use axum::http::HeaderMap;

pub mod admin;
pub mod admin_mgmt;
pub mod archive;
pub mod manage;
pub mod newsletter;
pub mod subscribe;
pub mod template;
pub mod tracking;
pub mod upload;

/// Extract client IP from `X-Forwarded-For` header, falling back to `ConnectInfo`.
pub(crate) fn extract_client_ip(
    headers: &HeaderMap,
    connect_info: &ConnectInfo<SocketAddr>,
) -> IpAddr {
    if let Some(forwarded_for) = headers.get("x-forwarded-for") {
        if let Ok(value) = forwarded_for.to_str() {
            if let Some(first_ip) = value.split(',').next() {
                if let Ok(ip) = first_ip.trim().parse::<IpAddr>() {
                    return ip;
                }
            }
        }
    }
    connect_info.0.ip()
}
