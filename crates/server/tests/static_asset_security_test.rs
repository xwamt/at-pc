use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::response::Response;
use axum::Router;
use tower::ServiceExt;

use at_pc_server::config::ServerConfig;
use at_pc_server::mcp::create_mcp_http_router;
use at_pc_server::router::McpRouter;
use at_pc_server::ws::registry::TerminalRegistry;

const AUTH_TOKEN: &str = "static-assets-test-token";
const OUTSIDE_SECRET: &str = "outside-static-root-secret-6f0b7b31";
const SAFE_ASSET: &str = "safe-static-asset-18c2d84a";
const HOT_RELOAD_HTML: &str = "<html>debug-hot-reload-bootstrap-b1c50c8e</html>";

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "at-pc-static-security-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temporary test directory");
        Self { path }
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct EnvironmentGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvironmentGuard {
    fn set_path(key: &'static str, value: &Path) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn test_app() -> Router {
    let registry = Arc::new(TerminalRegistry::new());
    let router = Arc::new(McpRouter::new(registry));
    let config = ServerConfig {
        auth_token: Some(AUTH_TOKEN.to_string()),
        ..Default::default()
    };
    create_mcp_http_router(router, config)
}

async fn get(app: &Router, uri: &str, authenticated: bool) -> Response {
    let mut request = Request::builder().uri(uri).method("GET");
    if authenticated {
        request = request.header("Authorization", format!("Bearer {AUTH_TOKEN}"));
    }
    app.clone()
        .oneshot(request.body(Body::empty()).expect("build request"))
        .await
        .expect("route request")
}

async fn response_body(response: Response) -> Vec<u8> {
    to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read response body")
        .to_vec()
}

async fn assert_rejected_without_leak(app: &Router, uri: &str) {
    let response = get(app, uri, true).await;
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "unsafe static path was not rejected: {uri}"
    );
    let body = response_body(response).await;
    assert!(
        !body
            .windows(OUTSIDE_SECRET.len())
            .any(|window| window == OUTSIDE_SECRET.as_bytes()),
        "rejection leaked outside file contents for {uri}"
    );
}

#[tokio::test]
async fn static_assets_are_authenticated_and_confined_to_the_asset_root() {
    let sandbox = TestDirectory::new();
    let frontend = sandbox.path.join("frontend");
    fs::create_dir(&frontend).expect("create frontend directory");
    fs::write(frontend.join("safe.txt"), SAFE_ASSET).expect("write safe asset");
    fs::write(frontend.join("index.html"), HOT_RELOAD_HTML)
        .expect("write debug bootstrap sentinel");

    let outside_file = sandbox.path.join("outside-secret.txt");
    fs::write(&outside_file, OUTSIDE_SECRET).expect("write outside sentinel");

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside_file, frontend.join("outside-link.txt"))
            .expect("create escaping file symlink");
        let outside_directory = sandbox.path.join("outside-directory");
        fs::create_dir(&outside_directory).expect("create outside directory");
        fs::write(outside_directory.join("nested-secret.txt"), OUTSIDE_SECRET)
            .expect("write nested outside sentinel");
        std::os::unix::fs::symlink(&outside_directory, frontend.join("outside-directory-link"))
            .expect("create escaping directory symlink");
    }

    let _environment = EnvironmentGuard::set_path("AT_PC_FRONTEND_DIR", &frontend);
    let app = test_app();

    // Only the exact HTML bootstrap endpoints remain public. Disk hot-loading is debug-only.
    for uri in ["/", "/dashboard", "/index.html"] {
        let response = get(&app, uri, false).await;
        assert_eq!(response.status(), StatusCode::OK, "public bootstrap {uri}");
        let body = response_body(response).await;
        if cfg!(debug_assertions) {
            assert_eq!(body, HOT_RELOAD_HTML.as_bytes());
        } else {
            assert!(!body
                .windows(HOT_RELOAD_HTML.len())
                .any(|window| window == HOT_RELOAD_HTML.as_bytes()));
            assert!(String::from_utf8_lossy(&body).contains("Obsidian Edition"));
        }
    }

    let response = get(&app, "/static/index.html", false).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(!response_body(response)
        .await
        .windows(OUTSIDE_SECRET.len())
        .any(|window| window == OUTSIDE_SECRET.as_bytes()));

    // A valid embedded asset is available only after authentication.
    let response = get(&app, "/static/index.html", true).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/html; charset=utf-8"
    );

    // Disk hot-loading is a debug-only facility; release builds must use embedded assets.
    let response = get(&app, "/static/safe.txt", true).await;
    if cfg!(debug_assertions) {
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_body(response).await, SAFE_ASSET.as_bytes());
    } else {
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_ne!(response_body(response).await, SAFE_ASSET.as_bytes());
    }

    // Missing debug disk assets must safely fall back to the embedded bundle.
    fs::remove_file(frontend.join("index.html")).expect("remove debug index fixture");
    let response = get(&app, "/static/index.html", true).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(String::from_utf8_lossy(&response_body(response).await).contains("Obsidian Edition"));

    let absolute_path_uri = {
        #[cfg(unix)]
        {
            format!("/static/{}", outside_file.display())
        }
        #[cfg(windows)]
        {
            format!(
                "/static/{}",
                outside_file.to_string_lossy().replace('\\', "%5C")
            )
        }
    };
    let encoded_absolute_path_uri = {
        #[cfg(unix)]
        {
            format!(
                "/static/%2F{}",
                outside_file.display().to_string().trim_start_matches('/')
            )
        }
        #[cfg(windows)]
        {
            format!(
                "/static/{}",
                outside_file.to_string_lossy().replace('\\', "%5C")
            )
        }
    };

    let mut unsafe_uris = vec![
        "/static/../outside-secret.txt".to_string(),
        "/static/%2e%2e/outside-secret.txt".to_string(),
        "/static/%2E%2E%2Foutside-secret.txt".to_string(),
        "/static/%252e%252e/outside-secret.txt".to_string(),
        "/static/%255c..%255coutside-secret.txt".to_string(),
        "/static/%5c..%5coutside-secret.txt".to_string(),
        "/static/C:%5cWindows%5csystem.ini".to_string(),
        "/static/C:/Windows/system.ini".to_string(),
        "/static/%5c%5cserver%5cshare%5csecret.txt".to_string(),
        absolute_path_uri,
        encoded_absolute_path_uri,
    ];
    #[cfg(unix)]
    unsafe_uris.extend([
        "/static/outside-link.txt".to_string(),
        "/static/outside-directory-link/nested-secret.txt".to_string(),
    ]);

    for uri in unsafe_uris {
        assert_rejected_without_leak(&app, &uri).await;
    }

    let response = get(&app, "/static/does-not-exist.txt", true).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(!response_body(response)
        .await
        .windows(OUTSIDE_SECRET.len())
        .any(|window| window == OUTSIDE_SECRET.as_bytes()));
}
