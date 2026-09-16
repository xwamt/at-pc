//! P2 performance probe: prints human-readable tables for the dimensions that
//! Criterion cannot express as a single number (geometry changes, detection
//! rates, per-response cost budgets, prototype headroom).
//!
//! Run with: `cargo run --release -p at-pc-benchmarks --example perf_probe`

use at_pc_desktop_core::{compute_block_hashes, encode_jpeg, fast_rgba_to_rgb, should_send_frame};
use at_pc_protocol::messages::BinaryDesktopFrame;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::handler::AgentMessageHandler;
use at_pc_server::ws::registry::TerminalRegistry;
use base64::Engine;
use image::{Rgb, RgbImage, Rgba, RgbaImage};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::runtime::Builder;

/// Median wall time of `iterations` runs, in milliseconds.
fn median_ms<T>(iterations: usize, mut body: impl FnMut() -> T) -> f64 {
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = Instant::now();
        let value = body();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
        drop(value);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    samples[samples.len() / 2]
}

fn rgba_fixture(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        let detail = (x ^ y) & 0x1f;
        Rgba([
            (x.wrapping_mul(13) ^ y.wrapping_mul(7)) as u8,
            (x.wrapping_add(y.wrapping_mul(3)).wrapping_add(detail)) as u8,
            (x.wrapping_mul(5).wrapping_add(y.wrapping_mul(11))) as u8,
            255,
        ])
    })
}

fn rgb_fixture(width: u32, height: u32) -> RgbImage {
    let source = rgba_fixture(width, height);
    RgbImage::from_fn(width, height, |x, y| {
        let pixel = source.get_pixel(x, y).0;
        Rgb([pixel[0], pixel[1], pixel[2]])
    })
}

fn heading(title: &str) {
    println!("\n{}", "=".repeat(96));
    println!("{title}");
    println!("{}", "=".repeat(96));
}

/// Resolution -> output geometry and per-stage cost, so the 1920 px policy
/// boundary is visible as a discontinuity rather than a smooth curve.
fn resolution_policy_table() {
    heading("1. RESOLUTION POLICY: output geometry and cost across the 1920 px boundary");
    println!(
        "{:<18} {:>12} {:>8} {:>10} {:>10} {:>10} {:>9} {:>10}",
        "source", "output", "scaled", "convert_ms", "encode_ms", "total_ms", "fps_cap", "jpeg_KB"
    );

    for (label, width, height) in [
        ("1920x1080", 1920_u32, 1080_u32),
        ("1921x1080", 1921, 1080),
        ("2560x1440", 2560, 1440),
        ("3440x1440", 3440, 1440),
        ("3840x2160", 3840, 2160),
    ] {
        let rgba = rgba_fixture(width, height);
        let (_, out_w, out_h) = fast_rgba_to_rgb(&rgba);
        let convert = median_ms(7, || fast_rgba_to_rgb(&rgba));
        let (rgb, _, _) = fast_rgba_to_rgb(&rgba);
        let encode = median_ms(5, || encode_jpeg(&rgb, 55).expect("encode"));
        let size = encode_jpeg(&rgb, 55).expect("encode").len() as f64 / 1024.0;
        let total = convert + encode;
        println!(
            "{:<18} {:>12} {:>8} {:>10.2} {:>10.2} {:>10.2} {:>9.1} {:>10.1}",
            label,
            format!("{out_w}x{out_h}"),
            if out_w == width { "no" } else { "HALF" },
            convert,
            encode,
            total,
            1000.0 / total,
            size
        );
    }
    println!("\nfps_cap = single-core ceiling if nothing else ran; 15 fps target needs total <= 66.7 ms.");
}

/// Which stage owns the frame budget.
fn stage_attribution() {
    heading("2. STAGE ATTRIBUTION at 1920x1080 (q55)");
    let rgba = rgba_fixture(1920, 1080);
    let rgb = rgb_fixture(1920, 1080);

    let hash = median_ms(40, || compute_block_hashes(rgba.as_raw(), 1920, 1080));
    let convert = median_ms(15, || fast_rgba_to_rgb(&rgba));
    let encode = median_ms(10, || encode_jpeg(&rgb, 55).expect("encode"));
    let total = hash + convert + encode;

    for (stage, ms) in [
        ("dirty-check block hash", hash),
        ("RGBA -> RGB", convert),
        ("JPEG encode q55", encode),
    ] {
        println!(
            "{:<20} {:>8.3} ms   {:>6.1}%",
            stage,
            ms,
            ms / total * 100.0
        );
    }
    println!("{:<20} {:>8.3} ms", "TOTAL per frame", total);

    let bytes_read = rgba.as_raw().len() as f64;
    let bytes_written = 1920.0 * 1080.0 * 3.0;
    println!(
        "\nRGBA->RGB effective bandwidth: {:.2} GB/s ({} MB read + {} MB written per frame)",
        (bytes_read + bytes_written) / (convert / 1000.0) / 1e9,
        bytes_read / 1e6,
        bytes_written / 1e6
    );
}

