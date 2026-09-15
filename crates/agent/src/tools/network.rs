//! Network diagnostics tools.
//! Provides active listening port and TCP/UDP connection inspection (netstat),
//! and target network reachability, DNS resolution, and port connectivity tests.

use serde::{Deserialize, Serialize};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// A single active TCP/UDP network connection or listening socket.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkConnectionInfo {
    pub protocol: String,
    pub local_address: String,
    pub local_port: u16,
    pub remote_address: String,
    pub remote_port: u16,
    pub state: String,
    pub pid: Option<u32>,
}

/// Result of network connection listing operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListConnectionsResult {
    pub total_connections: usize,
    pub connections: Vec<NetworkConnectionInfo>,
}

/// Result of network reachability and connectivity test.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkTestResult {
    pub target_host: String,
    pub target_port: Option<u16>,
    pub resolved_ips: Vec<String>,
    pub reachable: bool,
    pub latency_ms: Option<u64>,
    pub message: String,
}

/// Lists active TCP/UDP connections and listening ports.
///
/// # Arguments
/// * `state_filter` - Optional filter for connection state (e.g. `"LISTEN"`, `"ESTABLISHED"`).
/// * `port_filter` - Optional port number to filter by.
/// * `limit` - Maximum number of connection records to return (default: 100).
pub fn list_network_connections(
    state_filter: Option<&str>,
    port_filter: Option<u16>,
    limit: Option<usize>,
) -> Result<ListConnectionsResult, String> {
    let max_count = limit.unwrap_or(100).clamp(1, 500);

    #[cfg(windows)]
    {
        list_windows_network_connections(state_filter, port_filter, max_count)
    }

    #[cfg(not(windows))]
    {
        list_unix_network_connections(state_filter, port_filter, max_count)
    }
}

#[cfg(windows)]
fn list_windows_network_connections(
    state_filter: Option<&str>,
    port_filter: Option<u16>,
    max_count: usize,
) -> Result<ListConnectionsResult, String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut cmd = Command::new("netstat");
    cmd.args(["-ano"]);
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run netstat on Windows: {}", e))?;

    let text = String::from_utf8_lossy(&output.stdout);
    let mut connections = Vec::new();

    let state_query = state_filter.map(|s| s.trim().to_uppercase());

    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }

        let proto = parts[0].to_uppercase();
        if proto != "TCP" && proto != "UDP" {
            continue;
        }

        let (local, remote, state, pid_idx) = if proto == "TCP" && parts.len() >= 5 {
            (parts[1], parts[2], parts[3].to_string(), 4)
        } else if proto == "UDP" {
            (parts[1], parts[2], "NONE".to_string(), 3)
        } else {
            continue;
        };

        if let Some(ref st) = state_query {
            if !state.to_uppercase().contains(st) {
                continue;
            }
        }

        let local_port = local.rsplit(':').next().and_then(|p| p.parse::<u16>().ok()).unwrap_or(0);
        let remote_port = remote.rsplit(':').next().and_then(|p| p.parse::<u16>().ok()).unwrap_or(0);

        if let Some(pf) = port_filter {
            if local_port != pf && remote_port != pf {
                continue;
            }
        }

        let pid = parts.get(pid_idx).and_then(|p| p.parse::<u32>().ok());

        connections.push(NetworkConnectionInfo {
            protocol: proto,
            local_address: local.to_string(),
            local_port,
            remote_address: remote.to_string(),
            remote_port,
            state,
            pid,
        });

        if connections.len() >= max_count {
            break;
        }
    }

    Ok(ListConnectionsResult {
        total_connections: connections.len(),
        connections,
    })
}

