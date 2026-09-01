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
}

static GLOBAL_REGISTRY: OnceLock<Arc<ProcessRegistry>> = OnceLock::new();

impl ProcessRegistry {
    /// Creates a new `ProcessRegistry` instance.
    pub fn new() -> Self {
        Self {
            active_processes: RwLock::new(HashMap::new()),
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

    /// Unregisters a completed child process by its internal execution ID.
    pub fn unregister_process(&self, id: u64) {
        if let Ok(mut procs) = self.active_processes.write() {
            procs.remove(&id);
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

    /// Forcibly terminates all registered active child processes and clears the registry.
    /// Returns the number of processes targeted for termination.
    pub fn kill_all_active(&self) -> usize {
        let pids: Vec<u32> = if let Ok(mut procs) = self.active_processes.write() {
            let drained: Vec<u32> = procs.drain().map(|(_, pid)| pid).collect();
            drained
        } else {
            Vec::new()
        };

        if pids.is_empty() {
            return 0;
        }

        let total_killed = pids.len();

        // 1. Direct OS termination command for immediate SIGKILL / tree termination
        for &pid in &pids {
            #[cfg(unix)]
            {
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
        }

        // 2. Sysinfo kill fallback / verification
        let mut sys = System::new_all();
        sys.refresh_processes();
        for &pid in &pids {
            let pid_obj = Pid::from_u32(pid);
            if let Some(proc_data) = sys.processes().get(&pid_obj) {
                let _ = proc_data
                    .kill_with(Signal::Kill)
                    .unwrap_or_else(|| proc_data.kill());
            }
        }

        total_killed
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
}
