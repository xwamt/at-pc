use at_pc_protocol::tools::tool_registry;
use at_pc_server::config::{is_tool_allowed_for_role, Role, ServerConfig};
use at_pc_server::mcp::extract_auth_token;
use at_pc_server::router::generate_call_id;
use axum::http::{header, HeaderMap, HeaderValue};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

fn headers(values: &[(&'static str, &'static str)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in values {
        headers.insert(*name, HeaderValue::from_static(value));
    }
    headers
}

#[test]
fn auth_token_sources_have_stable_precedence_and_aliases() {
    let all_sources = headers(&[
        (header::AUTHORIZATION.as_str(), "raw-authorization"),
        (header::COOKIE.as_str(), "auth_token=cookie-token"),
    ]);
    assert_eq!(
        extract_auth_token(&all_sources, Some("pin=query-token")),
        Some("raw-authorization".to_string())
    );

    let cookie = headers(&[(header::COOKIE.as_str(), "other=x; AUTH_TOKEN=cookie-token")]);
    assert_eq!(
        extract_auth_token(&cookie, Some("token=query-token")),
        Some("cookie-token".to_string())
    );

    assert_eq!(
        extract_auth_token(&HeaderMap::new(), Some("ignored=x&PIN=query-token")),
        Some("query-token".to_string())
    );
}

#[test]
fn auth_query_percent_decode_covers_spaces_unicode_and_malformed_input() {
    let empty = HeaderMap::new();
    assert_eq!(
        extract_auth_token(&empty, Some("pin=hello+world%21")),
        Some("hello world!".to_string())
    );
    assert_eq!(
        extract_auth_token(&empty, Some("token=%E4%BD%A0%E5%A5%BD")),
        Some("你好".to_string())
    );

    for malformed in ["%", "%A", "%GG", "%FF"] {
        assert_eq!(
            extract_auth_token(&empty, Some(&format!("token={malformed}"))),
            Some(malformed.to_string()),
            "decode failure intentionally falls back to the original token"
        );
    }

    assert_eq!(
        extract_auth_token(&empty, Some("token=+++&pin=fallback")),
        Some("fallback".to_string()),
        "decoded whitespace-only values should not mask a later usable alias"
    );
}

#[test]
fn token_to_role_resolution_covers_explicit_legacy_and_dev_mode_branches() {
    let mut roles = HashMap::new();
    roles.insert("shared".to_string(), Role::Viewer);
    roles.insert("operator".to_string(), Role::Operator);
    let configured = ServerConfig {
        auth_token: Some("shared".to_string()),
        roles,
        ..Default::default()
    };

    assert_eq!(
        configured.get_role_for_token(" shared "),
        Some(Role::Viewer)
    );
    assert_eq!(
        configured.get_role_for_token("operator"),
        Some(Role::Operator)
    );
    assert_eq!(configured.get_role_for_token("unknown"), None);
    assert_eq!(configured.get_role_for_token("   "), None);

    let legacy = ServerConfig {
        auth_token: Some("legacy".to_string()),
        ..Default::default()
    };
    assert_eq!(legacy.get_role_for_token("legacy"), Some(Role::Admin));
    assert_eq!(legacy.get_role_for_token("other"), None);

    let dev = ServerConfig::default();
    assert_eq!(
        dev.get_role_for_token("any-non-empty-token"),
        Some(Role::Admin)
    );
}

#[test]
fn rbac_role_matrix_covers_every_registered_and_server_meta_tool() {
    let roles = [Role::Viewer, Role::Operator, Role::Admin];

    for spec in tool_registry() {
        for role in roles {
            assert_eq!(
                is_tool_allowed_for_role(role, spec.name),
                role >= spec.required_role,
                "role {role} disagrees for registered tool {} requiring {}",
                spec.name,
                spec.required_role
            );
        }
    }

    for (tool, required_role) in [
        ("list_terminals", Role::Viewer),
        ("select_terminal", Role::Viewer),
        ("get_active_terminal", Role::Viewer),
        ("list_pending_calls", Role::Viewer),
        ("rename_terminal", Role::Operator),
        ("cancel_tool", Role::Operator),
        ("cancel_task", Role::Operator),
    ] {
        for role in roles {
            assert_eq!(
                is_tool_allowed_for_role(role, tool),
                role >= required_role,
                "role {role} disagrees for server meta-tool {tool}"
            );
        }
    }

    for role in roles {
        assert!(
            !is_tool_allowed_for_role(role, "future_unknown_tool"),
            "unknown tools must fail closed for {role}"
        );
    }
}

#[test]
fn call_ids_are_unique_sequentially_and_across_threads() {
    let sequential: HashSet<_> = (0..10_000).map(|_| generate_call_id()).collect();
    assert_eq!(sequential.len(), 10_000);
    assert!(sequential.iter().all(|id| {
        let mut parts = id.split('-');
        parts.next() == Some("call")
            && parts
                .next()
                .and_then(|part| part.parse::<i64>().ok())
                .is_some()
            && parts
                .next()
                .and_then(|part| part.parse::<u64>().ok())
                .is_some()
            && parts.next().is_none()
    }));

    let concurrent = Arc::new(Mutex::new(HashSet::new()));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let concurrent = Arc::clone(&concurrent);
            std::thread::spawn(move || {
                let local: Vec<_> = (0..2_000).map(|_| generate_call_id()).collect();
                concurrent.lock().unwrap().extend(local);
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(concurrent.lock().unwrap().len(), 16_000);
}
