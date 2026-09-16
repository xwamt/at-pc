//! P2 deep-dive performance benchmarks.
//!
//! These target the dimensions the P1-7 baseline deliberately excluded from
//! `p1_hot_paths`: full per-frame pipeline cost, the resolution-scaling policy,
//! registry write-path scaling, and the real cost of delivering one desktop
//! frame to a dashboard viewer.
//!
//! Every benchmark calls production code. Nothing here is a re-implementation.

use at_pc_desktop_core::{compute_block_hashes, encode_jpeg, fast_rgba_to_rgb};
use at_pc_protocol::messages::BinaryDesktopFrame;
use at_pc_protocol::models::TerminalInfo;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::handler::AgentMessageHandler;
use at_pc_server::ws::registry::TerminalRegistry;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use image::{Rgb, RgbImage, Rgba, RgbaImage};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::mpsc;

/// Synthetic desktop frame: gradients plus high-frequency detail, so JPEG
/// payload sizes land in the same order of magnitude as a real desktop.
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

/// Full single-frame CPU work as the agent performs it: dirty check, RGBA->RGB,
/// JPEG encode. The inverse of the measured time is the single-core fps ceiling.
fn pipeline_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame/pipeline_full");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(5));

    for (label, width, height) in [
        ("1920x1080", 1920, 1080),
        ("2560x1440", 2560, 1440),
        ("3840x2160", 3840, 2160),
    ] {
        let rgba = rgba_fixture(width, height);
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::from_parameter(label), &rgba, |b, rgba| {
            b.iter(|| {
                let hash = compute_block_hashes(
                    black_box(rgba.as_raw()),
                    black_box(rgba.width()),
                    black_box(rgba.height()),
                );
                let (rgb, _, _) = fast_rgba_to_rgb(black_box(rgba));
                let jpeg = encode_jpeg(black_box(&rgb), 55).expect("jpeg encode");
                black_box((hash, jpeg.len()))
            })
        });
    }
    group.finish();
}

/// Stage attribution at 1920x1080 so the dominant cost is unambiguous.
fn attribution_benches(c: &mut Criterion) {
    let rgba = rgba_fixture(1920, 1080);
    let rgb = rgb_fixture(1920, 1080);

    let mut group = c.benchmark_group("frame/attribution_1080p");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(4));

    group.throughput(Throughput::Bytes(rgba.as_raw().len() as u64));
    group.bench_function("stage_hash", |b| {
        b.iter(|| compute_block_hashes(black_box(rgba.as_raw()), black_box(1920), black_box(1080)))
    });

    group.throughput(Throughput::Elements(1920 * 1080));
    group.bench_function("stage_rgba_to_rgb", |b| {
        b.iter(|| fast_rgba_to_rgb(black_box(&rgba)))
    });
    group.throughput(Throughput::Elements(1920 * 1080));
    group.bench_function("stage_jpeg_encode_q55", |b| {
        b.iter(|| encode_jpeg(black_box(&rgb), 55).expect("jpeg encode"))
    });
    group.finish();
}

/// Output geometry and cost for resolutions around the 1920 px policy boundary.
fn resolution_policy_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame/resolution_policy");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(4));

    for (label, width, height) in [
        ("1920x1080_no_scale", 1920, 1080),
        ("1921x1080_just_over", 1921, 1080),
        ("2560x1440_half", 2560, 1440),
        ("3440x1440_half", 3440, 1440),
        ("3840x2160_half", 3840, 2160),
    ] {
        let rgba = rgba_fixture(width, height);
        group.throughput(Throughput::Elements(u64::from(width) * u64::from(height)));
        group.bench_with_input(BenchmarkId::from_parameter(label), &rgba, |b, rgba| {
            b.iter(|| fast_rgba_to_rgb(black_box(rgba)))
        });
    }
    group.finish();
}

/// JPEG quality sweep: encoder time and resulting payload size.
fn quality_sweep_benches(c: &mut Criterion) {
    let rgb = rgb_fixture(1920, 1080);
    let mut group = c.benchmark_group("frame/jpeg_quality_1080p");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(4));
    group.throughput(Throughput::Elements(1920 * 1080));

    for quality in [40_u8, 55, 70, 85, 95] {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("q{quality}")),
            &quality,
            |b, quality| {
                b.iter(|| encode_jpeg(black_box(&rgb), black_box(*quality)).expect("encode"))
            },
        );
    }
    group.finish();
}

