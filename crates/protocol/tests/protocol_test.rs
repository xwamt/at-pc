use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::{HeartbeatMetrics, TerminalInfo, ToolCallPayload, ToolResultPayload};

#[test]
fn test_register_message_serialization() {
    let info = TerminalInfo {
        terminal_id: "test-pc-01".to_string(),
        hostname: "DESKTOP-TEST".to_string(),
        username: "user".to_string(),
        lan_ip: "192.168.1.100".to_string(),
        os_version: "Windows 11".to_string(),
        agent_version: "0.3.0".to_string(),
    };
    let msg = AgentToServerMessage::Register {
        info,
        auth_token: Some("secret123".to_string()),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("test-pc-01"));
    let deserialized: AgentToServerMessage = serde_json::from_str(&json).unwrap();
    match deserialized {
        AgentToServerMessage::Register { info, auth_token } => {
            assert_eq!(info.terminal_id, "test-pc-01");
            assert_eq!(auth_token, Some("secret123".to_string()));
        }
        _ => panic!("Expected Register variant"),
    }
}

#[test]
fn test_heartbeat_messages_serialization() {
    let metrics = HeartbeatMetrics {
        cpu_usage_percent: 25.5,
        memory_used_mb: 4096,
        memory_total_mb: 16384,
        uptime_secs: 7200,
        timestamp: 1725180000,
    };
    let hb = AgentToServerMessage::Heartbeat {
        terminal_id: "node-1".to_string(),
        metrics: metrics.clone(),
    };
    let json = serde_json::to_string(&hb).unwrap();
    assert!(json.contains("node-1"));
    let deserialized: AgentToServerMessage = serde_json::from_str(&json).unwrap();
    match deserialized {
        AgentToServerMessage::Heartbeat { terminal_id, metrics: m } => {
            assert_eq!(terminal_id, "node-1");
            assert_eq!(m.cpu_usage_percent, 25.5);
            assert_eq!(m.memory_used_mb, 4096);
            assert_eq!(m.memory_total_mb, 16384);
            assert_eq!(m.uptime_secs, 7200);
            assert_eq!(m.timestamp, 1725180000);
        }
        _ => panic!("Expected Heartbeat variant"),
    }

    let ack = ServerToAgentMessage::HeartbeatAck {
        server_timestamp: 1725180001,
    };
    let ack_json = serde_json::to_string(&ack).unwrap();
    let deserialized_ack: ServerToAgentMessage = serde_json::from_str(&ack_json).unwrap();
    match deserialized_ack {
        ServerToAgentMessage::HeartbeatAck { server_timestamp } => {
            assert_eq!(server_timestamp, 1725180001);
        }
        _ => panic!("Expected HeartbeatAck variant"),
    }
}

#[test]
fn test_invoke_tool_and_tool_result_serialization() {
    let invoke = ServerToAgentMessage::InvokeTool {
        call_id: "call-123".to_string(),
        tool_name: "exec_powershell".to_string(),
        arguments: serde_json::json!({ "script": "Get-Process" }),
        timeout_secs: 30,
    };
    let invoke_json = serde_json::to_string(&invoke).unwrap();
    let deserialized_invoke: ServerToAgentMessage = serde_json::from_str(&invoke_json).unwrap();
    match deserialized_invoke {
        ServerToAgentMessage::InvokeTool {
            call_id,
            tool_name,
            arguments,
            timeout_secs,
        } => {
            assert_eq!(call_id, "call-123");
            assert_eq!(tool_name, "exec_powershell");
            assert_eq!(arguments["script"], "Get-Process");
            assert_eq!(timeout_secs, 30);
        }
        _ => panic!("Expected InvokeTool variant"),
    }

    let result = AgentToServerMessage::ToolResult {
        call_id: "call-123".to_string(),
        success: true,
        result: serde_json::json!({ "stdout": "running", "exit_code": 0 }),
        error: None,
        duration_ms: 120,
    };
    let result_json = serde_json::to_string(&result).unwrap();
    let deserialized_result: AgentToServerMessage = serde_json::from_str(&result_json).unwrap();
    match deserialized_result {
        AgentToServerMessage::ToolResult {
            call_id,
            success,
            result,
            error,
            duration_ms,
        } => {
            assert_eq!(call_id, "call-123");
            assert!(success);
            assert_eq!(result["stdout"], "running");
            assert_eq!(error, None);
            assert_eq!(duration_ms, 120);
        }
        _ => panic!("Expected ToolResult variant"),
    }
}

#[test]
fn test_disconnect_and_cancel_serialization() {
    let disconnect = AgentToServerMessage::Disconnect {
        terminal_id: "test-pc-01".to_string(),
        reason: "User requested disconnect".to_string(),
    };
    let disc_json = serde_json::to_string(&disconnect).unwrap();
    let deserialized_disc: AgentToServerMessage = serde_json::from_str(&disc_json).unwrap();
    match deserialized_disc {
        AgentToServerMessage::Disconnect { terminal_id, reason } => {
            assert_eq!(terminal_id, "test-pc-01");
            assert_eq!(reason, "User requested disconnect");
        }
        _ => panic!("Expected Disconnect variant"),
    }

    let cancel = ServerToAgentMessage::CancelTool {
        call_id: "call-999".to_string(),
    };
    let cancel_json = serde_json::to_string(&cancel).unwrap();
    let deserialized_cancel: ServerToAgentMessage = serde_json::from_str(&cancel_json).unwrap();
    match deserialized_cancel {
        ServerToAgentMessage::CancelTool { call_id } => {
            assert_eq!(call_id, "call-999");
        }
        _ => panic!("Expected CancelTool variant"),
    }
}

#[test]
fn test_register_ack_serialization() {
    let ack = ServerToAgentMessage::RegisterAck {
        success: true,
        message: Some("Welcome".to_string()),
        heartbeat_interval_secs: 5,
    };
    let ack_json = serde_json::to_string(&ack).unwrap();
    let deserialized: ServerToAgentMessage = serde_json::from_str(&ack_json).unwrap();
    match deserialized {
        ServerToAgentMessage::RegisterAck {
            success,
            message,
            heartbeat_interval_secs,
        } => {
            assert!(success);
            assert_eq!(message, Some("Welcome".to_string()));
            assert_eq!(heartbeat_interval_secs, 5);
        }
        _ => panic!("Expected RegisterAck variant"),
    }
}

#[test]
fn test_tool_payloads_serialization() {
    let call_payload = ToolCallPayload {
        call_id: "call-1".to_string(),
        tool_name: "test_tool".to_string(),
        arguments: serde_json::json!({ "foo": "bar" }),
        timeout_secs: 10,
    };
    let call_json = serde_json::to_string(&call_payload).unwrap();
    let deser_call: ToolCallPayload = serde_json::from_str(&call_json).unwrap();
    assert_eq!(deser_call.call_id, "call-1");
    assert_eq!(deser_call.tool_name, "test_tool");
    assert_eq!(deser_call.arguments["foo"], "bar");
    assert_eq!(deser_call.timeout_secs, 10);

    let res_payload = ToolResultPayload {
        call_id: "call-1".to_string(),
        success: true,
        result: serde_json::json!({ "status": "ok" }),
        error: None,
        duration_ms: 45,
    };
    let res_json = serde_json::to_string(&res_payload).unwrap();
    let deser_res: ToolResultPayload = serde_json::from_str(&res_json).unwrap();
    assert_eq!(deser_res.call_id, "call-1");
    assert!(deser_res.success);
    assert_eq!(deser_res.result["status"], "ok");
    assert_eq!(deser_res.duration_ms, 45);
}