/// The dirty-frame check uses 64x64 word-wise block hashes. This measures how
/// often a realistic small UI change is actually noticed.
fn dedup_detection_rate() {
    heading("3. DIRTY-FRAME DETECTION RATE (word-wise 64x64 block hashes)");
    let width = 1920_u32;
    let height = 1080_u32;
    let base = rgba_fixture(width, height);
    let base_hashes = compute_block_hashes(base.as_raw(), width, height);
    let raw_len = base.as_raw().len();
    println!(
        "frame 1920x1080 RGBA = {} bytes; 64x64 tiles = {} blocks (word-wise hashed)",
        raw_len,
        base_hashes.len(),
    );

    // A localized change: draw a filled rectangle at a deterministic series of
    // positions and count how often the hash notices it.
    for (label, rect_w, rect_h) in [
        ("caret 2x18 (text cursor)", 2_u32, 18_u32),
        ("clock digit 10x16", 10, 16),
        ("taskbar badge 16x16", 16, 16),
        ("cursor 32x32", 32, 32),
        ("window band 400x200", 400, 200),
    ] {
        let mut detected = 0_usize;
        let trials = 64_usize;
        for trial in 0..trials {
            let mut frame = base.clone();
            let origin_x = ((trial * 137) % (width as usize - rect_w as usize - 1)) as u32;
            let origin_y = ((trial * 71) % (height as usize - rect_h as usize - 1)) as u32;
            for y in origin_y..origin_y + rect_h {
                for x in origin_x..origin_x + rect_w {
                    let pixel = frame.get_pixel_mut(x, y);
                    // Keep alpha intact; change the sampled channel destructively.
                    pixel.0[0] = !pixel.0[0];
                    pixel.0[1] = !pixel.0[1];
                    pixel.0[2] = !pixel.0[2];
                }
            }
            if compute_block_hashes(frame.as_raw(), width, height) != base_hashes {
                detected += 1;
            }
        }
        println!(
            "{:<24} detected {:>3}/{trials}  = {:>5.1}%  -> worst-case visual staleness {:>5} ms",
            label,
            detected,
            detected as f64 / trials as f64 * 100.0,
            if detected == trials { 0 } else { 1000 }
        );
    }

    let keepalive = Duration::from_millis(1000);
    println!(
        "\nkeepalive decision when undetected: should_send_frame(same,same,999ms,1s) = {}",
        should_send_frame(base_hashes[0], base_hashes[0], Duration::from_millis(999), keepalive)
    );
}

