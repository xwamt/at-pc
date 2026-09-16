// at-pc/crates/server/tests/registry_scaling_test.rs
use at_pc_protocol::models::TerminalInfo;
use at_pc_server::ws::registry::TerminalRegistry;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

#[tokio::test]
async fn test_update_terminal_meta_latency_scaled_1000() {
    let registry = Arc::new(TerminalRegistry::new());
    let (tx, _rx) = mpsc::unbounded_channel();

    // 1. Register 1,000 terminals
    for i in 0..1000 {
        let info = TerminalInfo {
            terminal_id: format!("term-{:04}", i),
            hostname: format!("HOST-{:04}", i),
            username: "tester".to_string(),
            lan_ip: "10.0.0.1".to_string(),
            os_version: "Linux".to_string(),
            agent_version: "1.0.0".to_string(),
        };
        registry.register(info, tx.clone()).await;
    }
    assert_eq!(registry.count().await, 1000);

    // Warm-up to eliminate cold cache/initial runtime priming
    for i in 0..50 {
        let term_id = format!("term-{:04}", i);
        let _ = registry
            .update_terminal_meta(
                &term_id,
                Some(format!("warmup-{}", i)),
                Some("warmup".to_string()),
                Some(vec!["warmup".to_string()]),
            )
            .await;
    }

    // 2. Measure update_terminal_meta latency across 1,000 updates
    let count = 1000;
    let start = Instant::now();
    for i in 0..count {
        let term_id = format!("term-{:04}", i);
        let res = registry
            .update_terminal_meta(
                &term_id,
                Some(format!("bench-name-{:04}", i)),
                Some(format!("notes for terminal {:04}", i)),
                Some(vec!["production".to_string(), "scaled".to_string()]),
            )
            .await;
        assert!(res.is_ok());
    }
    let total_duration = start.elapsed();
    let avg_latency = total_duration / count as u32;

    println!(
        "\n[Benchmark] 1000 terminals: total update time = {:?}, avg latency = {:?} (target < 50µs / 0.05ms)",
        total_duration, avg_latency
    );

    // Target: update_terminal_meta latency < 0.05ms (50µs)
    assert!(
        avg_latency < Duration::from_micros(50),
        "Expected avg update_terminal_meta latency < 0.05ms (50µs), but got {:?}",
        avg_latency
    );

    // 3. Verify list_terminals efficiency on 1,000 terminals
    let start_list = Instant::now();
    let list = registry.list_terminals().await;
    let list_duration = start_list.elapsed();

    println!(
        "[Benchmark] list_terminals on 1000 terminals: {:?}",
        list_duration
    );

    assert_eq!(list.len(), 1000);
    assert_eq!(list[0].info.terminal_id, "term-0000");
    assert_eq!(list[0].custom_name.as_deref(), Some("bench-name-0000"));
    assert_eq!(list[999].info.terminal_id, "term-0999");
    assert_eq!(list[999].custom_name.as_deref(), Some("bench-name-0999"));
}
