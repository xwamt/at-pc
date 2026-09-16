use at_pc_protocol::tools::{
    agent_tool, tool_registry, validate_tool_registry, AgentToolKind, DispatchKind, Role, ToolRisk,
    ALL_AGENT_TOOL_KINDS,
};
use std::collections::HashSet;

#[test]
fn registry_is_unique_complete_and_structurally_valid() {
    let specs = tool_registry();
    assert_eq!(specs.len(), ALL_AGENT_TOOL_KINDS.len());
    validate_tool_registry(specs).unwrap();

    let names = specs.iter().map(|spec| spec.name).collect::<HashSet<_>>();
    assert_eq!(names.len(), specs.len());
    let kinds = specs
        .iter()
        .map(|spec| spec.agent_kind())
        .collect::<HashSet<_>>();
    assert_eq!(kinds.len(), ALL_AGENT_TOOL_KINDS.len());
    for kind in ALL_AGENT_TOOL_KINDS {
        assert!(kinds.contains(&kind));
        let spec = agent_tool(kind.as_str()).expect("kind name must resolve");
        assert_eq!(spec.dispatch, DispatchKind::Agent(kind));
        assert!(!spec.description.is_empty());
        assert!(spec.input_schema["properties"].is_object());
        assert!(spec.input_schema["required"].is_array());
    }
}

#[test]
fn every_role_and_risk_category_has_an_explicit_contract() {
    let roles = tool_registry()
        .iter()
        .map(|spec| spec.required_role)
        .collect::<HashSet<_>>();
    assert_eq!(
        roles,
        HashSet::from([Role::Viewer, Role::Operator, Role::Admin])
    );

    let risks = tool_registry()
        .iter()
        .map(|spec| spec.risk)
        .collect::<HashSet<_>>();
    assert_eq!(
        risks,
        HashSet::from([
            ToolRisk::ReadOnly,
            ToolRisk::SystemMutation,
            ToolRisk::ComputerControl,
        ])
    );
}

#[test]
fn lookup_rejects_unknown_and_server_only_names() {
    assert!(agent_tool("not_a_tool").is_none());
    assert!(agent_tool("cancel_tool").is_none());
    assert!(agent_tool("cancel_task").is_none());
}

#[test]
fn validation_fails_explicitly_for_duplicate_contracts() {
    let mut invalid = tool_registry().to_vec();
    invalid.push(invalid[0].clone());
    let error = validate_tool_registry(&invalid).unwrap_err();
    assert!(error.to_string().contains("duplicate tool name"));

    assert_eq!(
        AgentToolKind::GetSystemOverview.as_str(),
        "get_system_overview"
    );
}

#[test]
fn validation_rejects_malformed_property_and_required_schemas() {
    let mut missing_property = tool_registry().to_vec();
    missing_property[0].input_schema["required"] = serde_json::json!(["absent"]);
    assert!(validate_tool_registry(&missing_property)
        .unwrap_err()
        .to_string()
        .contains("requires unknown property"));

    let mut non_string_required = tool_registry().to_vec();
    non_string_required[0].input_schema["required"] = serde_json::json!([1]);
    assert!(validate_tool_registry(&non_string_required)
        .unwrap_err()
        .to_string()
        .contains("required entries must be strings"));

    let mut invalid_type = tool_registry().to_vec();
    invalid_type[1].input_schema["properties"]["script"]["type"] =
        serde_json::json!("invalid-type");
    assert!(validate_tool_registry(&invalid_type)
        .unwrap_err()
        .to_string()
        .contains("unsupported type"));

    for spec in tool_registry() {
        for property in spec.input_schema["properties"]
            .as_object()
            .unwrap()
            .values()
        {
            assert!(property["description"]
                .as_str()
                .is_some_and(|description| !description.is_empty()));
        }
    }
}
