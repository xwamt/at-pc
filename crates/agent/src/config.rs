//! Agent configuration module.
//! Reads `agent_config.toml` with fallback to default and environment variables,
//! and generates unique device identifiers.

use at_pc_protocol::models::TerminalInfo;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use sysinfo::{Networks, System};

use crate::tools::sysinfo::get_local_ips;

/// Server connection configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerConfig {
    #[serde(default = "default_server_url")]
    pub url: String,
    #[serde(default)]
    pub auth_token: Option<String>,
    #[serde(default = "default_reconnect_interval_secs")]
    pub reconnect_interval_secs: u64,
}

fn default_server_url() -> String {
    std::env::var("AT_PC_SERVER_URL")
        .or_else(|_| std::env::var("DEFAULT_SERVER_URL"))
        .unwrap_or_else(|_| "ws://127.0.0.1:9801/ws".to_string())
}

fn default_reconnect_interval_secs() -> u64 {
    5
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            url: default_server_url(),
            auth_token: std::env::var("AT_PC_AUTH_TOKEN").ok(),
            reconnect_interval_secs: default_reconnect_interval_secs(),
        }
    }
}

/// Device identification configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceConfig {
    #[serde(default = "default_device_id")]
    pub device_id: String,
    #[serde(default)]
    pub device_name: String,
}

fn default_device_id() -> String {
    "auto".to_string()
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            device_id: default_device_id(),
            device_name: String::new(),
        }
    }
}

/// Main Agent configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AgentConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub device: DeviceConfig,
}

impl AgentConfig {
    /// Loads configuration from `agent_config.toml` or returns default config.
    pub fn load_from_file_or_default(custom_path: Option<&Path>) -> Self {
        let candidate_paths = if let Some(p) = custom_path {
            vec![p.to_path_buf()]
        } else {
            let mut paths = vec![PathBuf::from("agent_config.toml")];
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(parent) = exe_path.parent() {
                    paths.push(parent.join("agent_config.toml"));
                }
            }
            paths
        };

        for path in candidate_paths {
            if path.exists() && path.is_file() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(config) = toml::from_str::<AgentConfig>(&content) {
                        return config;
                    }
                }
            }
        }

        AgentConfig::default()
    }

    /// Resolves the effective device ID (either configured explicitly or generated deterministically from hostname + MAC).
    pub fn resolve_device_id(&self) -> String {
        if self.device.device_id != "auto" && !self.device.device_id.trim().is_empty() {
            return self.device.device_id.trim().to_string();
        }

        let hostname = System::host_name()
            .or_else(|| std::env::var("HOSTNAME").ok())
            .or_else(|| std::env::var("COMPUTERNAME").ok())
            .unwrap_or_else(|| "pc".to_string());

        let clean_hostname = hostname
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect::<String>()
            .to_lowercase();

        // Extract first valid MAC address deterministically by sorting network interface names
        let networks = Networks::new_with_refreshed_list();
        let mut iface_list: Vec<(&String, &sysinfo::NetworkData)> = networks.iter().collect();
        iface_list.sort_by(|(a, _), (b, _)| a.cmp(b));

        let mut mac_opt = None;
        for (_name, data) in iface_list {
            let mac_str = data.mac_address().to_string();
            let clean_mac: String = mac_str.chars().filter(|c| c.is_ascii_hexdigit()).collect();
            if !clean_mac.is_empty() && clean_mac != "000000000000" {
                mac_opt = Some(clean_mac);
                break;
            }
        }

        if let Some(mac) = mac_opt {
            let suffix_len = mac.len().min(6);
            let suffix = &mac[mac.len() - suffix_len..];
            format!("{}-{}", clean_hostname, suffix.to_lowercase())
        } else {
            format!("{}-agent", clean_hostname)
        }
    }

    /// Resolves the device display name.
    pub fn resolve_device_name(&self) -> String {
        if !self.device.device_name.trim().is_empty() {
            return self.device.device_name.trim().to_string();
        }
        System::host_name().unwrap_or_else(|| "Unknown-PC".to_string())
    }

    /// Builds a `TerminalInfo` struct from this configuration and system metrics.
    pub fn to_terminal_info(&self) -> TerminalInfo {
        let terminal_id = self.resolve_device_id();
        let hostname = self.resolve_device_name();

        let username = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "user".to_string());

        let ips = get_local_ips();
        let lan_ip = ips.first().cloned().unwrap_or_else(|| "127.0.0.1".to_string());

        let os_name = System::name().unwrap_or_else(|| std::env::consts::OS.to_string());
        let os_version_str = System::os_version().unwrap_or_default();
        let os_version = if os_version_str.is_empty() {
            os_name
        } else {
            format!("{} {}", os_name, os_version_str)
        };

        let agent_version = env!("CARGO_PKG_VERSION").to_string();

        TerminalInfo {
            terminal_id,
            hostname,
            username,
            lan_ip,
            os_version,
            agent_version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_resolution() {
        let config = AgentConfig::default();
        assert!(!config.server.url.is_empty());
        assert_eq!(config.server.reconnect_interval_secs, 5);

        let device_id = config.resolve_device_id();
        assert!(!device_id.is_empty());

        let info = config.to_terminal_info();
        assert_eq!(info.terminal_id, device_id);
        assert!(!info.hostname.is_empty());
        assert!(!info.agent_version.is_empty());
    }

    #[test]
    fn test_custom_device_id() {
        let mut config = AgentConfig::default();
        config.device.device_id = "custom-agent-99".to_string();
        assert_eq!(config.resolve_device_id(), "custom-agent-99");
    }
}
