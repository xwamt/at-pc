//! Authentication utilities for HTTP/SSE requests.

use crate::utils::security::verify_pin;

/// Validates the Authorization header value against an expected PIN.
///
/// Supports `Bearer <PIN>`, `bearer <PIN>`, and direct `<PIN>` formats.
/// Returns true if the PIN is non-empty and matches `expected_pin`.
pub fn validate_auth_header(expected_pin: &str, header_val: Option<&str>) -> bool {
    if expected_pin.is_empty() {
        return false;
    }

    let Some(raw_val) = header_val else {
        return false;
    };

    let trimmed = raw_val.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Check for "Bearer " prefix (case-insensitive)
    let token = if trimmed.len() >= 7 && trimmed[..7].eq_ignore_ascii_case("bearer ") {
        trimmed[7..].trim()
    } else {
        trimmed
    };

    verify_pin(expected_pin, token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_auth_header() {
        let pin = "1234";
        assert!(validate_auth_header(pin, Some("Bearer 1234")));
        assert!(validate_auth_header(pin, Some("bearer 1234")));
        assert!(validate_auth_header(pin, Some("Bearer  1234 ")));
        assert!(validate_auth_header(pin, Some("1234")));

        assert!(!validate_auth_header(pin, Some("Bearer 5678")));
        assert!(!validate_auth_header(pin, Some("Bearer ")));
        assert!(!validate_auth_header(pin, Some("")));
        assert!(!validate_auth_header(pin, None));
        assert!(!validate_auth_header("", Some("Bearer 1234")));
    }
}
