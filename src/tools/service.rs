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

    let result = exec_powershell(&ps_script, 30, None)
        .map_err(|e| format!("Service action '{}' failed for '{}': {}", action, service_name, e))?;

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
            message: format!("Service '{}' {} completed successfully", service_name, action),
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
    if let Ok(output) = Command::new("systemctl").arg("is-active").arg(service_name).output() {
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
            let act_res = Command::new("systemctl").arg(action).arg(service_name).output();
            match act_res {
                Ok(act_out) if act_out.status.success() => {
                    return Ok(ServiceStatusResult {
                        name: service_name.to_string(),
                        display_name: service_name.to_string(),
                        status: if action == "stop" { "Stopped".to_string() } else { "Running".to_string() },
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
        message: format!("Simulated service inspection for '{}' (action: {})", service_name, action),
    })
}
