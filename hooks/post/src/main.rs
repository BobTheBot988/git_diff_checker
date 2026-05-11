#!/usr/bin/env -S cargo -E run
/// Qwen Code PostToolUse Hook: Check if git_diff_checker detected and reverted changes.
use common::{
    Hook, HookDecision, HookEngine, HookEventName, HookHandler, HookOutput, HookType,
    PostToolUseHookOutput,
};
use std::path::{Path, PathBuf};
use std::{env, io, process};

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

fn run_git_diff_checker(project_root: &Path) -> Result<String, String> {
    let binary_path = project_root
        .join("target")
        .join("release")
        .join("git_diff_checker");

    // Try release binary first, then fall back to cargo run
    let output = if binary_path.exists() {
        process::Command::new(binary_path)
            .current_dir(project_root)
            .output()
    } else {
        process::Command::new("cargo")
            .args(["run", "--release"])
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
    fn execute(&self, hook: &mut Hook) -> Result<HookOutput, ()> {
        let project_root = get_project_root(hook);

        // Ensure we are actually in a PostToolUse context
        let _input = match hook.0.as_post_tool_use() {
            Some(i) => i,
            None => return Err(()), // Or handle as unexpected event
        };

        // 1. Run the external checker
        let check_output = match run_git_diff_checker(&project_root) {
            Ok(out) => out,
            Err(e) => {
                return Ok(HookOutput::PostTool(PostToolUseHookOutput {
                    cont: Some(false),
                    stop_reason: Some(e),
                    suppress_output: None,
                    system_message: None,
                    reason: None,
                    hook_specific_output: None,
                    decision: HookDecision::Deny,
                }));
            }
        };

        // 2. Parse Results
        let (detected, reverted) = parse_git_diff_checker_output(&check_output);

        // 3. Construct the response
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

        // Use a decision logic: If modifications were detected but NOT reverted, we might want to block.
        // If reverted or clean, we allow.
        let decision = if detected && !reverted {
            HookDecision::Deny
        } else {
            HookDecision::Allow
        };

        // In your common lib, you might want to add a `make_post_tool_output` similar to the pre_tool one.
        // For now, we construct the struct directly:
        Ok(HookOutput::PostTool(PostToolUseHookOutput {
            cont: Some(true),
            stop_reason: None,
            suppress_output: None,
            system_message: None,
            reason: Some(reason),
            hook_specific_output: None, // You can populate this with a HashMap if needed
            decision,
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
