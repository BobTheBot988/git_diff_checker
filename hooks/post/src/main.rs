#!/usr/bin/env -S cargo -E run
/// Qwen Code PostToolUse Hook: Check if git_diff_checker detected and reverted changes.
use common::{
    Hook, HookDecision, HookEngine, HookEventName, HookHandler, HookOutput, HookType,
    PostToolUseHookOutput,
};
use std::path::{Path, PathBuf};
use std::{io, process};

// ==========================================
// Git Diff Checker Logic
// ==========================================

fn get_project_root(hook: &Hook) -> PathBuf {
    // Prefer the CWD from the command request if available
    hook.4
        .as_ref()
        .and_then(|req| req.cwd.clone())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn run_git_diff_checker(
    project_root: &Path,
    repo_path: &Path,
    filename: &Path,
) -> Result<String, String> {
    let binary_path = project_root
        .join("target")
        .join("release")
        .join("git_diff_checker");

    // Try release binary first, then fall back to cargo run with args
    let output = if binary_path.exists() {
        process::Command::new(binary_path)
            .current_dir(project_root)
            .args([
                "-r",
                repo_path.to_str().unwrap(),
                "-f",
                filename.to_str().unwrap(),
            ])
            .output()
    } else {
        process::Command::new("cargo")
            .args([
                "run",
                "--release",
                "--",
                "-r",
                repo_path.to_str().unwrap(),
                "-f",
                filename.to_str().unwrap(),
            ])
            .current_dir(project_root)
            .output()
    };

    let output = output.map_err(|e| format!("Failed to run git_diff_checker: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    Ok(format!("{}\n{}", stdout, stderr))
}

fn parse_git_diff_checker_output(output: &str) -> (bool, bool) {
    let modification_detected = output.contains("MODIFICATIONS DETECTED")
        || output.contains("Found")
        || output.contains("hunk(s) in the diff");

    let reversion_performed =
        output.contains("Successfully reverted") && output.contains("hunk(s)");

    (modification_detected, reversion_performed)
}

// ==========================================
// Plugin Implementation
// ==========================================

/// Find the git repository root by traversing upward from a file path
fn find_git_root(start_path: &Path) -> Option<PathBuf> {
    let mut current = start_path.canonicalize().ok()?;
    loop {
        let git_path = current.join(".git");
        if git_path.exists() {
            return Some(current);
        }
        if current.parent()? == current {
            return None;
        }
        current = current.parent()?.to_path_buf();
    }
}

struct GitDiffPlugin;

impl HookHandler for GitDiffPlugin {
    fn execute(&self, hook: &mut Hook) -> Result<HookOutput, HookOutput> {
        let project_root = get_project_root(hook);

        // Get the CommandRequest to access tool_input
        let req = hook.4.clone().expect("Error command req");

        // Extract file_path from tool_input
        let file_path_arg = match req.tool_input.as_ref() {
            Some(tool_input) => tool_input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("src/hello_world.c")
                .to_string(),
            None => "src/hello_world.c".to_string(),
        };

        let file_path = Path::new(&file_path_arg);

        // Find the git repository root by traversing upward from file_path
        let repo_path =
            find_git_root(file_path).unwrap_or_else(|| Path::new("test/test1").to_path_buf());

        // The filename is the relative path from repo root to the file
        let filename = file_path
            .strip_prefix(&repo_path)
            .unwrap_or(file_path)
            .to_path_buf();

        // Ensure we are actually in a PostToolUse context
        let _input = match hook.0.as_post_tool_use() {
            Some(i) => i,
            None => panic!("as_post_tool_use failed"), // Or handle as unexpected event
        };

        let check_output = match run_git_diff_checker(&project_root, &repo_path, &filename) {
            Ok(out) => out,
            Err(e) => {
                let err_output = PostToolUseHookOutput {
                    cont: Some(false),
                    stop_reason: Some(e.clone()),
                    suppress_output: None,
                    system_message: None,
                    reason: Some(e.clone()),
                    hook_specific_output: None,
                    decision: HookDecision::Deny,
                };
                return Err(HookOutput::PostTool(err_output));
            }
        };

        let (detected, reverted) = parse_git_diff_checker_output(&check_output);

        let reason = format!(
            "git_diff_checker: {}{}",
            if detected {
                "modifications detected. "
            } else {
                ""
            },
            if reverted {
                "Changes reverted successfully."
            } else {
                "No unauthorized changes detected."
            }
        );

        // Build hook_specific_output with updatedToolOutput for the model
        let mut hook_specific_output = std::collections::HashMap::new();
        hook_specific_output.insert(
            "hookEventName".to_string(),
            serde_json::Value::String("PostToolUse".to_string()),
        );

        if detected {
            // Include updatedToolOutput matching the edit tool's output schema
            let updated_tool_output = serde_json::json!({
                "content": "Unauthorized modifications detected and reverted.",
                "file_path": filename.to_str().unwrap_or("unknown"),
                "changes_applied": false
            });
            hook_specific_output.insert("updatedToolOutput".to_string(), updated_tool_output);

            // For PostToolUse, decision is at top level of post_tool
            let block_output = PostToolUseHookOutput {
                cont: None,
                stop_reason: Some(reason.clone()),
                suppress_output: None,
                system_message: Some("You are wrong".to_string()),
                reason: Some(reason),
                hook_specific_output: Some(hook_specific_output),
                decision: HookDecision::Block,
            };
            return Ok(HookOutput::PostTool(block_output));
        }

        hook_specific_output.insert(
            "updatedToolOutput".to_string(),
            serde_json::json!({
                "content": "No unauthorized changes detected.",
                "file_path": filename.to_str().unwrap_or("unknown"),
                "changes_applied": true
            }),
        );

        Ok(HookOutput::PostTool(PostToolUseHookOutput {
            cont: None,
            stop_reason: None,
            suppress_output: None,
            system_message: None,
            reason: Some(reason),
            hook_specific_output: Some(hook_specific_output),
            decision: HookDecision::Allow,
        }))
    }
}

fn main() -> io::Result<()> {
    let plugin = GitDiffPlugin;

    // Initialize hook (reads from stdin via Hook::new)
    let h = Hook::new(HookEventName::PostToolUse, HookType::Command);

    // Engine handles execution and automatic printing of JSON to stdout
    HookEngine::run_hook(plugin, h);

    Ok(())
}
