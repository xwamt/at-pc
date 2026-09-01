use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{oneshot, RwLock};
use tracing::{debug, info};
use serde_json::Value;

use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::TerminalStatus;
use crate::ws::handler::AgentMessageHandler;
use crate::ws::registry::{TerminalEntry, TerminalRegistry};

static CALL_COUNTER: AtomicU64 = AtomicU64::new(1);

/// MCP Tool Router that handles terminal selection, tool dispatch, and response correlation
pub struct McpRouter {
    registry: Arc<TerminalRegistry>,
    pending_calls: Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>>,
    active_terminal_id: Arc<RwLock<Option<String>>>,
}

impl McpRouter {
    /// Create a new MCP router instance
    pub fn new(registry: Arc<TerminalRegistry>) -> Self {
        Self {
            registry,
            pending_calls: Arc::new(Mutex::new(HashMap::new())),
            active_terminal_id: Arc::new(RwLock::new(None)),
        }
    }

    /// Access the underlying terminal registry
    pub fn registry(&self) -> &Arc<TerminalRegistry> {
        &self.registry
    }

    /// List all registered terminals with status and latest metrics
    pub async fn list_terminals(&self) -> Vec<TerminalEntry> {
        self.registry.list_terminals().await
    }

    /// Get details of a single terminal
    pub async fn get_terminal(&self, terminal_id: &str) -> Option<TerminalEntry> {
        self.registry.get_terminal(terminal_id).await
    }

    /// Set the active target terminal ID for this session
    pub async fn select_terminal(&self, terminal_id: &str) -> Result<TerminalEntry, String> {
        let entry = self
            .registry
            .get_terminal(terminal_id)
            .await
            .ok_or_else(|| format!("Terminal '{}' not found in registry", terminal_id))?;

        let mut active = self.active_terminal_id.write().await;
        *active = Some(terminal_id.to_string());
        info!("Active terminal set to: {}", terminal_id);
        Ok(entry)
    }

    /// Get the currently active terminal ID
    pub async fn get_active_terminal_id(&self) -> Option<String> {
        let active = self.active_terminal_id.read().await;
        active.clone()
    }

    /// Set active terminal ID directly
    pub async fn set_active_terminal_id(&self, terminal_id: Option<String>) {
        let mut active = self.active_terminal_id.write().await;
        *active = terminal_id;
    }

    /// Get details of currently active terminal
    pub async fn get_active_terminal(&self) -> Option<TerminalEntry> {
        let active_id = self.get_active_terminal_id().await?;
        self.registry.get_terminal(&active_id).await
    }

    /// Resolves target terminal for a tool call:
    /// 1. Uses explicit `terminal_id` if provided and non-empty.
    /// 2. Otherwise uses session `active_terminal_id`.
    /// 3. If no active terminal is set, falls back to the single online terminal if exactly 1 exists.
    /// 4. Otherwise returns an informative error.
    pub async fn resolve_target_terminal(&self, explicit_id: Option<&str>) -> Result<String, String> {
        if let Some(id) = explicit_id {
            let trimmed = id.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }

        if let Some(active_id) = self.get_active_terminal_id().await {
            return Ok(active_id);
        }

        let all_terminals = self.registry.list_terminals().await;
        let online_terminals: Vec<_> = all_terminals
            .into_iter()
            .filter(|t| t.status == TerminalStatus::Online)
            .collect();

        match online_terminals.len() {
            1 => {
                let fallback_id = online_terminals[0].info.terminal_id.clone();
                debug!("Auto-selected single online terminal: {}", fallback_id);
                Ok(fallback_id)
            }
            0 => Err("No online terminals connected. Please start at-pc-agent on the target machine.".to_string()),
            count => Err(format!(
                "Multiple terminals online ({count}). Please specify 'terminal_id' or select an active terminal using select_terminal.",
            )),
        }
    }

    /// Invoke a tool on a specific terminal asynchronously with timeout protection
    pub async fn invoke_tool(
        &self,
        terminal_id: &str,
        tool_name: &str,
        arguments: Value,
        timeout_secs: u64,
    ) -> Result<Value, String> {
        let term_entry = self.registry.get_terminal(terminal_id).await;
        if term_entry.is_none() {
            return Err(format!("Terminal '{}' not found in registry", terminal_id));
        }
        if term_entry.as_ref().map(|t| t.status) == Some(TerminalStatus::Offline) {
            return Err(format!("Terminal '{}' is currently offline", terminal_id));
        }

        let call_seq = CALL_COUNTER.fetch_add(1, Ordering::Relaxed);
        let call_id = format!("call-{}-{}", chrono::Utc::now().timestamp_millis(), call_seq);

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending_calls.lock().unwrap();
            pending.insert(call_id.clone(), tx);
        }

