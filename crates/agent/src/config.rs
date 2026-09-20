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
    /// Optional custom CA certificate path (PEM) for TLS / WSS verification
    #[serde(default)]
    pub ca_cert_path: Option<PathBuf>,
    /// Optional client certificate path (PEM) for mTLS authentication
    #[serde(default)]
    pub client_cert_path: Option<PathBuf>,
    /// Optional client private key path (PEM) for mTLS authentication
    #[serde(default)]
    pub client_key_path: Option<PathBuf>,
    /// Accept self-signed / invalid TLS certs (useful for internal testing)
    #[serde(default)]
    pub insecure_skip_verify: bool,
}

fn default_server_url() -> String {
    std::env::var("AT_PC_SERVER_URL")
        .or_else(|_| std::env::var("DEFAULT_SERVER_URL"))
        .unwrap_or_else(|_| {
            const COMPILED_URL: Option<&'static str> = option_env!("AT_PC_SERVER_URL");
            const COMPILED_DEFAULT: Option<&'static str> = option_env!("DEFAULT_SERVER_URL");
            COMPILED_URL
                .or(COMPILED_DEFAULT)
                .unwrap_or("ws://127.0.0.1:9801/ws")
                .to_string()
        })
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
            ca_cert_path: None,
            client_cert_path: None,
            client_key_path: None,
            insecure_skip_verify: false,
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
    /// Whether Computer-Use (remote simulated mouse and keyboard control) is enabled
    #[serde(default)]
    pub enable_computer_use: bool,
}

impl AgentConfig {
    /// Loads configuration from `agent_config.toml` or returns default config.
    pub fn load_from_file_or_default(custom_path: Option<&Path>) -> Self {
        let candidate_paths = if let Some(p) = custom_path {
            vec![p.to_path_buf()]
        } else {
            let mut paths = vec![
                PathBuf::from("agent_config.toml"),
                PathBuf::from("config.toml"),
            ];
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(parent) = exe_path.parent() {
                    paths.push(parent.join("agent_config.toml"));
                    paths.push(parent.join("config.toml"));
                }
            }
            paths
        };

        let mut config = AgentConfig::default();
        let mut loaded = false;
        for path in candidate_paths {
            if path.exists() && path.is_file() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(parsed) = toml::from_str::<AgentConfig>(&content) {
                        tracing::info!("Successfully loaded configuration from {:?}", path);
                        config = parsed;
                        loaded = true;
                        break;
                    }
                }
            }
        }

        if !loaded {
            tracing::info!(
                "No valid config file found; using default config (target server: {})",
                default_server_url()
            );
        }

        if let Ok(v) = std::env::var("AT_PC_ENABLE_COMPUTER_USE") {
            if v == "1" || v.eq_ignore_ascii_case("true") {
                config.enable_computer_use = true;
            }
        }
        if let Ok(ca) = std::env::var("AT_PC_CA_CERT") {
            config.server.ca_cert_path = Some(PathBuf::from(ca));
        }
        if let Ok(cc) = std::env::var("AT_PC_CLIENT_CERT") {
            config.server.client_cert_path = Some(PathBuf::from(cc));
        }
        if let Ok(ck) = std::env::var("AT_PC_CLIENT_KEY") {
            config.server.client_key_path = Some(PathBuf::from(ck));
        }
        if let Ok(insecure) = std::env::var("AT_PC_INSECURE") {
            if insecure == "1" || insecure.eq_ignore_ascii_case("true") {
                config.server.insecure_skip_verify = true;
            }
        }

        config
    }

    /// Finds the preferred configuration file path to read from or write to.
    pub fn find_config_path(custom_path: Option<&Path>) -> PathBuf {
        if let Some(p) = custom_path {
            return p.to_path_buf();
        }

        let candidates = [
            PathBuf::from("agent_config.toml"),
            PathBuf::from("config.toml"),
        ];

        for c in &candidates {
            if c.exists() && c.is_file() {
                return c.clone();
            }
        }

        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(parent) = exe_path.parent() {
                let p1 = parent.join("agent_config.toml");
                if p1.exists() && p1.is_file() {
                    return p1;
                }
                let p2 = parent.join("config.toml");
                if p2.exists() && p2.is_file() {
                    return p2;
                }
                return p1;
            }
        }

        PathBuf::from("agent_config.toml")
    }

    /// Updates the server URL and writes the configuration to disk.
    pub fn save_server_url(new_url: &str, custom_path: Option<&Path>) -> Result<PathBuf, String> {
        let path = Self::find_config_path(custom_path);
        let mut config = if path.exists() && path.is_file() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|content| toml::from_str::<AgentConfig>(&content).ok())
                .unwrap_or_default()
        } else {
            AgentConfig::default()
        };

        config.server.url = new_url.trim().to_string();

        let toml_str = toml::to_string_pretty(&config)
            .map_err(|e| format!("Failed to serialize config to TOML: {}", e))?;

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                let _ = std::fs::create_dir_all(parent);
            }
        }

        std::fs::write(&path, toml_str)
            .map_err(|e| format!("Failed to write config file to {:?}: {}", path, e))?;

        tracing::info!("Saved updated server URL '{}' to {:?}", new_url, path);
        Ok(path)
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
        iface_list.sort_by_key(|(a, _)| *a);

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
        let lan_ip = ips
            .iter()
            .find(|ip| ip.contains('.') && !ip.starts_with("127."))
            .cloned()
            .or_else(|| ips.first().cloned())
            .unwrap_or_else(|| "127.0.0.1".to_string());

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

    #[test]
    fn test_save_server_url() {
        let temp_dir = std::env::temp_dir().join(format!("at_agent_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let config_path = temp_dir.join("test_config.toml");

        let res = AgentConfig::save_server_url("ws://10.10.10.10:9801/ws", Some(&config_path));
        assert!(res.is_ok());

        let loaded = AgentConfig::load_from_file_or_default(Some(&config_path));
        assert_eq!(loaded.server.url, "ws://10.10.10.10:9801/ws");

        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
