use at_pc_protocol::messages::BinaryDesktopFrame;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::handler::AgentMessageHandler;
use at_pc_server::ws::registry::TerminalRegistry;
use base64::Engine;
use std::sync::Arc;

#[tokio::test]
async fn test_frame_precomputed_base64_on_ingestion() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = McpRouter::new(registry);

    let test_bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0xFF, 0xD9];
    let expected_b64 = base64::prelude::BASE64_STANDARD.encode(&test_bytes);

    let binary_frame = BinaryDesktopFrame {
        display_index: 0,
        width: 1920,
        height: 1080,
        timestamp: 1_700_000_000_123,
        data: test_bytes.clone(),
    };

    // Ingest frame into router cache
    router.handle_desktop_frame_binary("test-terminal", binary_frame);

    // 1. Immediately verify get_latest_desktop_frame returns the precomputed base64
    let cached = router
        .get_latest_desktop_frame("test-terminal")
        .await
        .expect("Cached frame must exist");

    assert_eq!(cached.display_index, 0);
    assert_eq!(cached.width, 1920);
    assert_eq!(cached.height, 1080);
    assert_eq!(cached.format, "jpeg");
    assert_eq!(cached.timestamp, 1_700_000_000_123);
    assert_eq!(cached.data, expected_b64);
    assert_eq!(cached.raw_bytes, test_bytes);

    // 2. Multiple successive calls yield identical data without re-encoding
    for _ in 0..10 {
        let frame = router
            .get_latest_desktop_frame("test-terminal")
            .await
            .expect("Cached frame exists");
        assert_eq!(frame.data, expected_b64);
        assert_eq!(frame.raw_bytes, test_bytes);
    }

    // 3. Verify get_latest_desktop_frame_raw returns identical raw bytes and metadata
    let (d_idx, w, h, ts, raw) = router
        .get_latest_desktop_frame_raw("test-terminal")
        .await
        .expect("Raw frame must exist");
    assert_eq!(d_idx, 0);
    assert_eq!(w, 1920);
    assert_eq!(h, 1080);
    assert_eq!(ts, 1_700_000_000_123);
    assert_eq!(raw, test_bytes);
}

#[tokio::test]
async fn test_frame_precomputed_b64_concurrent_readers() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry));

    let sample_payload = vec![42u8; 1024];
    let expected_b64 = base64::prelude::BASE64_STANDARD.encode(&sample_payload);

    router.handle_desktop_frame_binary(
        "concurrent-term",
        BinaryDesktopFrame {
            display_index: 1,
            width: 2560,
            height: 1440,
            timestamp: 5000,
            data: sample_payload.clone(),
        },
    );

    let mut handles = Vec::new();
    for _ in 0..20 {
        let r = router.clone();
        let b64_expected = expected_b64.clone();
        let raw_expected = sample_payload.clone();
        handles.push(tokio::spawn(async move {
            let frame = r
                .get_latest_desktop_frame("concurrent-term")
                .await
                .expect("frame");
            assert_eq!(frame.data, b64_expected);
            assert_eq!(frame.raw_bytes, raw_expected);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn test_frame_keepalive_preserves_precomputed_b64() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = McpRouter::new(registry);

    let test_bytes = vec![0x11, 0x22, 0x33, 0x44];
    let expected_b64 = base64::prelude::BASE64_STANDARD.encode(&test_bytes);

    router.handle_desktop_frame_binary(
        "term-ka",
        BinaryDesktopFrame {
            display_index: 0,
            width: 1280,
            height: 720,
            timestamp: 1000,
            data: test_bytes.clone(),
        },
    );

    // Keepalive frame (empty payload) with newer timestamp
    router.handle_desktop_frame_binary(
        "term-ka",
        BinaryDesktopFrame {
            display_index: 0,
            width: 1280,
            height: 720,
            timestamp: 2000,
            data: Vec::new(),
        },
    );

    let frame = router
        .get_latest_desktop_frame("term-ka")
        .await
        .expect("Cached frame must exist");

    assert_eq!(frame.timestamp, 2000, "Timestamp should update on keepalive");
    assert_eq!(frame.data, expected_b64, "Precomputed base64 must remain intact");
    assert_eq!(frame.raw_bytes, test_bytes, "Raw bytes must remain intact");
}

#[tokio::test]
async fn test_frame_older_timestamp_is_ignored() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = McpRouter::new(registry);

    let initial_bytes = vec![0x01, 0x02];
    let initial_b64 = base64::prelude::BASE64_STANDARD.encode(&initial_bytes);

    router.handle_desktop_frame_binary(
        "term-order",
        BinaryDesktopFrame {
            display_index: 0,
            width: 800,
            height: 600,
            timestamp: 5000,
            data: initial_bytes.clone(),
        },
    );

    // Older frame arrives late
    let older_bytes = vec![0x99, 0x99];
    router.handle_desktop_frame_binary(
        "term-order",
        BinaryDesktopFrame {
            display_index: 0,
            width: 800,
            height: 600,
            timestamp: 4000,
            data: older_bytes,
        },
    );

    let frame = router
        .get_latest_desktop_frame("term-order")
        .await
        .expect("Cached frame must exist");

    assert_eq!(frame.timestamp, 5000, "Timestamp must not regress");
    assert_eq!(frame.data, initial_b64, "Precomputed b64 must not be overwritten by older frame");
    assert_eq!(frame.raw_bytes, initial_bytes);
}
