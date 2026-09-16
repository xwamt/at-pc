use base64::Engine;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{oneshot, RwLock};
use tracing::{debug, info};

use crate::ws::handler::AgentMessageHandler;
use crate::ws::registry::{TerminalEntry, TerminalRegistry};
use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::TerminalStatus;
use at_pc_protocol::tools::agent_tool;

static CALL_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Generates a globally unique call ID for requests and audits
pub fn generate_call_id() -> String {
    let call_seq = CALL_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "call-{}-{}",
        chrono::Utc::now().timestamp_millis(),
        call_seq
    )
}

/// Cached desktop video frame for remote desktop viewing
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DesktopFrameCache {
    pub display_index: u32,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub data: String,
    #[serde(skip)]
    pub raw_bytes: Vec<u8>,
    pub timestamp: u64,
}

/// Detailed information regarding an active in-flight tool call
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PendingCallDetail {
    pub call_id: String,
    pub terminal_id: String,
    pub tool_name: String,
    pub elapsed_ms: u64,
    pub timeout_secs: u64,
}

struct PendingCallEntry {
    terminal_id: String,
    tool_name: String,
    start_time: std::time::Instant,
    timeout_secs: u64,
    tx: oneshot::Sender<Result<Value, String>>,
}

/// MCP Tool Router that handles terminal selection, tool dispatch, and response correlation
pub struct McpRouter {
    registry: Arc<TerminalRegistry>,
    pending_calls: Arc<Mutex<HashMap<String, PendingCallEntry>>>,
    active_terminal_id: Arc<RwLock<Option<String>>>,
    session_active_terminals: Arc<RwLock<HashMap<String, String>>>,
    desktop_frames: Arc<std::sync::RwLock<HashMap<String, DesktopFrameCache>>>,
    audit_logger: Option<Arc<crate::audit::AuditLogger>>,
}

impl McpRouter {
    /// Create a new MCP router instance
    pub fn new(registry: Arc<TerminalRegistry>) -> Self {
        Self {
            registry,
            pending_calls: Arc::new(Mutex::new(HashMap::new())),
            active_terminal_id: Arc::new(RwLock::new(None)),
            session_active_terminals: Arc::new(RwLock::new(HashMap::new())),
            desktop_frames: Arc::new(std::sync::RwLock::new(HashMap::new())),
            audit_logger: None,
        }
    }

    /// Attaches an AuditLogger to record all tool invocations and authorization decisions
    pub fn with_audit_logger(mut self, audit_logger: Arc<crate::audit::AuditLogger>) -> Self {
        self.audit_logger = Some(audit_logger);
        self
    }

    /// Returns reference to optional audit logger
    pub fn audit_logger(&self) -> Option<&Arc<crate::audit::AuditLogger>> {
        self.audit_logger.as_ref()
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

    /// Look up a terminal by terminal_id, custom_name, or hostname
    pub async fn find_terminal(&self, query: &str) -> Option<TerminalEntry> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return None;
        }

        // 1. Direct terminal_id match
        if let Some(entry) = self.registry.get_terminal(trimmed).await {
            return Some(entry);
        }

        // 2. Search by custom_name (exact then case-insensitive)
        let all = self.registry.list_terminals().await;
        for t in &all {
            if let Some(ref cn) = t.custom_name {
                if cn.eq_ignore_ascii_case(trimmed) {
                    return Some(t.clone());
                }
            }
        }

        // 3. Search by hostname (exact then case-insensitive)
        for t in &all {
            if t.info.hostname.eq_ignore_ascii_case(trimmed) {
                return Some(t.clone());
            }
        }