        let msg = ServerToAgentMessage::InvokeTool {
            call_id: call_id.clone(),
            tool_name: tool_name.to_string(),
            arguments,
            timeout_secs,
        };

        if let Err(e) = self.registry.send_to_terminal(terminal_id, msg).await {
            let mut pending = self.pending_calls.lock().unwrap();
            pending.remove(&call_id);
            return Err(format!("Failed to send tool invocation to terminal '{}': {}", terminal_id, e));
        }

        let effective_timeout = if timeout_secs == 0 { 35 } else { timeout_secs };

        match tokio::time::timeout(Duration::from_secs(effective_timeout), rx).await {
            Ok(Ok(Ok(result_value))) => Ok(result_value),
            Ok(Ok(Err(err_msg))) => Err(err_msg),
            Ok(Err(_oneshot_closed)) => {
                Err("Tool execution channel closed unexpectedly".to_string())
            }
            Err(_elapsed) => {
                {
                    let mut pending = self.pending_calls.lock().unwrap();
                    pending.remove(&call_id);
                }
                // Send best-effort CancelTool
                let _ = self.registry.send_to_terminal(
                    terminal_id,
                    ServerToAgentMessage::CancelTool { call_id: call_id.clone() },
                ).await;

                Err(format!(
                    "Tool '{}' execution timed out after {}s on terminal '{}'",
                    tool_name, effective_timeout, terminal_id
                ))
            }
        }
    }

    /// Process incoming AgentToServerMessage to resolve pending tool calls
    pub fn process_tool_result(&self, msg: AgentToServerMessage) {
        if let AgentToServerMessage::ToolResult {
            call_id,
            success,
            result,
            error,
            duration_ms,
        } = msg
        {
            let tx_opt = {
                let mut pending = self.pending_calls.lock().unwrap();
                pending.remove(&call_id)
            };

            if let Some(tx) = tx_opt {
                debug!(
                    "Resolved pending call [{}] in {}ms (success: {})",
                    call_id, duration_ms, success
                );
                if success {
                    let _ = tx.send(Ok(result));
                } else {
                    let err = error.unwrap_or_else(|| "Tool execution failed on agent".to_string());
                    let _ = tx.send(Err(err));
                }
            } else {
                debug!("Received ToolResult for unknown or expired call [{}]", call_id);
            }
        }
    }

    /// Async wrapper for process_tool_result
    pub async fn handle_tool_result(&self, msg: AgentToServerMessage) {
        self.process_tool_result(msg);
    }

    /// Dispatches any MCP tool call (server meta-tools or forwarded diagnostic tools)
    pub async fn dispatch_tool_call(&self, tool_name: &str, arguments: Value) -> Result<Value, String> {
        match tool_name {
            "list_terminals" => {
                let terminals = self.list_terminals().await;
                serde_json::to_value(terminals).map_err(|e| e.to_string())
            }
            "select_terminal" => {
                let tid = arguments
                    .get("terminal_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required argument 'terminal_id'".to_string())?;

                let entry = self.select_terminal(tid).await?;
                serde_json::to_value(entry).map_err(|e| e.to_string())
            }
            "get_active_terminal" => {
                let active = self.get_active_terminal().await;
                match active {
                    Some(entry) => serde_json::to_value(entry).map_err(|e| e.to_string()),
                    None => Ok(serde_json::json!({
                        "active_terminal": null,
                        "message": "No active terminal selected"
                    })),
                }
            }
            // Forwarded diagnostic tools
            "get_system_overview"
            | "exec_powershell"
            | "exec_cmd"
            | "list_processes"
            | "kill_process"
            | "manage_service"
            | "read_text_file"
            | "write_text_file"
            | "capture_screen" => {
                let explicit_tid = arguments
                    .get("terminal_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let target_terminal_id = self.resolve_target_terminal(explicit_tid.as_deref()).await?;

                let timeout_secs = arguments
                    .get("timeout_secs")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(35);

                self.invoke_tool(&target_terminal_id, tool_name, arguments, timeout_secs).await
            }
            unknown => Err(format!("Unknown or unsupported tool: '{}'", unknown)),
        }
    }
}

impl AgentMessageHandler for McpRouter {
    fn handle_tool_result(&self, msg: AgentToServerMessage) {
        self.process_tool_result(msg);
    }
}
