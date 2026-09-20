// at-pc/crates/server/tests/registry_test.rs
use at_pc_protocol::messages::ServerToAgentMessage;
use at_pc_protocol::models::{HeartbeatMetrics, TerminalInfo};
use at_pc_server::config::ServerConfig;
use at_pc_server::ws::registry::{TerminalRegistry, TerminalStatus};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_terminal_registration_and_heartbeat() {
    let registry = Arc::new(TerminalRegistry::new());
    let (tx, _rx) = mpsc::unbounded_channel();

    let info = TerminalInfo {
        terminal_id: "node-1".to_string(),
        hostname: "HOST-1".to_string(),
        username: "admin".to_string(),
        lan_ip: "10.0.0.2".to_string(),
        os_version: "Windows 10".to_string(),
        agent_version: "0.3.0".to_string(),
    };

    registry.register(info.clone(), tx).await;
    assert_eq!(
        registry.get_status("node-1").await,
        Some(TerminalStatus::Online)
    );

    let metrics = HeartbeatMetrics {
        cpu_usage_percent: 12.5,
        memory_used_mb: 2048,
        memory_total_mb: 8192,
        uptime_secs: 3600,
        timestamp: 1725180000,
    };
    registry.update_heartbeat("node-1", metrics).await.unwrap();

    let list = registry.list_terminals().await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].info.terminal_id, "node-1");
    assert_eq!(list[0].status, TerminalStatus::Online);
    assert!(list[0].latest_metrics.is_some());
}

#[tokio::test]
async fn test_terminal_unregister_and_lookup() {
    let registry = Arc::new(TerminalRegistry::new());
    let (tx, _rx) = mpsc::unbounded_channel();

    let info = TerminalInfo {
        terminal_id: "node-2".to_string(),
        hostname: "HOST-2".to_string(),
        username: "user".to_string(),
        lan_ip: "192.168.1.100".to_string(),
        os_version: "macOS 14".to_string(),
        agent_version: "0.3.0".to_string(),
    };

    registry.register(info, tx).await;
    assert_eq!(registry.count().await, 1);

    let single = registry.get_terminal("node-2").await;
    assert!(single.is_some());
    assert_eq!(single.unwrap().info.hostname, "HOST-2");

    let removed = registry.unregister("node-2").await;
    assert!(removed.is_some());
    assert_eq!(registry.count().await, 0);
    assert_eq!(registry.get_status("node-2").await, None);
}

#[tokio::test]
async fn test_offline_sweep_threshold() {
    // Registry with short threshold 50ms for testing
    let registry = Arc::new(TerminalRegistry::with_threshold(Duration::from_millis(50)));
    let (tx, _rx) = mpsc::unbounded_channel();

    let info = TerminalInfo {
        terminal_id: "node-3".to_string(),
        hostname: "HOST-3".to_string(),
        username: "operator".to_string(),
        lan_ip: "10.1.1.5".to_string(),
        os_version: "Windows 11".to_string(),
        agent_version: "0.3.0".to_string(),
    };

    registry.register(info, tx).await;
    assert_eq!(
        registry.get_status("node-3").await,
        Some(TerminalStatus::Online)
    );

    // Wait past threshold
    tokio::time::sleep(Duration::from_millis(80)).await;

    let swept = registry.sweep_offline().await;
    assert_eq!(swept, vec!["node-3".to_string()]);
    assert_eq!(
        registry.get_status("node-3").await,
        Some(TerminalStatus::Offline)
    );

    // Send heartbeat to recover
    let metrics = HeartbeatMetrics {
        cpu_usage_percent: 5.0,
        memory_used_mb: 1024,
        memory_total_mb: 4096,
        uptime_secs: 100,
        timestamp: 1725180050,
    };
    registry.update_heartbeat("node-3", metrics).await.unwrap();
    assert_eq!(
        registry.get_status("node-3").await,
        Some(TerminalStatus::Online)
    );
}

#[tokio::test]
async fn test_send_to_terminal_channel() {
    let registry = Arc::new(TerminalRegistry::new());
    let (tx, mut rx) = mpsc::unbounded_channel();

    let info = TerminalInfo {
        terminal_id: "node-4".to_string(),
        hostname: "HOST-4".to_string(),
        username: "tester".to_string(),
        lan_ip: "10.1.1.9".to_string(),
        os_version: "Windows 10".to_string(),
        agent_version: "0.3.0".to_string(),
    };

    registry.register(info, tx).await;

    let cancel_msg = ServerToAgentMessage::CancelTool {
        call_id: "call-123".to_string(),
    };
    registry
        .send_to_terminal("node-4", cancel_msg.clone())
        .await
        .unwrap();

    let received = rx.recv().await.unwrap();
    assert_eq!(received, cancel_msg);
}

#[test]
fn test_server_config_defaults() {
    let config = ServerConfig::default();
    assert_eq!(config.ws_port, 9801);
    assert_eq!(config.ws_path, "/ws");
    assert_eq!(config.mcp_port, 9800);
    assert_eq!(config.heartbeat_interval_secs, 5);
    assert_eq!(config.offline_threshold_secs, 15);
    assert_eq!(config.listen_host, "0.0.0.0");
    assert!(config.allowed_origins.is_empty());
    assert!(config.auth_token.is_none());
}