        None
    }

    /// Set the active target terminal ID for this session (supports terminal_id or custom_name)
    pub async fn select_terminal(
        &self,
        terminal_id_or_name: &str,
    ) -> Result<TerminalEntry, String> {
        let entry = self
            .find_terminal(terminal_id_or_name)
            .await
            .ok_or_else(|| {
                format!(
                    "Terminal '{}' not found in registry (matched by ID, custom name, or hostname)",
                    terminal_id_or_name
                )
            })?;

        let actual_tid = entry.info.terminal_id.clone();
        let mut active = self.active_terminal_id.write().await;
        *active = Some(actual_tid.clone());
        info!(
            "Active terminal set to: {} (input: {})",
            actual_tid, terminal_id_or_name
        );
        Ok(entry)
    }

    /// Set the active target terminal ID for a specific session ID (supports terminal_id or custom_name)
    pub async fn select_terminal_for_session(
        &self,
        session_id: &str,
        terminal_id_or_name: &str,
    ) -> Result<TerminalEntry, String> {
        let entry = self
            .find_terminal(terminal_id_or_name)
            .await
            .ok_or_else(|| {
                format!(
                    "Terminal '{}' not found in registry (matched by ID, custom name, or hostname)",
                    terminal_id_or_name
                )
            })?;

        let actual_tid = entry.info.terminal_id.clone();
        let mut sessions = self.session_active_terminals.write().await;
        sessions.insert(session_id.to_string(), actual_tid.clone());
        info!(
            "Active terminal for session '{}' set to: {} (input: {})",
            session_id, actual_tid, terminal_id_or_name
        );
        Ok(entry)
    }

    /// Get the currently active terminal ID (global)
    pub async fn get_active_terminal_id(&self) -> Option<String> {
        let active = self.active_terminal_id.read().await;
        active.clone()
    }

    /// Get the active terminal ID for a specific session, falling back to global active terminal
    pub async fn get_active_terminal_id_for_session(
        &self,
        session_id: Option<&str>,
    ) -> Option<String> {
        if let Some(sid) = session_id {
            let sessions = self.session_active_terminals.read().await;
            if let Some(tid) = sessions.get(sid) {
                return Some(tid.clone());
            }
        }
        self.get_active_terminal_id().await
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
    pub async fn resolve_target_terminal(
        &self,
        explicit_id: Option<&str>,
    ) -> Result<String, String> {
        self.resolve_target_terminal_with_session(explicit_id, None)
            .await
    }

    /// Resolves target terminal for a tool call taking session ID into account:
    pub async fn resolve_target_terminal_with_session(
        &self,
        explicit_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<String, String> {
        if let Some(id) = explicit_id {
            let trimmed = id.trim();
            if !trimmed.is_empty() {
                if let Some(entry) = self.find_terminal(trimmed).await {
                    return Ok(entry.info.terminal_id);
                }
                return Ok(trimmed.to_string());
            }
        }

        if let Some(active_id) = self.get_active_terminal_id_for_session(session_id).await {
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

        let call_id = generate_call_id();

        let effective_timeout = if timeout_secs == 0 { 35 } else { timeout_secs };

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending_calls.lock().unwrap();
            pending.insert(
                call_id.clone(),
                PendingCallEntry {
                    terminal_id: terminal_id.to_string(),
                    tool_name: tool_name.to_string(),
                    start_time: std::time::Instant::now(),
                    timeout_secs: effective_timeout,
                    tx,
                },
            );
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
            return Err(format!(
                "Failed to send tool invocation to terminal '{}': {}",
                terminal_id, e
            ));
        }

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
                let _ = self
                    .registry
                    .send_to_terminal(
                        terminal_id,
                        ServerToAgentMessage::CancelTool {
                            call_id: call_id.clone(),
                        },
                    )
                    .await;

                Err(format!(
                    "Tool '{}' execution timed out after {}s on terminal '{}'",
                    tool_name, effective_timeout, terminal_id
                ))
            }
        }
    }

    /// Cancels an in-flight tool invocation by call_id, notifying both agent and local waiter
    pub async fn cancel_tool_call(&self, call_id: &str) -> Result<(), String> {
        let entry = {
            let mut pending = self.pending_calls.lock().unwrap();
            pending.remove(call_id).ok_or_else(|| {
                format!(
                    "Pending tool call '{}' not found or already completed",
                    call_id
                )
            })?
        };

        let msg = ServerToAgentMessage::CancelTool {
            call_id: call_id.to_string(),
        };

        if let Err(e) = self
            .registry
            .send_to_terminal(&entry.terminal_id, msg)
            .await
        {
            tracing::warn!(
                "Failed to send CancelTool message to terminal '{}': {}",
                entry.terminal_id,
                e
            );
        }

        let _ = entry.tx.send(Err(format!(
            "Tool execution was cancelled for call '{}'",
            call_id
        )));
        info!(
            "Cancelled in-flight tool call [{}] on terminal [{}]",
            call_id, entry.terminal_id
        );
        Ok(())
    }

    /// Immediately abort and fail all in-flight pending calls for a disconnected terminal
    pub fn abort_pending_calls_for_terminal(&self, terminal_id: &str, reason: &str) -> usize {
        let mut pending = self.pending_calls.lock().unwrap();
        let target_keys: Vec<String> = pending
            .iter()
            .filter(|(_, entry)| entry.terminal_id == terminal_id)
            .map(|(cid, _)| cid.clone())
            .collect();

        let count = target_keys.len();
        for cid in &target_keys {
            if let Some(entry) = pending.remove(cid) {
                let _ = entry.tx.send(Err(format!(
                    "Terminal '{}' disconnected: {}",
                    terminal_id, reason
                )));
            }
        }

        if count > 0 {
            info!(
                "Aborted {} in-flight tool call(s) for terminal '{}' (reason: {})",
                count, terminal_id, reason
            );
        }
        count
    }

    /// Lists currently in-flight pending tool call IDs and their associated terminal IDs
    pub fn list_pending_calls(&self) -> Vec<(String, String)> {
        let pending = self.pending_calls.lock().unwrap();
        pending
            .iter()
            .map(|(cid, entry)| (cid.clone(), entry.terminal_id.clone()))
            .collect()
    }

    /// Lists rich details of active in-flight pending calls, optionally filtered by terminal ID
    pub fn list_pending_call_details(
        &self,
        filter_terminal_id: Option<&str>,
    ) -> Vec<PendingCallDetail> {
        let pending = self.pending_calls.lock().unwrap();
        let mut list: Vec<PendingCallDetail> = pending
            .iter()
            .filter(|(_, entry)| filter_terminal_id.is_none_or(|tid| entry.terminal_id == tid))
            .map(|(cid, entry)| PendingCallDetail {
                call_id: cid.clone(),
                terminal_id: entry.terminal_id.clone(),
                tool_name: entry.tool_name.clone(),
                elapsed_ms: entry.start_time.elapsed().as_millis() as u64,
                timeout_secs: entry.timeout_secs,
            })
            .collect();
        list.sort_by(|a, b| a.call_id.cmp(&b.call_id));
        list
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
                pending.remove(&call_id).map(|entry| entry.tx)
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
                debug!(
                    "Received ToolResult for unknown or expired call [{}]",
                    call_id
                );
            }
        }
    }

    /// Async wrapper for process_tool_result
    pub async fn handle_tool_result(&self, msg: AgentToServerMessage) {
        self.process_tool_result(msg);
    }

    /// Dispatches any MCP tool call (server meta-tools or forwarded diagnostic tools)
    pub async fn dispatch_tool_call(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<Value, String> {
        self.dispatch_tool_call_with_role(tool_name, arguments, None, None, None, None)
            .await
    }

    /// Dispatches any MCP tool call with optional session isolation context
    pub async fn dispatch_tool_call_with_session(
        &self,
        tool_name: &str,
        arguments: Value,
        session_id: Option<&str>,
    ) -> Result<Value, String> {
        self.dispatch_tool_call_with_role(tool_name, arguments, session_id, None, None, None)
            .await
    }

    /// Dispatches any MCP tool call with RBAC permission enforcement and persistent audit logging
    pub async fn dispatch_tool_call_with_role(
        &self,
        tool_name: &str,
        arguments: Value,
        session_id: Option<&str>,
        role: Option<crate::config::Role>,
        client_ip: Option<&str>,
        token_prefix: Option<&str>,
    ) -> Result<Value, String> {
        let explicit_tid = arguments
            .get("terminal_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let target_tid = if explicit_tid.is_some() {
            explicit_tid.clone()
        } else {
            self.resolve_target_terminal_with_session(None, session_id)
                .await
                .ok()
        };

        // 1. Enforce RBAC permission if role is specified
        if let Some(r) = role {
            if !crate::config::is_tool_allowed_for_role(r, tool_name) {
                let err_msg = format!(
                    "Forbidden: Role '{}' is not authorized to execute tool '{}'",
                    r, tool_name
                );
                if let Some(ref logger) = self.audit_logger {
                    logger
                        .log_async(crate::audit::AuditRecord {
                            id: format!("audit-{}", generate_call_id()),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                            role: Some(r.to_string()),
                            token_prefix: token_prefix.map(|s| s.to_string()),
                            client_ip: client_ip.map(|s| s.to_string()),
                            terminal_id: target_tid.clone(),
                            action: format!("tool:{}", tool_name),
                            tool_name: Some(tool_name.to_string()),
                            arguments: Some(arguments.clone()),
                            status: "DENIED".to_string(),
                            error: Some(err_msg.clone()),
                            duration_ms: Some(0),
                        })
                        .await;
                }
                return Err(err_msg);
            }
        }

        let start_time = std::time::Instant::now();
        let result = self
            .execute_dispatch_inner(tool_name, arguments.clone(), session_id)
            .await;
        let elapsed_ms = start_time.elapsed().as_millis() as u64;

        // 2. Persistent audit trail
        if let Some(ref logger) = self.audit_logger {
            match &result {
                Ok(_) => {
                    logger
                        .log_async(crate::audit::AuditRecord {
                            id: format!("audit-{}", generate_call_id()),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                            role: role.map(|r| r.to_string()),
                            token_prefix: token_prefix.map(|s| s.to_string()),
                            client_ip: client_ip.map(|s| s.to_string()),
                            terminal_id: target_tid.clone(),
                            action: format!("tool:{}", tool_name),
                            tool_name: Some(tool_name.to_string()),
                            arguments: Some(arguments),
                            status: "SUCCESS".to_string(),
                            error: None,
                            duration_ms: Some(elapsed_ms),
                        })
                        .await;
                }
                Err(e) => {
                    logger
                        .log_async(crate::audit::AuditRecord {
                            id: format!("audit-{}", generate_call_id()),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                            role: role.map(|r| r.to_string()),
                            token_prefix: token_prefix.map(|s| s.to_string()),
                            client_ip: client_ip.map(|s| s.to_string()),
                            terminal_id: target_tid,
                            action: format!("tool:{}", tool_name),
                            tool_name: Some(tool_name.to_string()),
                            arguments: Some(arguments),
                            status: "FAILED".to_string(),
                            error: Some(e.clone()),
                            duration_ms: Some(elapsed_ms),
                        })
                        .await;
                }
            }
        }

        result
    }

    /// Returns whether a name is a protocol-registered Agent tool that this Router forwards.
    pub fn is_forwarded_tool(tool_name: &str) -> bool {
        agent_tool(tool_name).is_some()
    }

    async fn execute_dispatch_inner(
        &self,
        tool_name: &str,
        arguments: Value,
        session_id: Option<&str>,
    ) -> Result<Value, String> {
        if let Some(spec) = agent_tool(tool_name) {
            let explicit_tid = arguments
                .get("terminal_id")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let sid_opt = arguments
                .get("session_id")
                .and_then(|v| v.as_str())
                .or(session_id);
            let target_terminal_id = self
                .resolve_target_terminal_with_session(explicit_tid.as_deref(), sid_opt)
                .await?;
            let timeout_secs = arguments
                .get("timeout_secs")
                .and_then(|v| v.as_u64())
                .unwrap_or(35);
            return self
                .invoke_tool(&target_terminal_id, spec.name, arguments, timeout_secs)
                .await;
        }

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

                let sid_opt = arguments
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .or(session_id);

                if let Some(sid) = sid_opt {
                    let entry = self.select_terminal_for_session(sid, tid).await?;
                    serde_json::to_value(entry).map_err(|e| e.to_string())
                } else {
                    let entry = self.select_terminal(tid).await?;
                    serde_json::to_value(entry).map_err(|e| e.to_string())
                }
            }
            "rename_terminal" => {
                let tid_raw = arguments
                    .get("terminal_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required argument 'terminal_id'".to_string())?;

                let target_entry = self.find_terminal(tid_raw).await;
                let actual_tid = match target_entry {
                    Some(ref e) => e.info.terminal_id.as_str(),
                    None => tid_raw,
                };

                let custom_name = arguments
                    .get("custom_name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let notes = arguments
                    .get("notes")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let tags = arguments.get("tags").and_then(|v| v.as_array()).map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect::<Vec<String>>()
                });

                let meta = self
                    .registry
                    .update_terminal_meta(actual_tid, custom_name, notes, tags)
                    .await?;

                serde_json::to_value(meta).map_err(|e| e.to_string())
            }
            "get_active_terminal" => {
                let sid_opt = arguments
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .or(session_id);

                let active_tid = self.get_active_terminal_id_for_session(sid_opt).await;
                match active_tid {
                    Some(ref tid) => {
                        let entry = self.registry.get_terminal(tid).await;
                        match entry {
                            Some(e) => serde_json::to_value(e).map_err(|err| err.to_string()),
                            None => Ok(serde_json::json!({
                                "active_terminal": tid,
                                "status": "Not found in registry"
                            })),
                        }
                    }
                    None => Ok(serde_json::json!({
                        "active_terminal": null,
                        "message": "No active terminal selected"
                    })),
                }
            }
            "list_pending_calls" => {
                let tid = arguments.get("terminal_id").and_then(|v| v.as_str());
                let calls = self.list_pending_call_details(tid);
                serde_json::to_value(calls).map_err(|e| e.to_string())
            }
            // `cancel_task` is a compatibility alias. It is intentionally not listed and never forwarded.
            "cancel_tool" | "cancel_task" => {
                let cid = arguments
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required argument 'call_id'".to_string())?;

                self.cancel_tool_call(cid).await?;
                Ok(serde_json::json!({
                    "success": true,
                    "message": format!("Tool call '{}' cancelled successfully", cid)
                }))
            }
            unknown => Err(format!("Unknown or unsupported tool: '{}'", unknown)),
        }
    }

    /// Retrieve the most recently cached desktop frame for a terminal
    pub async fn get_latest_desktop_frame(&self, terminal_id: &str) -> Option<DesktopFrameCache> {
        let frames = self.desktop_frames.read().unwrap();
        frames.get(terminal_id).cloned()
    }

    /// Retrieve the raw JPEG bytes of the latest desktop frame for a terminal
    pub async fn get_latest_desktop_frame_raw(
        &self,
        terminal_id: &str,
    ) -> Option<(u32, u32, u32, u64, Vec<u8>)> {
        let frames = self.desktop_frames.read().unwrap();
        let frame = frames.get(terminal_id)?;
        Some((
            frame.display_index,
            frame.width,
            frame.height,
            frame.timestamp,
            frame.raw_bytes.clone(),
        ))
    }

    /// Instruct an agent terminal to start streaming its desktop frames
    pub async fn start_desktop_stream(
        &self,
        terminal_id: &str,
        fps: u32,
        quality: u8,
        display_index: u32,
        scale: f32,
    ) -> Result<(), String> {
        self.desktop_frames.write().unwrap().remove(terminal_id);
        let msg = ServerToAgentMessage::StartDesktopStream {
            display_index,
            fps: if fps == 0 { 15 } else { fps },
            quality: if quality == 0 { 60 } else { quality },
            scale,
        };
        self.registry.send_to_terminal(terminal_id, msg).await
    }

    /// Instruct an agent terminal to stop streaming its desktop frames
    pub async fn stop_desktop_stream(&self, terminal_id: &str) -> Result<(), String> {
        self.desktop_frames.write().unwrap().remove(terminal_id);
        let msg = ServerToAgentMessage::StopDesktopStream;
        self.registry.send_to_terminal(terminal_id, msg).await
    }

    /// Send a simulated remote mouse/keyboard input event to an agent terminal
    pub async fn send_desktop_input(
        &self,
        terminal_id: &str,
        event: at_pc_protocol::models::DesktopInputEvent,
    ) -> Result<(), String> {
        let msg = ServerToAgentMessage::DesktopInput { event };
        if let Err(e) = self.registry.send_to_terminal(terminal_id, msg).await {
            tracing::warn!("send_desktop_input failed for [{}]: {}", terminal_id, e);
            return Err(e);
        }
        Ok(())
    }
}

