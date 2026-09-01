//! Command execution diagnostics tool.
//! Executes PowerShell scripts and shell/CMD commands silently in the background
//! with timeout protection and stdout/stderr capture.

use crate::tools::process_registry::ProcessRegistry;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

static NEXT_PROC_ID: AtomicU64 = AtomicU64::new(1);

struct SubprocessGuard {
    id: u64,
    registry: Arc<ProcessRegistry>,
}

impl Drop for SubprocessGuard {
    fn drop(&mut self) {
        self.registry.unregister_process(self.id);
    }
}

/// Output of a command execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
}

#[cfg(windows)]
fn apply_no_window(cmd: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
#[allow(dead_code)]
fn apply_no_window(_cmd: &mut Command) {}

/// Runs a command with the given timeout in seconds and captures its output.
#[allow(dead_code)]
pub fn run_command(cmd: Command, timeout_secs: u64) -> Result<CommandResult, String> {
    run_command_with_registry(cmd, timeout_secs, &ProcessRegistry::global())
}

/// Runs a command with the given timeout and registers it in the specified ProcessRegistry.
pub fn run_command_with_registry(
    mut cmd: Command,
    timeout_secs: u64,
    registry: &Arc<ProcessRegistry>,
) -> Result<CommandResult, String> {
    let timeout = if timeout_secs == 0 { 30 } else { timeout_secs };
    let timeout_duration = Duration::from_secs(timeout);
    let start_time = Instant::now();

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn process: {}", e))?;

    let proc_id = NEXT_PROC_ID.fetch_add(1, Ordering::SeqCst);
    registry.register_process(proc_id, child.id());
    let _guard = SubprocessGuard {
        id: proc_id,
        registry: registry.clone(),
    };

    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();

    let stdout_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stdout_pipe.take() {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

    let stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stderr_pipe.take() {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

    let poll_interval = Duration::from_millis(10);
    let exit_status;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                exit_status = Some(status);
                break;
            }
            Ok(None) => {
                if start_time.elapsed() >= timeout_duration {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_handle.join();
                    let _ = stderr_handle.join();
                    return Err(format!("Command timed out after {} seconds", timeout));
                }
                std::thread::sleep(poll_interval);
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_handle.join();
                let _ = stderr_handle.join();
                return Err(format!("Error waiting for process: {}", e));
            }
        }
    }

    let stdout_bytes = stdout_handle.join().unwrap_or_default();
    let stderr_bytes = stderr_handle.join().unwrap_or_default();

    let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
    let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();
    let duration_ms = start_time.elapsed().as_millis() as u64;
    let exit_code = exit_status.and_then(|s| s.code()).unwrap_or(-1);

    Ok(CommandResult {
        stdout,
        stderr,
        exit_code,
        duration_ms,
    })
}

/// Executes a PowerShell script in a hidden background process.
pub fn exec_powershell(
    script: &str,
    timeout_secs: u64,
    cwd: Option<&str>,
) -> Result<CommandResult, String> {
    exec_powershell_with_registry(script, timeout_secs, cwd, &ProcessRegistry::global())
}

/// Executes a PowerShell script with a specific ProcessRegistry.
pub fn exec_powershell_with_registry(
    script: &str,
    timeout_secs: u64,
    cwd: Option<&str>,
    registry: &Arc<ProcessRegistry>,
) -> Result<CommandResult, String> {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("powershell.exe");
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ]);
        apply_no_window(&mut cmd);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        run_command_with_registry(cmd, timeout_secs, registry)
    }

    #[cfg(not(windows))]
    {
        let mut pwsh_cmd = Command::new("pwsh");
        pwsh_cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        if let Some(dir) = cwd {
            pwsh_cmd.current_dir(dir);
        }

        match run_command_with_registry(pwsh_cmd, timeout_secs, registry) {
            Ok(res) => Ok(res),
            Err(e) if e.contains("No such file or directory") || e.contains("not found") => {
                let mut sh_cmd = Command::new("sh");
                sh_cmd.args(["-c", script]);
                if let Some(dir) = cwd {
                    sh_cmd.current_dir(dir);
                }
                run_command_with_registry(sh_cmd, timeout_secs, registry)
            }
            Err(e) => Err(e),
        }
    }
}

/// Executes a command in the system shell (`cmd.exe` on Windows, `sh` on Unix).
pub fn exec_cmd(
    command: &str,
    timeout_secs: u64,
    cwd: Option<&str>,
) -> Result<CommandResult, String> {
    exec_cmd_with_registry(command, timeout_secs, cwd, &ProcessRegistry::global())
}

/// Executes a command in the system shell with a specific ProcessRegistry.
pub fn exec_cmd_with_registry(
    command: &str,
    timeout_secs: u64,
    cwd: Option<&str>,
    registry: &Arc<ProcessRegistry>,
) -> Result<CommandResult, String> {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/C", command]);
        apply_no_window(&mut cmd);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        run_command_with_registry(cmd, timeout_secs, registry)
    }

    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", command]);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        run_command_with_registry(cmd, timeout_secs, registry)
    }
}
