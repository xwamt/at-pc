use at_pc_server::keystore::{
    compute_hash, constant_time_compare, parse_credential, AuthError, KeyKind, KeyStore,
};
use at_pc_server::rate_limit::{AuthRateLimiter, RateLimitDecision};
use at_pc_server::Role;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn temp_data_dir(test_name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "atpc-test-{}-{}-{}",
        test_name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn test_key_format_parsing_valid() {
    let valid_secret = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopq";
    assert_eq!(valid_secret.len(), 43);

    // SecretKey
    let cred_sk = format!("atpc_sk_01234567_{valid_secret}");
    let (kind, id, secret) = parse_credential(&cred_sk).expect("valid sk parse");
    assert_eq!(kind, KeyKind::SecretKey);
    assert_eq!(id, "01234567");
    assert_eq!(secret, valid_secret);

    // DeviceToken
    let cred_dv = format!("atpc_dv_89abcdef_{valid_secret}");
    let (kind, id, secret) = parse_credential(&cred_dv).expect("valid dv parse");
    assert_eq!(kind, KeyKind::DeviceToken);
    assert_eq!(id, "89abcdef");
    assert_eq!(secret, valid_secret);

    // Base64url chars: '-' and '_' in secret
    let b64url_secret = "ABC-_0123456789abcdefghijklmnopqrstuvwxyz01";
    assert_eq!(b64url_secret.len(), 43);
    let cred_b64url = format!("atpc_sk_01234567_{b64url_secret}");
    let (_, _, secret) = parse_credential(&cred_b64url).expect("valid base64url characters");
    assert_eq!(secret, b64url_secret);
}

#[test]
fn test_key_format_parsing_malformed() {
    let valid_secret = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopq";

    // Wrong prefix
    assert_eq!(
        parse_credential(&format!("wrong_sk_01234567_{valid_secret}")),
        Err(AuthError::InvalidFormat)
    );
    // Uppercase prefix
    assert_eq!(
        parse_credential(&format!("ATPC_sk_01234567_{valid_secret}")),
        Err(AuthError::InvalidFormat)
    );
    // Wrong kind
    assert_eq!(
        parse_credential(&format!("atpc_xx_01234567_{valid_secret}")),
        Err(AuthError::InvalidFormat)
    );
    // Uppercase kind
    assert_eq!(
        parse_credential(&format!("atpc_SK_01234567_{valid_secret}")),
        Err(AuthError::InvalidFormat)
    );
    // Empty
    assert_eq!(parse_credential(""), Err(AuthError::InvalidFormat));
    // Missing secret
    assert_eq!(
        parse_credential("atpc_sk_01234567"),
        Err(AuthError::InvalidFormat)
    );
    assert_eq!(
        parse_credential("atpc_sk_01234567_"),
        Err(AuthError::InvalidFormat)
    );
    // Missing id and secret
    assert_eq!(parse_credential("atpc_sk_"), Err(AuthError::InvalidFormat));
}

#[test]
fn test_key_format_parsing_lengths_and_charsets() {
    let valid_secret = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopq";

    // Id too short (7 chars)
    assert_eq!(
        parse_credential(&format!("atpc_sk_0123456_{valid_secret}")),
        Err(AuthError::InvalidFormat)
    );
    // Id too long (9 chars)
    assert_eq!(
        parse_credential(&format!("atpc_sk_012345678_{valid_secret}")),
        Err(AuthError::InvalidFormat)
    );

    // Non-Crockford32 chars in ID (i, l, o, u excluded)
    assert_eq!(
        parse_credential(&format!("atpc_sk_0123456i_{valid_secret}")),
        Err(AuthError::InvalidFormat)
    );
    assert_eq!(
        parse_credential(&format!("atpc_sk_0123456l_{valid_secret}")),
        Err(AuthError::InvalidFormat)
    );
    assert_eq!(
        parse_credential(&format!("atpc_sk_0123456o_{valid_secret}")),
        Err(AuthError::InvalidFormat)
    );
    assert_eq!(
        parse_credential(&format!("atpc_sk_0123456u_{valid_secret}")),
        Err(AuthError::InvalidFormat)
    );
    // Uppercase in ID rejected (Crockford base32 lowercase required)
    assert_eq!(
        parse_credential(&format!("atpc_sk_0123456A_{valid_secret}")),
        Err(AuthError::InvalidFormat)
    );
    // Symbol in ID
    assert_eq!(
        parse_credential(&format!("atpc_sk_0123456!_{valid_secret}")),
        Err(AuthError::InvalidFormat)
    );

    // Secret too short (42 chars)
    let short_sec = &valid_secret[..42];
    assert_eq!(
        parse_credential(&format!("atpc_sk_01234567_{short_sec}")),
        Err(AuthError::InvalidFormat)
    );
    // Secret too long (44 chars)
    let long_sec = format!("{valid_secret}X");
    assert_eq!(
        parse_credential(&format!("atpc_sk_01234567_{long_sec}")),
        Err(AuthError::InvalidFormat)
    );

    // Invalid characters in secret
    let space_sec = format!("{} ", &valid_secret[..42]);
    assert_eq!(
        parse_credential(&format!("atpc_sk_01234567_{space_sec}")),
        Err(AuthError::InvalidFormat)
    );
    let plus_sec = format!("{}+", &valid_secret[..42]);
    assert_eq!(
        parse_credential(&format!("atpc_sk_01234567_{plus_sec}")),
        Err(AuthError::InvalidFormat)
    );
    let slash_sec = format!("{}/", &valid_secret[..42]);
    assert_eq!(
        parse_credential(&format!("atpc_sk_01234567_{slash_sec}")),
        Err(AuthError::InvalidFormat)
    );
    let equal_sec = format!("{}=", &valid_secret[..42]);
    assert_eq!(
        parse_credential(&format!("atpc_sk_01234567_{equal_sec}")),
        Err(AuthError::InvalidFormat)
    );
}

#[test]
fn test_salt_hash_determinism_and_differentiation() {
    let salt1 = "0123456789abcdef0123456789abcdef";
    let salt2 = "0123456789abcdef0123456789abcdee"; // 1 bit different
    let secret1 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrs";
    let secret2 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrt"; // 1 char different

    let hash1a = compute_hash(salt1, secret1);
    let hash1b = compute_hash(salt1, secret1);
    assert_eq!(hash1a, hash1b, "hash computation must be deterministic");
    assert_eq!(hash1a.len(), 64, "sha256 hex string must be 64 characters");

    let hash_diff_salt = compute_hash(salt2, secret1);
    assert_ne!(
        hash1a, hash_diff_salt,
        "changing salt must completely change hash"
    );

    let hash_diff_secret = compute_hash(salt1, secret2);
    assert_ne!(
        hash1a, hash_diff_secret,
        "changing secret must completely change hash"
    );
}

#[test]
fn test_constant_time_comparison() {
    let salt = "a1b2c3d4e5f60718293a4b5c6d7e8f90";
    let secret = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrs";
    let hash = compute_hash(salt, secret);
    let hash_bytes = hash.as_bytes();

    // Identical
    assert!(constant_time_compare(hash_bytes, hash_bytes));

    // Tampered first byte
    let mut tampered_first = hash_bytes.to_vec();
    tampered_first[0] ^= 0x01;
    assert!(!constant_time_compare(hash_bytes, &tampered_first));

    // Tampered middle byte
    let mut tampered_mid = hash_bytes.to_vec();
    let mid = hash_bytes.len() / 2;
    tampered_mid[mid] ^= 0x01;
    assert!(!constant_time_compare(hash_bytes, &tampered_mid));

    // Tampered last byte
    let mut tampered_last = hash_bytes.to_vec();
    let last = hash_bytes.len() - 1;
    tampered_last[last] ^= 0x01;
    assert!(!constant_time_compare(hash_bytes, &tampered_last));

    // Different length
    assert!(!constant_time_compare(hash_bytes, &hash_bytes[..63]));
    assert!(!constant_time_compare(&hash_bytes[..63], hash_bytes));
}

#[test]
fn test_keystore_issue_and_verify() {
    let dir = temp_data_dir("issue-verify");
    let keys_file = dir.join("keys.json");
    let keystore = KeyStore::load_or_create(&keys_file).expect("keystore creation");

    // File permissions on Unix should be 0600
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&keys_file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "keys.json permissions must be 0600");
    }

    // Issue an Admin key
    let (admin_record, admin_key) = keystore
        .issue(
            "admin-1".to_string(),
            KeyKind::SecretKey,
            Some(Role::Admin),
            vec!["console".to_string(), "mcp".to_string()],
            None,
            None,
        )
        .expect("issue admin key");

    assert_eq!(admin_record.name, "admin-1");
    assert_eq!(admin_record.kind, KeyKind::SecretKey);
    assert_eq!(admin_record.role, Some(Role::Admin));

    // Plaintext key not in record
    assert_eq!(admin_record.hash.len(), 64);
    assert_eq!(admin_record.salt.len(), 32);

    // Verify succeeds
    let principal = keystore.verify(&admin_key).expect("verify admin key");
    assert_eq!(principal.id, admin_record.id);
    assert_eq!(principal.role, Some(Role::Admin));
    assert_eq!(principal.kind, KeyKind::SecretKey);
    assert!(principal.has_scope("console"));
    assert!(principal.has_scope("mcp"));

    // Verify with tampered secret -> HashMismatch
    let tampered_key = format!("{}x", &admin_key[..admin_key.len() - 1]);
    assert_eq!(
        keystore.verify(&tampered_key),
        Err(AuthError::HashMismatch)
    );

    // Non-existent key -> KeyNotFound
    let non_existent = "atpc_sk_00000000_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopq";
    assert_eq!(
        keystore.verify(non_existent),
        Err(AuthError::KeyNotFound)
    );

    // Wrong kind: verify with dv prefix against sk key -> WrongKind
    let dv_mismatch = format!("atpc_dv_{}", &admin_key[8..]);
    assert_eq!(keystore.verify(&dv_mismatch), Err(AuthError::WrongKind));

    // List and get_summary
    let list = keystore.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, admin_record.id);
    assert_eq!(list[0].name, "admin-1");

    let summary = keystore
        .get_summary(&admin_record.id)
        .expect("summary exists");
    assert_eq!(summary.id, admin_record.id);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_expiration() {
    let dir = temp_data_dir("expiration");
    let keys_file = dir.join("keys.json");
    let keystore = KeyStore::load_or_create(&keys_file).expect("keystore creation");

    // 1. Expired key (now - 60s)
    let past = (chrono::Utc::now() - chrono::Duration::seconds(60)).to_rfc3339();
    let (_, expired_key) = keystore
        .issue(
            "expired-key".to_string(),
            KeyKind::SecretKey,
            Some(Role::Viewer),
            vec!["console".to_string()],
            None,
            Some(past),
        )
        .expect("issue expired key");

    assert_eq!(keystore.verify(&expired_key), Err(AuthError::Expired));

    // 2. Valid key (now + 3600s)
    let future = (chrono::Utc::now() + chrono::Duration::seconds(3600)).to_rfc3339();
    let (_, valid_key) = keystore
        .issue(
            "valid-key".to_string(),
            KeyKind::SecretKey,
            Some(Role::Viewer),
            vec!["console".to_string()],
            None,
            Some(future),
        )
        .expect("issue valid key");

    let principal = keystore.verify(&valid_key).expect("verify future key");
    assert_eq!(principal.name, "valid-key");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_revocation() {
    let dir = temp_data_dir("revocation");
    let keys_file = dir.join("keys.json");
    let keystore = KeyStore::load_or_create(&keys_file).expect("keystore creation");

    let (rec, key) = keystore
        .issue(
            "op-key".to_string(),
            KeyKind::SecretKey,
            Some(Role::Operator),
            vec!["console".to_string()],
            None,
            None,
        )
        .expect("issue key");

    // Verifies before revocation
    assert!(keystore.verify(&key).is_ok());

    // Revoke
    keystore.revoke(&rec.id).expect("revoke key");

    // Verification fails with Revoked
    assert_eq!(keystore.verify(&key), Err(AuthError::Revoked));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_guard_against_revoking_last_admin_key() {
    let dir = temp_data_dir("last-admin-guard");
    let keys_file = dir.join("keys.json");
    let keystore = KeyStore::load_or_create(&keys_file).expect("keystore creation");

    // Issue initial Admin key
    let (admin1, admin1_key) = keystore
        .issue(
            "admin-root".to_string(),
            KeyKind::SecretKey,
            Some(Role::Admin),
            vec!["console".to_string(), "mcp".to_string()],
            None,
            None,
        )
        .expect("issue admin 1");

    // Issue Viewer key
    let (viewer, viewer_key) = keystore
        .issue(
            "viewer-1".to_string(),
            KeyKind::SecretKey,
            Some(Role::Viewer),
            vec!["console".to_string()],
            None,
            None,
        )
        .expect("issue viewer");

    // Viewer can be revoked
    keystore.revoke(&viewer.id).expect("viewer can be revoked");
    assert_eq!(keystore.verify(&viewer_key), Err(AuthError::Revoked));

    // Attempting to revoke the ONLY active admin key must fail
    let err = keystore.revoke(&admin1.id);
    assert!(err.is_err(), "revoking last active admin must return error");
    assert!(
        err.unwrap_err().contains("last active admin"),
        "error message should explain guard"
    );

    // Admin1 key must still be valid
    assert!(keystore.verify(&admin1_key).is_ok());

    // Attempting to delete the ONLY active admin key must fail
    let del_err = keystore.delete(&admin1.id);
    assert!(del_err.is_err(), "deleting last active admin must return error");

    // Issue second Admin key
    let (admin2, admin2_key) = keystore
        .issue(
            "admin-secondary".to_string(),
            KeyKind::SecretKey,
            Some(Role::Admin),
            vec!["console".to_string(), "mcp".to_string()],
            None,
            None,
        )
        .expect("issue admin 2");

    // Now admin1 can be revoked because admin2 is still active
    keystore
        .revoke(&admin1.id)
        .expect("revoking admin1 succeeds when admin2 exists");
    assert_eq!(keystore.verify(&admin1_key), Err(AuthError::Revoked));

    // But now admin2 is the LAST active admin key: revoking admin2 must fail!
    let err2 = keystore.revoke(&admin2.id);
    assert!(
        err2.is_err(),
        "revoking admin2 (now last active admin) must return error"
    );
    assert!(keystore.verify(&admin2_key).is_ok());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_key_rotation_with_grace_hours() {
    let dir = temp_data_dir("rotation");
    let keys_file = dir.join("keys.json");
    let keystore = KeyStore::load_or_create(&keys_file).expect("keystore creation");

    // 1. Rotate with grace_hours = 0 (immediate revocation)
    let (orig_record, orig_key) = keystore
        .issue(
            "service-worker".to_string(),
            KeyKind::SecretKey,
            Some(Role::Operator),
            vec!["console".to_string()],
            None,
            None,
        )
        .expect("issue original key");

    let (rot0_record, rot0_key) = keystore
        .rotate(&orig_record.id, Some(0))
        .expect("rotate with grace 0");

    assert_ne!(rot0_record.id, orig_record.id);
    assert_eq!(rot0_record.name, orig_record.name);
    assert_eq!(rot0_record.role, orig_record.role);
    assert_eq!(rot0_record.scopes, orig_record.scopes);

    // New key works immediately
    assert!(keystore.verify(&rot0_key).is_ok());
    // Old key is revoked immediately
    assert_eq!(keystore.verify(&orig_key), Err(AuthError::Revoked));

    // 2. Rotate with grace_hours = 24
    let (rot24_record, rot24_key) = keystore
        .rotate(&rot0_record.id, Some(24))
        .expect("rotate with grace 24");

    assert_ne!(rot24_record.id, rot0_record.id);

    // New key works immediately
    assert!(keystore.verify(&rot24_key).is_ok());

    // Previous key (rot0) STILL works because it has 24h grace period
    assert!(
        keystore.verify(&rot0_key).is_ok(),
        "old key should still work during grace period"
    );

    // Check that rot0_record in keystore has expires_at set in the future
    let rot0_summary = keystore.get_summary(&rot0_record.id).unwrap();
    assert!(rot0_summary.expires_at.is_some());
    let exp_dt =
        chrono::DateTime::parse_from_rfc3339(rot0_summary.expires_at.as_ref().unwrap()).unwrap();
    assert!(exp_dt > chrono::Utc::now());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_bootstrap_migration() {
    let dir = temp_data_dir("bootstrap-mig");
    let bootstrap_path = dir.join("bootstrap_key.txt");

    // Generate a valid bootstrap key and write bootstrap_key.txt
    let root_key = at_pc_server::generate_api_key().expect("generate bootstrap key");
    let banner = format!("at-pc server root API key\n  {root_key}\n");
    std::fs::write(&bootstrap_path, banner).expect("write bootstrap file");

    let keys_file = dir.join("keys.json");
    assert!(!keys_file.exists());

    // load_or_create should detect bootstrap_key.txt, migrate it, and remove the file
    let keystore = KeyStore::load_or_create(&keys_file).expect("load_or_create migrate");

    // bootstrap_key.txt is retained with 0600 so operator can read and manually remove it
    assert!(
        bootstrap_path.exists(),
        "bootstrap_key.txt is kept until manual deletion"
    );
    // keys.json must exist
    assert!(keys_file.exists(), "keys.json must be created");

    // The root key must authenticate as Admin named "bootstrap-root"
    let principal = keystore
        .verify(&root_key)
        .expect("bootstrap key must authenticate");
    assert_eq!(principal.role, Some(Role::Admin));
    assert_eq!(principal.name, "bootstrap-root");

    // keys.json must not leak the plaintext root key
    let content = std::fs::read_to_string(&keys_file).expect("read keys.json");
    assert!(
        !content.contains(&root_key),
        "keys.json must never contain the plaintext key"
    );
    assert!(content.contains("sha256-salt-v1"));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_rate_limiting_logic() {
    let limiter = AuthRateLimiter::new();
    let ip: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
    let other_ip: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 101));

    // Initial check is allowed
    assert_eq!(limiter.check(ip), RateLimitDecision::Allowed);

    // Failures 1 through 9 should be Allowed
    for i in 1..=9 {
        let decision = limiter.record_failure(ip);
        assert_eq!(
            decision,
            RateLimitDecision::Allowed,
            "failure #{i} should be allowed"
        );
        assert_eq!(limiter.check(ip), RateLimitDecision::Allowed);
    }

    // 10th failure triggers RateLimited { retry_after_secs: 60 }
    let tenth = limiter.record_failure(ip);
    assert_eq!(
        tenth,
        RateLimitDecision::RateLimited {
            retry_after_secs: 60
        }
    );
    assert_eq!(
        limiter.check(ip),
        RateLimitDecision::RateLimited {
            retry_after_secs: 60
        }
    );

    // Other IP is unaffected
    assert_eq!(limiter.check(other_ip), RateLimitDecision::Allowed);

    // Success resets failure count
    limiter.record_success(ip);
    assert_eq!(
        limiter.check(ip),
        RateLimitDecision::Allowed,
        "success must reset rate limit state"
    );

    // Test ban logic: 30 failures in 10 minutes bans IP for 15 minutes (900s)
    for _ in 0..29 {
        limiter.record_failure(ip);
    }
    // 30th failure triggers Ban
    let thirtieth = limiter.record_failure(ip);
    assert_eq!(
        thirtieth,
        RateLimitDecision::Banned {
            retry_after_secs: 900
        }
    );
    match limiter.check(ip) {
        RateLimitDecision::Banned { retry_after_secs } => {
            assert!(
                (899..=900).contains(&retry_after_secs),
                "retry_after_secs should be 900 (or 899 if subsecond elapsed)"
            );
        }
        other => panic!("expected Banned, got {other:?}"),
    }

    // Further failures while banned return Banned
    let thirty_first = limiter.record_failure(ip);
    match thirty_first {
        RateLimitDecision::Banned { retry_after_secs } => {
            assert!(retry_after_secs <= 900 && retry_after_secs > 0);
        }
        other => panic!("expected Banned, got {other:?}"),
    }

    // Success resets the ban
    limiter.record_success(ip);
    assert_eq!(limiter.check(ip), RateLimitDecision::Allowed);
}

#[test]
fn test_rate_limiting_rolling_window() {
    let limiter = AuthRateLimiter::new();
    let ip: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
    let t0 = Instant::now();

    // 9 failures at t0
    for _ in 0..9 {
        assert_eq!(
            limiter.record_failure_at(ip, t0),
            RateLimitDecision::Allowed
        );
    }

    // 10th failure at t0 + 61 seconds:
    // The previous 9 failures are outside the 60-second window!
    let t_61s = t0 + Duration::from_secs(61);
    let dec = limiter.record_failure_at(ip, t_61s);
    assert_eq!(
        dec,
        RateLimitDecision::Allowed,
        "failure after 61s should see rolling window reset"
    );
}