impl AgentMessageHandler for McpRouter {
    fn handle_tool_result(&self, msg: AgentToServerMessage) {
        self.process_tool_result(msg);
    }

    fn handle_terminal_disconnected(&self, terminal_id: &str, reason: &str) {
        self.abort_pending_calls_for_terminal(terminal_id, reason);
    }

    fn handle_desktop_frame_binary(
        &self,
        terminal_id: &str,
        frame: at_pc_protocol::messages::BinaryDesktopFrame,
    ) {
        let mut frames = self.desktop_frames.write().unwrap();
        if let Some(existing) = frames.get(terminal_id) {
            if frame.timestamp < existing.timestamp {
                return;
            }
        }
        if frame.data.is_empty() {
            if let Some(existing) = frames.get_mut(terminal_id) {
                existing.timestamp = frame.timestamp;
            } else {
                tracing::warn!(
                    "dropping empty desktop keepalive for [{}]: no cached frame yet",
                    terminal_id
                );
            }
            return;
        }
        let data = base64::prelude::BASE64_STANDARD.encode(&frame.data);
        frames.insert(
            terminal_id.to_string(),
            DesktopFrameCache {
                display_index: frame.display_index,
                width: frame.width,
                height: frame.height,
                format: "jpeg".to_string(),
                data,
                raw_bytes: frame.data,
                timestamp: frame.timestamp,
            },
        );
    }
}
