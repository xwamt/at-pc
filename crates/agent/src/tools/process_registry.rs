//! Subprocess tracking and Kill Switch registry.
//! Allows registering spawned child process handles/PIDs, removing them upon completion,
//! and forcibly killing all active child processes during emergency stop.

use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, OnceLock, RwLock};
use sysinfo::{Pid, Signal, System};

/// Thread-safe registry tracking all active background command subprocesses.
#[derive(Debug, Default)]
pub struct ProcessRegistry {
    active_processes: RwLock<HashMap<u64, u32>>,
    call_processes: RwLock<HashMap<String, Vec<u32>>>,
    proc_call_map: RwLock<HashMap<u64, String>>,
}

static GLOBAL_REGISTRY: OnceLock<Arc<ProcessRegistry>> = OnceLock::new();

impl ProcessRegistry {
    /// Creates a new `ProcessRegistry` instance.
    pub fn new() -> Self {
        Self {
            active_processes: RwLock::new(HashMap::new()),
            call_processes: RwLock::new(HashMap::new()),
            proc_call_map: RwLock::new(HashMap::new()),
        }
    }

    /// Returns the global singleton `ProcessRegistry` instance.
    pub fn global() -> Arc<Self> {
        GLOBAL_REGISTRY
            .get_or_init(|| Arc::new(ProcessRegistry::new()))
            .clone()
    }

    /// Registers a newly spawned child process with its internal execution ID and OS PID.
    pub fn register_process(&self, id: u64, pid: u32) {
        if let Ok(mut procs) = self.active_processes.write() {
            procs.insert(id, pid);
        }
    }

    /// Registers a child process associated with a specific tool invocation call ID.
    pub fn register_process_with_call(&self, call_id: &str, id: u64, pid: u32) {
        self.register_process(id, pid);
        if !call_id.is_empty() {
            if let Ok(mut calls) = self.call_processes.write() {
                calls.entry(call_id.to_string()).or_default().push(pid);
            }
            if let Ok(mut mapping) = self.proc_call_map.write() {
                mapping.insert(id, call_id.to_string());
            }
        }
    }

    /// Unregisters a completed child process by its internal execution ID.
    pub fn unregister_process(&self, id: u64) {
        if let Ok(mut procs) = self.active_processes.write() {
            let pid_opt = procs.remove(&id);
            if let Ok(mut mapping) = self.proc_call_map.write() {
                if let Some(call_id) = mapping.remove(&id) {
                    if let Some(pid) = pid_opt {
                        if let Ok(mut calls) = self.call_processes.write() {
                            if let Some(pids) = calls.get_mut(&call_id) {
                                pids.retain(|&p| p != pid);
                                if pids.is_empty() {
                                    calls.remove(&call_id);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Returns the count of currently registered active child processes.
    pub fn active_count(&self) -> usize {
        self.active_processes
            .read()
            .map(|p| p.len())
            .unwrap_or_default()
    }

    /// Returns a list of all currently active PIDs.
    pub fn get_active_pids(&self) -> Vec<u32> {
        self.active_processes
            .read()
            .map(|p| p.values().copied().collect())
            .unwrap_or_default()
    }

    /// Forcibly kills the entire process tree associated with a specific call ID.
    /// Returns the number of processes targeted.
    pub fn kill_call_processes(&self, call_id: &str) -> usize {
        let pids = {
            let mut calls = match self.call_processes.write() {
                Ok(c) => c,
                Err(_) => return 0,
            };
            calls.remove(call_id).unwrap_or_default()
        };

        if pids.is_empty() {
            return 0;
        }

        // Clean up from active_processes as well
        if let Ok(mut procs) = self.active_processes.write() {
            procs.retain(|_, pid| !pids.contains(pid));
        }

        if let Ok(mut mapping) = self.proc_call_map.write() {
            mapping.retain(|_, cid| cid != call_id);
        }

        for &pid in &pids {
            kill_process_tree(pid);
        }

        pids.len()
    }

    /// Forcibly terminates all registered active child processes and clears the registry.
    /// Returns the number of processes targeted for termination.
    pub fn kill_all_active(&self) -> usize {
        let pids: Vec<u32> = if let Ok(mut procs) = self.active_processes.write() {
            let drained: Vec<u32> = procs.drain().map(|(_, pid)| pid).collect();
            drained
        } else {
            Vec::new()
        };

        if let Ok(mut calls) = self.call_processes.write() {
            calls.clear();
        }
        if let Ok(mut mapping) = self.proc_call_map.write() {
            mapping.clear();
        }

        if pids.is_empty() {
            return 0;
        }

        for &pid in &pids {
            kill_process_tree(pid);
        }

        pids.len()
    }
}

/// Forcibly terminates a process and all its children (process tree).
pub fn kill_process_tree(pid: u32) {
    // 1. Direct OS termination command for immediate SIGKILL / tree termination
    #[cfg(unix)]
    {
        // Try killing process group first, then direct PID
        let _ = Command::new("kill")
            .args(["-9", &format!("-{}", pid)])
            .output();
        let _ = Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output();
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let mut cmd = Command::new("taskkill");
        cmd.args(["/F", "/T", "/PID", &pid.to_string()]);
        cmd.creation_flags(CREATE_NO_WINDOW);
        let _ = cmd.output();
    }

    // 2. Sysinfo recursive child process hunt and termination
    let mut sys = System::new();
    sys.refresh_processes();

    // Find and terminate all child processes recursively
    let mut to_kill = vec![pid];
    let mut i = 0;
    while i < to_kill.len() {
        let parent_target = to_kill[i];
        for (p, proc_data) in sys.processes() {
            if let Some(ppid) = proc_data.parent() {
                if ppid.as_u32() == parent_target && !to_kill.contains(&p.as_u32()) {
                    to_kill.push(p.as_u32());
                }
            }
        }
        i += 1;
    }

    for p in to_kill {
        let pid_obj = Pid::from_u32(p);
        if let Some(proc_data) = sys.processes().get(&pid_obj) {
            let _ = proc_data
                .kill_with(Signal::Kill)
                .unwrap_or_else(|| proc_data.kill());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_registry_basic_flow() {
        let registry = ProcessRegistry::new();
        assert_eq!(registry.active_count(), 0);

        registry.register_process(1, 12345);
        registry.register_process(2, 67890);
        assert_eq!(registry.active_count(), 2);

        let pids = registry.get_active_pids();
        assert!(pids.contains(&12345));
        assert!(pids.contains(&67890));

        registry.unregister_process(1);
        assert_eq!(registry.active_count(), 1);

        registry.unregister_process(2);
        assert_eq!(registry.active_count(), 0);
    }

    #[test]
    fn test_process_registry_call_scoped_tracking() {
        let registry = ProcessRegistry::new();
        registry.register_process_with_call("call-abc", 10, 99991);
        registry.register_process_with_call("call-abc", 11, 99992);
        registry.register_process_with_call("call-xyz", 12, 99993);

        assert_eq!(registry.active_count(), 3);

        // Kill processes for call-abc
        let killed = registry.kill_call_processes("call-abc");
        assert_eq!(killed, 2);
        assert_eq!(registry.active_count(), 1);

        // call-xyz remains
        assert_eq!(registry.get_active_pids(), vec![99993]);
        registry.unregister_process(12);
        assert_eq!(registry.active_count(), 0);
    }
}