/// Cost of one `/desktop/frame` poll, decomposed, plus the resulting budget at
/// the frontend's 35 ms poll cadence.
fn frame_delivery_cost() {
    heading("4. FRAME DELIVERY COST per HTTP poll, decomposed");
    let rgb = rgb_fixture(1920, 1080);
    let jpeg = encode_jpeg(&rgb, 55).expect("encode");
    let jpeg_len = jpeg.len();

    let registry = Arc::new(TerminalRegistry::new());
    let router = McpRouter::new(Arc::clone(&registry));
    router.handle_desktop_frame_binary(
        "probe",
        BinaryDesktopFrame::new(0, 1920, 1080, 1_700_000_000_000, jpeg.clone()),
    );
    let runtime = Builder::new_current_thread().build().expect("runtime");

    let clone_ms = median_ms(20, || jpeg.clone());
    let base64_ms = median_ms(20, || base64::prelude::BASE64_STANDARD.encode(&jpeg));
    let accessor_ms = median_ms(20, || {
        runtime
            .block_on(router.get_latest_desktop_frame("probe"))
            .expect("cached")
    });
    let raw_accessor_ms = median_ms(20, || {
        runtime
            .block_on(router.get_latest_desktop_frame_raw("probe"))
            .expect("cached")
    });

    let json_body = serde_json::json!({
        "success": true,
        "display_index": 0,
        "width": 1920,
        "height": 1080,
        "format": "jpeg",
        "data": base64::prelude::BASE64_STANDARD.encode(&jpeg),
        "timestamp": 1_700_000_000_000_u64,
    });
    let json_ms = median_ms(10, || serde_json::to_vec(&json_body).expect("serialize"));
    let json_len = serde_json::to_vec(&json_body).expect("serialize").len();

    println!("JPEG payload                       {:>9} bytes", jpeg_len);
    println!(
        "base64 payload                     {:>9} bytes  (+{:.0}%)",
        jpeg_len * 4 / 3,
        (jpeg_len as f64 * 4.0 / 3.0 / jpeg_len as f64 - 1.0) * 100.0
    );
    println!("JSON response body                 {:>9} bytes", json_len);
    println!();
    for (stage, ms) in [
        ("frame clone (Vec<u8>)", clone_ms),
        ("base64 encode", base64_ms),
        ("production accessor (clone+base64)", accessor_ms),
        ("production raw accessor (clone only)", raw_accessor_ms),
        ("serde_json body serialize", json_ms),
    ] {
        println!("{:<38} {:>8.3} ms", stage, ms);
    }

    let per_poll = accessor_ms + json_ms;
    let polls_per_second = 1000.0 / 35.0;
    println!("\nper poll (accessor + json)          {:>8.3} ms", per_poll);
    println!(
        "frontend poll cadence               {:>8.1} polls/s per viewer",
        polls_per_second
    );
    println!(
        "serialized work per viewer          {:>8.1} ms/s  ({:.2}% of one core)",
        per_poll * polls_per_second,
        per_poll * polls_per_second / 10.0
    );
    println!(
        "allocated+copied per viewer         {:>8.1} MB/s",
        (json_len as f64 * polls_per_second) / 1e6
    );
    for viewers in [1_usize, 3, 5] {
        println!(
            "{:>2} concurrent viewers              {:>8.2} cores of serialized + {:.0} MB/s on the wire",
            viewers,
            per_poll * polls_per_second * viewers as f64 / 1000.0,
            (json_len as f64 * polls_per_second * viewers as f64) / 1e6
        );
    }
    println!("\nraw endpoint alternative (no base64, no JSON, browser-native JPEG decode):");
    println!(
        "{:<38} {:>8.3} ms   -> {:.2} MB/s per viewer, allocated per poll {} bytes",
        "raw accessor only",
        raw_accessor_ms,
        (jpeg_len as f64 * polls_per_second) / 1e6,
        jpeg_len
    );
}

/// Registry write path scaling, measured outside Criterion for a compact table.
fn registry_write_scaling() {
    heading("5. REGISTRY WRITE PATH SCALING (snapshot clone per mutation)");
    let runtime = Builder::new_current_thread().build().expect("runtime");
    println!(
        "{:>10} {:>16} {:>16} {:>16}",
        "terminals", "update_meta_ms", "list_ms", "register_ms"
    );

    for terminals in [10_usize, 100, 1000, 5000] {
        let registry = TerminalRegistry::new();
        let mut receivers = Vec::with_capacity(terminals);
        runtime.block_on(async {
            for index in 0..terminals {
                let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
                registry.register(terminal_info(index), sender).await;
                receivers.push(receiver);
            }
        });

        let update = median_ms(9, || {
            runtime.block_on(async {
                registry
                    .update_terminal_meta("terminal-00000", Some("renamed".into()), None, None)
                    .await
                    .expect("update")
            })
        });
        let list = median_ms(9, || runtime.block_on(registry.list_terminals()));
        let register = median_ms(9, || {
            runtime.block_on(async {
                let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
                registry.register(terminal_info(0), sender).await;
            })
        });

        println!(
            "{:>10} {:>16.3} {:>16.3} {:>16.3}",
            terminals, update, list, register
        );
    }
    println!("\nupdate_meta/register persist a full-map clone per call; growth with terminal count is expected.");
}

