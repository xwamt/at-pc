use at_pc_agent::tools::dispatch::domain_handle_fns;
use at_pc_protocol::tools::{AgentToolKind, ALL_AGENT_TOOL_KINDS};
use std::collections::HashSet;

fn expected_domain_kinds() -> [(&'static str, &'static [AgentToolKind]); 7] {
    use AgentToolKind as K;
    [
        (
            "system",
            &[
                K::GetSystemOverview,
                K::ManageService,
                K::GetEventLogs,
                K::ListNetworkConnections,
                K::TestNetwork,
            ],
        ),
        (
            "process",
            &[
                K::ExecPowershell,
                K::ExecCmd,
                K::ListProcesses,
                K::KillProcess,
            ],
        ),
        (
            "file",
            &[
                K::ReadTextFile,
                K::WriteTextFile,
                K::ListDirectory,
                K::SearchFiles,
            ],
        ),
        (
            "screen",
            &[
                K::CaptureScreen,
                K::ListMonitors,
                K::GetMarkedScreen,
                K::ClickMark,
            ],
        ),
        (
            "uia",
            &[
                K::GetUiTree,
                K::ClickElement,
                K::SetElementText,
                K::BatchActions,
            ],
        ),
        (
            "input",
            &[
                K::MouseClick,
                K::MouseMove,
                K::MouseDrag,
                K::MouseScroll,
                K::TypeText,
                K::PressKey,
                K::KeyDown,
                K::KeyUp,
                K::Hotkey,
            ],
        ),
        ("tui", &[K::ListWindows, K::FocusWindow, K::CloseWindow]),
    ]
}

#[test]
fn every_agent_tool_kind_is_owned_by_exactly_one_spec_domain() {
    let expected = expected_domain_kinds();
    let mut table_kinds = HashSet::new();
    for (domain, kinds) in expected {
        for kind in kinds {
            assert!(
                table_kinds.insert(*kind),
                "{kind:?} is duplicated in the expected table (seen again in {domain})"
            );
        }
    }
    assert_eq!(
        table_kinds.len(),
        ALL_AGENT_TOOL_KINDS.len(),
        "expected table must cover all {} kinds without gaps, got {}",
        ALL_AGENT_TOOL_KINDS.len(),
        table_kinds.len()
    );
    for kind in ALL_AGENT_TOOL_KINDS {
        assert!(
            table_kinds.contains(&kind),
            "{kind:?} is in ALL_AGENT_TOOL_KINDS but missing from the expected table"
        );
    }

    for kind in ALL_AGENT_TOOL_KINDS {
        let handlers: Vec<&'static str> = domain_handle_fns()
            .into_iter()
            .filter(|(_, handles)| handles(kind))
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            handlers.len(),
            1,
            "{kind:?} should be owned by exactly one domain, got {handlers:?}"
        );
    }

    for (domain, kinds) in expected {
        let handles = domain_handle_fns()
            .into_iter()
            .find(|(name, _)| *name == domain)
            .map(|(_, handles)| handles)
            .expect("expected domain missing from handlers");
        for kind in ALL_AGENT_TOOL_KINDS {
            assert_eq!(
                handles(kind),
                kinds.contains(&kind),
                "{domain}::handles({kind:?}) must match the spec table"
            );
        }
    }
}
