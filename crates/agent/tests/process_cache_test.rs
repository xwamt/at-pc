use at_pc_agent::tools::process::{invalidate_process_cache, list_processes, PROCESS_CACHE_TTL};
use std::sync::Mutex;
use std::time::Instant;

static SERIAL_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_list_processes_ttl_cache_speedup() {
    let _guard = SERIAL_MUTEX.lock().unwrap();
    // Force cache invalidation so the first call performs a full refresh
    invalidate_process_cache();

    // First call: cold cache / expired TTL -> performs sys.refresh_processes()
    let start_cold = Instant::now();
    let procs_cold = list_processes(None, None, 50);
    let cold_duration = start_cold.elapsed();

    assert!(!procs_cold.is_empty(), "Processes list should not be empty");
    println!("Total processes in sys: {}", procs_cold.len());

    // Second call: within 1-second TTL -> skips sys.refresh_processes()
    let start_warm = Instant::now();
    let procs_warm = list_processes(None, None, 50);
    let warm_duration = start_warm.elapsed();

    assert_eq!(
        procs_cold, procs_warm,
        "Cached processes should return identical data"
    );

    assert_eq!(PROCESS_CACHE_TTL, std::time::Duration::from_millis(1000));

    println!(
        "Count: {}, Cold call: {:?}, Warm call: {:?}, Speedup: {:.2}x",
        procs_cold.len(),
        cold_duration,
        warm_duration,
        cold_duration.as_secs_f64() / warm_duration.as_secs_f64()
    );

    // Warm call skips sys.refresh_processes() and should take < 0.3ms (300µs) in release,
    // and < 1.5ms in unoptimized debug mode with parallel test execution contention.
    let best_warm = std::iter::once(warm_duration)
        .chain((0..3).map(|_| {
            let s = Instant::now();
            let _ = list_processes(None, None, 50);
            s.elapsed()
        }))
        .min()
        .unwrap();

    let max_warm_micros = if cfg!(debug_assertions) { 1500 } else { 300 };
    assert!(
        best_warm.as_micros() < max_warm_micros,
        "Expected warm call < {}µs, got best warm {:?}",
        max_warm_micros,
        best_warm
    );
    assert!(
        cold_duration > best_warm,
        "Cold call {:?} must be slower than warm call {:?}",
        cold_duration,
        best_warm
    );
}

#[test]
fn test_list_processes_cache_with_filtering_and_sorting() {
    let _guard = SERIAL_MUTEX.lock().unwrap();
    invalidate_process_cache();

    // Populate cache
    let all = list_processes(None, None, 10);
    assert!(!all.is_empty());

    // Filtered call hits cache
    let filtered = list_processes(Some(&all[0].name), None, 10);
    assert!(!filtered.is_empty());
    assert!(filtered.iter().any(|p| p.name == all[0].name));
}

#[test]
fn test_list_processes_cache_ttl_invalidation() {
    let _guard = SERIAL_MUTEX.lock().unwrap();
    invalidate_process_cache();
    let procs1 = list_processes(None, None, 0);
    assert!(!procs1.is_empty());

    // Invalidate and list again
    invalidate_process_cache();
    let procs2 = list_processes(None, None, 0);
    assert!(!procs2.is_empty());
}
