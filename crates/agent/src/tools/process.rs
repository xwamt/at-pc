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

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const PROCESS_CACHE_TTL: Duration = Duration::from_millis(1000);

struct CachedSystemState {
    sys: System,
    last_refresh: Instant,
}

static SYSTEM_CACHE: OnceLock<Mutex<CachedSystemState>> = OnceLock::new();

fn get_cached_system() -> &'static Mutex<CachedSystemState> {
    SYSTEM_CACHE.get_or_init(|| {
        let mut sys = System::new();
        sys.refresh_processes();
        Mutex::new(CachedSystemState {
            sys,
            last_refresh: Instant::now(),
        })
    })
}

/// Invalidates the process cache so that the next `list_processes` call refreshes unconditionally.
pub fn invalidate_process_cache() {
    let mut guard = get_cached_system().lock().unwrap();
    guard.last_refresh = Instant::now()
        .checked_sub(PROCESS_CACHE_TTL + Duration::from_millis(100))
        .unwrap_or_else(Instant::now);
}

fn format_process_status(status: sysinfo::ProcessStatus) -> String {
    match status {
        sysinfo::ProcessStatus::Idle => "Idle".to_string(),
        sysinfo::ProcessStatus::Run => "Run".to_string(),
        sysinfo::ProcessStatus::Sleep => "Sleep".to_string(),
        sysinfo::ProcessStatus::Stop => "Stop".to_string(),
        sysinfo::ProcessStatus::Zombie => "Zombie".to_string(),
        sysinfo::ProcessStatus::Tracing => "Tracing".to_string(),
        sysinfo::ProcessStatus::Dead => "Dead".to_string(),
        sysinfo::ProcessStatus::Wakekill => "Wakekill".to_string(),
        sysinfo::ProcessStatus::Waking => "Waking".to_string(),
        sysinfo::ProcessStatus::Parked => "Parked".to_string(),
        sysinfo::ProcessStatus::LockBlocked => "LockBlocked".to_string(),
        sysinfo::ProcessStatus::UninterruptibleDiskSleep => "UninterruptibleDiskSleep".to_string(),
        other => format!("{:?}", other),
    }
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
    let sys_guard = get_cached_system();
    let mut guard = sys_guard.lock().unwrap();
    if guard.last_refresh.elapsed() >= PROCESS_CACHE_TTL {
        guard.sys.refresh_processes();
        guard.last_refresh = Instant::now();
    }

    let filter_lower = filter.map(|f| f.trim().to_lowercase());

    let mut proc_refs: Vec<(&Pid, &sysinfo::Process)> = guard
        .sys
        .processes()
        .iter()
        .filter(|(pid, proc_data)| {
            if let Some(ref query) = filter_lower {
                let raw_name = proc_data.name();
                let pid_u32 = pid.as_u32();
                let matches_name = raw_name.to_lowercase().contains(query);
                let matches_pid = pid_u32.to_string().contains(query);
                let matches_cmd = proc_data
                    .cmd()
                    .iter()
                    .any(|arg| arg.to_lowercase().contains(query));

                matches_name || matches_pid || matches_cmd
            } else {
                true
            }
        })
        .collect();

    // Sort references
    match sort_by.unwrap_or("memory").to_lowercase().as_str() {
        "cpu" => {
            proc_refs.sort_by(|a, b| {
                b.1.cpu_usage()
                    .partial_cmp(&a.1.cpu_usage())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        "pid" => {
            proc_refs.sort_by_key(|p| p.0.as_u32());
        }
        "name" => {
            proc_refs.sort_by_key(|p| p.1.name().to_lowercase());
        }
        _ => {
            // Default: sort by memory descending
            proc_refs.sort_by_key(|p| std::cmp::Reverse(p.1.memory()));
        }
    }

    if limit > 0 && proc_refs.len() > limit {
        proc_refs.truncate(limit);
    }

    proc_refs
        .into_iter()
        .map(|(pid, proc_data)| ProcessInfo {
            pid: pid.as_u32(),
            name: proc_data.name().to_string(),
            cpu_usage: proc_data.cpu_usage(),
            memory_mb: proc_data.memory() / (1024 * 1024),
            exe_path: proc_data.exe().map(|p| p.to_string_lossy().to_string()),
            cmd: proc_data.cmd().to_vec(),
            status: format_process_status(proc_data.status()),
            start_time: proc_data.start_time(),
            parent_pid: proc_data.parent().map(|p| p.as_u32()),
        })
        .collect()
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
                proc_data
                    .kill_with(Signal::Kill)
                    .unwrap_or_else(|| proc_data.kill())
            } else {
                proc_data
                    .kill_with(Signal::Term)
                    .unwrap_or_else(|| proc_data.kill())
            };

            if killed {
                invalidate_process_cache();
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
                    proc_data
                        .kill_with(Signal::Kill)
                        .unwrap_or_else(|| proc_data.kill())
                } else {
                    proc_data
                        .kill_with(Signal::Term)
                        .unwrap_or_else(|| proc_data.kill())
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

        if !terminated_pids.is_empty() {
            invalidate_process_cache();
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
