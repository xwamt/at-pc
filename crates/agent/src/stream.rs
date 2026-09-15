//! Low-latency remote desktop streaming module using xcap and binary JPEG compression.
//! Streams screen frames as efficient binary WebSocket frames with dirty-screen deduplication
//! and channel isolation to avoid saturating control and heartbeat channels.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use image::codecs::jpeg::JpegEncoder;
use tokio::sync::mpsc;
use tracing::{debug, info};
use xcap::Monitor;

use at_pc_protocol::messages::{AgentToServerMessage, BinaryDesktopFrame};

/// Ultra-fast RGBA -> RGB converter with optional 2x subsampling for high-DPI screens (>1920px).
/// Operates directly on raw memory with zero external filtering overhead (sub-millisecond CPU time).
fn fast_rgba_to_rgb(rgba: &xcap::image::RgbaImage) -> (image::RgbImage, u32, u32) {
    let orig_w = rgba.width();
    let orig_h = rgba.height();
    let raw = rgba.as_raw();

    if orig_w > 1920 {
        let target_w = orig_w / 2;
        let target_h = orig_h / 2;
        let mut rgb_data = Vec::with_capacity((target_w * target_h * 3) as usize);

        let src_stride = (orig_w * 4) as usize;
        for y in 0..target_h {
            let row_offset = (y * 2) as usize * src_stride;
            for x in 0..target_w {
                let pixel_offset = row_offset + (x * 2) as usize * 4;
                rgb_data.push(raw[pixel_offset]);
                rgb_data.push(raw[pixel_offset + 1]);
                rgb_data.push(raw[pixel_offset + 2]);
            }
        }

        let rgb_img = image::RgbImage::from_raw(target_w, target_h, rgb_data)
            .unwrap_or_else(|| image::DynamicImage::ImageRgba8(rgba.clone()).into_rgb8());
        (rgb_img, target_w, target_h)
    } else {
        let mut rgb_data = Vec::with_capacity((orig_w * orig_h * 3) as usize);
        for chunk in raw.chunks_exact(4) {
            rgb_data.push(chunk[0]);
            rgb_data.push(chunk[1]);
            rgb_data.push(chunk[2]);
        }
        let rgb_img = image::RgbImage::from_raw(orig_w, orig_h, rgb_data)
            .unwrap_or_else(|| image::DynamicImage::ImageRgba8(rgba.clone()).into_rgb8());
        (rgb_img, orig_w, orig_h)
    }
}

/// Compute a fast 64-bit sampling hash of raw image buffer to detect unchanged/static screens.
fn compute_sample_hash(raw_rgba: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for chunk in raw_rgba.chunks(256) {
        if let Some(&b) = chunk.first() {
            hash = hash.wrapping_mul(0x100000001b3) ^ (b as u64);
        }
    }
    hash
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
            "Starting binary remote desktop stream gen={} (display: {}, fps: {}, quality: {}%)",
            my_gen, display_index, target_fps, jpeg_quality
        );

        tokio::task::spawn_blocking(move || {
            let mut cached_monitor = find_monitor(display_index);
            let mut last_hash: u64 = 0;
            let mut last_sent_time = Instant::now() - Duration::from_secs(10);

            while is_running.load(Ordering::SeqCst) && gen_ref.load(Ordering::SeqCst) == my_gen {
                let start_time = Instant::now();

                // 1. Ensure monitor is available (cached)
                let monitor = match cached_monitor.as_ref() {
                    Some(m) => m,
                    None => {
                        cached_monitor = find_monitor(display_index);
                        if cached_monitor.is_none() {
                            std::thread::sleep(Duration::from_millis(200));
                            continue;
                        }
                        cached_monitor.as_ref().unwrap()
                    }
                };

                // 2. Capture monitor screen
                let image = match monitor.capture_image() {
                    Ok(img) => img,
                    Err(e) => {
                        debug!("Monitor capture failed (refreshing monitor handle): {}", e);
                        cached_monitor = None;
                        std::thread::sleep(Duration::from_millis(150));
                        continue;
                    }
                };

                // 3. Static screen dirty check (skip re-encoding if screen unchanged, unless 1s keepalive)
                let current_hash = compute_sample_hash(image.as_raw());
                let is_static = current_hash == last_hash;
                if is_static && last_sent_time.elapsed() < Duration::from_millis(1000) {
                    let elapsed = start_time.elapsed();
                    if elapsed < frame_interval {
                        std::thread::sleep(frame_interval - elapsed);
                    } else {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    continue;
                }
                last_hash = current_hash;
                last_sent_time = Instant::now();

                // 4. Fast RGBA -> RGB conversion with ultra-light stride subsampling
                let (rgb_img, width, height) = fast_rgba_to_rgb(&image);

                // 5. Compress directly to JPEG
                let mut jpeg_bytes = Vec::with_capacity((width * height / 5) as usize);
                let mut cursor = Cursor::new(&mut jpeg_bytes);
                let encoder = JpegEncoder::new_with_quality(&mut cursor, jpeg_quality);
                if let Err(e) = rgb_img.write_with_encoder(encoder) {
                    debug!("Failed to encode JPEG frame: {}", e);
                    continue;
                }

                let now_ms = chrono::Utc::now().timestamp_millis() as u64;
                let bin_frame = BinaryDesktopFrame::new(display_index, width, height, now_ms, jpeg_bytes);
                let encoded_payload = bin_frame.encode();

                // 6. Non-blocking channel send (drops stale frames if channel congested)
                match binary_tx.try_send(encoded_payload) {
                    Ok(_) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        // Drop frame when network/writer is congested (standard video conflation)
                        debug!("Binary stream frame dropped due to backpressure");
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        debug!("Binary stream channel closed. Terminating stream loop.");
                        is_running.store(false, Ordering::SeqCst);
                        break;
                    }
                }

                // 7. Yield properly to ensure low CPU usage (<5%)
                let elapsed = start_time.elapsed();
                if elapsed < frame_interval {
                    std::thread::sleep(frame_interval - elapsed);
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            debug!("Remote desktop streaming background loop gen={} terminated cleanly", my_gen);
        });
    }

    /// Legacy compatibility forwarder to start streaming
    pub fn start(
        &self,
        display_index: u32,
        fps: u32,
        quality: u8,
        outbound_tx: mpsc::UnboundedSender<AgentToServerMessage>,
    ) {
        let (bin_tx, mut bin_rx) = mpsc::channel::<Vec<u8>>(2);
        self.start_binary(display_index, fps, quality, bin_tx);

        let is_running = self.is_streaming.clone();
        tokio::spawn(async move {
            use base64::Engine;
            while is_running.load(Ordering::SeqCst) {
                if let Some(bin_data) = bin_rx.recv().await {
                    if let Ok(frame) = BinaryDesktopFrame::decode(&bin_data) {
                        let b64 = base64::prelude::BASE64_STANDARD.encode(&frame.data);
                        let msg = AgentToServerMessage::DesktopFrame {
                            display_index: frame.display_index,
                            width: frame.width,
                            height: frame.height,
                            format: "jpeg".to_string(),
                            data: b64,
                            timestamp: frame.timestamp,
                        };
                        if outbound_tx.send(msg).is_err() {
                            break;
                        }
                    }
                } else {
                    break;
                }
            }
        });
    }
}
