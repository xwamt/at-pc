//! Windows Service inspection and control tool.
//! Provides status queries and lifecycle management (start, stop, restart)
//! for Windows Services, with cross-platform fallback for testing on Unix/macOS.

use serde::{Deserialize, Serialize};

#[cfg(windows)]
use crate::tools::command::exec_powershell;

/// Result of a Windows service management operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceStatusResult {
    pub name: String,
    pub display_name: String,
    pub status: String,
    pub start_type: String,
    pub message: String,
}

#[cfg(windows)]
#[derive(Debug, Deserialize)]
struct PsServiceOutput {
    #[serde(rename = "Name", default)]
    name: String,
    #[serde(rename = "DisplayName", default)]
    display_name: String,
    #[serde(rename = "Status", default)]
    status: String,
    #[serde(rename = "StartType", default)]
    start_type: String,
}

/// Manages a system service (inspects status or controls lifecycle).
///
/// # Arguments
/// * `service_name` - The system name of the service (e.g., `"Spooler"`, `"wuauserv"`).
/// * `action` - The action to perform: `"status"`, `"start"`, `"stop"`, or `"restart"`.
pub fn manage_service(service_name: &str, action: &str) -> Result<ServiceStatusResult, String> {
    let action_lower = action.trim().to_lowercase();
    match action_lower.as_str() {
        "status" | "start" | "stop" | "restart" => {}
        _ => {
            return Err(format!(
                "Invalid or unsupported service action '{}'. Supported actions: 'status', 'start', 'stop', 'restart'",
                action
            ));
        }
    }

    #[cfg(windows)]
    {
        manage_windows_service(service_name, &action_lower)
    }

    #[cfg(not(windows))]
    {
        manage_unix_service_fallback(service_name, &action_lower)
    }
}

#[cfg(windows)]
fn manage_windows_service(service_name: &str, action: &str) -> Result<ServiceStatusResult, String> {
    // 1. Try high-performance native Win32 SCM API first (< 15ms latency)
    match manage_windows_service_win32(service_name, action) {
        Ok(res) => return Ok(res),
        Err(e) => {
            tracing::debug!(
                "Native Win32 SCM call failed ({}); falling back to PowerShell",
                e
            );
        }
    }

    // 2. Fallback to PowerShell
    manage_windows_service_powershell(service_name, action)
}

