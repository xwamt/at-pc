//! Unit tests for stream capture loop idle dynamic backoff mechanism (Milestone 1 Task 3: P0-C).
//!
//! Verifies:
//! 1. Consecutive unchanged frames increment the counter and transition into an idle backoff state (500ms / 2 Hz) at >= 5.
//! 2. Producing a dirty frame (PreparedFrame::Ready) immediately resets the counter to 0 and restores full active fps (e.g. 15 fps / 66ms).
//! 3. Keepalive and error frames do not erroneously reset idle backoff.
//! 4. Counter uses saturating arithmetic to prevent integer overflow.
//! 5. Source code contract ensures the streaming loop maintains consecutive_unchanged and dynamic throttling.

use std::time::Duration;

use at_pc_agent::stream::{
    compute_backoff_interval, next_consecutive_unchanged, update_consecutive_unchanged,
    DesktopStreamController, PreparedFrame, IDLE_BACKOFF_THRESHOLD, IDLE_FRAME_INTERVAL,
};

#[test]
fn consecutive_unchanged_transitions_to_idle_and_resets_on_dirty() {
    let target_fps = 15;
    let active_interval = Duration::from_millis((1000 / target_fps) as u64); // 66ms
    let mut consecutive_unchanged: u32 = 0;

    // Initial state: 0 unchanged frames -> active interval (66ms / 15 fps)
    assert_eq!(consecutive_unchanged, 0);
    assert_eq!(
        compute_backoff_interval(active_interval, consecutive_unchanged),
        active_interval,
        "Initial frame interval must be full active fps"
    );

    // Feed 4 consecutive unchanged frames (below threshold of 5)
    for i in 1..=4 {
        update_consecutive_unchanged(&mut consecutive_unchanged, &PreparedFrame::Unchanged);
        assert_eq!(consecutive_unchanged, i);
        assert_eq!(
            compute_backoff_interval(active_interval, consecutive_unchanged),
            active_interval,
            "Frame {} (< 5) must remain at active interval",
            i
        );
    }

    // Feed 5th unchanged frame -> threshold reached, transitions to idle backoff (500ms / 2 Hz)
    update_consecutive_unchanged(&mut consecutive_unchanged, &PreparedFrame::Unchanged);
    assert_eq!(consecutive_unchanged, IDLE_BACKOFF_THRESHOLD);
    assert_eq!(
        compute_backoff_interval(active_interval, consecutive_unchanged),
        IDLE_FRAME_INTERVAL,
        "At 5 consecutive unchanged frames, stream must throttle to idle interval (500ms / 2 Hz)"
    );

    // Further unchanged frames remain in idle backoff
    for _ in 0..10 {
        update_consecutive_unchanged(&mut consecutive_unchanged, &PreparedFrame::Unchanged);
        assert!(consecutive_unchanged >= IDLE_BACKOFF_THRESHOLD);
        assert_eq!(
            compute_backoff_interval(active_interval, consecutive_unchanged),
            IDLE_FRAME_INTERVAL,
            "Stream must stay throttled at 500ms while static"
        );
    }

    // Now a dirty frame is produced (PreparedFrame::Ready)
    let dirty_frame = PreparedFrame::Ready {
        hashes: vec![101, 102, 103],
        width: 1920,
        height: 1080,
        jpeg_bytes: vec![0xFF, 0xD8, 0xFF, 0xD9],
    };
    update_consecutive_unchanged(&mut consecutive_unchanged, &dirty_frame);

    // Counter must immediately reset to 0 and stream must run at full 15 fps
    assert_eq!(
        consecutive_unchanged, 0,
        "Dirty frame must immediately reset consecutive_unchanged counter to 0"
    );
    assert_eq!(
        compute_backoff_interval(active_interval, consecutive_unchanged),
        active_interval,
        "Dirty frame must immediately restore full 15 fps (66ms interval)"
    );
}

#[test]
fn next_consecutive_unchanged_helper_matches_state_updates() {
    assert_eq!(next_consecutive_unchanged(0, &PreparedFrame::Unchanged), 1);
    assert_eq!(next_consecutive_unchanged(4, &PreparedFrame::Unchanged), 5);
    assert_eq!(next_consecutive_unchanged(5, &PreparedFrame::Unchanged), 6);

    let dirty_frame = PreparedFrame::Ready {
        hashes: vec![1],
        width: 64,
        height: 64,
        jpeg_bytes: vec![],
    };
    assert_eq!(next_consecutive_unchanged(10, &dirty_frame), 0);
    assert_eq!(next_consecutive_unchanged(0, &dirty_frame), 0);
}

