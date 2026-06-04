/// Claude Code Stop Hook: Check if coverage report exists before allowing stop.
///
/// Blocks the stop with a message telling the model to use mcp_synth to test
/// with halmos when coverage.info is missing from the current working directory.
use common::{
    CoverageJson, Hook, HookDecision, HookEngine, HookEventName, HookHandler, HookOutput, HookType,
    StopHookOutput,
};
use std::path::PathBuf;

// ==========================================
// Coverage Path Resolution
// ==========================================

fn get_coverage_path(hook: &Hook) -> PathBuf {
    match hook.4.as_ref() {
        Some(req) => match req.cwd.clone() {
            Some(cwd) => cwd.join("coverage.info"),
            None => PathBuf::from("coverage.info"),
        },
        None => PathBuf::from("coverage.info"),
    }
}

fn get_coverage_json_path(hook: &Hook) -> PathBuf {
    match hook.4.as_ref() {
        Some(req) => match req.cwd.clone() {
            Some(cwd) => cwd.join("coverage.json"),
            None => PathBuf::from("coverage.json"),
        },
        None => PathBuf::from("coverage.json"),
    }
}

fn parse_coverage_json(path: &PathBuf) -> Result<CoverageJson, String> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return Err(format!("Failed to read coverage.json: {}", e));
        }
    };
    match serde_json::from_str::<CoverageJson>(&content) {
        Ok(c) => Ok(c),
        Err(e) => Err(format!("Failed to parse coverage.json: {}", e)),
    }
}

fn serialize_coverage_json(coverage: &CoverageJson) -> String {
    match serde_json::to_string_pretty(coverage) {
        Ok(s) => s,
        Err(_e) => {
            format!(
                "{{ \"exitcode\": {} }}",
                coverage.exitcode
            )
        }
    }
}

// ==========================================
// Plugin Implementation
// ==========================================

struct StopPlugin;

impl HookHandler for StopPlugin {
    fn execute(&self, hook: &mut Hook) -> Result<HookOutput, String> {
        // Primary check: coverage.json with exitcode validation
        let coverage_json_path = get_coverage_json_path(hook);
        if coverage_json_path.exists() {
            let coverage = match parse_coverage_json(&coverage_json_path) {
                Ok(c) => c,
                Err(e) => {
                    return Ok(HookOutput::Stop(StopHookOutput {
                        decision: Some(HookDecision::Block),
                        reason: Some(e),
                        cont: Some(true),
                        stop_reason: None,
                        suppress_output: None,
                        system_message: None,
                        terminal_sequence: None,
                    }));
                }
            };

            if coverage.exitcode == 0 {
                return Ok(HookOutput::Stop(StopHookOutput {
                    decision: None,
                    reason: None,
                    cont: Some(true),
                    stop_reason: None,
                    suppress_output: None,
                    system_message: None,
                    terminal_sequence: None,
                }));
            }

            let json_str = serialize_coverage_json(&coverage);
            let reason = format!(
                "Halmos tests failed with exitcode: {}\n\nCoverage report:\n{}",
                coverage.exitcode, json_str
            );

            return Ok(HookOutput::Stop(StopHookOutput {
                decision: Some(HookDecision::Block),
                reason: Some(reason),
                cont: Some(true),
                stop_reason: None,
                suppress_output: None,
                system_message: None,
                terminal_sequence: None,
            }));
        }

        // Fallback: check coverage.info for backward compat
        let coverage_path = get_coverage_path(hook);
        if coverage_path.exists() {
            return Ok(HookOutput::Stop(StopHookOutput {
                decision: None,
                reason: None,
                cont: Some(true),
                stop_reason: None,
                suppress_output: None,
                system_message: None,
                terminal_sequence: None,
            }));
        }

        let reason = format!(
            "Coverage report not found at {}. \
             Use the mcp_synth tool to test with halmos before stopping.",
            coverage_path.display()
        );

        Ok(HookOutput::Stop(StopHookOutput {
            decision: Some(HookDecision::Block),
            reason: Some(reason),
            cont: Some(true),
            stop_reason: None,
            suppress_output: None,
            system_message: None,
            terminal_sequence: None,
        }))
    }
}