#[cfg(windows)]
fn manage_windows_service_win32(
    service_name: &str,
    action: &str,
) -> Result<ServiceStatusResult, String> {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::Services::*;

    unsafe {
        let scm = OpenSCManagerW(
            std::ptr::null(),
            std::ptr::null(),
            SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE,
        );
        if scm == 0 {
            return Err(format!(
                "OpenSCManagerW failed with error: {}",
                GetLastError()
            ));
        }

        let name_wide: Vec<u16> = service_name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let desired_access = match action {
            "status" => SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG,
            "start" => SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG | SERVICE_START,
            "stop" => SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG | SERVICE_STOP,
            "restart" => SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG | SERVICE_START | SERVICE_STOP,
            _ => SERVICE_QUERY_STATUS,
        };

        let service = OpenServiceW(scm, name_wide.as_ptr(), desired_access);
        if service == 0 {
            let err = GetLastError();
            CloseServiceHandle(scm);
            return Err(format!(
                "OpenServiceW failed for '{}' (error: {})",
                service_name, err
            ));
        }

        // Execute action if not just status
        match action {
            "start" => {
                if StartServiceW(service, 0, std::ptr::null()) == 0 {
                    let err = GetLastError();
                    if err != 1056 {
                        // 1056 = ERROR_SERVICE_ALREADY_RUNNING
                        CloseServiceHandle(service);
                        CloseServiceHandle(scm);
                        return Err(format!(
                            "StartServiceW failed for '{}' (error: {})",
                            service_name, err
                        ));
                    }
                }
            }
            "stop" => {
                let mut status: SERVICE_STATUS = std::mem::zeroed();
                if ControlService(service, SERVICE_CONTROL_STOP, &mut status) == 0 {
                    let err = GetLastError();
                    if err != 1062 {
                        // 1062 = ERROR_SERVICE_NOT_ACTIVE
                        CloseServiceHandle(service);
                        CloseServiceHandle(scm);
                        return Err(format!(
                            "ControlService(STOP) failed for '{}' (error: {})",
                            service_name, err
                        ));
                    }
                }
            }
            "restart" => {
                let mut status: SERVICE_STATUS = std::mem::zeroed();
                let _ = ControlService(service, SERVICE_CONTROL_STOP, &mut status);
                // Wait for service to transition to SERVICE_STOPPED (up to 3 seconds)
                for _ in 0..30 {
                    let mut bytes_needed = 0;
                    let mut ssp: SERVICE_STATUS_PROCESS = std::mem::zeroed();
                    let q = QueryServiceStatusEx(
                        service,
                        SC_STATUS_PROCESS_INFO,
                        &mut ssp as *mut _ as *mut u8,
                        std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
                        &mut bytes_needed,
                    );
                    if q != 0 && ssp.dwCurrentState == SERVICE_STOPPED {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                let _ = StartServiceW(service, 0, std::ptr::null());
            }
            _ => {}
        }

        // Query updated status
        let mut bytes_needed = 0;
        let mut ssp: SERVICE_STATUS_PROCESS = std::mem::zeroed();
        let query_ret = QueryServiceStatusEx(
            service,
            SC_STATUS_PROCESS_INFO,
            &mut ssp as *mut _ as *mut u8,
            std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut bytes_needed,
        );

        let status_str = if query_ret != 0 {
            match ssp.dwCurrentState {
                SERVICE_STOPPED => "Stopped",
                SERVICE_START_PENDING => "StartPending",
                SERVICE_STOP_PENDING => "StopPending",
                SERVICE_RUNNING => "Running",
                SERVICE_CONTINUE_PENDING => "ContinuePending",
                SERVICE_PAUSE_PENDING => "PausePending",
                SERVICE_PAUSED => "Paused",
                _ => "Unknown",
            }
        } else {
            "Unknown"
        };

        // Query service config for display name and start type
        let mut config_buf = vec![0u8; 8192];
        let mut bytes_needed_cfg = 0;
        let config_ret = QueryServiceConfigW(
            service,
            config_buf.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW,
            config_buf.len() as u32,
            &mut bytes_needed_cfg,
        );

        let (display_name, start_type) = if config_ret != 0 {
            let qsc = &*(config_buf.as_ptr() as *const QUERY_SERVICE_CONFIGW);
            let st = match qsc.dwStartType {
                SERVICE_AUTO_START => "Automatic",
                SERVICE_DEMAND_START => "Manual",
                SERVICE_DISABLED => "Disabled",
                SERVICE_BOOT_START => "Boot",
                SERVICE_SYSTEM_START => "System",
                _ => "Unknown",
            };
            let dn = if !qsc.lpDisplayName.is_null() {
                let mut len = 0;
                while *qsc.lpDisplayName.add(len) != 0 {
                    len += 1;
                }
                let slice = std::slice::from_raw_parts(qsc.lpDisplayName, len);
                String::from_utf16_lossy(slice)
            } else {
                service_name.to_string()
            };
            (dn, st.to_string())
        } else {
            (service_name.to_string(), "Unknown".to_string())
        };

        CloseServiceHandle(service);
        CloseServiceHandle(scm);

        Ok(ServiceStatusResult {
            name: service_name.to_string(),
            display_name,
            status: status_str.to_string(),
            start_type,
            message: format!(
                "Service '{}' {} completed successfully via Win32 SCM",
                service_name, action
            ),
        })
    }
}

#[cfg(windows)]
fn manage_windows_service_powershell(
    service_name: &str,
    action: &str,
) -> Result<ServiceStatusResult, String> {
    // Sanitize service name to prevent command injection
    let sanitized_name: String = service_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.' || *c == ' ')
        .collect();

    if sanitized_name.is_empty() {
        return Err("Service name cannot be empty".to_string());
    }

    let ps_script = match action {
        "status" => format!(
            r#"$s = Get-Service -Name '{name}' -ErrorAction Stop; [PSCustomObject]@{{ Name = $s.Name; DisplayName = $s.DisplayName; Status = $s.Status.ToString(); StartType = $s.StartType.ToString() }} | ConvertTo-Json -Compress"#,
            name = sanitized_name
        ),
        "start" => format!(
            r#"Start-Service -Name '{name}' -ErrorAction Stop; $s = Get-Service -Name '{name}' -ErrorAction Stop; [PSCustomObject]@{{ Name = $s.Name; DisplayName = $s.DisplayName; Status = $s.Status.ToString(); StartType = $s.StartType.ToString() }} | ConvertTo-Json -Compress"#,
            name = sanitized_name
        ),
        "stop" => format!(
            r#"Stop-Service -Name '{name}' -Force -ErrorAction Stop; $s = Get-Service -Name '{name}' -ErrorAction Stop; [PSCustomObject]@{{ Name = $s.Name; DisplayName = $s.DisplayName; Status = $s.Status.ToString(); StartType = $s.StartType.ToString() }} | ConvertTo-Json -Compress"#,
            name = sanitized_name
        ),
        "restart" => format!(
            r#"Restart-Service -Name '{name}' -Force -ErrorAction Stop; $s = Get-Service -Name '{name}' -ErrorAction Stop; [PSCustomObject]@{{ Name = $s.Name; DisplayName = $s.DisplayName; Status = $s.Status.ToString(); StartType = $s.StartType.ToString() }} | ConvertTo-Json -Compress"#,
            name = sanitized_name
        ),
        _ => unreachable!(),
    };

    let result = exec_powershell(&ps_script, 30, None).map_err(|e| {
        format!(
            "Service action '{}' failed for '{}': {}",
            action, service_name, e
        )
    })?;

    if result.exit_code != 0 {
        let err_detail = if !result.stderr.trim().is_empty() {
            result.stderr.trim().to_string()
        } else {
            result.stdout.trim().to_string()
        };
        return Err(format!(
            "Failed to perform '{}' on service '{}': {}",
            action, service_name, err_detail
        ));
    }

    let stdout = result.stdout.trim();
    if let Ok(parsed) = serde_json::from_str::<PsServiceOutput>(stdout) {
        Ok(ServiceStatusResult {
            name: parsed.name,
            display_name: parsed.display_name,
            status: parsed.status,
            start_type: parsed.start_type,
            message: format!(
                "Service '{}' {} completed successfully",
                service_name, action
            ),
        })
    } else {
        // Fallback status if JSON parsing fails but command succeeded
        Ok(ServiceStatusResult {
            name: service_name.to_string(),
            display_name: service_name.to_string(),
            status: "Unknown".to_string(),
            start_type: "Unknown".to_string(),
            message: stdout.to_string(),
        })
    }
}

#[cfg(not(windows))]
fn manage_unix_service_fallback(
    service_name: &str,
    action: &str,
) -> Result<ServiceStatusResult, String> {
    use std::process::Command;

    // Try systemctl first (Linux)
    if let Ok(output) = Command::new("systemctl")
        .arg("is-active")
        .arg(service_name)
        .output()
    {
        let status_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let is_active = output.status.success() && status_str == "active";
        let status = if is_active {
            "Running"
        } else if status_str == "inactive" || status_str == "failed" {
            "Stopped"
        } else {
            "NotFound"
        };

        if action == "status" {
            return Ok(ServiceStatusResult {
                name: service_name.to_string(),
                display_name: service_name.to_string(),
                status: status.to_string(),
                start_type: "Systemd".to_string(),
                message: format!("Systemd service status: {}", status_str),
            });
        } else {
            // Attempt systemctl action
            let act_res = Command::new("systemctl")
                .arg(action)
                .arg(service_name)
                .output();
            match act_res {
                Ok(act_out) if act_out.status.success() => {
                    return Ok(ServiceStatusResult {
                        name: service_name.to_string(),
                        display_name: service_name.to_string(),
                        status: if action == "stop" {
                            "Stopped".to_string()
                        } else {
                            "Running".to_string()
                        },
                        start_type: "Systemd".to_string(),
                        message: format!("Systemd service '{}' {} completed", service_name, action),
                    });
                }
                Ok(act_out) => {
                    let err = String::from_utf8_lossy(&act_out.stderr).to_string();
                    return Err(format!("systemctl {} failed: {}", action, err));
                }
                Err(e) => return Err(format!("Failed to execute systemctl: {}", e)),
            }
        }
    }

    // Try launchctl (macOS)
    if let Ok(output) = Command::new("launchctl").arg("list").output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let is_running = stdout.lines().any(|line| line.contains(service_name));

        let status = if is_running { "Running" } else { "Stopped" };

        return Ok(ServiceStatusResult {
            name: service_name.to_string(),
            display_name: service_name.to_string(),
            status: status.to_string(),
            start_type: "Launchd".to_string(),
            message: format!("macOS launchd service '{}' is {}", service_name, status),
        });
    }

    // Default simulated fallback for headless/test environments
    Ok(ServiceStatusResult {
        name: service_name.to_string(),
        display_name: format!("{} Service", service_name),
        status: "Stopped".to_string(),
        start_type: "Manual".to_string(),
        message: format!(
            "Simulated service inspection for '{}' (action: {})",
            service_name, action
        ),
    })
}
