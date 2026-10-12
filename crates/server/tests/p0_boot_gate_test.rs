use at_pc_server::{is_loopback, validate_boot_gate, BootGateError, ServerConfig};

#[test]
fn test_boot_gate_rejects_non_loopback_without_credentials() {
    let cfg = ServerConfig {
        listen_host: "0.0.0.0".to_string(),
        ..Default::default()
    };
    let res = validate_boot_gate(&cfg, false, false);
    assert_eq!(
        res,
        Err(BootGateError::MissingCredentials {
            host: "0.0.0.0".to_string(),
        })
    );

    // Verify error message formatting provides clear remediation instructions
    let err_msg = format!("{}", res.unwrap_err());
    assert!(
        err_msg.contains("refusing to start"),
        "error must mention refusing to start"
    );
    assert!(
        err_msg.contains("0.0.0.0"),
        "error must mention non-loopback host"
    );
    assert!(
        err_msg.contains("--insecure-no-auth"),
        "error must mention --insecure-no-auth solution"
    );
}

#[test]
fn test_boot_gate_allows_loopback_without_credentials() {
    let cfg = ServerConfig {
        listen_host: "127.0.0.1".to_string(),
        ..Default::default()
    };
    assert_eq!(validate_boot_gate(&cfg, false, false), Ok(()));
}

#[test]
fn test_boot_gate_rejects_non_loopback_without_tls_unless_allow_plaintext() {
    let cfg = ServerConfig {
        listen_host: "0.0.0.0".to_string(),
        allow_plaintext: false,
        ..Default::default()
    };
    assert_eq!(
        validate_boot_gate(&cfg, true, false),
        Err(BootGateError::TlsRequired {
            host: "0.0.0.0".to_string(),
        })
    );

    let cfg_allowed = ServerConfig {
        listen_host: "0.0.0.0".to_string(),
        allow_plaintext: true,
        ..Default::default()
    };
    assert_eq!(validate_boot_gate(&cfg_allowed, true, false), Ok(()));
}

#[test]
fn test_boot_gate_allows_non_loopback_with_credentials_and_allow_plaintext() {
    let cfg = ServerConfig {
        listen_host: "0.0.0.0".to_string(),
        allow_plaintext: true,
        ..Default::default()
    };
    assert_eq!(validate_boot_gate(&cfg, true, false), Ok(()));
}

#[test]
fn test_boot_gate_allows_non_loopback_insecure_mode_with_allow_plaintext() {
    let cfg = ServerConfig {
        listen_host: "0.0.0.0".to_string(),
        allow_plaintext: true,
        ..Default::default()
    };
    assert_eq!(validate_boot_gate(&cfg, false, true), Ok(()));
}

#[test]
fn test_legacy_config_gate_rejects_plaintext_roles_when_keystore_active() {
    let mut cfg = ServerConfig::default();
    cfg.roles
        .insert("secret".to_string(), at_pc_server::Role::Admin);

    assert_eq!(
        at_pc_server::config::validate_legacy_config_gate(&cfg, true),
        Err(BootGateError::PlaintextRolesNotAllowed)
    );
    assert_eq!(
        at_pc_server::config::validate_legacy_config_gate(&cfg, false),
        Ok(())
    );
}

#[test]
fn test_is_loopback_boundary_cases() {
    // Loopback hosts (must return true)
    let loopback_cases = [
        "127.0.0.1",
        "::1",
        "localhost",
        "127.0.0.2",
        "127.255.255.254",
        // Extended boundaries
        "127.5.5.5",
        "LOCALHOST",
        "  localhost  ",
        "  127.0.0.1  ",
    ];
    for host in loopback_cases {
        assert!(
            is_loopback(host),
            "expected host '{host}' to be identified as loopback"
        );
    }

    // Non-loopback hosts (must return false)
    let non_loopback_cases = [
        "0.0.0.0",
        "::",
        "192.168.1.1",
        "10.0.0.1",
        "8.8.8.8",
        "invalid_host",
        // Extended boundaries
        "192.168.1.10",
        "",
        "example.com",
    ];
    for host in non_loopback_cases {
        assert!(
            !is_loopback(host),
            "expected host '{host}' to NOT be identified as loopback"
        );
    }
}
