//! Command execution diagnostics tool.
//! Executes PowerShell scripts and shell/CMD commands silently in the background
//! with timeout protection and stdout/stderr capture.

use crate::tools::process_registry::{kill_process_tree, ProcessRegistry};
use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::Command as TokioCommand;

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
    cmd: Command,
    timeout_secs: u64,
    registry: &Arc<ProcessRegistry>,
) -> Result<CommandResult, String> {
    run_command_with_registry_and_call(cmd, timeout_secs, registry, None)
}

/// Runs a command with the given timeout, registering it with both internal ID and optional call_id.
pub fn run_command_with_registry_and_call(
    cmd: Command,
    timeout_secs: u64,
    registry: &Arc<ProcessRegistry>,
    call_id: Option<&str>,
) -> Result<CommandResult, String> {
    let timeout = if timeout_secs == 0 { 30 } else { timeout_secs };
    let timeout_duration = Duration::from_secs(timeout);
    let registry = registry.clone();
    let call_id = call_id.map(str::to_string);
    block_on_command(async move {
        run_command_async(cmd, timeout, timeout_duration, registry, call_id.as_deref()).await
    })
}

fn block_on_command<T, F>(fut: F) -> T
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            let (tx, rx) = std::sync::mpsc::channel();
            handle.spawn(async move {
                let _ = tx.send(fut.await);
            });
            rx.recv().expect("command task dropped")
        }
        Err(_) => tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .expect("failed to build command runtime")
            .block_on(fut),
    }
}

async fn run_command_async(
    mut cmd: Command,
    timeout: u64,
    timeout_duration: Duration,
    registry: Arc<ProcessRegistry>,
    call_id: Option<&str>,
) -> Result<CommandResult, String> {
    let start_time = Instant::now();

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        apply_no_window(&mut cmd);
    }

    let mut tokio_cmd = TokioCommand::from(cmd);
    tokio_cmd.kill_on_drop(true);

    let mut child = tokio_cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn process: {}", e))?;

    let pid = child
        .id()
        .ok_or_else(|| "Failed to get process id".to_string())?;
    let proc_id = NEXT_PROC_ID.fetch_add(1, Ordering::SeqCst);
    if let Some(cid) = call_id {
        registry.register_process_with_call(cid, proc_id, pid);
    } else {
        registry.register_process(proc_id, pid);
    }
    let _guard = SubprocessGuard {
        id: proc_id,
        registry: registry.clone(),
    };

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stdout_pipe {
            let _ = pipe.read_to_end(&mut buf).await;
        }
        buf
    });
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stderr_pipe {
            let _ = pipe.read_to_end(&mut buf).await;
        }
        buf
    });

    match tokio::time::timeout(timeout_duration, child.wait()).await {
        Ok(Ok(status)) => {
            let stdout_bytes = tokio::time::timeout(Duration::from_secs(2), stdout_task)
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default();
            let stderr_bytes = tokio::time::timeout(Duration::from_secs(2), stderr_task)
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default();
            let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
            let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();
            let duration_ms = start_time.elapsed().as_millis() as u64;
            let exit_code = status.code().unwrap_or(-1);

            Ok(CommandResult {
                stdout,
                stderr,
                exit_code,
                duration_ms,
            })
        }
        Ok(Err(e)) => {
            kill_process_tree(pid);
            let _ = child.kill().await;
            Err(format!("Error waiting for process: {}", e))
        }
        Err(_) => {
            kill_process_tree(pid);
            let _ = child.start_kill();
            let _ = child.wait().await;
            Err(format!("Command timed out after {} seconds", timeout))
        }
    }
}

/// Executes a PowerShell script in a hidden background process.
pub fn exec_powershell(
    script: &str,
    timeout_secs: u64,
    cwd: Option<&str>,
) -> Result<CommandResult, String> {
    exec_powershell_with_registry(script, timeout_secs, cwd, &ProcessRegistry::global())
}

/// Inspects a command string and blocks destructive file deletion or formatting operations.
pub fn check_command_safety(command: &str) -> Result<(), String> {
    let lower = command.to_lowercase();
    let trimmed = lower.trim();

    let dangerous_keywords = [
        "del /",
        "del /f",
        "del /s",
        "del /q",
        "rmdir /s",
        "rd /s",
        "remove-item",
        "format ",
        "diskpart",
        "rm -rf",
        "rm -r ",
        "rm -fr",
        "mkfs",
        ":(){ :|:& };:",
    ];

    for kw in &dangerous_keywords {
        if trimmed.contains(kw) {
            return Err(format!(
                "Security Guardrail: Execution blocked! Command contains potentially destructive operation '{}'. To prevent accidental data loss, delete/format operations are restricted.",
                kw
            ));
        }
    }

    Ok(())
}

