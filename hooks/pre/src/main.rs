#!/usr/bin/env -S cargo -E run
use common::{
    HookDecision, HookEventName, HookInput, HookOutput, PreToolUseHookOutput, PreToolUseInput,
};
use std::collections::HashMap;
/// Qwen Code PreToolUse Hook: Enforce src/ directory whitelist for file modifications.
///
/// This hook:
/// - Allows read operations (read_file, glob, grep_search) on any file
/// - Allows write/edit operations ONLY on files inside src/
/// - Denies write/edit operations on files outside src/
use std::env;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process;

fn get_project_root() -> PathBuf {
    // hook/ is at project root, so parent of this script's directory is root
    let script_dir = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap();

    script_dir.parent().map(|p| p.to_path_buf()).unwrap()
}

fn is_in_src_dir(file_path: &str, project_root: &Path) -> bool {
    let abs_path = project_root
        .join(file_path)
        .canonicalize()
        .unwrap_or_else(|_| {
            // If canonicalize fails (e.g., file doesn't exist yet), use normalize
            project_root.join(file_path)
        });
    let src_dir = project_root.join("src");

    abs_path
        .strip_prefix(&src_dir)
        .map(|p| !p.as_os_str().is_empty())
        .unwrap_or(false)
}

fn handle_pre_tool_use(input: &HookInput) -> HookOutput {
    // 1. Extract the data from the Enum variant
    let input_data = if let HookInput::PreToolUse(data) = input {
        data
    } else {
        panic!("handle_pre_tool_use called with non-PreToolUse input!");
    };

    let project_root = get_project_root();
    let tool_name = input_data.tool_name.as_str();

    // 2. Read operations
    let read_tools = ["read_file", "glob", "grep_search", "list_directory"];
    if read_tools.contains(&tool_name) {
        return make_pre_tool_output(
            common::HookDecision::Allow,
            true,
            format!("Read operation '{}' is allowed on any file", tool_name),
        );
    }

    // 3. Write/Edit operations
    let write_tools = ["write_file", "edit"];
    if write_tools.contains(&tool_name) {
        // Use .as_str() directly on the Value
        let file_path = input_data
            .tool_input
            .get("file")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if file_path.is_empty() {
            return make_pre_tool_output(
                common::HookDecision::Deny,
                true,
                format!("No file path provided for {}", tool_name),
            );
        }

        // Logic check: only allow if in src/
        if is_in_src_dir(file_path, &project_root) {
            return make_pre_tool_output(
                common::HookDecision::Allow,
                true,
                format!("File '{}' is inside src/ whitelist", file_path),
            );
        } else {
            return make_pre_tool_output(
                common::HookDecision::Deny,
                false, // Stop execution because it's outside src/
                format!(
                    "Only files inside src/ can be modified. '{}' is outside.",
                    file_path
                ),
            );
        }
    }

    // 4. Default fallback
    make_pre_tool_output(
        common::HookDecision::Allow,
        true,
        format!("Tool '{}' allowed (not a restricted operation)", tool_name),
    )
}

/// Fixed helper using serde_json::Value
fn make_pre_tool_output(decision: common::HookDecision, cont: bool, reason: String) -> HookOutput {
    let mut map = std::collections::HashMap::new();
    map.insert(
        "hook_event_name".to_string(),
        serde_json::Value::from("pre_tool_use"),
    );
    map.insert(
        "permission_decision".to_string(),
        serde_json::Value::from(decision.to_string()),
    );
    map.insert(
        "permission_decision_reason".to_string(),
        serde_json::Value::from(reason),
    );

    HookOutput::PreTool(PreToolUseHookOutput {
        cont: Some(cont),
        stop_reason: None,
        suppress_output: None,
        system_message: None,
        reason: None,
        hook_specific_output: Some(map),
        decision,
    })
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();

    // Check for --help flag
    if args.len() > 1 && (args[1] == "--help" || args[1] == "-h") {
        println!(
            "Qwen Code PreToolUse Hook: Enforce src/ directory whitelist for file modifications
            
Usage: {} [options]

This hook intercepts Qwen Code tool calls and enforces:
- Read operations (read_file, glob, grep_search) allowed on any file
- Write/Edit operations (write_file, edit) only allowed inside src/

Options:
  --help, -h    Show this help message",
            args[0]
        );
        return Ok(());
    }

    // Read input from stdin
    let mut input_str = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut input_str) {
        eprintln!("Error reading stdin: {}", e);
        process::exit(2);
    }

    // Parse JSON input
    let input: PreToolUseInput = match serde_json::from_str(&input_str) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Error parsing JSON: {}", e);
            process::exit(2);
        }
    };

    // Handle only PreToolUse events
    // if input= "PreToolUse" {
    //     process::exit(0);
    // }

    // Process and output result
    let output = handle_pre_tool_use(&HookInput::PreToolUse(input));
    let output_json = serde_json::to_string_pretty(&output).unwrap();
    println!("{}", output_json);

    process::exit(0);
}
