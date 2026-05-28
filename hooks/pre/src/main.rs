#!/usr/bin/env -S cargo -E run
/// Qwen Code PreToolUse Hook: Enforce path whitelist and blacklist for file modifications.
///
/// This hook:
/// - Allows read operations (read_file, glob, grep_search) on any file
/// - Allows write/edit operations ONLY on files inside whitelisted directories AND not in blocked paths
/// - Parses Bash commands to detect file writes outside whitelisted directories
/// - Whitelist configured via HOOK_ALLOWED_DIRS env var (comma-separated, default: "src")
/// - Blacklist configured via HOOK_BLOCKED_PATHS env var (comma-separated, default: none)
use common::{Hook, HookEngine, HookHandler, HookOutput, PreToolUseHookOutput};
use std::path::Path;

// ==========================================
// Whitelist Configuration
// ==========================================

/// Parse the whitelist of allowed directories from the HOOK_ALLOWED_DIRS env var.
/// Defaults to ["src"] for backward compatibility.
fn parse_allowed_dirs() -> Vec<String> {
    match std::env::var("HOOK_ALLOWED_DIRS") {
        Ok(val) => val.split(',').map(|s| s.trim().to_string()).collect(),
        Err(_) => vec!["src".to_string()],
    }
}

// ==========================================
// Blacklist Configuration
// ==========================================

/// Parse the blacklist of blocked paths from the HOOK_BLOCKED_PATHS env var.
/// Defaults to empty (no blocked paths).
fn parse_blocked_paths() -> Vec<String> {
    match std::env::var("HOOK_BLOCKED_PATHS") {
        Ok(val) => val.split(',').map(|s| s.trim().to_string()).collect(),
        Err(_) => Vec::new(),
    }
}

// ==========================================
// Path Validation
// ==========================================

/// Validate a file path for suspicious patterns (path traversal, control chars, etc.)
/// Uses shell-sanitize-rules to reject dangerous path strings.
fn validate_file_path(path: &str) -> bool {
    use shell_sanitize_rules::presets;
    let sanitizer = presets::file_path();
    let result = sanitizer.sanitize(path);
    match result {
        Ok(_) => true,
        Err(_) => false,
    }
}

// ==========================================
// Directory Whitelist Check
// ==========================================

/// Check if a file path is within any of the allowed directories.
/// Handles both absolute and relative paths with canonicalization.
fn is_in_allowed_dirs(file_path: &str, project_root: &Path, allowed_dirs: &[String]) -> bool {
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

    for dir in allowed_dirs {
        let allowed_path = project_root.join(dir);
        let canonical_allowed = allowed_path.canonicalize().unwrap_or(allowed_path);
        if canonical_target.starts_with(&canonical_allowed) {
            return true;
        }
    }

    false
}

// ==========================================
// Path Blacklist Check
// ==========================================

/// Check if a file path matches any blocked path (file or directory).
/// Uses same canonicalization logic as the whitelist check.
fn is_in_blocked_paths(file_path: &str, project_root: &Path, blocked_paths: &[String]) -> bool {
    if blocked_paths.is_empty() {
        return false;
    }

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

    for entry in blocked_paths {
        let blocked_path = project_root.join(entry);
        let canonical_blocked = blocked_path.canonicalize().unwrap_or(blocked_path);
        if canonical_target.starts_with(&canonical_blocked) {
            return true;
        }
    }

    false
}

// ==========================================
// Bash Command Parsing
// ==========================================

