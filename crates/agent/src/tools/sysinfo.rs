//! System overview and hardware diagnostics.

use serde::{Deserialize, Serialize};
use sysinfo::{Disks, Networks, System};

/// Disk information summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub total_space_mb: u64,
    pub available_space_mb: u64,
    pub file_system: String,
    pub is_removable: bool,
}

/// Network interface summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkInterfaceInfo {
    pub name: String,
    pub mac_address: String,
    pub received_bytes: u64,
    pub transmitted_bytes: u64,
    pub total_received_bytes: u64,
    pub total_transmitted_bytes: u64,
}

/// Comprehensive hardware, operating system, and runtime overview.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SystemOverview {
    pub os_name: String,
    pub os_version: String,
    pub host_name: String,
    pub kernel_version: String,
    pub cpu_model: String,
    pub cpu_cores: usize,
    pub cpu_usage_percent: f32,
    pub total_memory_mb: u64,
    pub used_memory_mb: u64,
    pub total_swap_mb: u64,
    pub used_swap_mb: u64,
    pub uptime_secs: u64,
    pub boot_time_secs: u64,
    pub disks: Vec<DiskInfo>,
    pub networks: Vec<NetworkInterfaceInfo>,
    pub local_ips: Vec<String>,
    pub default_gateway: String,
    pub dns_servers: Vec<String>,
}

/// Returns all non-loopback local IPv4 / IPv6 addresses, deduplicated.
pub fn get_local_ips() -> Vec<String> {
    let mut ips = Vec::new();
    if let Ok(interfaces) = local_ip_address::list_afinet_netifas() {
        for (_name, ip) in interfaces {
            if !ip.is_loopback() {
                let ip_str = ip.to_string();
                if !ips.contains(&ip_str) {
                    ips.push(ip_str);
                }
            }
        }
    }
    if ips.is_empty() {
        if let Ok(ip) = local_ip_address::local_ip() {
            if !ip.is_loopback() {
                let ip_str = ip.to_string();
                if !ips.contains(&ip_str) {
                    ips.push(ip_str);
                }
            }
        }
    }
    ips
}

/// Discovers the system default gateway address, returning "unknown" on failure.
#[cfg(target_os = "macos")]
pub fn get_default_gateway() -> String {
    // 1. Try route -n get default
    if let Ok(out) = std::process::Command::new("route")
        .args(["-n", "get", "default"])
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("gateway:") {
                    let gw = trimmed.trim_start_matches("gateway:").trim().to_string();
                    if !gw.is_empty() {
                        return gw;
                    }
                }
            }
        }
    }

    // 2. Try netstat -rn -f inet
    if let Ok(out) = std::process::Command::new("netstat")
        .args(["-rn", "-f", "inet"])
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 && (parts[0] == "default" || parts[0] == "0.0.0.0") {
                    let gw = parts[1];
                    if !gw.starts_with("link#") && !gw.is_empty() {
                        return gw.to_string();
                    }
                }
            }
        }
    }

    "unknown".to_string()
}

#[cfg(target_os = "linux")]
pub fn get_default_gateway() -> String {
    // 1. Try ip route show default
    if let Ok(out) = std::process::Command::new("ip")
        .args(["route", "show", "default"])
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(via_idx) = parts.iter().position(|&p| p == "via") {
                    if let Some(gw) = parts.get(via_idx + 1) {
                        return gw.to_string();
                    }
                }
            }
        }
    }

    // 2. Try /proc/net/route
    if let Ok(content) = std::fs::read_to_string("/proc/net/route") {
        for line in content.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 && parts[1] == "00000000" {
                if let Ok(hex_val) = u32::from_str_radix(parts[2], 16) {
                    let ip = std::net::Ipv4Addr::from(hex_val.to_be());
                    if !ip.is_unspecified() {
                        return ip.to_string();
                    }
                }
            }
        }
    }

    // 3. Fallback: route -n or netstat -rn
    if let Ok(out) = std::process::Command::new("route")
        .args(["-n"])
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 && (parts[0] == "0.0.0.0" || parts[0] == "default") {
                    let gw = parts[1];
                    if gw != "0.0.0.0" && !gw.is_empty() {
                        return gw.to_string();
                    }
                }
            }
        }
    }

    "unknown".to_string()
}

#[cfg(target_os = "windows")]
pub fn get_default_gateway() -> String {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    // 1. Fast route print 0.0.0.0 via cmd (avoids PowerShell overhead / hang)
    let mut cmd = std::process::Command::new("cmd");
    cmd.args(["/C", "route print 0.0.0.0"]);
    cmd.creation_flags(CREATE_NO_WINDOW);
    if let Ok(out) = cmd.output() {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 && parts[0] == "0.0.0.0" && parts[1] == "0.0.0.0" {
                    let gw = parts[2];
                    if gw != "0.0.0.0" && !gw.is_empty() {
                        return gw.to_string();
                    }
                }
            }
        }
    }

    // 2. Fallback: netstat -rn
    let mut cmd = std::process::Command::new("cmd");
    cmd.args(["/C", "netstat -rn"]);
    cmd.creation_flags(CREATE_NO_WINDOW);
    if let Ok(out) = cmd.output() {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 && (parts[0] == "0.0.0.0" || parts[0] == "default") {
                    let gw = parts[2];
                    if gw != "0.0.0.0" && !gw.is_empty() {
                        return gw.to_string();
                    }
                }
            }
        }
    }

    "unknown".to_string()
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn get_default_gateway() -> String {
    "unknown".to_string()
}

