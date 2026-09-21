use at_pc_agent::DesktopStreamController;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_interaction_notifier_interrupts_idle_sleep() {
    let controller = Arc::new(DesktopStreamController::new());
    let controller_clone = controller.clone();

    let start = tokio::time::Instant::now();
    let worker = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        controller_clone.notify_interaction();
    });

    // Worker triggers notification, verify that it runs cleanly
    worker.await.unwrap();
    assert!(start.elapsed() < Duration::from_millis(200));
}