fn main() {
    let plugin = StopPlugin;

    // Initialize hook (reads from stdin via Hook::new)
    let h = Hook::new(HookEventName::Stop, HookType::Command);

    // Engine handles execution and automatic printing of JSON to stdout
    HookEngine::run_hook(plugin, h);

    // Always exit with code 0 - JSON output handles blocking decisions
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{CommandRequest, Hook, HookEventName, HookInput, HookType, PermissionMode, StopInput};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_dir(label: &str) -> PathBuf {
        let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("stop_hook_test_{}_{}", label, n))
    }

    fn create_test_hook(cwd: &str) -> Hook {
        let input = StopInput {
            session_id: None,
            transcript_path: None,
            permission_mode: PermissionMode::Default,
            effort: None,
            tool_input: HashMap::new(),
        };

        let req = CommandRequest {
            hook_event_name: Some("Stop".to_string()),
            cwd: Some(PathBuf::from(cwd)),
            tool_input: None,
            extra_fields: HashMap::new(),
        };

        Hook(
            HookInput::Stop(input),
            None,
            HookEventName::Stop,
            HookType::Command,
            Some(req),
        )
    }

    #[test]
    fn test_allows_stop_when_coverage_json_exitcode_0() {
        let dir = test_dir("json_ok");
        std::fs::create_dir_all(&dir).expect("failed to create test dir");
        let json_data = r#"{"exitcode": 0, "test_results": {}}"#;
        std::fs::write(dir.join("coverage.json"), json_data)
            .expect("failed to write coverage.json");

        let mut hook = create_test_hook(dir.to_str().expect("non-utf8 path"));
        let plugin = StopPlugin;
        let result = plugin.execute(&mut hook).expect("execute failed");
        let output = result.as_stop().expect("expected Stop output");

        assert!(output.decision.is_none(), "should not block when exitcode is 0");

        std::fs::remove_dir_all(&dir).expect("failed to clean up test dir");
    }

    #[test]
    fn test_blocks_when_coverage_json_exitcode_nonzero() {
        let dir = test_dir("json_fail");
        std::fs::create_dir_all(&dir).expect("failed to create test dir");
        let json_data = r#"{"exitcode": 1, "test_results": {"Test.sol:Test": [{"name": "check_test", "exitcode": 1}]}}"#;
        std::fs::write(dir.join("coverage.json"), json_data)
            .expect("failed to write coverage.json");

        let mut hook = create_test_hook(dir.to_str().expect("non-utf8 path"));
        let plugin = StopPlugin;
        let result = plugin.execute(&mut hook).expect("execute failed");
        let output = result.as_stop().expect("expected Stop output");

        assert_eq!(
            output.decision,
            Some(HookDecision::Block),
            "should block when exitcode is non-zero"
        );

        let reason = output.reason.as_ref().expect("reason should be present");
        assert!(
            reason.contains("exitcode: 1"),
            "reason should mention exitcode 1, got: {}",
            reason
        );
        assert!(
            reason.contains("coverage"),
            "reason should mention coverage, got: {}",
            reason
        );

        std::fs::remove_dir_all(&dir).expect("failed to clean up test dir");
    }

    #[test]
    fn test_blocks_when_coverage_json_parse_error() {
        let dir = test_dir("json_bad");
        std::fs::create_dir_all(&dir).expect("failed to create test dir");
        std::fs::write(dir.join("coverage.json"), "not valid json at all")
            .expect("failed to write coverage.json");

        let mut hook = create_test_hook(dir.to_str().expect("non-utf8 path"));
        let plugin = StopPlugin;
        let result = plugin.execute(&mut hook).expect("execute failed");
        let output = result.as_stop().expect("expected Stop output");

        assert_eq!(
            output.decision,
            Some(HookDecision::Block),
            "should block when coverage.json is malformed"
        );

        let reason = output.reason.as_ref().expect("reason should be present");
        assert!(
            reason.contains("parse"),
            "reason should mention parse error, got: {}",
            reason
        );

        std::fs::remove_dir_all(&dir).expect("failed to clean up test dir");
    }

    #[test]
    fn test_reason_includes_failed_test_details() {
        let dir = test_dir("json_details");
        std::fs::create_dir_all(&dir).expect("failed to create test dir");
        let json_data = r#"{
            "exitcode": 4,
            "test_results": {
                "test/Lottery.t.sol:LotteryTest": [
                    {"name": "check_LotSystemInvariants", "exitcode": 0}
                ],
                "test/Taxpayer.t.sol:TaxpayerTest": [
                    {"name": "check_SystemInvariants", "exitcode": 4}
                ]
            }
        }"#;
        std::fs::write(dir.join("coverage.json"), json_data)
            .expect("failed to write coverage.json");

        let mut hook = create_test_hook(dir.to_str().expect("non-utf8 path"));
        let plugin = StopPlugin;
        let result = plugin.execute(&mut hook).expect("execute failed");
        let output = result.as_stop().expect("expected Stop output");

        assert_eq!(
            output.decision,
            Some(HookDecision::Block),
            "should block when tests fail"
        );

        let reason = output.reason.as_ref().expect("reason should be present");
        assert!(
            reason.contains("exitcode: 4"),
            "reason should mention exitcode 4, got: {}",
            reason
        );
        assert!(
            reason.contains("check_SystemInvariants"),
            "reason should include failing test name, got: {}",
            reason
        );
        assert!(
            reason.contains("TaxpayerTest"),
            "reason should include failing contract, got: {}",
            reason
        );

        std::fs::remove_dir_all(&dir).expect("failed to clean up test dir");
    }

    #[test]
    fn test_falls_back_to_coverage_info() {
        let dir = test_dir("fallback");
        std::fs::create_dir_all(&dir).expect("failed to create test dir");
        // Only write coverage.info, no coverage.json
        std::fs::write(dir.join("coverage.info"), "test coverage data")
            .expect("failed to write coverage.info");

        let mut hook = create_test_hook(dir.to_str().expect("non-utf8 path"));
        let plugin = StopPlugin;
        let result = plugin.execute(&mut hook).expect("execute failed");
        let output = result.as_stop().expect("expected Stop output");

        assert!(
            output.decision.is_none(),
            "should not block when coverage.info exists as fallback"
        );

        std::fs::remove_dir_all(&dir).expect("failed to clean up test dir");
    }

    #[test]
    fn test_blocks_stop_when_no_coverage_files() {
        let dir = test_dir("none");
        std::fs::create_dir_all(&dir).expect("failed to create test dir");
        // No coverage files at all

        let mut hook = create_test_hook(dir.to_str().expect("non-utf8 path"));
        let plugin = StopPlugin;
        let result = plugin.execute(&mut hook).expect("execute failed");
        let output = result.as_stop().expect("expected Stop output");

        assert_eq!(
            output.decision,
            Some(HookDecision::Block),
            "should block when no coverage files exist"
        );

        let reason = output.reason.as_ref().expect("reason should be present");
        assert!(
            reason.contains("coverage.info"),
            "reason should mention coverage.info, got: {}",
            reason
        );
        assert!(
            reason.contains("mcp_synth"),
            "reason should mention mcp_synth, got: {}",
            reason
        );

        std::fs::remove_dir_all(&dir).expect("failed to clean up test dir");
    }

    #[test]
    fn test_reason_includes_checked_path_when_missing() {
        let dir = test_dir("path");
        std::fs::create_dir_all(&dir).expect("failed to create test dir");

        let mut hook = create_test_hook(dir.to_str().expect("non-utf8 path"));
        let plugin = StopPlugin;
        let result = plugin.execute(&mut hook).expect("execute failed");
        let output = result.as_stop().expect("expected Stop output");

        let reason = output.reason.as_ref().expect("reason should be present");
        let expected_path = dir.join("coverage.info").to_string_lossy().to_string();
        assert!(
            reason.contains(&expected_path),
            "reason should contain the full coverage.info path, got: {}",
            reason
        );

        std::fs::remove_dir_all(&dir).expect("failed to clean up test dir");
    }
}
