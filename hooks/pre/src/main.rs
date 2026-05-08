#!/usr/bin/env -S cargo -E run
use common::{CommandRequest, HookInput, HookOutput, PreToolUseHookOutput, PreToolUseInput};
use std::collections::HashMap;
/// Qwen Code PreToolUse Hook: Enforce src/ directory whitelist for file modifications.
///
/// This hook:
/// - Allows read operations (read_file, glob, grep_search) on any file
/// - Allows write/edit operations ONLY on files inside src/
/// - Denies write/edit operations on files outside src/
use std::env;
use std::fs::File;
use std::io::prelude::*;
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
    eprintln!("TARGET: {:?}", canonical_target);
    eprintln!("WHITELIST: {:?}", canonical_src);

    let result = canonical_target.starts_with(&canonical_src);
    eprintln!("RESULT: {}", result);

    result
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
        return PreToolUseHookOutput::make_pre_tool_output(
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
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if file_path.is_empty() {
            return PreToolUseHookOutput::make_pre_tool_output(
                common::HookDecision::Deny,
                true,
                format!("No file path provided for {}", tool_name),
            );
        }

        // Logic check: only allow if in src/
        if is_in_src_dir(file_path, &project_root) {
            return PreToolUseHookOutput::make_pre_tool_output(
                common::HookDecision::Allow,
                true,
                format!("File '{}' is inside src/ whitelist", file_path),
            );
        } else {
            return PreToolUseHookOutput::make_pre_tool_output(
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
    PreToolUseHookOutput::make_pre_tool_output(
        common::HookDecision::Allow,
        true,
        format!("Tool '{}' allowed (not a restricted operation)", tool_name),
    )
}

/// Fixed helper using serde_json::Value

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();

    // ... (Keep your --help logic here) ...

    // 1. Setup debug logging
    let mut debug_file = File::create("/tmp/foo.txt").unwrap();

    // 2. Use Deserializer to handle multiple JSON objects (Concatenated JSON)
    let stdin = io::stdin();
    let reader = stdin.lock();
    let stream = serde_json::Deserializer::from_reader(reader).into_iter::<CommandRequest>();

    let mut found_event = false;

    for item in stream {
        match item {
            Ok(request) => {
                // 3. Try to convert the generic Request into our specific PreToolUseInput
                if let Ok(pre_tool_input) = PreToolUseInput::try_from(request) {
                    found_event = true;

                    // Log the validated input to our debug file
                    let json_bytes =
                        serde_json::to_vec(&pre_tool_input).expect("Failed to serialize");
                    debug_file.write_all(&json_bytes)?;

                    // 4. Process the hook logic
                    let output = handle_pre_tool_use(&HookInput::PreToolUse(pre_tool_input));

                    // 5. Output result and exit
                    let output_json = serde_json::to_string_pretty(&output).unwrap();
                    println!("{}", output_json);
                    process::exit(0);
                }
            }
            Err(e) => {
                // If it's a parse error on a chunk that isn't ours, we can often ignore it,
                // but for debugging hooks, it's better to see it in stderr.
                eprintln!("Wait: JSON chunk error: {}", e);
            }
        }
    }

    if !found_event {
        eprintln!("Error: No valid PreToolUse event found in input stream.");
        process::exit(2);
    }

    Ok(())
}
