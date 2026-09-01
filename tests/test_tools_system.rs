use at_pc::tools::{process, sysinfo};

#[test]
fn test_get_system_overview() {
    let overview = sysinfo::get_system_overview();
    assert!(!overview.os_name.is_empty(), "os_name should not be empty");
    assert!(overview.total_memory_mb > 0, "total_memory_mb should be > 0");
    assert!(!overview.cpu_model.is_empty(), "cpu_model should not be empty");
    assert!(overview.cpu_cores > 0, "cpu_cores should be > 0");
    // Verify JSON serialization works
    let json = serde_json::to_string(&overview).expect("SystemOverview should serialize to JSON");
    assert!(!json.is_empty());
}

#[test]
fn test_get_system_overview_includes_network_details() {
    let overview = sysinfo::get_system_overview();
    assert!(!overview.local_ips.is_empty(), "local_ips should not be empty");
    assert!(!overview.default_gateway.is_empty(), "default_gateway should not be empty");
    assert!(!overview.dns_servers.is_empty(), "dns_servers should not be empty");
}

#[test]
fn test_network_helper_functions_direct() {
    let ips = sysinfo::get_local_ips();
    assert!(!ips.is_empty(), "get_local_ips() should return at least one IP on active host");
    for ip in &ips {
        assert!(!ip.is_empty());
        assert!(!ip.starts_with("127."), "Loopback IPv4 should be excluded: {}", ip);
        assert_ne!(ip, "::1", "Loopback IPv6 should be excluded");
    }

    let gw = sysinfo::get_default_gateway();
    assert!(!gw.is_empty(), "get_default_gateway() should return a non-empty string or 'unknown'");

    let dns = sysinfo::get_dns_servers();
    // DNS may be empty in some mock/isolated envs, but should be a valid Vec
    for server in &dns {
        assert!(!server.is_empty(), "DNS server entries should not be empty");
    }
}

#[test]
fn test_dispatch_get_system_overview() {
    let res = at_pc::tools::dispatch_mcp_tool("get_system_overview", serde_json::json!({})).unwrap();
    assert!(res.get("local_ips").is_some());
    assert!(res.get("default_gateway").is_some());
    assert!(res.get("dns_servers").is_some());
    assert!(res.get("disks").is_some());
    assert!(res.get("networks").is_some());
}



#[test]
fn test_list_processes_default_and_limit() {
    let procs = process::list_processes(None, Some("memory"), 10);
    assert!(!procs.is_empty(), "Process list should not be empty");
    assert!(procs.len() <= 10, "Process count should respect limit of 10");

    let first = &procs[0];
    assert!(!first.name.is_empty());
}

#[test]
fn test_list_processes_sorting() {
    // Sort by pid
    let procs_pid = process::list_processes(None, Some("pid"), 20);
    if procs_pid.len() >= 2 {
        for window in procs_pid.windows(2) {
            assert!(window[0].pid <= window[1].pid, "Should be sorted ascending by PID");
        }
    }

    // Sort by memory (descending)
    let procs_mem = process::list_processes(None, Some("memory"), 20);
    if procs_mem.len() >= 2 {
        for window in procs_mem.windows(2) {
            assert!(window[0].memory_mb >= window[1].memory_mb, "Should be sorted descending by memory");
        }
    }

    // Sort by cpu (descending)
    let procs_cpu = process::list_processes(None, Some("cpu"), 20);
    if procs_cpu.len() >= 2 {
        for window in procs_cpu.windows(2) {
            assert!(window[0].cpu_usage >= window[1].cpu_usage, "Should be sorted descending by CPU");
        }
    }
}

#[test]
fn test_list_processes_filter() {
    let current_pid = std::process::id();
    let all_procs = process::list_processes(None, None, 500);
    let current = all_procs.iter().find(|p| p.pid == current_pid);

    if let Some(p) = current {
        let filtered = process::list_processes(Some(&p.name), None, 50);
        assert!(!filtered.is_empty(), "Filtered list should find process by name");
        assert!(filtered.iter().any(|item| item.pid == current_pid));
    }
}

#[test]
fn test_kill_process_invalid_args() {
    // Neither pid nor name specified
    let res = process::kill_process(None, None, false);
    assert!(res.is_err(), "kill_process without PID or name should error");

    // Non-existent PID
    let res_pid = process::kill_process(Some(99999999), None, false);
    assert!(res_pid.is_err(), "kill_process with invalid PID should error");
}
