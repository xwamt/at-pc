//! Low-latency remote desktop streaming module using xcap and binary JPEG compression.
//! Streams screen frames as efficient binary WebSocket frames with dirty-screen deduplication
//! and channel isolation to avoid saturating control and heartbeat channels.

use at_pc_desktop_core::{
    compute_block_hashes, decide_frame_send, encode_jpeg, fast_rgba_to_rgb_scaled, is_frame_dirty,
    FrameSendDecision, STREAM_KEEPALIVE_INTERVAL,
};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{debug, info};
use xcap::Monitor;

use at_pc_protocol::messages::BinaryDesktopFrame;

#[derive(Debug, Clone)]
pub enum PreparedFrame {
    Unchanged,
    Keepalive,
    CaptureError(String),
    EncodeError {
        hashes: Vec<u64>,
        error: String,
    },
    Ready {
        hashes: Vec<u64>,
        width: u32,
        height: u32,
        jpeg_bytes: Vec<u8>,
    },
}

/// Number of consecutive unchanged frames required before entering idle dynamic backoff.
pub const IDLE_BACKOFF_THRESHOLD: u32 = 5;

/// Frame interval when stream is in idle dynamic backoff (2 Hz = 500ms).
pub const IDLE_FRAME_INTERVAL: Duration = Duration::from_millis(500);

/// Computes the effective loop interval given the active target interval and unchanged frame count.
/// When `consecutive_unchanged >= IDLE_BACKOFF_THRESHOLD` (5), throttles to `IDLE_FRAME_INTERVAL` (500ms / 2 Hz).
/// Otherwise returns `active_interval` (e.g. 66ms for 15 fps).
pub fn compute_backoff_interval(active_interval: Duration, consecutive_unchanged: u32) -> Duration {
    if consecutive_unchanged >= IDLE_BACKOFF_THRESHOLD {
        IDLE_FRAME_INTERVAL
    } else {
        active_interval
    }
}

/// Updates the consecutive unchanged frame counter based on the prepared frame:
/// - `PreparedFrame::Ready` resets counter to 0 (screen is dirty, immediate full fps).
/// - `PreparedFrame::Unchanged` increments counter (`saturating_add(1)`).
/// - Other frames do not alter the counter.
pub fn update_consecutive_unchanged(counter: &mut u32, prepared: &PreparedFrame) {
    match prepared {
        PreparedFrame::Ready { .. } => {
            *counter = 0;
        }
        PreparedFrame::Unchanged => {
            *counter = counter.saturating_add(1);
        }
        _ => {}
    }
}

/// Computes the next consecutive unchanged frame count based on the prepared frame.
pub fn next_consecutive_unchanged(current: u32, prepared: &PreparedFrame) -> u32 {
    let mut val = current;
    update_consecutive_unchanged(&mut val, prepared);
    val
}

/// Send-side stream state committed after a prepared frame is applied.
#[derive(Clone, Debug)]
pub struct StreamSendState {
    pub last_hashes: Vec<u64>,
    pub last_width: u32,
    pub last_height: u32,
    pub last_sent_time: Instant,
}

