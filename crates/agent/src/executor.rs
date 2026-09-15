//! Agent local tool execution engine.
//! Dispatches tool invocations asynchronously, tracks active calls,
//! and manages background process lifecycles and cancellation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::task::AbortHandle;
use serde_json::Value;

use crate::tools::dispatch_tool_with_call_id_and_options;
use crate::tools::process_registry::ProcessRegistry;

/// Handle tracking an active in-flight tool call
#[derive(Debug)]
pub struct CallHandle {
    pub abort_handle: AbortHandle,
    pub start_time: std::time::Instant,
    pub is_cancelled: Arc<std::sync::atomic::AtomicBool>,
}

/// Agent local execution engine
#[derive(Clone)]
pub struct AgentExecutor {
    process_registry: Arc<ProcessRegistry>,
    active_calls: Arc<Mutex<HashMap<String, CallHandle>>>,
    pub enable_computer_use: bool,
}

impl Default for AgentExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentExecutor {
    /// Creates a new AgentExecutor instance with its own isolated ProcessRegistry.
    pub fn new() -> Self {
        Self {
            process_registry: Arc::new(ProcessRegistry::new()),
            active_calls: Arc::new(Mutex::new(HashMap::new())),
            enable_computer_use: false,
        }
    }

    /// Creates an AgentExecutor with a custom ProcessRegistry.
    pub fn with_registry(process_registry: Arc<ProcessRegistry>) -> Self {
        Self {
            process_registry,
            active_calls: Arc::new(Mutex::new(HashMap::new())),
            enable_computer_use: false,
        }
    }

    /// Enables or disables computer-use tools execution
    pub fn with_computer_use(mut self, enabled: bool) -> Self {
        self.enable_computer_use = enabled;
        self
    }

    /// Dynamically toggles computer-use permission
    pub fn set_computer_use(&mut self, enabled: bool) {
        self.enable_computer_use = enabled;
    }

    /// Checks if computer-use is currently enabled
    pub fn is_computer_use_enabled(&self) -> bool {
        self.enable_computer_use
    }

    /// Executes an MCP diagnostic tool by name with arguments without explicit call tracking.
    pub async fn execute(&self, tool_name: &str, arguments: Value) -> Result<Value, String> {
        let call_id = format!("anon-{}", chrono::Utc::now().timestamp_micros());
        self.execute_with_call_id(&call_id, tool_name, arguments).await
    }

    /// Executes an MCP diagnostic tool by name with arguments and registers it for cancellation.
    pub async fn execute_with_call_id(
        &self,
        call_id: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<Value, String> {
        let name = tool_name.to_string();
        let registry = self.process_registry.clone();
        let cid = call_id.to_string();
        let cid_clone = cid.clone();
        let is_cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let is_cancelled_clone = is_cancelled.clone();
        let enable_cu = self.enable_computer_use;

        let should_review = matches!(
            name.as_str(),
            "mouse_click" | "click_element" | "click_mark" | "press_key" | "hotkey"
        );
        let before_state = if should_review {
            crate::tools::window::capture_active_window_state()
        } else {
            None
        };

        let join_handle = tokio::task::spawn_blocking(move || {
            dispatch_tool_with_call_id_and_options(
                &cid_clone,
                &name,
                arguments,
                &registry,
                enable_cu,
            )
        });

        let abort_handle = join_handle.abort_handle();
        {
            let mut calls = self.active_calls.lock().unwrap();
            calls.insert(
                cid.clone(),
                CallHandle {
                    abort_handle,
                    start_time: std::time::Instant::now(),
                    is_cancelled,
                },
            );
        }

        let res = join_handle.await;

        {
            let mut calls = self.active_calls.lock().unwrap();
            calls.remove(&cid);
        }

        if is_cancelled_clone.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(format!("Tool '{}' execution was cancelled", tool_name));
        }

        let final_res = match res {
            Ok(tool_res) => tool_res,
            Err(e) if e.is_cancelled() => Err(format!("Tool '{}' execution was cancelled", tool_name)),
            Err(e) => Err(format!("Task join error: {}", e)),
        };

        if should_review {
            crate::tools::window::review_action_loop(before_state, final_res).await
        } else {
            final_res
        }
    }

    /// Handles cancellation request for a tool execution call.
    /// Terminates all child processes spawned by this call and aborts the async task.
    pub async fn cancel(&self, call_id: &str) -> bool {
        // 1. Mark is_cancelled and abort handle
        let handle = {
            let mut calls = self.active_calls.lock().unwrap();
            calls.remove(call_id)
        };

        let had_handle = if let Some(ref h) = handle {
            h.is_cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
            h.abort_handle.abort();
            true
        } else {
            false
        };

        // 2. Kill any child process tree
        let killed = self.process_registry.kill_call_processes(call_id);

        if had_handle {
            tracing::info!("Cancelled call [{}] (killed {} subprocesses)", call_id, killed);
            true
        } else {
            tracing::debug!("Cancel requested for call [{}], but no active handle found (killed {} subprocesses)", call_id, killed);
            killed > 0
        }
    }

    /// Synchronous cancellation helper
    pub fn cancel_sync(&self, call_id: &str) -> bool {
        let killed = self.process_registry.kill_call_processes(call_id);
        let had_handle = {
            if let Ok(mut calls) = self.active_calls.lock() {
                if let Some(h) = calls.remove(call_id) {
                    h.is_cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
                    h.abort_handle.abort();
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        had_handle || killed > 0
    }

    /// Returns the number of currently active in-flight calls.
    pub async fn active_call_count(&self) -> usize {
        self.active_calls.lock().unwrap().len()
    }

    /// Emergency kill switch: terminates all active subprocesses.
    pub fn kill_all_processes(&self) -> usize {
        self.process_registry.kill_all_active()
    }

    /// Returns a reference to the process registry.
    pub fn process_registry(&self) -> Arc<ProcessRegistry> {
        self.process_registry.clone()
    }
}
