#!/usr/bin/env -S cargo -E run
/// Qwen Code PreToolUse Hook: Enforce src/ directory whitelist for file modifications.
///
/// This hook:
/// - Allows read operations (read_file, glob, grep_search) on any file
/// - Allows write/edit operations ONLY on files inside src/
/// - Denies write/edit operations on files outside src/
use common::{Hook, HookEngine, HookHandler, HookOutput, PreToolUseHookOutput};
use std::path::Path;

fn is_in_src_dir(file_path: &str, project_root: &Path) -> bool {
    let p = Path::new(file_path);
    let abs_path = if p.is_absolute() {
        p.to_path_buf()
    } else {
        project_root.join(file_path)
    };

    let canonical_target = abs_path.canonicalize().unwrap_or_else(|_| {
        abs_path
            .parent()
            .and_then(|parent| parent.canonicalize().ok())
            .map(|parent| parent.join(abs_path.file_name().unwrap_or_default()))
            .unwrap_or(abs_path)
    });

    let src_dir = project_root.join("src");
    let canonical_src = src_dir.canonicalize().unwrap_or(src_dir);

    // THIS IS THE CRITICAL DEBUG PRINT
    // eprintln!("TARGET: {:?}", canonical_target);
    // eprintln!("WHITELIST: {:?}", canonical_src);

    let result = canonical_target.starts_with(&canonical_src);
    // eprintln!("RESULT: {}", result);

    result
}
pub struct MyPlugin;

impl HookHandler for MyPlugin {
    fn execute(&self, hook: &mut Hook) -> Result<HookOutput, String> {
        let project_root = hook.4.clone().unwrap().cwd.unwrap();
        let res = hook.0.as_pre_tool_use();
        let hi = match res {
            Some(a) => a,
            None => panic!("as_pre_tool_use failed!"),
        };

        let tool_name: &str = hi.tool_name.as_str();

        // 2. Read operations
        let read_tools = ["read_file", "glob", "grep_search", "list_directory"];
        if read_tools.contains(&tool_name) {
            return Ok(PreToolUseHookOutput::make_pre_tool_output(
                common::HookDecision::Allow,
                true,
                format!("Read operation '{}' is allowed on any file", tool_name),
            ));
        }

        let write_tools = ["write_file", "edit"];
        if write_tools.contains(&tool_name) {
            let file_path = hi
                .tool_input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if file_path.is_empty() {
                return Ok(PreToolUseHookOutput::make_pre_tool_output(
                    common::HookDecision::Deny,
                    true,
                    format!("No file path provided for {}", tool_name),
                ));
            }

            // Logic check: only allow if in src/
            if is_in_src_dir(file_path, &project_root) {
                return Ok(PreToolUseHookOutput::make_pre_tool_output(
                    common::HookDecision::Allow,
                    true,
                    format!("File '{}' is inside src/ whitelist", file_path),
                ));
            } else {
                return Ok(PreToolUseHookOutput::make_pre_tool_output(
                    common::HookDecision::Deny,
                    true,
                    format!(
                        "Only files inside src/ can be modified. '{}' is outside.",
                        file_path
                    ),
                ));
            }
        }

        // 4. Default fallback
        Ok(PreToolUseHookOutput::make_pre_tool_output(
            common::HookDecision::Allow,
            true,
            format!("Tool '{}' allowed (not a restricted operation)", tool_name),
        ))
    }
}

fn main() {
    let myplugin = MyPlugin;
    let h = Hook::new(common::HookEventName::PreToolUse, common::HookType::Command);
    HookEngine::run_hook(myplugin, h);
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{CommandRequest, Hook, HookDecision, HookEventName, HookInput, HookType};
    use std::collections::HashMap;
    use std::path::PathBuf;

    // Helper to create a Hook object for testing
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
            tool_input: None,
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
    fn test_input1_deny_outside_src() {
        let plugin = MyPlugin;
        // Target is in /sc/ (not /src/)
        let mut hook = create_test_hook(
            "write_file",
            "/home/robertodr/gits/git_diff_checker/test/test1/sc/hello_world.c",
            "/home/robertodr/gits/git_diff_checker/test/test1",
        );

        let result = plugin.execute(&mut hook).expect("Error");
        let output = result.as_pre_tool().unwrap();

        assert_eq!(output.decision, HookDecision::Deny);
        assert_eq!(output.cont, Some(false));
        assert!(
            output.hook_specific_output.as_ref().unwrap()["permissionDecisionReason"]
                .as_str()
                .unwrap()
                .contains("outside")
        );
    }

    #[test]
    fn test_input2_allow_inside_src() {
        let plugin = MyPlugin;
        // Target is in /src/
        let mut hook = create_test_hook(
            "write_file",
            "/home/robertodr/gits/git_diff_checker/test/test1/src/hello_world.c",
            "/home/robertodr/gits/git_diff_checker/test/test1",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(output.decision, HookDecision::Allow);
        assert_eq!(output.cont, Some(true));
        assert!(
            output.hook_specific_output.as_ref().unwrap()["permissionDecisionReason"]
                .as_str()
                .unwrap()
                .contains("inside src/ whitelist")
        );
    }

    #[test]
    fn test_input3_allow_read_anywhere() {
        let plugin = MyPlugin;
        // read_file is unrestricted
        let mut hook = create_test_hook(
            "read_file",
            "/home/robertodr/gits/git_diff_checker/test/test1/sc/hello_world.c",
            "/home/robertodr/gits/git_diff_checker/test/test1",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(output.decision, HookDecision::Allow);
        assert!(
            output.hook_specific_output.as_ref().unwrap()["permissionDecisionReason"]
                .as_str()
                .unwrap()
                .contains("allowed on any file")
        );
    }

    #[test]
    fn test_input4_allow_unknown_tool() {
        let plugin = MyPlugin;
        // 'ex' is not in the restricted list
        let mut hook = create_test_hook(
            "ex",
            "/home/robertodr/gits/git_diff_checker/test/test1/sc/hello_world.c",
            "/home/robertodr/gits/git_diff_checker/test/test1",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(output.decision, HookDecision::Allow);
        assert!(
            output.hook_specific_output.as_ref().unwrap()["permissionDecisionReason"]
                .as_str()
                .unwrap()
                .contains("not a restricted operation")
        );
    }
}
