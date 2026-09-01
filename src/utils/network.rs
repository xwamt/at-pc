//! Network utilities for local IP detection and port availability.

use std::net::TcpListener;

/// Detects the local LAN IP address of the current host machine.
/// Falls back to "127.0.0.1" if detection fails.
pub fn get_lan_ip() -> String {
    local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

/// Finds the first available TCP port starting from `start` (inclusive) on 0.0.0.0.
/// If all ports up to 65535 are occupied, returns `start`.
pub fn find_available_port(start: u16) -> u16 {
    for port in start..=65535 {
        if TcpListener::bind(("0.0.0.0", port)).is_ok() {
            return port;
        }
    }
    start
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_lan_ip() {
        let ip = get_lan_ip();
        assert!(!ip.is_empty());
        assert!(ip.parse::<std::net::IpAddr>().is_ok());
    }

    #[test]
    fn test_find_available_port() {
        let port = find_available_port(9800);
        assert!(port >= 9800);
    }
}
