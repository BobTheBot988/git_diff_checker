/// Claude Code Stop Hook: Check if coverage report exists before allowing stop.
///
/// Blocks the stop with a message telling the model to use mcp_synth to test
/// with halmos when coverage.info is missing from the current working directory.
use common::{
    Hook, HookDecision, HookEngine, HookEventName, HookHandler, HookOutput, HookType, StopHookOutput,
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

// ==========================================
// Plugin Implementation
// ==========================================

struct StopPlugin;

impl HookHandler for StopPlugin {
    fn execute(&self, hook: &mut Hook) -> Result<HookOutput, String> {
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
    fn test_allows_stop_when_coverage_exists() {
        let dir = test_dir("exists");
        std::fs::create_dir_all(&dir).expect("failed to create test dir");
        std::fs::write(dir.join("coverage.info"), "test coverage data")
            .expect("failed to write coverage file");

        let mut hook = create_test_hook(dir.to_str().expect("non-utf8 path"));
        let plugin = StopPlugin;
        let result = plugin.execute(&mut hook).expect("execute failed");
        let output = result.as_stop().expect("expected Stop output");

        assert!(output.decision.is_none(), "should not block when coverage exists");

        std::fs::remove_dir_all(&dir).expect("failed to clean up test dir");
    }

    #[test]
    fn test_blocks_stop_when_coverage_missing() {
        let dir = test_dir("missing");
        std::fs::create_dir_all(&dir).expect("failed to create test dir");
        // Intentionally not creating coverage.info

        let mut hook = create_test_hook(dir.to_str().expect("non-utf8 path"));
        let plugin = StopPlugin;
        let result = plugin.execute(&mut hook).expect("execute failed");
        let output = result.as_stop().expect("expected Stop output");

        assert_eq!(
            output.decision,
            Some(HookDecision::Block),
            "should block when coverage is missing"
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
    fn test_reason_includes_checked_path() {
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
            "reason should contain the full path, got: {}",
            reason
        );

        std::fs::remove_dir_all(&dir).expect("failed to clean up test dir");
    }
}