/// Discovers system DNS servers, returning an empty list if none found.
#[cfg(target_os = "macos")]
pub fn get_dns_servers() -> Vec<String> {
    let mut servers = Vec::new();

    // 1. Try scutil --dns
    if let Ok(out) = std::process::Command::new("scutil")
        .arg("--dns")
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("nameserver[") {
                    if let Some((_, val)) = trimmed.split_once(':') {
                        let ip = val.trim();
                        if !ip.is_empty() && !servers.contains(&ip.to_string()) {
                            servers.push(ip.to_string());
                        }
                    }
                }
            }
        }
    }

    // 2. Fallback: /etc/resolv.conf
    if servers.is_empty() {
        if let Ok(content) = std::fs::read_to_string("/etc/resolv.conf") {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("nameserver") {
                    let parts: Vec<&str> = trimmed.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let ip = parts[1].to_string();
                        if !servers.contains(&ip) {
                            servers.push(ip);
                        }
                    }
                }
            }
        }
    }

    servers
}

#[cfg(target_os = "linux")]
pub fn get_dns_servers() -> Vec<String> {
    let mut servers = Vec::new();

    // 1. Read /etc/resolv.conf
    if let Ok(content) = std::fs::read_to_string("/etc/resolv.conf") {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("nameserver") {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 2 {
                    let ip = parts[1].to_string();
                    if !servers.contains(&ip) {
                        servers.push(ip);
                    }
                }
            }
        }
    }

    // 2. Fallback: resolvectl dns
    if servers.is_empty() {
        if let Ok(out) = std::process::Command::new("resolvectl")
            .arg("dns")
            .output()
        {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                for line in text.lines() {
                    if let Some((_, val)) = line.split_once(':') {
                        for ip in val.split_whitespace() {
                            let ip_str = ip.to_string();
                            if !servers.contains(&ip_str) {
                                servers.push(ip_str);
                            }
                        }
                    }
                }
            }
        }
    }

    servers
}

#[cfg(target_os = "windows")]
pub fn get_dns_servers() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut servers = Vec::new();

    // 1. Fast ipconfig /all via cmd (avoids PowerShell overhead / hang)
    let mut cmd = std::process::Command::new("cmd");
    cmd.args(["/C", "ipconfig /all"]);
    cmd.creation_flags(CREATE_NO_WINDOW);
    if let Ok(out) = cmd.output() {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            let mut in_dns_section = false;
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("DNS Servers") || trimmed.starts_with("DNS 服务器") {
                    if let Some((_, val)) = trimmed.split_once(':') {
                        let ip = val.trim();
                        if !ip.is_empty() && !servers.contains(&ip.to_string()) {
                            servers.push(ip.to_string());
                        }
                    }
                    in_dns_section = true;
                } else if in_dns_section {
                    if trimmed.is_empty() || trimmed.contains(':') {
                        in_dns_section = false;
                    } else {
                        let ip = trimmed.trim();
                        if !ip.is_empty() && !servers.contains(&ip.to_string()) {
                            servers.push(ip.to_string());
                        }
                    }
                }
            }
        }
    }

    servers
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn get_dns_servers() -> Vec<String> {
    Vec::new()
}

/// Gathers a snapshot of system hardware, OS version, memory, disks, and network metrics.
pub fn get_system_overview() -> SystemOverview {
    let mut sys = System::new_all();
    sys.refresh_all();

    // CPU information
    let cpu_model = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown CPU".to_string());
    let cpu_cores = sys.cpus().len();
    let cpu_usage_percent = sys.global_cpu_info().cpu_usage();

    // OS details
    let os_name = System::name().unwrap_or_else(|| std::env::consts::OS.to_string());
    let os_version = System::os_version().unwrap_or_else(|| "Unknown".to_string());
    let host_name = System::host_name().unwrap_or_else(|| "localhost".to_string());
    let kernel_version = System::kernel_version().unwrap_or_else(|| "Unknown".to_string());

    // Memory (bytes -> MB)
    let total_memory_mb = sys.total_memory() / (1024 * 1024);
    let used_memory_mb = sys.used_memory() / (1024 * 1024);
    let total_swap_mb = sys.total_swap() / (1024 * 1024);
    let used_swap_mb = sys.used_swap() / (1024 * 1024);

    // Disks
    let disks_list = Disks::new_with_refreshed_list();
    let disks = disks_list
        .iter()
        .map(|d| DiskInfo {
            name: d.name().to_string_lossy().to_string(),
            mount_point: d.mount_point().to_string_lossy().to_string(),
            total_space_mb: d.total_space() / (1024 * 1024),
            available_space_mb: d.available_space() / (1024 * 1024),
            file_system: d.file_system().to_string_lossy().to_string(),
            is_removable: d.is_removable(),
        })
        .collect();

    // Networks
    let networks_list = Networks::new_with_refreshed_list();
    let networks = networks_list
        .iter()
        .map(|(name, data)| NetworkInterfaceInfo {
            name: name.clone(),
            mac_address: data.mac_address().to_string(),
            received_bytes: data.received(),
            transmitted_bytes: data.transmitted(),
            total_received_bytes: data.total_received(),
            total_transmitted_bytes: data.total_transmitted(),
        })
        .collect();

    let local_ips = get_local_ips();
    let default_gateway = get_default_gateway();
    let dns_servers = get_dns_servers();

    SystemOverview {
        os_name,
        os_version,
        host_name,
        kernel_version,
        cpu_model,
        cpu_cores,
        cpu_usage_percent,
        total_memory_mb,
        used_memory_mb,
        total_swap_mb,
        used_swap_mb,
        uptime_secs: System::uptime(),
        boot_time_secs: System::boot_time(),
        disks,
        networks,
        local_ips,
        default_gateway,
        dns_servers,
    }
}
