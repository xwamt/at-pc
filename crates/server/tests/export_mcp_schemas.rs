use at_pc_server::mcp::tools::get_mcp_tool_definitions;
use serde_json::json;
use std::fs;
use std::path::Path;

#[test]
fn export_mcp_schemas_to_gemini() {
    let tools = get_mcp_tool_definitions();
    let target_dir = Path::new("/Users/clkj/.gemini/antigravity/mcp/at-pc");
    if !target_dir.exists() {
        return;
    }

    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        let description = tool["description"].as_str().unwrap_or("");
        let input_schema = &tool["inputSchema"];

        let mcp_format = json!({
            "name": name,
            "description": description,
            "parameters": input_schema
        });

        let file_path = target_dir.join(format!("{}.json", name));
        let content = serde_json::to_string_pretty(&mcp_format).unwrap();
        fs::write(&file_path, content).unwrap();
        println!("Exported schema for: {}", name);
    }
}
