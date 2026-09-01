//! Agent local tool execution engine.
//! Dispatches tool invocations asynchronously and manages background processes.

use std::sync::Arc;
use serde_json::Value;
use crate::tools::dispatch_tool;
use crate::tools::process_registry::ProcessRegistry;

/// Agent local execution engine
#[derive(Clone, Default)]
pub struct AgentExecutor {
    process_registry: Arc<ProcessRegistry>,
}

impl AgentExecutor {
    /// Creates a new AgentExecutor instance.
    pub fn new() -> Self {
        Self {
            process_registry: ProcessRegistry::global(),
        }
    }

    /// Executes an MCP diagnostic tool by name with arguments.
    /// Runs on a blocking thread to avoid blocking the Tokio async runtime.
    pub async fn execute(&self, tool_name: &str, arguments: Value) -> Result<Value, String> {
        let name = tool_name.to_string();
        tokio::task::spawn_blocking(move || {
            dispatch_tool(&name, arguments)
        })
        .await
        .map_err(|e| format!("Task join error: {}", e))?
    }

    /// Handles cancellation request for a tool execution call.
    pub fn cancel(&self, _call_id: &str) {
        // Can be extended for fine-grained per-call cancellation
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
