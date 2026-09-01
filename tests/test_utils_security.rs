use at_pc::server::auth;
use at_pc::utils::{network, security};

#[test]
fn test_pin_generation_format() {
    for _ in 0..100 {
        let pin = security::generate_pin();
        assert_eq!(pin.len(), 4);
        assert!(pin.chars().all(|c| c.is_ascii_digit()));
    }
}

#[test]
fn test_pin_verification() {
    assert!(security::verify_pin("1234", "1234"));
    assert!(!security::verify_pin("1234", "0000"));
    assert!(!security::verify_pin("1234", "123"));
    assert!(!security::verify_pin("1234", "12345"));
    assert!(!security::verify_pin("", ""));
}

#[test]
fn test_lan_ip_detection() {
    let ip = network::get_lan_ip();
    assert!(!ip.is_empty());
    assert!(ip.parse::<std::net::IpAddr>().is_ok());
}

#[test]
fn test_find_available_port() {
    let port = network::find_available_port(9800);
    assert!(port >= 9800);
    let listener = std::net::TcpListener::bind(("0.0.0.0", port));
    assert!(listener.is_ok());
}

#[test]
fn test_validate_auth_header() {
    let pin = "4829";
    assert!(auth::validate_auth_header(pin, Some("Bearer 4829")));
    assert!(auth::validate_auth_header(pin, Some("bearer 4829")));
    assert!(auth::validate_auth_header(pin, Some("Bearer  4829 ")));
    assert!(auth::validate_auth_header(pin, Some("4829")));

    assert!(!auth::validate_auth_header(pin, Some("Bearer 0000")));
    assert!(!auth::validate_auth_header(pin, Some("Bearer")));
    assert!(!auth::validate_auth_header(pin, Some("")));
    assert!(!auth::validate_auth_header(pin, None));
    assert!(!auth::validate_auth_header("", Some("Bearer ")));
}
