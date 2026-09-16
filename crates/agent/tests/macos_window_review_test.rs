//! Integration tests for Milestone 2 Task 8:
//! macOS direct `CGWindowListCopyWindowInfo` foreground window capture & latency check.

#[cfg(target_os = "macos")]
mod macos_tests {
    use at_pc_agent::tools::window::{capture_active_window_state, reset_window_mocks};
    use std::time::Instant;

    #[test]
    fn test_macos_direct_window_review_latency() {
        reset_window_mocks();

        // Warm-up run to ensure framework caches / symbols are primed
        let _ = capture_active_window_state();

        let mut samples = Vec::new();
        let mut last_state = None;
        for _ in 0..5 {
            let start = Instant::now();
            let state = capture_active_window_state();
            samples.push(start.elapsed());
            last_state = state;
        }
        samples.sort();
        let median = samples[samples.len() / 2];

        println!(
            "capture_active_window_state() median elapsed: {:?}, state: {:?}",
            median, last_state
        );

        assert!(
            last_state.is_some(),
            "Expected capture_active_window_state() to return Some on macOS"
        );

        let win = last_state.unwrap();
        assert!(!win.title.is_empty(), "Window title must not be empty");

        // Latency requirement: in release must be < 5ms (typically 0.3~0.6ms); in debug unoptimized < 15ms.
        let max_allowed_ms = if cfg!(debug_assertions) { 15 } else { 5 };
        assert!(
            median.as_millis() < max_allowed_ms,
            "capture_active_window_state median took {:?}, expected < {}ms",
            median,
            max_allowed_ms
        );
    }
}
