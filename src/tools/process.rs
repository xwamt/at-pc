//! Process inspection and management tools.

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, Signal, System};

/// Information summary for a running process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_usage: f32,
    pub memory_mb: u64,
    pub exe_path: Option<String>,
    pub cmd: Vec<String>,
    pub status: String,
    pub start_time: u64,
    pub parent_pid: Option<u32>,
}

/// Lists running processes with optional filtering, sorting, and limit.
///
/// # Arguments
/// * `filter` - Substring filter matched case-insensitively against process name, cmd, or PID.
/// * `sort_by` - Field to sort by: `"cpu"`, `"memory"`, `"pid"`, or `"name"`. Defaults to `"memory"`.
/// * `limit` - Maximum number of processes to return (0 or large value returns all).
pub fn list_processes(
    filter: Option<&str>,
    sort_by: Option<&str>,
    limit: usize,
) -> Vec<ProcessInfo> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let filter_lower = filter.map(|f| f.trim().to_lowercase());

    let mut procs: Vec<ProcessInfo> = sys
        .processes()
        .iter()
        .filter_map(|(pid, proc_data)| {
            let pid_u32 = pid.as_u32();
            let name = proc_data.name().to_string();
            let cmd: Vec<String> = proc_data.cmd().to_vec();

            if let Some(ref query) = filter_lower {
                let matches_name = name.to_lowercase().contains(query);
                let matches_pid = pid_u32.to_string().contains(query);
                let matches_cmd = cmd.iter().any(|arg| arg.to_lowercase().contains(query));

                if !matches_name && !matches_pid && !matches_cmd {
                    return None;
                }
            }

            Some(ProcessInfo {
                pid: pid_u32,
                name,
                cpu_usage: proc_data.cpu_usage(),
                memory_mb: proc_data.memory() / (1024 * 1024),
                exe_path: proc_data.exe().map(|p| p.to_string_lossy().to_string()),
                cmd,
                status: format!("{:?}", proc_data.status()),
                start_time: proc_data.start_time(),
                parent_pid: proc_data.parent().map(|p| p.as_u32()),
            })
        })
        .collect();

    // Sort
    match sort_by.unwrap_or("memory").to_lowercase().as_str() {
        "cpu" => {
            procs.sort_by(|a, b| {
                b.cpu_usage
                    .partial_cmp(&a.cpu_usage)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        "pid" => {
            procs.sort_by_key(|p| p.pid);
        }
        "name" => {
            procs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        }
        _ => {
            // Default: sort by memory descending
            procs.sort_by(|a, b| b.memory_mb.cmp(&a.memory_mb));
        }
    }

    if limit > 0 && procs.len() > limit {
        procs.truncate(limit);
    }

    procs
}

/// Terminates a process by PID or executable name.
///
/// # Arguments
/// * `pid` - Target process ID.
/// * `name` - Target process executable name (used if `pid` is not specified).
/// * `force` - Whether to send SIGKILL/terminate forcibly (`true`) or SIGTERM (`false`).
pub fn kill_process(pid: Option<u32>, name: Option<&str>, force: bool) -> Result<String, String> {
    if pid.is_none() && name.is_none() {
        return Err("Either 'pid' or 'name' must be provided to terminate a process".to_string());
    }

    let mut sys = System::new_all();
    sys.refresh_processes();

    if let Some(target_pid) = pid {
        let pid_obj = Pid::from_u32(target_pid);
        if let Some(proc_data) = sys.processes().get(&pid_obj) {
            let proc_name = proc_data.name().to_string();
            let killed = if force {
                proc_data.kill_with(Signal::Kill).unwrap_or_else(|| proc_data.kill())
            } else {
                proc_data.kill_with(Signal::Term).unwrap_or_else(|| proc_data.kill())
            };

            if killed {
                Ok(format!(
                    "Successfully terminated process PID {} ({})",
                    target_pid, proc_name
                ))
            } else {
                Err(format!(
                    "Failed to terminate process PID {} ({}) - permission denied or already exited",
                    target_pid, proc_name
                ))
            }
        } else {
            Err(format!("Process with PID {} not found", target_pid))
        }
    } else if let Some(target_name) = name {
        let target_lower = target_name.trim().to_lowercase();
        let target_exe_lower = if target_lower.ends_with(".exe") {
            target_lower.clone()
        } else {
            format!("{}.exe", target_lower)
        };

        let mut matched_pids: Vec<(u32, String)> = Vec::new();
        for (p, proc_data) in sys.processes() {
            let p_name = proc_data.name().to_lowercase();
            if p_name == target_lower || p_name == target_exe_lower {
                matched_pids.push((p.as_u32(), proc_data.name().to_string()));
            }
        }

        if matched_pids.is_empty() {
            return Err(format!(
                "No running processes found matching '{}'",
                target_name
            ));
        }

        let mut terminated_pids = Vec::new();
        let mut failed_pids = Vec::new();

        for (pid_val, p_name) in matched_pids {
            let pid_obj = Pid::from_u32(pid_val);
            if let Some(proc_data) = sys.processes().get(&pid_obj) {
                let killed = if force {
                    proc_data.kill_with(Signal::Kill).unwrap_or_else(|| proc_data.kill())
                } else {
                    proc_data.kill_with(Signal::Term).unwrap_or_else(|| proc_data.kill())
                };

                if killed {
                    terminated_pids.push(pid_val);
                } else {
                    failed_pids.push((pid_val, p_name));
                }
            }
        }

        if terminated_pids.is_empty() && !failed_pids.is_empty() {
            return Err(format!(
                "Failed to terminate processes matching '{}': {:?}",
                target_name, failed_pids
            ));
        }

        Ok(format!(
            "Terminated {} process(es) matching '{}': {:?}",
            terminated_pids.len(),
            target_name,
            terminated_pids
        ))
    } else {
        Err("Either 'pid' or 'name' must be provided to terminate a process".to_string())
    }
}