/// Extract suspected file paths from a Bash command string.
///
/// Uses `shlex` to tokenize the command, then looks for common write-operation
/// patterns: redirections (>), sed -i, tee, cp, mv, install, dd of=.
///
/// This is a heuristic and intentionally misses some edge cases.
/// The PostToolUse hook (which checks ALL modified files) is the safety net.
fn extract_paths_from_bash_command(command: &str) -> Vec<String> {
    let tokens = match shlex::split(command) {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut paths: Vec<String> = Vec::new();
    let n: usize = tokens.len();
    let mut i: usize = 0;

    while i < n {
        let token = &tokens[i];

        // Redirect patterns: ">" FILE, ">>" FILE, "1>" FILE, "2>" FILE
        if *token == ">" || *token == ">>" || *token == "1>" || *token == "2>" {
            if i + 1 < n {
                paths.push(tokens[i + 1].clone());
            }
        }

        // Redirect with no space: ">file" (shlex may tokenize as ">file")
        if token.starts_with('>') && token.len() > 1 && !token.contains('&') {
            let path = &token[1..];
            paths.push(path.to_string());
        }

        // tee writes to all its non-flag arguments
        if *token == "tee" {
            let mut j: usize = i + 1;
            while j < n && !tokens[j].starts_with('-') {
                paths.push(tokens[j].clone());
                j += 1;
            }
        }

        // sed -i: the target file is the last non-flag argument
        // sed -i 's/a/b/' file   → file is last arg
        // sed -ibak 's/a/b/' file → file is last arg
        // sed -i.bak 's/a/b/' file → file is last arg
        if *token == "sed" {
            let mut last_non_flag: Option<String> = None;
            let mut j: usize = i + 1;
            while j < n {
                if !tokens[j].starts_with('-') {
                    last_non_flag = Some(tokens[j].clone());
                }
                j += 1;
            }
            match last_non_flag {
                Some(f) => paths.push(f),
                None => {}
            }
        }

        // cp/mv: destination is the last non-flag argument
        if *token == "cp" || *token == "mv" {
            let mut candidate: Option<String> = None;
            let mut j: usize = i + 1;
            while j < n {
                if !tokens[j].starts_with('-') {
                    candidate = Some(tokens[j].clone());
                }
                j += 1;
            }
            match candidate {
                Some(dest) => paths.push(dest),
                None => {}
            }
        }

        // install: last non-flag argument is destination
        if *token == "install" {
            let mut candidate: Option<String> = None;
            let mut j: usize = i + 1;
            while j < n {
                if !tokens[j].starts_with('-') {
                    candidate = Some(tokens[j].clone());
                }
                j += 1;
            }
            match candidate {
                Some(dest) => paths.push(dest),
                None => {}
            }
        }

        // dd of=<path> syntax
        if token.starts_with("of=") && token.len() > 3 {
            let path = &token[3..];
            paths.push(path.to_string());
        }

        i += 1;
    }

    paths
}

// ==========================================
// Plugin Implementation
// ==========================================

pub struct MyPlugin;

impl HookHandler for MyPlugin {
    fn execute(&self, hook: &mut Hook) -> Result<HookOutput, String> {
        let allowed_dirs = parse_allowed_dirs();
        let blocked_paths = parse_blocked_paths();
        let project_root = hook.4.clone().unwrap().cwd.unwrap();
        let res = hook.0.as_pre_tool_use();
        let hi = match res {
            Some(a) => a,
            None => panic!("as_pre_tool_use failed!"),
        };

        let tool_name: &str = hi.tool_name.as_str();

        // Read operations — unrestricted
        let read_tools = ["read_file", "glob", "grep_search", "list_directory"];
        if read_tools.contains(&tool_name) {
            return Ok(PreToolUseHookOutput::make_pre_tool_output(
                common::HookDecision::Allow,
                true,
                format!("Read operation '{}' is allowed on any file", tool_name),
            ));
        }

        // Write/Edit operations — check file_path against whitelist
        let write_tools = ["Write", "Edit"];
        if write_tools.contains(&tool_name) {
            return handle_write_tool(&hi.tool_input, tool_name, &project_root, &allowed_dirs, &blocked_paths);
        }

        // Bash command — parse for file write operations
        if tool_name == "Bash" {
            return handle_bash_tool(&hi.tool_input, tool_name, &project_root, &allowed_dirs, &blocked_paths);
        }

        // Default fallback
        Ok(PreToolUseHookOutput::make_pre_tool_output(
            common::HookDecision::Allow,
            true,
            format!("Tool '{}' allowed (not a restricted operation)", tool_name),
        ))
    }
}

fn handle_write_tool(
    tool_input: &std::collections::HashMap<String, serde_json::Value>,
    tool_name: &str,
    project_root: &Path,
    allowed_dirs: &[String],
    blocked_paths: &[String],
) -> Result<HookOutput, String> {
    let file_path = tool_input
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

    if !is_in_allowed_dirs(file_path, project_root, allowed_dirs) {
        return Ok(PreToolUseHookOutput::make_pre_tool_output(
            common::HookDecision::Deny,
            true,
            format!(
                "Only files inside whitelisted directories can be modified. '{}' is outside.",
                file_path
            ),
        ));
    }

    if is_in_blocked_paths(file_path, project_root, blocked_paths) {
        return Ok(PreToolUseHookOutput::make_pre_tool_output(
            common::HookDecision::Deny,
            true,
            format!(
                "File '{}' is in a blocked path and cannot be modified.",
                file_path
            ),
        ));
    }

    return Ok(PreToolUseHookOutput::make_pre_tool_output(
        common::HookDecision::Allow,
        true,
        format!("File '{}' is inside whitelisted directory", file_path),
    ));
}

fn handle_bash_tool(
    tool_input: &std::collections::HashMap<String, serde_json::Value>,
    _tool_name: &str,
    project_root: &Path,
    allowed_dirs: &[String],
    blocked_paths: &[String],
) -> Result<HookOutput, String> {
    let command = tool_input
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if command.trim().is_empty() {
        return Ok(PreToolUseHookOutput::make_pre_tool_output(
            common::HookDecision::Allow,
            true,
            format!("Empty Bash command — allowed"),
        ));
    }

    // Block any forge command — model must use the Forge MCP server for testing
    if let Some(tokens) = shlex::split(command) {
        if tokens.first().map(|t| t.as_str()) == Some("forge") {
            return Ok(PreToolUseHookOutput::make_pre_tool_output(
                common::HookDecision::Deny,
                true,
                format!(
                    "The 'forge' command is not allowed. Please use the Forge MCP server for testing instead of running forge directly."
                ),
            ));
        }
    }

    let paths = extract_paths_from_bash_command(command);

    // If heuristic didn't find any write patterns, allow and let post-hook catch it
    if paths.is_empty() {
        return Ok(PreToolUseHookOutput::make_pre_tool_output(
            common::HookDecision::Allow,
            true,
            format!("Bash command '{}' — no write patterns detected", command),
        ));
    }

    // Check each extracted path against whitelist
    for path in &paths {
        if !validate_file_path(path) {
            return Ok(PreToolUseHookOutput::make_pre_tool_output(
                common::HookDecision::Deny,
                true,
                format!(
                    "Bash command contains suspicious path pattern: '{}'. Command was: {}",
                    path, command
                ),
            ));
        }

        if !is_in_allowed_dirs(path, project_root, allowed_dirs) {
            return Ok(PreToolUseHookOutput::make_pre_tool_output(
                common::HookDecision::Deny,
                true,
                format!(
                    "Bash command writes to '{}' which is outside allowed directories. Command was: {}",
                    path, command
                ),
            ));
        }

        if is_in_blocked_paths(path, project_root, blocked_paths) {
            return Ok(PreToolUseHookOutput::make_pre_tool_output(
                common::HookDecision::Deny,
                true,
                format!(
                    "Bash command writes to '{}' which is in a blocked path. Command was: {}",
                    path, command
                ),
            ));
        }
    }

    // All extracted paths pass validation and whitelist
    Ok(PreToolUseHookOutput::make_pre_tool_output(
        common::HookDecision::Allow,
        true,
        format!("Bash command allowed — all file paths are within allowed directories"),
    ))
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
    use common::{CommandRequest, Hook, HookEventName, HookInput, HookType};
    use std::collections::HashMap;
    use std::path::PathBuf;

    // Helper to create a Hook object for testing
    fn create_test_hook_with_command(tool: &str, command: &str, cwd: &str) -> Hook {
        let mut tool_input = HashMap::new();
        tool_input.insert(
            "command".to_string(),
            serde_json::Value::String(command.to_string()),
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
        // Target is in /sc/ (not /src/) using Write tool
        let mut hook = create_test_hook(
            "Write",
            "/home/robertodr/gits/git_diff_checker/test/test1/sc/hello_world.c",
            "/home/robertodr/gits/git_diff_checker/test/test1",
        );

        let result = plugin.execute(&mut hook).expect("Error");
        let output = result.as_pre_tool().unwrap();

        assert_eq!(
            output.hook_specific_output.as_ref().unwrap()["permissionDecision"]
                .as_str()
                .unwrap(),
            "deny",
            "outside src should be denied"
        );
    }

    #[test]
    fn test_input2_allow_inside_src() {
        let plugin = MyPlugin;
        // Target is in /src/ using Write tool
        let mut hook = create_test_hook(
            "Write",
            "/home/robertodr/gits/git_diff_checker/test/test1/src/hello_world.c",
            "/home/robertodr/gits/git_diff_checker/test/test1",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(
            output.hook_specific_output.as_ref().unwrap()["permissionDecision"]
                .as_str()
                .unwrap(),
            "allow",
            "inside src should be allowed"
        );
    }

    #[test]
    fn test_input3_allow_read_anywhere() {
        let plugin = MyPlugin;
        let mut hook = create_test_hook(
            "read_file",
            "/home/robertodr/gits/git_diff_checker/test/test1/sc/hello_world.c",
            "/home/robertodr/gits/git_diff_checker/test/test1",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(
            output.hook_specific_output.as_ref().unwrap()["permissionDecision"]
                .as_str()
                .unwrap(),
            "allow",
            "read anywhere should be allowed"
        );
    }

    #[test]
    fn test_input4_allow_unknown_tool() {
        let plugin = MyPlugin;
        let mut hook = create_test_hook(
            "ex",
            "/home/robertodr/gits/git_diff_checker/test/test1/sc/hello_world.c",
            "/home/robertodr/gits/git_diff_checker/test/test1",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(
            output.hook_specific_output.as_ref().unwrap()["permissionDecision"]
                .as_str()
                .unwrap(),
            "allow",
            "unknown tool should be allowed"
        );
    }

    // ==========================================
    // Bash Command Tests
    // ==========================================

    #[test]
    fn test_bash_write_to_allowed_dir() {
        let plugin = MyPlugin;
        // sed -i writing to a file inside src/
        let mut hook = create_test_hook_with_command(
            "Bash",
            "sed -i 's/foo/bar/' src/hello_world.c",
            "/home/robertodr/gits/git_diff_checker/test/test1",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(
            output.hook_specific_output.as_ref().unwrap()["permissionDecision"]
                .as_str()
                .unwrap(),
            "allow",
            "Bash sed -i inside src/ should be allowed"
        );
    }

    #[test]
    fn test_bash_write_to_forbidden_dir() {
        let plugin = MyPlugin;
        // echo redirect to a file outside src/
        let mut hook = create_test_hook_with_command(
            "Bash",
            "echo 'new content' > config/settings.json",
            "/home/robertodr/gits/git_diff_checker/test/test1",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(
            output.hook_specific_output.as_ref().unwrap()["permissionDecision"]
                .as_str()
                .unwrap(),
            "deny",
            "Bash write to config/ should be denied with default whitelist"
        );
    }

    #[test]
    fn test_bash_tee_outside_src() {
        let plugin = MyPlugin;
        // tee writing to a file outside src/
        let mut hook = create_test_hook_with_command(
            "Bash",
            "echo 'content' | tee docs/readme.md",
            "/home/robertodr/gits/git_diff_checker/test/test1",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(
            output.hook_specific_output.as_ref().unwrap()["permissionDecision"]
                .as_str()
                .unwrap(),
            "deny",
            "Bash tee to docs/ should be denied with default whitelist"
        );
    }

    #[test]
    fn test_bash_no_write_cmd_allowed() {
        let plugin = MyPlugin;
        // Compilation command — no file writes detected by heuristic
        let mut hook = create_test_hook_with_command(
            "Bash",
            "gcc -c src/hello_world.c -o build/foo.o",
            "/home/robertodr/gits/git_diff_checker/test/test1",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(
            output.hook_specific_output.as_ref().unwrap()["permissionDecision"]
                .as_str()
                .unwrap(),
            "allow",
            "Bash compile command with no write patterns should be allowed"
        );
    }

    #[test]
    fn test_bash_read_cmd_allowed() {
        let plugin = MyPlugin;
        // cat is a read operation — should be allowed
        let mut hook = create_test_hook_with_command(
            "Bash",
            "cat /etc/passwd",
            "/home/robertodr/gits/git_diff_checker/test/test1",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(
            output.hook_specific_output.as_ref().unwrap()["permissionDecision"]
                .as_str()
                .unwrap(),
            "allow",
            "Bash cat (read) should be allowed"
        );
    }

    // ==========================================
    // Unit tests for extract_paths_from_bash_command
    // ==========================================

    #[test]
    fn test_extract_redirect_path() {
        let paths = extract_paths_from_bash_command("echo hello > /tmp/out.txt");
        assert!(paths.contains(&"/tmp/out.txt".to_string()));
    }

    #[test]
    fn test_extract_sed_i_path() {
        let paths = extract_paths_from_bash_command("sed -i 's/a/b/' src/file.c");
        assert!(paths.contains(&"src/file.c".to_string()));
    }

    #[test]
    fn test_extract_no_write_patterns() {
        let paths = extract_paths_from_bash_command("ls -la");
        assert!(paths.is_empty());
    }

    #[test]
    fn test_extract_tee_path() {
        let paths = extract_paths_from_bash_command("echo data | tee output.log");
        assert!(paths.contains(&"output.log".to_string()));
    }

    #[test]
    fn test_extract_malformed_command() {
        // Unclosed quote — shlex returns None
        let paths = extract_paths_from_bash_command("echo 'unclosed");
        assert!(paths.is_empty());
    }

    // ==========================================
    // Forge Command Blocking Tests (/tmp/prehooktest)
    // ==========================================

    #[test]
    fn test_forge_test_is_denied() {
        let plugin = MyPlugin;
        let mut hook = create_test_hook_with_command(
            "Bash",
            "forge test",
            "/tmp/prehooktest",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(
            output
                .hook_specific_output
                .as_ref()
                .unwrap()["permissionDecision"]
                .as_str()
                .unwrap(),
            "deny",
            "forge test should be denied"
        );
    }

    #[test]
    fn test_forge_build_is_denied() {
        let plugin = MyPlugin;
        let mut hook = create_test_hook_with_command(
            "Bash",
            "forge build",
            "/tmp/prehooktest",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(
            output
                .hook_specific_output
                .as_ref()
                .unwrap()["permissionDecision"]
                .as_str()
                .unwrap(),
            "deny",
            "forge build should be denied"
        );
    }

    #[test]
    fn test_forge_script_is_denied() {
        let plugin = MyPlugin;
        let mut hook = create_test_hook_with_command(
            "Bash",
            "forge script script/Deploy.s.sol --broadcast",
            "/tmp/prehooktest",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(
            output
                .hook_specific_output
                .as_ref()
                .unwrap()["permissionDecision"]
                .as_str()
                .unwrap(),
            "deny",
            "forge script should be denied"
        );
    }

    #[test]
    fn test_forge_deny_message_mentions_mcp_server() {
        let plugin = MyPlugin;
        let mut hook = create_test_hook_with_command(
            "Bash",
            "forge test --match-test testFoo",
            "/tmp/prehooktest",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        let reason = output
            .hook_specific_output
            .as_ref()
            .unwrap()["permissionDecisionReason"]
            .as_str()
            .unwrap();
        assert!(
            reason.contains("MCP server"),
            "deny reason should mention MCP server, got: {}",
            reason
        );
    }

    #[test]
    fn test_forge_deny_reason_present() {
        let plugin = MyPlugin;
        let mut hook = create_test_hook_with_command(
            "Bash",
            "forge install",
            "/tmp/prehooktest",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        let reason = output.reason.as_ref().unwrap();
        assert!(
            reason.contains("forge"),
            "reason should reference forge command, got: {}",
            reason
        );
    }

    #[test]
    fn test_non_forge_command_allowed() {
        let plugin = MyPlugin;
        // Any command NOT starting with 'forge' should not trigger the block
        let mut hook = create_test_hook_with_command(
            "Bash",
            "forgeapp --version",
            "/tmp/prehooktest",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(
            output
                .hook_specific_output
                .as_ref()
                .unwrap()["permissionDecision"]
                .as_str()
                .unwrap(),
            "allow",
            "forgeapp (unrelated command) should be allowed"
        );
    }

    // ==========================================
    // Blocked Path Tests
    // ==========================================

    #[test]
    fn test_is_in_blocked_paths_matches_file() {
        let root = Path::new("/home/robertodr/gits/git_diff_checker/test/test1");
        let blocked = vec!["src/test.txt".to_string()];
        assert!(
            is_in_blocked_paths("src/test.txt", root, &blocked),
            "should match src/test.txt"
        );
    }

    #[test]
    fn test_is_in_blocked_paths_no_match_for_unblocked() {
        let root = Path::new("/home/robertodr/gits/git_diff_checker/test/test1");
        let blocked = vec!["src/test.txt".to_string()];
        assert!(
            !is_in_blocked_paths("src/hello_world.c", root, &blocked),
            "should not match src/hello_world.c"
        );
    }

    #[test]
    fn test_is_in_blocked_paths_empty_blocklist() {
        let root = Path::new("/home/robertodr/gits/git_diff_checker/test/test1");
        let blocked: Vec<String> = Vec::new();
        assert!(
            !is_in_blocked_paths("src/test.txt", root, &blocked),
            "empty blocklist should match nothing"
        );
    }

    #[test]
    fn test_write_blocked_path_denies() {
        // Set env var so execute() picks it up
        std::env::set_var("HOOK_BLOCKED_PATHS", "src/test.txt");
        let plugin = MyPlugin;
        let mut hook = create_test_hook(
            "Write",
            "/home/robertodr/gits/git_diff_checker/test/test1/src/test.txt",
            "/home/robertodr/gits/git_diff_checker/test/test1",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(
            output.hook_specific_output.as_ref().unwrap()["permissionDecision"]
                .as_str()
                .unwrap(),
            "deny",
            "write to blocked path should be denied"
        );
    }

    #[test]
    fn test_write_allowed_path_not_blocked() {
        std::env::set_var("HOOK_BLOCKED_PATHS", "src/test.txt");
        let plugin = MyPlugin;
        let mut hook = create_test_hook(
            "Write",
            "/home/robertodr/gits/git_diff_checker/test/test1/src/hello_world.c",
            "/home/robertodr/gits/git_diff_checker/test/test1",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(
            output.hook_specific_output.as_ref().unwrap()["permissionDecision"]
                .as_str()
                .unwrap(),
            "allow",
            "write to unblocked path in allowed dir should be allowed"
        );
    }

    #[test]
    fn test_bash_blocked_path_denies() {
        std::env::set_var("HOOK_BLOCKED_PATHS", "src/test.txt");
        let plugin = MyPlugin;
        let mut hook = create_test_hook_with_command(
            "Bash",
            "echo 'data' >> src/test.txt",
            "/home/robertodr/gits/git_diff_checker/test/test1",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(
            output.hook_specific_output.as_ref().unwrap()["permissionDecision"]
                .as_str()
                .unwrap(),
            "deny",
            "bash write to blocked path should be denied"
        );
    }

    #[test]
    fn test_bash_allowed_path_not_blocked() {
        std::env::set_var("HOOK_BLOCKED_PATHS", "src/test.txt");
        let plugin = MyPlugin;
        let mut hook = create_test_hook_with_command(
            "Bash",
            "sed -i 's/foo/bar/' src/hello_world.c",
            "/home/robertodr/gits/git_diff_checker/test/test1",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(
            output.hook_specific_output.as_ref().unwrap()["permissionDecision"]
                .as_str()
                .unwrap(),
            "allow",
            "bash write to unblocked path in allowed dir should be allowed"
        );

        // Clean up env var to avoid leaking into subsequent tests
        std::env::remove_var("HOOK_BLOCKED_PATHS");
    }

    // ==========================================
    // Absolute Path Outside Repo Test
    // ==========================================
    fn test_bash_write_absolute_path_outside_repo() {
        let plugin = MyPlugin;
        // Write to /tmp/ which is outside the project
        let mut hook = create_test_hook_with_command(
            "Bash",
            "echo 'hello' > /tmp/test.txt",
            "/home/robertodr/gits/git_diff_checker/test/test1",
        );

        let result = plugin.execute(&mut hook).unwrap();
        let output = result.as_pre_tool().unwrap();

        assert_eq!(
            output.hook_specific_output.as_ref().unwrap()["permissionDecision"]
                .as_str()
                .unwrap(),
            "deny",
            "Bash write to absolute /tmp/ path should be denied"
        );
    }
}
