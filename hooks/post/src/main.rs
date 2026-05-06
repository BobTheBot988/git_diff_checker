#!/usr/bin/env -S cargo -E run
/// Qwen Code PostToolUse Hook: Check if git_diff_checker detected and reverted changes.
///
/// This hook:
/// - Runs git_diff_checker after tool execution
/// - Detects if unauthorized changes were made to files outside src/
/// - If changes detected and reverted: reports success
/// - Outputs JSON for Qwen Code compatibility
use std::env;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Deserialize)]
struct HookInput {
    hook_event_name: String,
    tool_name: String,
    tool_input: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct HookOutput {
    hook_specific_output: HookSpecificOutput,
}

#[derive(Debug, Serialize)]
struct HookSpecificOutput {
    hook_event_name: String,
    check_result: String,
    check_result_reason: String,
    reversion_performed: bool,
}

#[derive(Error, Debug)]
enum HookError {
    #[error("Failed to run git_diff_checker: {0}")]
    CommandFailed(String),
    #[error("Failed to parse git_diff_checker output: {0}")]
    ParseError(String),
}

fn get_project_root() -> PathBuf {
    // The hooks are at hooks/target/release/, project root is hooks/../
    let script_dir = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    // Walk up three levels: post_hook -> target -> hooks -> project root
    script_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn run_git_diff_checker(_repo_path: &str, _filename: &str) -> Result<String, HookError> {
    let project_root = get_project_root();
    let binary_path = project_root
        .join("target")
        .join("release")
        .join("git_diff_checker");

    // Try release binary first, then fall back to cargo run
    let output = if binary_path.exists() {
        process::Command::new(binary_path.to_str().unwrap())
            .current_dir(&project_root)
            .output()
    } else {
        process::Command::new("cargo")
            .args(["run", "--release"])
            .current_dir(&project_root)
            .output()
    };

    let output = output
        .map_err(|e| HookError::CommandFailed(format!("Failed to run git_diff_checker: {}", e)))?;

    // Combine stdout and stderr
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    Ok(format!("{}\n{}", stdout, stderr))
}

fn parse_git_diff_checker_output(output: &str) -> Result<(bool, bool), HookError> {
    // Returns (modification_detected, reversion_performed)

    let modification_detected = output.contains("MODIFICATIONS DETECTED")
        || output.contains("Found")
        || output.contains("hunk(s) in the diff");

    let reversion_performed =
        output.contains("Successfully reverted") && output.contains("hunk(s)");

    // Check for "No modifications detected" - clean state
    let clean_state = output.contains("No modifications detected");

    if clean_state {
        Ok((false, false))
    } else if modification_detected || reversion_performed {
        Ok((modification_detected, reversion_performed))
    } else {
        Err(HookError::ParseError(format!(
            "Could not determine check result from output: {}",
            output
        )))
    }
}

fn handle_post_tool_use(input: &HookInput) -> Result<HookOutput, HookError> {
    let project_root = get_project_root();
    let repo_path = project_root.join("test").join("test1");
    let filename = "hello_world.c";

    let repo_path_str = repo_path.to_string_lossy().to_string();

    // Run git_diff_checker
    let output = run_git_diff_checker(&repo_path_str, filename)?;

    // Parse output
    let (modification_detected, reversion_performed) = parse_git_diff_checker_output(&output)?;

    Ok(HookOutput {
        hook_specific_output: HookSpecificOutput {
            hook_event_name: input.hook_event_name.clone(),
            check_result: if reversion_performed {
                "reverted".to_string()
            } else if modification_detected {
                "detected".to_string()
            } else {
                "clean".to_string()
            },
            check_result_reason: format!(
                "git_diff_checker: {}{}",
                if modification_detected {
                    format!("modifications detected. ")
                } else {
                    String::new()
                },
                if reversion_performed {
                    "Changes reverted successfully.".to_string()
                } else {
                    "No unauthorized changes detected.".to_string()
                }
            ),
            reversion_performed,
        },
    })
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && (args[1] == "--help" || args[1] == "-h") {
        println!(
            "Qwen Code PostToolUse Hook: Check git_diff_checker results

Usage: {} [options]

This hook runs git_diff_checker after tool execution to detect and revert
unauthorized changes to files outside src/.

Options:
  --help, -h    Show this help message",
            args[0]
        );
        return Ok(());
    }

    let mut input_str = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut input_str) {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("Error reading stdin: {}", e),
        ));
    }

    let input: HookInput = match serde_json::from_str(&input_str) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Error parsing JSON: {}", e);
            process::exit(2);
        }
    };

    // Handle only PostToolUse events
    if input.hook_event_name != "PostToolUse" {
        process::exit(0);
    }

    match handle_post_tool_use(&input) {
        Ok(output) => {
            let output_json = serde_json::to_string_pretty(&output).unwrap();
            println!("{}", output_json);
            process::exit(0);
        }
        Err(e) => {
            eprintln!("Hook error: {}", e);
            // Create error output even on failure
            let error_output = HookOutput {
                hook_specific_output: HookSpecificOutput {
                    hook_event_name: input.hook_event_name.clone(),
                    check_result: "error".to_string(),
                    check_result_reason: e.to_string(),
                    reversion_performed: false,
                },
            };
            let output_json = serde_json::to_string_pretty(&error_output).unwrap();
            println!("{}", output_json);
            process::exit(2);
        }
    }
}
