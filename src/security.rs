use hmac::{Hmac, Mac};
use rand::Rng;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Generate a random secret code (32 bytes → 64 hex chars).
pub fn generate_secret_code() -> String {
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes);
    hex::encode(bytes)
}

/// Generate a random token (32 bytes → 64 hex chars).
pub fn generate_token() -> String {
    generate_secret_code()
}

/// Generate a short ucode (8 hex chars).
pub fn generate_ucode() -> String {
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 4];
    rng.fill(&mut bytes);
    hex::encode(bytes)
}

/// Compute `admin_link` = `SHA256`(`secret_code` || email).
pub fn compute_admin_link(secret_code: &str, email: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret_code.as_bytes());
    hasher.update(email.as_bytes());
    hex::encode(hasher.finalize())
}

/// Compute openhash = HMAC-SHA256(secret_code, "ucode:topic:url").
/// For open-tracking (no URL), pass `url = ""`.
pub fn compute_openhash(secret_code: &str, ucode: &str, topic: &str, url: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret_code.as_bytes()).expect("HMAC accepts any key length");
    let message = format!("{ucode}:{topic}:{url}");
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Constant-time comparison for `admin_link` verification.
pub fn verify_admin_link(provided: &str, expected: &str) -> bool {
    let a = provided.as_bytes();
    let b = expected.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

/// Verify openhash using constant-time comparison.
/// For open-tracking (no URL), pass `url = ""`.
pub fn verify_openhash(
    secret_code: &str,
    ucode: &str,
    topic: &str,
    url: &str,
    provided: &str,
) -> bool {
    let expected = compute_openhash(secret_code, ucode, topic, url);
    verify_admin_link(provided, &expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_secret_code_length() {
        let code = generate_secret_code();
        assert_eq!(code.len(), 64);
        assert!(hex::decode(&code).is_ok());
    }

    #[test]
    fn test_generate_secret_code_uniqueness() {
        let a = generate_secret_code();
        let b = generate_secret_code();
        assert_ne!(a, b);
    }

    #[test]
    fn test_generate_ucode_length() {
        let ucode = generate_ucode();
        assert_eq!(ucode.len(), 8);
        assert!(hex::decode(&ucode).is_ok());
    }

    #[test]
    fn test_compute_admin_link_deterministic() {
        let link1 = compute_admin_link("abc123", "test@example.com");
        let link2 = compute_admin_link("abc123", "test@example.com");
        assert_eq!(link1, link2);
    }

    #[test]
    fn test_compute_admin_link_different_inputs() {
        let link1 = compute_admin_link("abc123", "test@example.com");
        let link2 = compute_admin_link("def456", "test@example.com");
        assert_ne!(link1, link2);
    }

    #[test]
    fn test_verify_admin_link_correct() {
        let link = compute_admin_link("secret", "user@test.com");
        assert!(verify_admin_link(&link, &link));
    }

    #[test]
    fn test_verify_admin_link_wrong() {
        assert!(!verify_admin_link("aaa", "bbb"));
    }

    #[test]
    fn test_verify_admin_link_different_lengths() {
        assert!(!verify_admin_link("short", "muchlongerstring"));
    }

    #[test]
    fn test_compute_openhash_deterministic() {
        let h1 = compute_openhash("secret", "abc123", "newsletter-01", "");
        let h2 = compute_openhash("secret", "abc123", "newsletter-01", "");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_compute_openhash_url_changes_hash() {
        let h1 = compute_openhash("secret", "abc123", "newsletter-01", "");
        let h2 = compute_openhash("secret", "abc123", "newsletter-01", "https://coscup.org");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_verify_openhash_correct() {
        let hash = compute_openhash("secret", "abc123", "newsletter-01", "");
        assert!(verify_openhash(
            "secret",
            "abc123",
            "newsletter-01",
            "",
            &hash
        ));
    }

    #[test]
    fn test_verify_openhash_correct_with_url() {
        let url = "https://coscup.org/2025";
        let hash = compute_openhash("secret", "abc123", "newsletter-01", url);
        assert!(verify_openhash(
            "secret",
            "abc123",
            "newsletter-01",
            url,
            &hash
        ));
    }

    #[test]
    fn test_verify_openhash_wrong_url() {
        let hash = compute_openhash("secret", "abc123", "newsletter-01", "https://coscup.org");
        assert!(!verify_openhash(
            "secret",
            "abc123",
            "newsletter-01",
            "https://evil.com",
            &hash
        ));
    }

    #[test]
    fn test_verify_openhash_wrong_topic() {
        let hash = compute_openhash("secret", "abc123", "newsletter-01", "");
        assert!(!verify_openhash(
            "secret",
            "abc123",
            "newsletter-02",
            "",
            &hash
        ));
    }

    #[test]
    fn test_verify_openhash_wrong_secret() {
        let hash = compute_openhash("secret", "abc123", "newsletter-01", "");
        assert!(!verify_openhash(
            "wrong",
            "abc123",
            "newsletter-01",
            "",
            &hash
        ));
    }

    #[test]
    fn test_generate_token_length() {
        let token = generate_token();
        assert_eq!(token.len(), 64);
    }
}
