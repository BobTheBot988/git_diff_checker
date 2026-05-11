#!/usr/bin/env -S cargo -E run
/// Qwen Code PreToolUse Hook: Enforce src/ directory whitelist for file modifications.
///
/// This hook:
/// - Allows read operations (read_file, glob, grep_search) on any file
/// - Allows write/edit operations ONLY on files inside src/
/// - Denies write/edit operations on files outside src/
use common::{Hook, HookEngine, HookHandler, HookOutput, PreToolUseHookOutput};
use std::io;
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
struct MyPlugin;

impl HookHandler for MyPlugin {
    fn execute(&self, hook: &mut Hook) -> Result<HookOutput, ()> {
        let project_root = hook.4.clone().unwrap().cwd.unwrap();
        let res = hook.0.as_pre_tool_use();
        let hi = match res {
            Some(a) => a,
            None => panic!(""),
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
                    false, // Stop execution because it's outside src/
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

fn main() -> io::Result<()> {
    let myplugin = MyPlugin;
    let h = Hook::new(common::HookEventName::PreToolUse, common::HookType::Command);
    HookEngine::run_hook(myplugin, h);
    Ok(())
}
