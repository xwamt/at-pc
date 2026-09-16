//! P1-6 item 2: desktop stream session must not monopolize spawn_blocking.

use std::time::{Duration, Instant};

use at_pc_agent::stream::{
    apply_send_outcome, should_apply_prepared, what_to_send, PreparedFrame, SendOutcome,
    StreamSendState, WhatToSend,
};
use at_pc_agent::DesktopStreamController;

#[test]
fn stream_rs_paces_with_tokio_sleep() {
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/stream.rs"));
    assert!(
        !src.contains("std::thread::sleep"),
        "stream pacing must not occupy a blocking thread with std::thread::sleep"
    );
    assert!(
        src.contains("tokio::time::sleep"),
        "stream pacing must use tokio::time::sleep"
    );
}

#[test]
fn stream_rs_spawn_blocking_only_for_frame_cpu() {
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/stream.rs"));
    assert!(
        src.contains("tokio::spawn(") || src.contains("tokio::task::spawn(async"),
        "streaming session must run as an async task, not a blocking-thread session"
    );

    let while_idx = src
        .find("while is_running")
        .expect("session loop should check is_running");
    let spawn_blocking_idx = src[while_idx..]
        .find("spawn_blocking")
        .map(|i| while_idx + i)
        .expect("convert + encode should still use spawn_blocking");
    let convert_idx = src
        .rfind("fast_rgba_to_rgb")
        .expect("frame convert must remain");
    let encode_idx = src.rfind("encode_jpeg").expect("frame encode must remain");

    assert!(
        while_idx < spawn_blocking_idx,
        "session loop must not live inside spawn_blocking"
    );
    assert!(
        spawn_blocking_idx < convert_idx,
        "spawn_blocking must wrap the convert CPU work"
    );
    assert!(
        spawn_blocking_idx < encode_idx,
        "spawn_blocking must wrap the encode CPU work"
    );
    assert!(
        src.contains("fast_rgba_to_rgb_scaled"),
        "stream convert must apply protocol scale"
    );
    assert!(
        src.contains("start_binary") && src.contains("scale: f32"),
        "start_binary must accept the protocol scale"
    );
}

#[test]
fn stream_rs_capture_image_inside_spawn_blocking() {
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/stream.rs"));
    let while_idx = src
        .find("while is_running")
        .expect("session loop should check is_running");
    let spawn_blocking_idx = src[while_idx..]
        .find("spawn_blocking")
        .map(|i| while_idx + i)
        .expect("per-frame work should use spawn_blocking");
    let capture_idx = src
        .rfind("capture_image")
        .expect("screen capture must remain");
    let hash_idx = src
        .rfind("compute_block_hashes")
        .expect("dirty-frame block hash must remain");

    assert!(
        spawn_blocking_idx < capture_idx,
        "capture_image must run inside the per-frame spawn_blocking closure, not on the tokio worker"
    );
    assert!(
        spawn_blocking_idx < hash_idx,
        "block hashes must run inside the per-frame spawn_blocking closure"
    );
    assert!(
        !src[while_idx..spawn_blocking_idx].contains("capture_image"),
        "async loop must not call capture_image before spawn_blocking"
    );
    assert!(
        !src.contains("compute_sample_hash"),
        "whole-frame sample hash must not drive stream dirty detection"
    );
    assert!(
        src.contains("STREAM_KEEPALIVE_INTERVAL"),
        "unchanged frames must use the 5s keepalive interval"
    );
    assert!(
        src.contains("FrameSendDecision::Keepalive"),
        "unchanged keepalive must skip JPEG"
    );
    assert!(
        !src.contains("from_millis(1000)"),
        "1s full re-encode keepalive must not remain"
    );
}

#[test]
fn stream_session_does_not_monopolize_blocking_pool() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(1)
        .enable_all()
        .build()
        .expect("runtime");

    rt.block_on(async {
        let controller = DesktopStreamController::new();
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        controller.start_binary(0, 15, 55, 1.0, tx);

        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(
            controller.is_streaming(),
            "stream session should be marked running"
        );

        let probe =
            tokio::time::timeout(Duration::from_secs(2), tokio::task::spawn_blocking(|| 1u8)).await;

        controller.stop();

        let joined = probe.expect(
            "stream session must not occupy the only spawn_blocking thread for its lifetime",
        );
        assert_eq!(joined.expect("probe must not panic"), 1);
        assert!(
            !controller.is_streaming(),
            "stop() must clear the streaming flag"
        );
    });
}

fn fresh_state(t0: Instant) -> StreamSendState {
    StreamSendState {
        last_hashes: Vec::new(),
        last_width: 0,
        last_height: 0,
        last_sent_time: t0,
    }
}

