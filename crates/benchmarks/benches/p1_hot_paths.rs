use at_pc_desktop_core::{compute_block_hashes, encode_jpeg, fast_rgba_to_rgb, should_send_frame};
use at_pc_protocol::models::TerminalInfo;
use at_pc_server::ws::registry::TerminalRegistry;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use image::{Rgb, RgbImage, Rgba, RgbaImage};
use std::time::Duration;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::mpsc;

fn rgba_fixture(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        Rgba([
            (x.wrapping_mul(13) ^ y.wrapping_mul(7)) as u8,
            (x.wrapping_add(y.wrapping_mul(3))) as u8,
            (x.wrapping_mul(5).wrapping_add(y.wrapping_mul(11))) as u8,
            255,
        ])
    })
}

fn rgb_fixture(width: u32, height: u32) -> RgbImage {
    RgbImage::from_fn(width, height, |x, y| {
        Rgb([
            (x.wrapping_mul(13) ^ y.wrapping_mul(7)) as u8,
            (x.wrapping_add(y.wrapping_mul(3))) as u8,
            (x.wrapping_mul(5).wrapping_add(y.wrapping_mul(11))) as u8,
        ])
    })
}

fn frame_benches(c: &mut Criterion) {
    let mut conversion = c.benchmark_group("frame/rgba_to_rgb");
    for (label, width, height) in [
        ("1920x1080_copy", 1920, 1080),
        ("3840x2160_half_scale", 3840, 2160),
    ] {
        let rgba = rgba_fixture(width, height);
        conversion.throughput(Throughput::Elements(u64::from(width) * u64::from(height)));
        conversion.bench_function(label, |b| b.iter(|| fast_rgba_to_rgb(black_box(&rgba))));
    }
    conversion.finish();

    let mut jpeg = c.benchmark_group("frame/jpeg_encode");
    for (width, height, quality) in [(1280, 720, 55), (1920, 1080, 60), (3840, 2160, 85)] {
        let rgb = rgb_fixture(width, height);
        jpeg.throughput(Throughput::Elements(u64::from(width) * u64::from(height)));
        jpeg.bench_with_input(
            BenchmarkId::new(format!("{width}x{height}"), format!("q{quality}")),
            &quality,
            |b, quality| b.iter(|| encode_jpeg(black_box(&rgb), black_box(*quality)).unwrap()),
        );
    }
    jpeg.finish();

    let rgba = rgba_fixture(1920, 1080);
    let mut change = c.benchmark_group("frame/change_detection");
    change.throughput(Throughput::Bytes(rgba.as_raw().len() as u64));
    change.bench_function("block_hash_1920x1080_rgba", |b| {
        b.iter(|| compute_block_hashes(black_box(rgba.as_raw()), black_box(1920), black_box(1080)))
    });
    change.throughput(Throughput::Elements(1));
    for (label, current, previous, elapsed) in [
        ("decision_changed", 2, 1, Duration::ZERO),
        ("decision_static_skip", 1, 1, Duration::from_millis(999)),
        ("decision_static_keepalive", 1, 1, Duration::from_secs(1)),
    ] {
        change.bench_function(label, |b| {
            b.iter(|| {
                should_send_frame(
                    black_box(current),
                    black_box(previous),
                    black_box(elapsed),
                    black_box(Duration::from_secs(1)),
                )
            })
        });
    }
    change.finish();
}

struct RegistryFixture {
    runtime: Runtime,
    registry: TerminalRegistry,
    _receivers: Vec<mpsc::UnboundedReceiver<at_pc_protocol::messages::ServerToAgentMessage>>,
}

fn terminal_info(index: usize) -> TerminalInfo {
    TerminalInfo {
        terminal_id: format!("terminal-{index:05}"),
        hostname: format!("host-{index:05}"),
        username: "benchmark".to_string(),
        lan_ip: format!("10.0.{}.{}", (index / 254) % 254, index % 254 + 1),
        os_version: "benchmark-os".to_string(),
        agent_version: "benchmark-agent".to_string(),
    }
}

fn registry_fixture(active: usize, offline: usize) -> RegistryFixture {
    let runtime = Builder::new_current_thread().build().unwrap();
    let registry = TerminalRegistry::new();
    let mut receivers = Vec::with_capacity(active);

    runtime.block_on(async {
        for index in 0..active {
            let (sender, receiver) = mpsc::unbounded_channel();
            registry.register(terminal_info(index), sender).await;
            receivers.push(receiver);
        }
        for index in active..active + offline {
            registry
                .meta_store()
                .record_registration(&terminal_info(index))
                .await;
        }
    });

    RegistryFixture {
        runtime,
        registry,
        _receivers: receivers,
    }
}

fn registry_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("registry/list");
    for (label, active, offline) in [
        ("empty", 0, 0),
        ("active_100", 100, 0),
        ("active_1000", 1000, 0),
        ("mixed_500_active_500_offline", 500, 500),
    ] {
        let fixture = registry_fixture(active, offline);
        group.throughput(Throughput::Elements((active + offline) as u64));
        group.bench_function(label, |b| {
            b.iter(|| black_box(fixture.runtime.block_on(fixture.registry.list_terminals())))
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    targets = frame_benches, registry_benches
}
criterion_main!(benches);
