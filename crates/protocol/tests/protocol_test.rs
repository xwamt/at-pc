use at_pc_protocol::messages::{AgentToServerMessage, ServerToAgentMessage};
use at_pc_protocol::models::{
    HeartbeatMetrics, MarkedScreenResponse, ScreenMark, TerminalInfo, ToolCallPayload,
    ToolResultPayload,
};

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
        AgentToServerMessage::Heartbeat {
            terminal_id,
            metrics: m,
        } => {
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
        AgentToServerMessage::Disconnect {
            terminal_id,
            reason,
        } => {
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

#[test]
fn test_desktop_streaming_and_input_serialization() {
    use at_pc_protocol::models::DesktopInputEvent;

    // 1. StartDesktopStream
    let start_msg = ServerToAgentMessage::StartDesktopStream {
        display_index: 0,
        fps: 15,
        quality: 60,
        scale: 1.0,
    };
    let start_json = serde_json::to_string(&start_msg).unwrap();
    let deser_start: ServerToAgentMessage = serde_json::from_str(&start_json).unwrap();
    match deser_start {
        ServerToAgentMessage::StartDesktopStream { fps, quality, .. } => {
            assert_eq!(fps, 15);
            assert_eq!(quality, 60);
        }
        _ => panic!("Expected StartDesktopStream"),
    }

    // 3. DesktopInput MouseMove & Click
    let input_msg = ServerToAgentMessage::DesktopInput {
        event: DesktopInputEvent::MouseMove { x: 500, y: 300 },
    };
    let input_json = serde_json::to_string(&input_msg).unwrap();
    let deser_input: ServerToAgentMessage = serde_json::from_str(&input_json).unwrap();
    match deser_input {
        ServerToAgentMessage::DesktopInput { event } => {
            assert_eq!(event, DesktopInputEvent::MouseMove { x: 500, y: 300 });
        }
        _ => panic!("Expected DesktopInput"),
    }
}

#[test]
fn test_binary_desktop_frame_codec() {
    use at_pc_protocol::messages::BinaryDesktopFrame;

    let raw_bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46];
    let frame = BinaryDesktopFrame::new(1, 1920, 1080, 1725180000123, raw_bytes.clone());

    let encoded = frame.encode();
    assert_eq!(encoded.len(), 24 + raw_bytes.len());
    assert_eq!(&encoded[0..4], b"DFRM");

    let decoded = BinaryDesktopFrame::decode(&encoded).expect("Decode should succeed");
    assert_eq!(decoded.display_index, 1);
    assert_eq!(decoded.width, 1920);
    assert_eq!(decoded.height, 1080);
    assert_eq!(decoded.timestamp, 1725180000123);
    assert_eq!(decoded.data, raw_bytes);

    // Test error cases
    assert!(BinaryDesktopFrame::decode(&[0u8; 10]).is_err());
    let mut bad_magic = encoded.clone();
    bad_magic[0] = b'X';
    assert!(BinaryDesktopFrame::decode(&bad_magic).is_err());
}

#[test]
fn test_ui_element_and_tree_response_serialization() {
    use at_pc_protocol::models::{UiElement, UiTreeResponse};

    let el1 = UiElement {
        id: 1,
        control_type: "Button".to_string(),
        name: "添加设备".to_string(),
        value: None,
        rect: [250, 210, 120, 36],
        enabled: true,
        help_text: None,
    };
    let el2 = UiElement {
        id: 2,
        control_type: "Edit".to_string(),
        name: "搜索设置".to_string(),
        value: Some("test_query".to_string()),
        rect: [500, 180, 200, 32],
        enabled: true,
        help_text: Some("Type to search".to_string()),
    };

    let response = UiTreeResponse {
        active_window: "设置".to_string(),
        window_bounds: [200, 150, 960, 640],
        elements: vec![el1, el2],
        total_elements: 2,
        query: None,
        query_matched: None,
        compact: None,
        display_index: None,
    };

    let json_str = serde_json::to_string(&response).unwrap();
    assert!(json_str.contains("\"type\":\"Button\""));
    assert!(json_str.contains("\"name\":\"添加设备\""));
    assert!(json_str.contains("\"active_window\":\"设置\""));
    assert!(!json_str.contains("\"display_index\""));

    let deserialized: UiTreeResponse = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.active_window, "设置");
    assert_eq!(deserialized.total_elements, 2);
    assert_eq!(deserialized.elements[0].id, 1);
    assert_eq!(deserialized.elements[0].control_type, "Button");
    assert_eq!(
        deserialized.elements[1].value,
        Some("test_query".to_string())
    );
    assert_eq!(deserialized.display_index, None);

    // Test serialization with display_index Some(1)
    let response_with_display = UiTreeResponse {
        active_window: "Secondary Window".to_string(),
        window_bounds: [1920, 0, 1920, 1080],
        elements: vec![],
        total_elements: 0,
        query: None,
        query_matched: None,
        compact: None,
        display_index: Some(1),
    };
    let json_with_display = serde_json::to_string(&response_with_display).unwrap();
    assert!(json_with_display.contains("\"display_index\":1"));
    let deser_with_display: UiTreeResponse = serde_json::from_str(&json_with_display).unwrap();
    assert_eq!(deser_with_display.display_index, Some(1));
}

