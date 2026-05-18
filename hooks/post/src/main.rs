#!/usr/bin/env -S cargo -E run
/// Qwen Code PostToolUse Hook: Check if git_diff_checker detected and reverted changes.
///
/// Uses git_diff_checker as a Rust library for line-level selective revert.
/// Injects detailed additionalContext telling the model exactly which lines
/// were reverted vs preserved.
use common::{
    Hook, HookDecision, HookEngine, HookEventName, HookHandler, HookOutput, HookType,
    PostToolUseHookOutput,
};
use git_diff_checker::selective_revert_all;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process;

// ==========================================
// Git Diff Checker Logic
// ==========================================

fn get_project_root(hook: &Hook) -> PathBuf {
    // Prefer the CWD from the command request if available
    match hook.4.as_ref() {
        Some(req) => match req.cwd.clone() {
            Some(cwd) => cwd,
            None => PathBuf::from("."),
        },
        None => PathBuf::from("."),
    }
}

// ==========================================
// Plugin Implementation
// ==========================================

struct GitDiffPlugin;

impl HookHandler for GitDiffPlugin {
    fn execute(&self, hook: &mut Hook) -> Result<HookOutput, String> {
        let project_root = get_project_root(hook);

        // Find the git repository root using libgit2
        let git_root = match git_diff_checker::get_git_root(&project_root.to_string_lossy()) {
            Ok(root) => root,
            Err(_e) => {
                // Fall back to project_root if no git root found
                project_root.clone()
            }
        };
        let git_root_str = git_root.to_string_lossy().to_string();

        // Ensure we are actually in a PostToolUse context
        let _input = match hook.0.as_post_tool_use() {
            Some(i) => i,
            None => process::exit(0),
        };

        // Run selective revert using the library
        let results = match selective_revert_all(&git_root_str) {
            Ok(r) => r,
            Err(e) => {
                let mut ctx: HashMap<String, serde_json::Value> = HashMap::new();
                ctx.insert("hookEventName".into(), "PostToolUse".into());
                ctx.insert(
                    "additionalContext".into(),
                    format!(
                        "The git diff checker encountered an error: {}. \
                         This may indicate a transient issue. \
                         Ensure files are being modified in allowed directories only.",
                        e
                    )
                    .into(),
                );
                let guidance_output = PostToolUseHookOutput {
                    cont: Some(true),
                    stop_reason: None,
                    suppress_output: None,
                    system_message: None,
                    reason: Some(e),
                    hook_specific_output: Some(ctx),
                    decision: None,
                };
                return Ok(HookOutput::PostTool(guidance_output));
            }
        };

        // Compute totals
        let mut total_reverted: usize = 0;
        let mut files_affected: usize = 0;
        for detail in &results {
            if detail.reverted_hunks > 0 {
                total_reverted += detail.reverted_hunks;
                files_affected += 1;
            }
        }

        let reason = format!(
            "git_diff_checker: {} hunk(s) reverted across {} file(s).",
            total_reverted, files_affected
        );

        if total_reverted > 0 {
            let mut context_lines: Vec<String> = Vec::new();
            context_lines.push(
                "Original committed lines were modified and have been reverted.".to_string(),
            );
            context_lines.push(
                "New code added by the agent has been preserved.".to_string(),
            );
            context_lines.push(String::new());

            for detail in results {
                if detail.reverted_hunks == 0 {
                    continue;
                }
                context_lines.push(format!("File: {}", detail.filename));
                if !detail.reverted_lines.is_empty() {
                    context_lines.push("  Reverted (restored to original):".to_string());
                    for line in &detail.reverted_lines {
                        context_lines.push(format!("    - {}", line));
                    }
                }
                if !detail.preserved_lines.is_empty() {
                    context_lines.push("  Preserved (model additions kept):".to_string());
                    for line in &detail.preserved_lines {
                        context_lines.push(format!("    + {}", line));
                    }
                }
                context_lines.push(String::new());
            }

            // Add guidelines with examples
            context_lines.push(
                "Only add new lines to existing files — do not modify or delete \
                 committed lines."
                    .to_string(),
            );
            context_lines.push(String::new());
            context_lines.push("Guidelines:".to_string());
            context_lines.push(
                "  DOABLE — Add content inside existing constructs".to_string(),
            );
            context_lines.push(
                "    Original:   fn example() { }".to_string(),
            );
            context_lines.push(
                "    You can:    fn example() {".to_string(),
            );
            context_lines.push(
                "                     // your code here".to_string(),
            );
            context_lines.push(
                "                 }".to_string(),
            );
            context_lines.push(
                "    (Opening braces and adding lines inside is OK — original line \
                 is not modified)"
                    .to_string(),
            );
            context_lines.push(String::new());
            context_lines.push(
                "  NOT DOABLE — Change or replace content of an original line".to_string(),
            );
            context_lines.push(
                "    Original:   class name is x".to_string(),
            );
            context_lines.push(
                "    You wrote:  class name is y // y added by the model".to_string(),
            );
            context_lines.push(
                "    This gets reverted because 'class name is y' modified the original \
                 line, even with a comment appended."
                    .to_string(),
            );
            context_lines.push(
                "    Instead, add a NEW line below it:".to_string(),
            );
            context_lines.push(
                "    class name is x".to_string(),
            );
            context_lines.push(
                "    // class name is y would be wrong".to_string(),
            );

            let context_str = context_lines.join("\n");

            let mut ctx: HashMap<String, serde_json::Value> = HashMap::new();
            ctx.insert("hookEventName".into(), "PostToolUse".into());
            ctx.insert("additionalContext".into(), context_str.into());

            let guidance_output = PostToolUseHookOutput {
                cont: Some(true),
                stop_reason: None,
                suppress_output: None,
                system_message: None,
                reason: Some(reason),
                hook_specific_output: Some(ctx),
                decision: None,
            };
            return Ok(HookOutput::PostTool(guidance_output));
        }

        // No modifications detected — simple approve
        let no_mod_reason = "git_diff_checker: No unauthorized changes detected.".to_string();
        Ok(HookOutput::PostTool(PostToolUseHookOutput {
            cont: Some(true),
            stop_reason: None,
            suppress_output: None,
            system_message: None,
            reason: Some(no_mod_reason),
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