/// Prototype conversions, to quantify how much headroom the production
/// RGBA->RGB loop leaves on the table.
fn conversion_headroom() {
    heading("6. RGBA->RGB CONVERSION HEADROOM (production vs prototypes)");
    // Production `fast_rgba_to_rgb` targets DEFAULT_TARGET_WIDTH (1280x720).
    // Test conversion headroom at this target geometry so production and prototypes
    // operate on the same pixel dimensions without geometry mismatch.
    let rgba = rgba_fixture(1280, 720);
    let production = median_ms(15, || fast_rgba_to_rgb(&rgba));
    let chunked = median_ms(15, || rgba_to_rgb_chunked(&rgba));
    let row_wise = median_ms(15, || rgba_to_rgb_rowwise(&rgba));

    let (reference, _, _) = fast_rgba_to_rgb(&rgba);
    for (name, candidate) in [
        ("chunked extend_from_slice", rgba_to_rgb_chunked(&rgba)),
        ("row-wise into preallocated", rgba_to_rgb_rowwise(&rgba)),
    ] {
        assert_eq!(
            (candidate.width(), candidate.height()),
            (reference.width(), reference.height()),
            "{name} geometry must match production"
        );
        assert_eq!(
            candidate.as_raw(),
            reference.as_raw(),
            "{name} must be byte-identical to production"
        );
    }

    println!("{:<34} {:>10} {:>10}", "implementation", "ms", "speedup");
    println!(
        "{:<34} {:>10.3} {:>10}",
        "production fast_rgba_to_rgb", production, "1.00x"
    );
    println!(
        "{:<34} {:>10.3} {:>10.2}x",
        "prototype: chunked extend_from_slice",
        chunked,
        production / chunked
    );
    println!(
        "{:<34} {:>10.3} {:>10.2}x",
        "prototype: row-wise preallocated",
        row_wise,
        production / row_wise
    );
    println!("\nAll prototypes verified byte-identical to the production output.");
}

/// Prototype 1: copy three bytes per pixel with one slice call.
fn rgba_to_rgb_chunked(rgba: &RgbaImage) -> RgbImage {
    let raw = rgba.as_raw();
    let mut out = Vec::with_capacity(raw.len() / 4 * 3);
    for pixel in raw.chunks_exact(4) {
        out.extend_from_slice(&pixel[..3]);
    }
    RgbImage::from_raw(rgba.width(), rgba.height(), out).expect("geometry preserved")
}

/// Prototype 2: write rows directly into an exactly-sized buffer.
fn rgba_to_rgb_rowwise(rgba: &RgbaImage) -> RgbImage {
    let (width, height) = (rgba.width() as usize, rgba.height() as usize);
    let raw = rgba.as_raw();
    let mut out = vec![0_u8; width * height * 3];
    for y in 0..height {
        let src = &raw[y * width * 4..(y + 1) * width * 4];
        let dst = &mut out[y * width * 3..(y + 1) * width * 3];
        for (pixel, target) in src.chunks_exact(4).zip(dst.chunks_exact_mut(3)) {
            target.copy_from_slice(&pixel[..3]);
        }
    }
    RgbImage::from_raw(width as u32, height as u32, out).expect("geometry preserved")
}

/// Real capture timing, if this machine grants capture permission.
fn capture_probe() {
    heading("7. REAL SCREEN CAPTURE (platform capture path)");
    match xcap::Monitor::all() {
        Ok(monitors) => {
            println!("monitors enumerated: {}", monitors.len());
            for monitor in &monitors {
                println!(
                    "  id={:?} name={:?} {:?}x{:?} scale={:?}",
                    monitor.id(),
                    monitor.name(),
                    monitor.width(),
                    monitor.height(),
                    monitor.scale_factor()
                );
            }
            let enumeration = median_ms(9, || xcap::Monitor::all().map(|m| m.len()));
            println!("\nMonitor::all() enumeration median: {enumeration:.3} ms");

            if let Some(monitor) = monitors.first() {
                match monitor.capture_image() {
                    Ok(image) => {
                        println!(
                            "capture_image() first frame: {}x{}",
                            image.width(),
                            image.height()
                        );
                        let capture = median_ms(9, || {
                            monitor.capture_image().map(|i| i.width()).unwrap_or(0)
                        });
                        println!("capture_image() median: {capture:.3} ms");
                        let samples = 5_usize;
                        println!("\nfirst {samples} consecutive captures (ms):");
                        for index in 0..samples {
                            let start = Instant::now();
                            let _ = monitor.capture_image();
                            println!("  #{} {:>8.3} ms", index + 1, start.elapsed().as_secs_f64() * 1000.0);
                        }

                        // End-to-end budget on a real frame: capture -> policy
                        // scaling -> encode, with realistic payload sizes.
                        if let Ok(frame) = monitor.capture_image() {
                            let (_, out_w, out_h) = fast_rgba_to_rgb(&frame);
                            println!(
                                "\nreal-frame end-to-end ({}x{} source -> {out_w}x{out_h} after policy):",
                                frame.width(),
                                frame.height()
                            );
                            println!(
                                "{:<10} {:>12} {:>12} {:>12} {:>12} {:>10}",
                                "quality", "payload_KB", "capture_ms", "convert_ms", "encode_ms", "fps_cap"
                            );
                            for quality in [40_u8, 55, 70, 85] {
                                let capture = median_ms(5, || {
                                    monitor.capture_image().map(|i| i.width()).unwrap_or(0)
                                });
                                let convert = median_ms(5, || fast_rgba_to_rgb(&frame));
                                let (rgb, _, _) = fast_rgba_to_rgb(&frame);
                                let encode = median_ms(5, || {
                                    encode_jpeg(&rgb, quality).expect("encode")
                                });
                                let payload = encode_jpeg(&rgb, quality).expect("encode").len();
                                let total = capture + convert + encode;
                                println!(
                                    "{:<10} {:>12.1} {:>12.2} {:>12.2} {:>12.2} {:>10.1}",
                                    format!("q{quality}"),
                                    payload as f64 / 1024.0,
                                    capture,
                                    convert,
                                    encode,
                                    1000.0 / total
                                );
                            }
                        }
                    }
                    Err(error) => println!(
                        "capture_image() unavailable: {error}\n  (screen recording permission or headless session)"
                    ),
                }
            }
        }
        Err(error) => println!("Monitor::all() unavailable: {error}"),
    }
}

