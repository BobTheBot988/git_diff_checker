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

struct GitDiffPlugin;

impl HookHandler for GitDiffPlugin {
    fn execute(&self, hook: &mut Hook) -> Result<HookOutput, String> {
        let project_root = get_project_root(hook);

        // Get the CommandRequest to access tool_input
        let req = hook.4.clone().expect("Error command req");

        // Extract file_path and repo_path from tool_input
        // tool_input contains: {"file_path": "...", "repo_path": "..."}
        let (repo_path, filename) = match req.tool_input.as_ref() {
            Some(tool_input) => {
                // Get repo_path from tool_input (optional, default to test/test1)
                let repo_path_arg = tool_input
                    .get("repo_path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("test/test1")
                    .to_string();

                // Get file_path from tool_input
                let file_path_arg = tool_input
                    .get("file_path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("src/hello_world.c");

                // Use canonicalize for guaranteed absolute paths
                let repo_path = Path::new(&repo_path_arg)
                    .canonicalize()
                    .unwrap_or_else(|_| repo_path_arg.into());
                let filename = Path::new(&file_path_arg)
                    .canonicalize()
                    .unwrap_or_else(|_| file_path_arg.into());

                (repo_path, filename)
            }
            None => {
                let repo_path = Path::new("test/test1")
                    .canonicalize()
                    .unwrap_or_else(|_| "test/test1".into());
                let filename = Path::new("src/hello_world.c")
                    .canonicalize()
                    .unwrap_or_else(|_| "src/hello_world.c".into());
                (repo_path, filename)
            }
        };

        // Ensure we are actually in a PostToolUse context
        let _input = match hook.0.as_post_tool_use() {
            Some(i) => i,
            None => panic!("as_post_tool_use failed"), // Or handle as unexpected event
        };

        let check_output = match run_git_diff_checker(&project_root, &repo_path, &filename) {
            Ok(out) => out,
            Err(e) => {
                return Err(format!("Failed git_diff_checker: {}", e));
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

        // Deny when modifications to original lines are detected.
        // The reversion is a cleanup, but the model should still be denied because
        // it violated the policy by modifying original lines.
        if detected {
            return Err(reason);
        }

        Ok(HookOutput::PostTool(PostToolUseHookOutput {
            cont: Some(true),
            stop_reason: None,
            suppress_output: None,
            system_message: None,
            reason: Some(reason),
            hook_specific_output: None,
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