/// Result of trying to apply a prepared frame to stream send-state.
pub struct StreamCommit {
    pub last_hashes: Vec<u64>,
    pub last_width: u32,
    pub last_height: u32,
    pub last_sent_time: Instant,
    pub stop: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendOutcome {
    NotAttempted,
    Ok,
    Full,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhatToSend {
    Nothing,
    Payload(Vec<u8>),
}

/// After `spawn_blocking` returns, discard the frame if the stream generation changed.
pub fn should_apply_prepared(my_gen: u64, current_gen: u64) -> bool {
    my_gen == current_gen
}

/// Decide whether the caller should `try_send` for this prepared frame.
pub fn what_to_send(
    state: &StreamSendState,
    prepared: &PreparedFrame,
    display_index: u32,
    now_ms: u64,
) -> WhatToSend {
    match prepared {
        PreparedFrame::Keepalive => {
            if state.last_width == 0 {
                WhatToSend::Nothing
            } else {
                let bin_frame = BinaryDesktopFrame::new(
                    display_index,
                    state.last_width,
                    state.last_height,
                    now_ms,
                    Vec::new(),
                );
                WhatToSend::Payload(bin_frame.encode())
            }
        }
        PreparedFrame::Ready {
            width,
            height,
            jpeg_bytes,
            ..
        } => {
            let bin_frame =
                BinaryDesktopFrame::new(display_index, *width, *height, now_ms, jpeg_bytes.clone());
            WhatToSend::Payload(bin_frame.encode())
        }
        PreparedFrame::Unchanged
        | PreparedFrame::CaptureError(_)
        | PreparedFrame::EncodeError { .. } => WhatToSend::Nothing,
    }
}

/// Apply channel outcome to send-state. Only a successful send advances the clock;
/// EncodeError never counts as sent.
pub fn apply_send_outcome(
    mut state: StreamSendState,
    prepared: PreparedFrame,
    outcome: SendOutcome,
    now: Instant,
) -> StreamCommit {
    let stop = matches!(outcome, SendOutcome::Closed);
    match prepared {
        PreparedFrame::EncodeError { .. }
        | PreparedFrame::Unchanged
        | PreparedFrame::CaptureError(_) => {}
        PreparedFrame::Keepalive => {
            if state.last_width > 0 && matches!(outcome, SendOutcome::Ok) {
                state.last_sent_time = now;
            }
        }
        PreparedFrame::Ready {
            hashes,
            width,
            height,
            ..
        } => match outcome {
            SendOutcome::Ok => {
                state.last_hashes = hashes;
                state.last_width = width;
                state.last_height = height;
                state.last_sent_time = now;
            }
            SendOutcome::Full if state.last_width > 0 => {
                // Dropping a stale live frame is OK; keepalive can still retry.
                state.last_hashes = hashes;
                state.last_width = width;
                state.last_height = height;
            }
            SendOutcome::Full | SendOutcome::NotAttempted | SendOutcome::Closed => {}
        },
    }
    StreamCommit {
        last_hashes: state.last_hashes,
        last_width: state.last_width,
        last_height: state.last_height,
        last_sent_time: state.last_sent_time,
        stop,
    }
}

/// Helper to locate the target monitor without constantly churning DirectX handles
fn find_monitor(display_index: u32) -> Option<Monitor> {
    let monitors = Monitor::all().ok()?;
    monitors
        .into_iter()
        .nth(display_index as usize)
        .or_else(|| Monitor::all().ok()?.into_iter().next())
}

/// Controller managing the background desktop streaming task
#[derive(Clone, Default)]
pub struct DesktopStreamController {
    is_streaming: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
}

impl DesktopStreamController {
    pub fn new() -> Self {
        Self {
            is_streaming: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Returns whether desktop streaming is currently active
    pub fn is_streaming(&self) -> bool {
        self.is_streaming.load(Ordering::SeqCst)
    }

    /// Stops desktop streaming immediately
    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if self.is_streaming.swap(false, Ordering::SeqCst) {
            info!("Stopped remote desktop stream");
        }
    }

    /// Starts desktop streaming task in background sending binary frames over dedicated channel
    pub fn start_binary(
        &self,
        display_index: u32,
        fps: u32,
        quality: u8,
        scale: f32,
        binary_tx: mpsc::Sender<Vec<u8>>,
    ) {
        let my_gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.is_streaming.store(true, Ordering::SeqCst);

        let is_running = self.is_streaming.clone();
        let gen_ref = self.generation.clone();
        let target_fps = if fps == 0 { 15 } else { fps.min(30) };
        let frame_interval = Duration::from_millis((1000 / target_fps) as u64);
        let jpeg_quality = if quality == 0 { 55 } else { quality.min(85) };

        info!(
            "Starting binary remote desktop stream gen={} (display: {}, fps: {}, quality: {}%, scale: {})",
            my_gen, display_index, target_fps, jpeg_quality, scale
        );

        tokio::spawn(async move {
            let mut cached_monitor = find_monitor(display_index);
            let mut state = StreamSendState {
                last_hashes: Vec::new(),
                last_width: 0,
                last_height: 0,
                last_sent_time: Instant::now() - Duration::from_secs(10),
            };
            let mut consecutive_unchanged: u32 = 0;

            while is_running.load(Ordering::SeqCst) && gen_ref.load(Ordering::SeqCst) == my_gen {
                let start_time = Instant::now();

                // 1. Ensure monitor is available (cached). find_monitor stays on the async task.
                if cached_monitor.is_none() {
                    cached_monitor = find_monitor(display_index);
                }
                let Some(monitor) = cached_monitor.clone() else {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                };

                let prev_hashes = state.last_hashes.clone();
                let elapsed_since_send = state.last_sent_time.elapsed();

                // 2-5. Capture + hash + convert + encode on a short blocking task (not the whole session)
                let prepared = tokio::task::spawn_blocking(move || {
                    let image = match monitor.capture_image() {
                        Ok(img) => img,
                        Err(e) => return PreparedFrame::CaptureError(e.to_string()),
                    };
                    let hashes =
                        compute_block_hashes(image.as_raw(), image.width(), image.height());
                    match decide_frame_send(
                        is_frame_dirty(&prev_hashes, &hashes),
                        elapsed_since_send,
                        STREAM_KEEPALIVE_INTERVAL,
                    ) {
                        FrameSendDecision::Skip => PreparedFrame::Unchanged,
                        FrameSendDecision::Keepalive => PreparedFrame::Keepalive,
                        FrameSendDecision::Encode => {
                            let (rgb_img, width, height) = fast_rgba_to_rgb_scaled(&image, scale);
                            match encode_jpeg(&rgb_img, jpeg_quality) {
                                Ok(jpeg_bytes) => PreparedFrame::Ready {
                                    hashes,
                                    width,
                                    height,
                                    jpeg_bytes,
                                },
                                Err(e) => PreparedFrame::EncodeError {
                                    hashes,
                                    error: e.to_string(),
                                },
                            }
                        }
                    }
                })
                .await;

                let prepared = match prepared {
                    Ok(frame) => frame,
                    Err(e) => {
                        debug!("Frame capture/convert/encode task failed: {}", e);
                        continue;
                    }
                };

                if !should_apply_prepared(my_gen, gen_ref.load(Ordering::SeqCst)) {
                    break;
                }

                // Maintain consecutive unchanged counter and idle dynamic backoff
                match &prepared {
                    PreparedFrame::Ready { .. } => {
                        consecutive_unchanged = 0;
                    }
                    PreparedFrame::Unchanged => {
                        consecutive_unchanged = consecutive_unchanged.saturating_add(1);
                    }
                    _ => {}
                }
                let current_interval =
                    compute_backoff_interval(frame_interval, consecutive_unchanged);

                match &prepared {
                    PreparedFrame::CaptureError(e) => {
                        debug!("Monitor capture failed (refreshing monitor handle): {}", e);
                        cached_monitor = None;
                        tokio::time::sleep(Duration::from_millis(150)).await;
                        continue;
                    }
                    PreparedFrame::Unchanged => {
                        let elapsed = start_time.elapsed();
                        if elapsed < current_interval {
                            tokio::time::sleep(current_interval - elapsed).await;
                        } else {
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                        continue;
                    }
                    PreparedFrame::EncodeError { error, .. } => {
                        debug!("Failed to encode JPEG frame: {}", error);
                    }
                    PreparedFrame::Keepalive | PreparedFrame::Ready { .. } => {}
                }

                let now_ms = chrono::Utc::now().timestamp_millis() as u64;
                let to_send = what_to_send(&state, &prepared, display_index, now_ms);
                let is_keepalive = matches!(prepared, PreparedFrame::Keepalive);
                let is_encode_error = matches!(prepared, PreparedFrame::EncodeError { .. });
                let outcome = match to_send {
                    WhatToSend::Nothing => SendOutcome::NotAttempted,
                    WhatToSend::Payload(payload) => match binary_tx.try_send(payload) {
                        Ok(_) => SendOutcome::Ok,
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            if is_keepalive {
                                debug!("Binary stream keepalive dropped due to backpressure");
                            } else {
                                debug!("Binary stream frame dropped due to backpressure");
                            }
                            SendOutcome::Full
                        }
                        Err(mpsc::error::TrySendError::Closed(_)) => {
                            debug!("Binary stream channel closed. Terminating stream loop.");
                            SendOutcome::Closed
                        }
                    },
                };

                let commit = apply_send_outcome(state, prepared, outcome, Instant::now());
                state = StreamSendState {
                    last_hashes: commit.last_hashes,
                    last_width: commit.last_width,
                    last_height: commit.last_height,
                    last_sent_time: commit.last_sent_time,
                };
                if commit.stop {
                    is_running.store(false, Ordering::SeqCst);
                    break;
                }
                if is_encode_error {
                    continue;
                }

                // 7. Yield properly to ensure low CPU usage (<5%)
                let elapsed = start_time.elapsed();
                if elapsed < current_interval {
                    tokio::time::sleep(current_interval - elapsed).await;
                } else {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
            debug!(
                "Remote desktop streaming background loop gen={} terminated cleanly",
                my_gen
            );
        });
    }
}