#[test]
fn test_window_info_and_state_diff_serialization() {
    use at_pc_protocol::models::{StateDiff, WindowInfo};

    let win = WindowInfo {
        hwnd: 0x1A2B3C,
        pid: 12345,
        title: "Calculator".to_string(),
        process_name: "CalculatorApp.exe".to_string(),
        is_minimized: false,
        is_foreground: true,
        rect: [100, 150, 800, 600],
        display_index: None,
    };

    let json_win = serde_json::to_string(&win).unwrap();
    assert!(json_win.contains("\"hwnd\":1715004"));
    assert!(json_win.contains("\"pid\":12345"));
    assert!(json_win.contains("\"title\":\"Calculator\""));
    assert!(json_win.contains("\"process_name\":\"CalculatorApp.exe\""));
    assert!(json_win.contains("\"is_minimized\":false"));
    assert!(json_win.contains("\"is_foreground\":true"));
    assert!(!json_win.contains("\"display_index\""));

    let deser_win: WindowInfo = serde_json::from_str(&json_win).unwrap();
    assert_eq!(deser_win, win);
    assert_eq!(deser_win.display_index, None);

    // Backwards compatibility: raw JSON without display_index field
    let raw_old_json = r#"{"hwnd":123,"pid":456,"title":"Old App","process_name":"old.exe","is_minimized":false,"is_foreground":true,"rect":[0,0,800,600]}"#;
    let old_deser: WindowInfo = serde_json::from_str(raw_old_json).unwrap();
    assert_eq!(old_deser.display_index, None);

    // Test serialization with display_index Some(2)
    let win_display = WindowInfo {
        display_index: Some(2),
        ..win.clone()
    };
    let json_win_display = serde_json::to_string(&win_display).unwrap();
    assert!(json_win_display.contains("\"display_index\":2"));
    let deser_win_display: WindowInfo = serde_json::from_str(&json_win_display).unwrap();
    assert_eq!(deser_win_display.display_index, Some(2));

    let diff = StateDiff {
        foreground_changed: true,
        previous_window: Some("Calculator".to_string()),
        current_window: Some("Save As".to_string()),
        modal_dialog_detected: true,
        dialog_title: Some("Save As".to_string()),
        ui_diff: None,
    };

    let json_diff = serde_json::to_string(&diff).unwrap();
    assert!(json_diff.contains("\"foreground_changed\":true"));
    assert!(json_diff.contains("\"previous_window\":\"Calculator\""));
    assert!(json_diff.contains("\"current_window\":\"Save As\""));
    assert!(json_diff.contains("\"modal_dialog_detected\":true"));
    assert!(json_diff.contains("\"dialog_title\":\"Save As\""));

    let deser_diff: StateDiff = serde_json::from_str(&json_diff).unwrap();
    assert_eq!(deser_diff, diff);

    // Test default StateDiff with None options skipped in serialization
    let default_diff = StateDiff {
        foreground_changed: false,
        previous_window: None,
        current_window: None,
        modal_dialog_detected: false,
        dialog_title: None,
        ui_diff: None,
    };
    let json_default = serde_json::to_string(&default_diff).unwrap();
    assert!(!json_default.contains("previous_window"));
    assert!(!json_default.contains("dialog_title"));
    let deser_default: StateDiff = serde_json::from_str(&json_default).unwrap();
    assert_eq!(deser_default, default_diff);
}

