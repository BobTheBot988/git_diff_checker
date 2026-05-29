/// Claude Code Stop Hook: Check if coverage report exists before allowing stop.
///
/// Blocks the stop with a message telling the model to use mcp_synth to test
/// with halmos when /tmp/coverage.info is missing.
use common::{
    Hook, HookDecision, HookEngine, HookEventName, HookHandler, HookOutput, HookType, StopHookOutput,
};

// ==========================================
// Plugin Implementation
// ==========================================

struct StopPlugin;

impl HookHandler for StopPlugin {
    fn execute(&self, _hook: &mut Hook) -> Result<HookOutput, String> {
        let coverage_path = std::path::Path::new("/tmp/coverage.info");
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

        Ok(HookOutput::Stop(StopHookOutput {
            decision: Some(HookDecision::Block),
            reason: Some(
                "Coverage report not found at /tmp/coverage.info. \
                 Use the mcp_synth tool to test with halmos before stopping."
                    .to_string(),
            ),
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