/// Registry write path. `snapshot_locked` clones the whole record map on every
/// mutation, so write cost is expected to grow with the number of known
/// terminals. These cases hold terminal count fixed and vary it across cases.
fn registry_write_benches(c: &mut Criterion) {
    let runtime = Builder::new_current_thread().build().expect("runtime");
    let mut group = c.benchmark_group("registry/write_path");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(4));

    for terminals in [10_usize, 100, 1000, 5000] {
        let registry = TerminalRegistry::new();
        let mut receivers = Vec::with_capacity(terminals);
        runtime.block_on(async {
            for index in 0..terminals {
                let (sender, receiver) = mpsc::unbounded_channel();
                registry.register(terminal_info(index), sender).await;
                receivers.push(receiver);
            }
        });

        group.throughput(Throughput::Elements(terminals as u64));
        group.bench_with_input(
            BenchmarkId::new("update_meta", terminals),
            &terminals,
            |b, terminals| {
                let mut counter = 0_usize;
                b.iter(|| {
                    counter = counter.wrapping_add(1);
                    let target = counter % terminals;
                    runtime.block_on(async {
                        registry
                            .update_terminal_meta(
                                &format!("terminal-{target:05}"),
                                Some("renamed".to_string()),
                                None,
                                None,
                            )
                            .await
                            .expect("meta update");
                    })
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("list_terminals", terminals),
            &terminals,
            |b, _| b.iter(|| black_box(runtime.block_on(registry.list_terminals()))),
        );

        drop(receivers);
        drop(registry);
    }
    group.finish();
}

/// Cost of serving one `/api/terminals/:id/desktop/frame` response, measured
/// through the production accessor. Includes the frame clone and the base64
/// encoding that the handler performs on every request.
fn frame_delivery_benches(c: &mut Criterion) {
    const QUALITY: u8 = 55;
    let runtime: Runtime = Builder::new_current_thread().build().expect("runtime");
    let rgb = rgb_fixture(1920, 1080);
    let jpeg = encode_jpeg(&rgb, QUALITY).expect("jpeg encode");
    let payload_bytes = jpeg.len();

    let registry = Arc::new(TerminalRegistry::new());
    let router = McpRouter::new(Arc::clone(&registry));
    router.handle_desktop_frame_binary(
        "probe-terminal",
        BinaryDesktopFrame::new(0, 1920, 1080, 1_700_000_000_000, jpeg.clone()),
    );

    let mut group = c.benchmark_group("frame/delivery_per_request");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(4));
    group.throughput(Throughput::Bytes(payload_bytes as u64));

    group.bench_function("get_latest_desktop_frame_base64", |b| {
        b.iter(|| {
            let frame = runtime
                .block_on(router.get_latest_desktop_frame(black_box("probe-terminal")))
                .expect("frame cached");
            black_box(frame.data.len())
        })
    });

    group.throughput(Throughput::Bytes(payload_bytes as u64));
    group.bench_function("get_latest_desktop_frame_raw", |b| {
        b.iter(|| {
            let frame = runtime
                .block_on(router.get_latest_desktop_frame_raw(black_box("probe-terminal")))
                .expect("frame cached");
            black_box(frame.4.len())
        })
    });

    group.finish();
}

fn terminal_info(index: usize) -> TerminalInfo {
    TerminalInfo {
        terminal_id: format!("terminal-{index:05}"),
        hostname: format!("host-{index:05}"),
        username: "benchmark".to_string(),
        lan_ip: "10.0.0.1".to_string(),
        os_version: "benchmark-os".to_string(),
        agent_version: "benchmark-agent".to_string(),
    }
}

criterion_group! {
    name = deep_benches;
    config = Criterion::default();
    targets =
        pipeline_benches,
        attribution_benches,
        resolution_policy_benches,
        quality_sweep_benches,
        registry_write_benches,
        frame_delivery_benches
}
criterion_main!(deep_benches);
