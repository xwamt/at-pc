use at_pc_server::router::McpRouter;
use at_pc_server::ws::registry::TerminalRegistry;
use std::sync::Arc;

#[tokio::test]
async fn test_multi_viewer_refcounting() {
    let registry = Arc::new(TerminalRegistry::new());
    let router = McpRouter::new(registry);

    assert_eq!(router.get_desktop_stream_subscribers("term-multi"), 0);

    let rx1 = router.subscribe_desktop_frames("term-multi");
    assert_eq!(router.get_desktop_stream_subscribers("term-multi"), 1);

    let rx2 = router.subscribe_desktop_frames("term-multi");
    assert_eq!(router.get_desktop_stream_subscribers("term-multi"), 2);

    drop(rx1);
    assert_eq!(router.get_desktop_stream_subscribers("term-multi"), 1);

    drop(rx2);
    assert_eq!(router.get_desktop_stream_subscribers("term-multi"), 0);
}