#[test]
fn test_som_screen_mark_and_response_serialization() {
    let mark1 = ScreenMark {
        id: 1,
        rect: [100, 150, 80, 32],
        center: [140, 166],
        label: Some("Submit Button".to_string()),
        control_type: Some("Button".to_string()),
    };
    let mark2 = ScreenMark {
        id: 2,
        rect: [200, 150, 240, 32],
        center: [320, 166],
        label: None,
        control_type: Some("Edit".to_string()),
    };

    let mark_json = serde_json::to_string(&mark1).unwrap();
    assert!(mark_json.contains("\"id\":1"));
    assert!(mark_json.contains("\"rect\":[100,150,80,32]"));
    assert!(mark_json.contains("\"center\":[140,166]"));
    assert!(mark_json.contains("\"label\":\"Submit Button\""));
    assert!(mark_json.contains("\"control_type\":\"Button\""));

    let deser_mark: ScreenMark = serde_json::from_str(&mark_json).unwrap();
    assert_eq!(deser_mark, mark1);

    let res = MarkedScreenResponse {
        display_index: 0,
        width: 1280,
        height: 720,
        format: "jpeg".to_string(),
        base64_data: "data:image/jpeg;base64,dGVzdA==".to_string(),
        raw_base64: "dGVzdA==".to_string(),
        image_base64: "dGVzdA==".to_string(),
        data_uri: "data:image/jpeg;base64,dGVzdA==".to_string(),
        marks: vec![mark1, mark2],
        total_marks: 2,
        source: "hybrid".to_string(),
        original_width: Some(2560),
        original_height: Some(1440),
        scale_factor: Some(0.5),
    };

    let res_json = serde_json::to_string(&res).unwrap();
    assert!(res_json.contains("\"width\":1280"));
    assert!(res_json.contains("\"height\":720"));
    assert!(res_json.contains("\"total_marks\":2"));
    assert!(res_json.contains("\"source\":\"hybrid\""));
    assert!(res_json.contains("\"original_width\":2560"));
    assert!(res_json.contains("\"scale_factor\":0.5"));

    let deser_res: MarkedScreenResponse = serde_json::from_str(&res_json).unwrap();
    assert_eq!(deser_res, res);
}

#[test]
fn test_monitor_info_serialization() {
    use at_pc_protocol::models::MonitorInfo;

    let monitor = MonitorInfo {
        display_index: 1,
        name: "DELL U2720Q".to_string(),
        is_primary: false,
        x: 1920,
        y: 0,
        width: 2560,
        height: 1440,
        scale_factor: 1.25,
    };

    let json_str = serde_json::to_string(&monitor).unwrap();
    assert!(json_str.contains("\"display_index\":1"));
    assert!(json_str.contains("\"name\":\"DELL U2720Q\""));
    assert!(json_str.contains("\"is_primary\":false"));
    assert!(json_str.contains("\"x\":1920"));
    assert!(json_str.contains("\"y\":0"));
    assert!(json_str.contains("\"width\":2560"));
    assert!(json_str.contains("\"height\":1440"));
    assert!(json_str.contains("\"scale_factor\":1.25"));

    let deserialized: MonitorInfo = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized, monitor);
}
