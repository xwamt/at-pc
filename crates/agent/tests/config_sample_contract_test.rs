use at_pc_agent::config::AgentConfig;
use std::path::PathBuf;

fn shipped_agent_config_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("agent_config.toml")
}

#[test]
fn shipped_agent_sample_keeps_computer_use_at_the_root_and_disabled() {
    let path = shipped_agent_config_path();
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read shipped agent sample {}: {error}", path.display()));
    let value: toml::Value = toml::from_str(&raw).expect("shipped agent sample must be valid TOML");

    assert_eq!(
        value
            .get("enable_computer_use")
            .and_then(toml::Value::as_bool),
        Some(false),
        "enable_computer_use must be a root-level false-by-default setting"
    );
    assert!(
        value
            .get("device")
            .and_then(toml::Value::as_table)
            .and_then(|device| device.get("enable_computer_use"))
            .is_none(),
        "enable_computer_use must not be nested under [device]"
    );

    let config: AgentConfig = toml::from_str(&raw).expect("sample must deserialize as AgentConfig");
    assert!(
        !config.enable_computer_use,
        "deserialized sample must keep remote input disabled"
    );
    assert_eq!(config.server.url, "ws://127.0.0.1:9801/ws");
}