fn terminal_info(index: usize) -> at_pc_protocol::models::TerminalInfo {
    at_pc_protocol::models::TerminalInfo {
        terminal_id: format!("terminal-{index:05}"),
        hostname: format!("host-{index:05}"),
        username: "probe".to_string(),
        lan_ip: "10.0.0.1".to_string(),
        os_version: "probe-os".to_string(),
        agent_version: "probe-agent".to_string(),
    }
}

/// Control path: frame framing copies and the per-input-event audit record.
fn control_path_cost() {
    heading("8. CONTROL PATH: frame framing copies and per-input audit cost");
    let payload = vec![0x5a_u8; 60_000];
    let frame = BinaryDesktopFrame::new(0, 1280, 800, 1_700_000_000_000, payload.clone());

    let encode = median_ms(20, || frame.encode());
    let encoded = frame.encode();
    let decode = median_ms(20, || BinaryDesktopFrame::decode(&encoded).expect("decode"));
    println!(
        "{:<40} {:>8.4} ms  ({} B payload)",
        "BinaryDesktopFrame::encode (agent, copy)",
        encode,
        payload.len()
    );
    println!(
        "{:<40} {:>8.4} ms  (server, copy)",
        "BinaryDesktopFrame::decode", decode
    );

    let path = std::env::temp_dir().join(format!("atpc_probe_audit_{}.jsonl", std::process::id()));
    let logger = at_pc_server::AuditLogger::new(Some(path.clone()));
    let runtime = Builder::new_current_thread().build().expect("runtime");
    let record = || at_pc_server::AuditRecord {
        id: "audit-probe".to_string(),
        timestamp: "2026-09-15T00:00:00Z".to_string(),
        role: Some("admin".to_string()),
        token_prefix: Some("dev***".to_string()),
        client_ip: Some("127.0.0.1".to_string()),
        terminal_id: Some("terminal-00000".to_string()),
        action: "api:desktop_input".to_string(),
        tool_name: None,
        arguments: None,
        status: "SUCCESS".to_string(),
        error: None,
        duration_ms: Some(0),
    };

    let per_record = median_ms(20, || {
        runtime.block_on(async {
            for _ in 0..50 {
                logger.log_async(record()).await;
            }
        })
    }) / 50.0;
    let events_per_second = 1000.0 / 35.0; // frontend mouse-move throttle
    println!(
        "\n{:<40} {:>8.4} ms",
        "audit log_async per record", per_record
    );
    println!(
        "at {events_per_second:.1} input events/s -> {:.3} ms/s of audit admission ({:.2}% of one core)",
        per_record * events_per_second,
        per_record * events_per_second / 10.0
    );

    let flush = median_ms(10, || runtime.block_on(logger.flush_async()));
    println!("{:<40} {:>8.4} ms", "audit flush (barrier + fsync)", flush);
    let tail = median_ms(10, || runtime.block_on(logger.read_recent_async(100)));
    println!(
        "{:<40} {:>8.4} ms",
        "audit read_recent(100) as /api/audit", tail
    );
    runtime.block_on(logger.shutdown_async()).ok();
    let _ = std::fs::remove_file(path);
}

fn main() {
    println!("at-pc performance probe");
    resolution_policy_table();
    stage_attribution();
    dedup_detection_rate();
    frame_delivery_cost();
    registry_write_scaling();
    conversion_headroom();
    control_path_cost();
    capture_probe();
}