#[test]
fn keepalive_and_errors_preserve_backoff_state() {
    let active_interval = Duration::from_millis(66);
    let mut count = 8; // already in idle backoff

    // Keepalive sent on unchanged desktop must not reset idle backoff
    update_consecutive_unchanged(&mut count, &PreparedFrame::Keepalive);
    assert_eq!(count, 8, "Keepalive must not reset consecutive_unchanged");
    assert_eq!(
        compute_backoff_interval(active_interval, count),
        IDLE_FRAME_INTERVAL,
        "Keepalive must retain 500ms idle backoff"
    );

    // CaptureError must not reset backoff counter
    update_consecutive_unchanged(
        &mut count,
        &PreparedFrame::CaptureError("display disconnected".into()),
    );
    assert_eq!(count, 8, "CaptureError must not reset counter");

    // EncodeError must not reset backoff counter
    update_consecutive_unchanged(
        &mut count,
        &PreparedFrame::EncodeError {
            hashes: vec![1, 2],
            error: "jpeg encode failed".into(),
        },
    );
    assert_eq!(count, 8, "EncodeError must not reset counter");

    // Once a valid dirty frame is ready, immediately reset
    let dirty = PreparedFrame::Ready {
        hashes: vec![99],
        width: 1280,
        height: 720,
        jpeg_bytes: vec![0xFF, 0xD8],
    };
    update_consecutive_unchanged(&mut count, &dirty);
    assert_eq!(count, 0);
    assert_eq!(
        compute_backoff_interval(active_interval, count),
        active_interval
    );
}

#[test]
fn consecutive_unchanged_counter_saturates_without_overflow() {
    let mut count = u32::MAX;
    update_consecutive_unchanged(&mut count, &PreparedFrame::Unchanged);
    assert_eq!(count, u32::MAX, "Counter must saturate at u32::MAX");
    assert_eq!(
        compute_backoff_interval(Duration::from_millis(66), count),
        IDLE_FRAME_INTERVAL
    );

    // Reset from saturated state
    let dirty = PreparedFrame::Ready {
        hashes: vec![],
        width: 100,
        height: 100,
        jpeg_bytes: vec![],
    };
    update_consecutive_unchanged(&mut count, &dirty);
    assert_eq!(count, 0);
}

#[test]
fn multiple_target_fps_configurations_throttle_to_idle_interval() {
    // 30 fps (33ms)
    let fps_30 = Duration::from_millis(1000 / 30);
    assert_eq!(compute_backoff_interval(fps_30, 0), fps_30);
    assert_eq!(compute_backoff_interval(fps_30, 4), fps_30);
    assert_eq!(compute_backoff_interval(fps_30, 5), IDLE_FRAME_INTERVAL);

    // 10 fps (100ms)
    let fps_10 = Duration::from_millis(1000 / 10);
    assert_eq!(compute_backoff_interval(fps_10, 0), fps_10);
    assert_eq!(compute_backoff_interval(fps_10, 4), fps_10);
    assert_eq!(compute_backoff_interval(fps_10, 5), IDLE_FRAME_INTERVAL);
}

#[test]
fn stream_rs_source_code_contract_for_backoff() {
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/stream.rs"));

    assert!(
        src.contains("consecutive_unchanged"),
        "stream.rs must maintain consecutive_unchanged counter"
    );
    assert!(
        src.contains("IDLE_BACKOFF_THRESHOLD") || src.contains("consecutive_unchanged >= 5"),
        "stream.rs must enforce idle threshold of 5"
    );
    assert!(
        src.contains("from_millis(500)"),
        "stream.rs must define 500ms (2 Hz) idle backoff interval"
    );
    assert!(
        src.contains("consecutive_unchanged = 0"),
        "stream.rs must reset consecutive_unchanged = 0 on dirty frame"
    );
    assert!(
        src.contains("saturating_add(1)"),
        "stream.rs must increment consecutive_unchanged with saturating_add(1)"
    );
}

#[tokio::test]
async fn desktop_stream_controller_runs_and_stops_with_backoff_enabled() {
    let controller = DesktopStreamController::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);

    // Start 15 fps stream
    controller.start_binary(0, 15, 55, 1.0, tx);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        controller.is_streaming(),
        "Stream should be marked streaming"
    );

    // Stop controller cleanly
    controller.stop();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !controller.is_streaming(),
        "Stream should be stopped after stop()"
    );

    // Drain any frames produced without panic
    while rx.try_recv().is_ok() {}
}
