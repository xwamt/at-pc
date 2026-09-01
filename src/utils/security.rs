//! Security utilities for random PIN generation and PIN verification.

use rand::Rng;

/// Generates a secure random 4-digit numeric PIN (e.g., "0482", "9182").
pub fn generate_pin() -> String {
    let mut rng = rand::thread_rng();
    let pin_num: u32 = rng.gen_range(0..10000);
    format!("{:04}", pin_num)
}

/// Verifies whether the provided PIN matches the expected PIN.
/// Returns false if either is empty.
pub fn verify_pin(expected: &str, provided: &str) -> bool {
    if expected.is_empty() || provided.is_empty() {
        return false;
    }
    expected.trim() == provided.trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_pin() {
        for _ in 0..50 {
            let pin = generate_pin();
            assert_eq!(pin.len(), 4);
            assert!(pin.chars().all(|c| c.is_ascii_digit()));
        }
    }

    #[test]
    fn test_verify_pin() {
        assert!(verify_pin("1234", "1234"));
        assert!(verify_pin("1234", " 1234 "));
        assert!(!verify_pin("1234", "5678"));
        assert!(!verify_pin("", "1234"));
        assert!(!verify_pin("1234", ""));
    }
}
