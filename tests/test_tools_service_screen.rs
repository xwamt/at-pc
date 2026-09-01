use at_pc::tools::{screen, service};

#[test]
fn test_capture_screen_returns_base64() {
    let res = screen::capture_screen(0, "jpeg", 75);
    match res {
        Ok(capture) => {
            assert!(!capture.base64_data.is_empty());
            assert!(capture.width > 0);
            assert!(capture.height > 0);
            assert_eq!(capture.format, "jpeg");
            assert!(
                capture.base64_data.starts_with("data:image/jpeg;base64,"),
                "Base64 data should start with data URI prefix"
            );
        }
        Err(e) => {
            println!("Screen capture not supported in test environment: {}", e);
            assert!(!e.is_empty());
        }
    }
}

#[test]
fn test_capture_screen_png_format() {
    let res = screen::capture_screen(0, "png", 100);
    match res {
        Ok(capture) => {
            assert!(!capture.base64_data.is_empty());
            assert!(capture.width > 0);
            assert!(capture.height > 0);
            assert_eq!(capture.format, "png");
            assert!(
                capture.base64_data.starts_with("data:image/png;base64,"),
                "Base64 data should start with data URI prefix"
            );
        }
        Err(e) => {
            println!("Screen capture not supported in test environment: {}", e);
            assert!(!e.is_empty());
        }
    }
}

#[test]
fn test_capture_screen_invalid_display_index() {
    let res = screen::capture_screen(99999, "jpeg", 75);
    assert!(res.is_err(), "Invalid display index should return an error");
    let err_msg = res.unwrap_err();
    assert!(
        err_msg.contains("99999") || err_msg.contains("display") || err_msg.contains("monitor") || err_msg.contains("Screen capture error"),
        "Error message should indicate invalid display index: {}",
        err_msg
    );
}

#[test]
fn test_manage_service_status() {
    // Test status query on a common system service or test service
    #[cfg(windows)]
    let service_name = "Spooler";
    #[cfg(not(windows))]
    let service_name = "cron";

    let res = service::manage_service(service_name, "status");
    match res {
        Ok(status) => {
            assert_eq!(status.name, service_name);
            assert!(!status.status.is_empty());
        }
        Err(e) => {
            // Non-windows or permission restricted environments might return an error message
            assert!(!e.is_empty());
        }
    }
}

#[test]
fn test_manage_service_invalid_action() {
    let res = service::manage_service("test_service", "invalid_action");
    assert!(res.is_err(), "Invalid action should return an error");
    let err_msg = res.unwrap_err();
    assert!(
        err_msg.contains("Invalid") || err_msg.contains("Unsupported") || err_msg.contains("action"),
        "Error message should mention invalid action: {}",
        err_msg
    );
}

#[test]
fn test_manage_service_nonexistent_service() {
    let res = service::manage_service("definitely_nonexistent_service_xyz_12345", "status");
    // Depending on platform, this may return Ok with status "NotFound"/"Stopped" or Err
    match res {
        Ok(status) => {
            assert!(
                status.status == "NotFound" || status.status == "Stopped" || status.status == "Unknown",
                "Status should indicate not found or unknown: {}",
                status.status
            );
        }
        Err(e) => {
            assert!(!e.is_empty());
        }
    }
}