/// Executes a PowerShell script with a specific ProcessRegistry.
pub fn exec_powershell_with_registry(
    script: &str,
    timeout_secs: u64,
    cwd: Option<&str>,
    registry: &Arc<ProcessRegistry>,
) -> Result<CommandResult, String> {
    exec_powershell_with_call(script, timeout_secs, cwd, registry, None)
}

/// Executes a PowerShell script with a specific ProcessRegistry and optional call_id for cancellation.
pub fn exec_powershell_with_call(
    script: &str,
    timeout_secs: u64,
    cwd: Option<&str>,
    registry: &Arc<ProcessRegistry>,
    call_id: Option<&str>,
) -> Result<CommandResult, String> {
    let script = script.to_string();
    let cwd = cwd.map(str::to_string);
    let registry = registry.clone();
    let call_id = call_id.map(str::to_string);
    block_on_command(async move {
        exec_powershell_with_call_async(
            &script,
            timeout_secs,
            cwd.as_deref(),
            &registry,
            call_id.as_deref(),
        )
        .await
    })
}

/// Async PowerShell execution for the production executor path.
pub async fn exec_powershell_with_call_async(
    script: &str,
    timeout_secs: u64,
    cwd: Option<&str>,
    registry: &Arc<ProcessRegistry>,
    call_id: Option<&str>,
) -> Result<CommandResult, String> {
    check_command_safety(script)?;
    let (timeout, timeout_duration) = command_timeout(timeout_secs);
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
        run_command_async(cmd, timeout, timeout_duration, registry.clone(), call_id).await
    }

    #[cfg(not(windows))]
    {
        let mut pwsh_cmd = Command::new("pwsh");
        pwsh_cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        if let Some(dir) = cwd {
            pwsh_cmd.current_dir(dir);
        }

        match run_command_async(
            pwsh_cmd,
            timeout,
            timeout_duration,
            registry.clone(),
            call_id,
        )
        .await
        {
            Ok(res) => Ok(res),
            Err(e) if e.contains("No such file or directory") || e.contains("not found") => {
                let mut sh_cmd = Command::new("sh");
                sh_cmd.args(["-c", script]);
                if let Some(dir) = cwd {
                    sh_cmd.current_dir(dir);
                }
                run_command_async(sh_cmd, timeout, timeout_duration, registry.clone(), call_id)
                    .await
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
    exec_cmd_with_call(command, timeout_secs, cwd, registry, None)
}

/// Executes a command in the system shell with a specific ProcessRegistry and optional call_id for cancellation.
pub fn exec_cmd_with_call(
    command: &str,
    timeout_secs: u64,
    cwd: Option<&str>,
    registry: &Arc<ProcessRegistry>,
    call_id: Option<&str>,
) -> Result<CommandResult, String> {
    let command = command.to_string();
    let cwd = cwd.map(str::to_string);
    let registry = registry.clone();
    let call_id = call_id.map(str::to_string);
    block_on_command(async move {
        exec_cmd_with_call_async(
            &command,
            timeout_secs,
            cwd.as_deref(),
            &registry,
            call_id.as_deref(),
        )
        .await
    })
}

/// Async shell command execution for the production executor path.
pub async fn exec_cmd_with_call_async(
    command: &str,
    timeout_secs: u64,
    cwd: Option<&str>,
    registry: &Arc<ProcessRegistry>,
    call_id: Option<&str>,
) -> Result<CommandResult, String> {
    check_command_safety(command)?;
    let (timeout, timeout_duration) = command_timeout(timeout_secs);
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/C", command]);
        apply_no_window(&mut cmd);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        run_command_async(cmd, timeout, timeout_duration, registry.clone(), call_id).await
    }

    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", command]);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        run_command_async(cmd, timeout, timeout_duration, registry.clone(), call_id).await
    }
}

fn command_timeout(timeout_secs: u64) -> (u64, Duration) {
    let timeout = if timeout_secs == 0 { 30 } else { timeout_secs };
    (timeout, Duration::from_secs(timeout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_command_safety() {
        // Safe commands
        assert!(check_command_safety("ipconfig /all").is_ok());
        assert!(check_command_safety("Get-Process").is_ok());
        assert!(check_command_safety("echo hello world").is_ok());
        assert!(check_command_safety("dir C:\\Windows").is_ok());
        assert!(check_command_safety("ls -la").is_ok());

        // Dangerous commands
        assert!(check_command_safety("del /f /q C:\\myfile.txt").is_err());
        assert!(check_command_safety("del /s /q test").is_err());
        assert!(check_command_safety("rmdir /s /q C:\\data").is_err());
        assert!(check_command_safety("rd /s /q D:\\").is_err());
        assert!(check_command_safety("Remove-Item -Path C:\\Temp -Recurse").is_err());
        assert!(check_command_safety("format D: /fs:NTFS").is_err());
        assert!(check_command_safety("rm -rf /tmp/abc").is_err());
    }
}
