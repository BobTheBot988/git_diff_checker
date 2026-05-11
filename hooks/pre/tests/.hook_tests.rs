// tests/hook_tests.rs
use common::{CommandRequest, Hook, HookDecision, HookEventName, HookHandler, HookInput, HookType};
use pre::MyPlugin; // Import your plugin from the lib
use std::collections::HashMap;
use std::path::PathBuf;

fn create_test_hook(tool: &str, file_path: &str, cwd: &str) -> Hook {
    let mut tool_input = HashMap::new();
    tool_input.insert(
        "file_path".to_string(),
        serde_json::Value::String(file_path.to_string()),
    );

    let input = common::PreToolUseInput {
        permission_mode: common::PermissionMode::Default,
        tool_name: tool.to_string(),
        tool_input,
        tool_use_id: "test_id".to_string(),
    };

    let req = CommandRequest {
        hook_event_name: Some("PreToolUse".to_string()),
        cwd: Some(PathBuf::from(cwd)),
        extra_fields: HashMap::new(),
    };

    Hook(
        HookInput::PreToolUse(input),
        None,
        HookEventName::PreToolUse,
        HookType::Command,
        Some(req),
    )
}

#[test]
fn test_deny_outside_src() {
    let plugin = MyPlugin;
    let mut hook = create_test_hook(
        "write_file",
        "/home/user/test/sc/hello.c",
        "/home/user/test",
    );

    let result = plugin.execute(&mut hook).unwrap();
    let output = result.as_pre_tool().unwrap();
    assert_eq!(output.decision, HookDecision::Deny);
}
