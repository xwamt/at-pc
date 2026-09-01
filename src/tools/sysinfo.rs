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
    }
}