#[cfg(not(windows))]
fn list_unix_network_connections(
    state_filter: Option<&str>,
    port_filter: Option<u16>,
    max_count: usize,
) -> Result<ListConnectionsResult, String> {
    use std::process::Command;

    // Try netstat -an
    let mut cmd = Command::new("netstat");
    cmd.args(["-an"]);

    let output = match cmd.output() {
        Ok(out) => out,
        Err(_) => {
            // Fallback to empty if netstat not available
            return Ok(ListConnectionsResult {
                total_connections: 0,
                connections: Vec::new(),
            });
        }
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let mut connections = Vec::new();
    let state_query = state_filter.map(|s| s.trim().to_uppercase());

    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }

        let proto = parts[0].to_uppercase();
        if !proto.starts_with("TCP") && !proto.starts_with("UDP") {
            continue;
        }

        let local = parts.get(3).cloned().unwrap_or_default();
        let remote = parts.get(4).cloned().unwrap_or_default();
        let state = parts.get(5).cloned().unwrap_or("NONE").to_string();

        if let Some(ref st) = state_query {
            if !state.to_uppercase().contains(st) {
                continue;
            }
        }

        let local_port = local
            .rsplit('.')
            .next()
            .or_else(|| local.rsplit(':').next())
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(0);

        let remote_port = remote
            .rsplit('.')
            .next()
            .or_else(|| remote.rsplit(':').next())
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(0);

        if let Some(pf) = port_filter {
            if local_port != pf && remote_port != pf {
                continue;
            }
        }

        connections.push(NetworkConnectionInfo {
            protocol: proto,
            local_address: local.to_string(),
            local_port,
            remote_address: remote.to_string(),
            remote_port,
            state,
            pid: None,
        });

        if connections.len() >= max_count {
            break;
        }
    }

    Ok(ListConnectionsResult {
        total_connections: connections.len(),
        connections,
    })
}

/// Tests target host reachability, DNS resolution, and TCP port connectivity.
///
/// # Arguments
/// * `target_host` - Hostname or IP to probe.
/// * `port` - Optional port number to test TCP connection.
/// * `timeout_ms` - Timeout in milliseconds (default: 3000ms).
pub fn test_network(
    target_host: &str,
    port: Option<u16>,
    timeout_ms: Option<u64>,
) -> Result<NetworkTestResult, String> {
    let host = target_host.trim();
    if host.is_empty() {
        return Err("Target host cannot be empty".to_string());
    }

    let timeout = Duration::from_millis(timeout_ms.unwrap_or(3000).clamp(500, 30_000));

    // 1. Resolve DNS
    let probe_port = port.unwrap_or(80);
    let host_port = format!("{}:{}", host, probe_port);

    let resolved_ips: Vec<String> = match host_port.to_socket_addrs() {
        Ok(addrs) => {
            let mut ips = Vec::new();
            for a in addrs {
                let s = a.ip().to_string();
                if !ips.contains(&s) {
                    ips.push(s);
                }
            }
            ips
        }
        Err(e) => {
            return Ok(NetworkTestResult {
                target_host: host.to_string(),
                target_port: port,
                resolved_ips: Vec::new(),
                reachable: false,
                latency_ms: None,
                message: format!("DNS resolution failed for '{}': {}", host, e),
            });
        }
    };

    // 2. If port is provided or using probe port, attempt TCP connection
    if let Some(target_port) = port {
        let target_addr_str = format!("{}:{}", host, target_port);
        let addrs: Vec<SocketAddr> = match target_addr_str.to_socket_addrs() {
            Ok(iter) => iter.collect(),
            Err(e) => {
                return Ok(NetworkTestResult {
                    target_host: host.to_string(),
                    target_port: Some(target_port),
                    resolved_ips,
                    reachable: false,
                    latency_ms: None,
                    message: format!("Failed to parse socket address: {}", e),
                });
            }
        };

        if let Some(target_sock) = addrs.first() {
            let start = Instant::now();
            match TcpStream::connect_timeout(target_sock, timeout) {
                Ok(_) => {
                    let elapsed = start.elapsed().as_millis() as u64;
                    return Ok(NetworkTestResult {
                        target_host: host.to_string(),
                        target_port: Some(target_port),
                        resolved_ips,
                        reachable: true,
                        latency_ms: Some(elapsed),
                        message: format!(
                            "Successfully connected to {}:{} in {}ms",
                            host, target_port, elapsed
                        ),
                    });
                }
                Err(e) => {
                    return Ok(NetworkTestResult {
                        target_host: host.to_string(),
                        target_port: Some(target_port),
                        resolved_ips,
                        reachable: false,
                        latency_ms: None,
                        message: format!(
                            "TCP connection to {}:{} failed: {}",
                            host, target_port, e
                        ),
                    });
                }
            }
        }
    }

    // DNS resolved without port test
    Ok(NetworkTestResult {
        target_host: host.to_string(),
        target_port: port,
        resolved_ips: resolved_ips.clone(),
        reachable: true,
        latency_ms: None,
        message: format!(
            "DNS resolution successful for '{}': {:?}",
            host, resolved_ips
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_network_connections_basic() {
        let res = list_network_connections(None, None, Some(20));
        assert!(res.is_ok());
    }

    #[test]
    fn test_test_network_dns_resolution() {
        let res = test_network("127.0.0.1", None, Some(1000)).unwrap();
        assert!(res.reachable);
        assert!(res.resolved_ips.contains(&"127.0.0.1".to_string()));
    }
}