#[test]
fn encode_error_does_not_commit_hashes_so_next_identical_still_encodes() {
    let t0 = Instant::now();
    let t1 = t0 + Duration::from_secs(1);
    let previous = vec![11_u64, 12, 13];
    let failed = vec![21_u64, 22, 23];
    let state = StreamSendState {
        last_hashes: previous.clone(),
        last_width: 64,
        last_height: 48,
        last_sent_time: t0,
    };
    let prepared = PreparedFrame::EncodeError {
        hashes: failed.clone(),
        error: "jpeg encode failed".to_string(),
    };

    assert!(
        matches!(what_to_send(&state, &prepared, 0, 1), WhatToSend::Nothing),
        "EncodeError must not produce a send payload"
    );
    let commit = apply_send_outcome(state, prepared, SendOutcome::NotAttempted, t1);
    assert_eq!(
        commit.last_hashes, previous,
        "EncodeError must leave last successful hashes unchanged"
    );
    assert_eq!(commit.last_width, 64);
    assert_eq!(commit.last_height, 48);
    assert_eq!(
        commit.last_sent_time, t0,
        "EncodeError must not advance last_sent_time"
    );
    assert!(!commit.stop);
}

#[test]
fn keepalive_with_zero_width_does_not_send_or_advance_clock() {
    let t0 = Instant::now();
    let t1 = t0 + Duration::from_secs(1);
    let state = fresh_state(t0);
    let prepared = PreparedFrame::Keepalive;

    assert!(
        matches!(what_to_send(&state, &prepared, 0, 1), WhatToSend::Nothing),
        "keepalive before first JPEG must not produce a send payload"
    );
    let commit = apply_send_outcome(state, prepared, SendOutcome::NotAttempted, t1);
    assert_eq!(commit.last_width, 0);
    assert!(commit.last_hashes.is_empty());
    assert_eq!(
        commit.last_sent_time, t0,
        "keepalive with last_width==0 must not advance last_sent_time"
    );
    assert!(!commit.stop);
}

#[test]
fn ready_full_before_first_success_leaves_state_uncommitted() {
    let t0 = Instant::now();
    let t1 = t0 + Duration::from_secs(1);
    let hashes = vec![31_u64, 32, 33];
    let state = fresh_state(t0);
    let prepared = PreparedFrame::Ready {
        hashes: hashes.clone(),
        width: 320,
        height: 240,
        jpeg_bytes: vec![0xFF, 0xD8, 0xFF, 0xD9],
    };

    assert!(
        matches!(
            what_to_send(&state, &prepared, 0, 1),
            WhatToSend::Payload(_)
        ),
        "first JPEG must still be offered to try_send"
    );
    let commit = apply_send_outcome(state, prepared, SendOutcome::Full, t1);
    assert!(
        commit.last_hashes.is_empty(),
        "Ready+Full with no successful frame must not update hashes"
    );
    assert_eq!(
        commit.last_width, 0,
        "must retry first JPEG, dims unchanged"
    );
    assert_eq!(commit.last_height, 0);
    assert_eq!(
        commit.last_sent_time, t0,
        "Ready+Full with no successful frame must not advance last_sent_time"
    );
    assert!(!commit.stop);
}

#[test]
fn keepalive_full_does_not_advance_clock() {
    let t0 = Instant::now();
    let t1 = t0 + Duration::from_secs(1);
    let state = StreamSendState {
        last_hashes: vec![1, 2, 3],
        last_width: 1920,
        last_height: 1080,
        last_sent_time: t0,
    };
    let prepared = PreparedFrame::Keepalive;

    assert!(matches!(
        what_to_send(&state, &prepared, 0, 1),
        WhatToSend::Payload(_)
    ));
    let commit = apply_send_outcome(state, prepared, SendOutcome::Full, t1);
    assert_eq!(commit.last_width, 1920);
    assert_eq!(commit.last_hashes, vec![1, 2, 3]);
    assert_eq!(
        commit.last_sent_time, t0,
        "keepalive Full must not advance last_sent_time"
    );
    assert!(!commit.stop);
}

#[test]
fn ready_ok_commits_hashes_dims_and_clock() {
    let t0 = Instant::now();
    let t1 = t0 + Duration::from_secs(1);
    let hashes = vec![41_u64, 42];
    let state = fresh_state(t0);
    let prepared = PreparedFrame::Ready {
        hashes: hashes.clone(),
        width: 800,
        height: 600,
        jpeg_bytes: vec![0xFF, 0xD8],
    };

    assert!(matches!(
        what_to_send(&state, &prepared, 0, 99),
        WhatToSend::Payload(_)
    ));
    let commit = apply_send_outcome(state, prepared, SendOutcome::Ok, t1);
    assert_eq!(commit.last_hashes, hashes);
    assert_eq!(commit.last_width, 800);
    assert_eq!(commit.last_height, 600);
    assert_eq!(commit.last_sent_time, t1);
    assert!(!commit.stop);
}

#[test]
fn should_apply_prepared_discards_stale_generation() {
    assert!(
        should_apply_prepared(1, 1),
        "matching generation must still be applied"
    );
    assert!(
        !should_apply_prepared(1, 2),
        "generation mismatch after await must be discarded"
    );
}
