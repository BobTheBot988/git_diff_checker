#!/usr/bin/env -S cargo -E run
/// Qwen Code PostToolUse Hook: Check if git_diff_checker detected and reverted changes.
///
/// This hook runs `git_diff_checker --all` to check ALL modified files in the
/// repository for unauthorized modifications to original committed lines.
/// This catches agents that try to circumvent the pre-tool-use whitelist by
/// editing files via Bash commands.
use common::{
    Hook, HookDecision, HookEngine, HookEventName, HookHandler, HookOutput, HookType,
    PostToolUseHookOutput,
};
use std::path::{Path, PathBuf};
use std::process;

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

/// Find the git repository root by traversing upward from a start path
fn find_git_root(start_path: &Path) -> Option<PathBuf> {
    let mut current = start_path.canonicalize().ok()?;
    loop {
        let git_path = current.join(".git");
        if git_path.exists() {
            return Some(current);
        }
        // Check parent, but stop at filesystem root
        let next = current.parent()?;
        if next == current {
            return None;
        }
        current = next.to_path_buf();
    }
}

fn run_git_diff_checker_all(project_root: &Path, git_root: &Path) -> Result<String, String> {
    let binary_path = project_root
        .join("target")
        .join("release")
        .join("git_diff_checker");

    // Try release binary first, then fall back to cargo run with args
    let output = if binary_path.exists() {
        process::Command::new(binary_path)
            .current_dir(project_root)
            .args([
                "--all",
                "-r",
                &git_root.to_string_lossy(),
            ])
            .output()
    } else {
        process::Command::new("cargo")
            .args([
                "run",
                "--release",
                "--",
                "--all",
                "-r",
                &git_root.to_string_lossy(),
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
    let modification_detected = output.contains("MODIFICATIONS DETECTED");

    let reversion_performed = output.contains("Successfully reverted") && output.contains("hunk(s)");

    (modification_detected, reversion_performed)
}

// ==========================================
// Plugin Implementation
// ==========================================

struct GitDiffPlugin;

impl HookHandler for GitDiffPlugin {
    fn execute(&self, hook: &mut Hook) -> Result<HookOutput, String> {
        let project_root = get_project_root(hook);

        // Find the git repository root by traversing upward from project_root
        let git_root = match find_git_root(&project_root) {
            Some(root) => root,
            None => {
                // Fall back to project_root if no git root found
                project_root.clone()
            }
        };

        // Ensure we are actually in a PostToolUse context
        let _input = match hook.0.as_post_tool_use() {
            Some(i) => i,
            None => panic!("as_post_tool_use failed"),
        };

        let check_output =
            match run_git_diff_checker_all(&project_root, &git_root) {
                Ok(out) => out,
                Err(e) => {
                    let error_output = PostToolUseHookOutput {
                        cont: Some(false),
                        stop_reason: Some(e.clone()),
                        suppress_output: None,
                        system_message: None,
                        reason: Some(e.clone()),
                        hook_specific_output: None,
                        decision: Some(HookDecision::Block),
                    };
                    return Ok(HookOutput::PostTool(error_output));
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

        // Block when modifications to original lines are detected
        if detected {
            let block_output = PostToolUseHookOutput {
                cont: Some(false),
                stop_reason: Some(reason.clone()),
                suppress_output: None,
                system_message: Some("You are wrong".to_string()),
                reason: Some(reason),
                hook_specific_output: None,
                decision: Some(HookDecision::Block),
            };
            return Ok(HookOutput::PostTool(block_output));
        }

        Ok(HookOutput::PostTool(PostToolUseHookOutput {
            cont: Some(true),
            stop_reason: None,
            suppress_output: None,
            system_message: None,
            reason: Some(reason),
            hook_specific_output: None,
            decision: Some(HookDecision::Approve),
        }))
    }
}

fn main() {
    let plugin = GitDiffPlugin;

    // Initialize hook (reads from stdin via Hook::new)
    let h = Hook::new(HookEventName::PostToolUse, HookType::Command);

    // Engine handles execution and automatic printing of JSON to stdout
    HookEngine::run_hook(plugin, h);

    // Always exit with code 0 - JSON output handles blocking decisions
    std::process::exit(0);
}
