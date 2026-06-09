/// PostToolUse hook for synthesis tool stop/continue decisions.
///
/// Inspects the rendered `tool_response` from synthesis tools (forge_build,
/// forge_test, run_synthesis) and decides whether the agent should stop
/// (all invariants proven / partial proof) or continue (failure / no match).
use std::collections::HashMap;
use std::process;

use common::{
    Hook, HookDecision, HookEngine, HookEventName, HookHandler, HookOutput, HookType,
    PostToolUseHookOutput,
};

// ==========================================
// Classifier
// ==========================================

/// Classification of a synthesis tool response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Classification {
    FullSuccess,
    PartialSuccess,
    Failure,
    Default,
}

/// Recursively check whether any string value at any depth in a
/// `serde_json::Value` contains the given substring pattern.
fn value_contains_string(value: &serde_json::Value, pattern: &str) -> bool {
    match value {
        serde_json::Value::String(s) => s.contains(pattern),
        serde_json::Value::Object(map) => map.values().any(|v| value_contains_string(v, pattern)),
        serde_json::Value::Array(arr) => arr.iter().any(|v| value_contains_string(v, pattern)),
        _ => false,
    }
}

/// Check whether any entry in `map` (at any nesting depth) contains `pattern`.
fn contains_pattern(map: &HashMap<String, serde_json::Value>, pattern: &str) -> bool {
    map.values().any(|v| value_contains_string(v, pattern))
}

/// Classify a tool response by searching for known result markers.
///
/// Priority (highest first):
/// 1. `"Halmos: all invariants proven"`  — full success → stop
/// 2. `"Halmos: partial proof"`          — partial success → stop
/// 3. `"Compilation failed."` / `"Tests failed."` / `"Halmos counterexample found."` — continue
/// 4. No match                           — default → continue
fn classify(map: &HashMap<String, serde_json::Value>) -> Classification {
    if contains_pattern(map, "Halmos: all invariants proven") {
        Classification::FullSuccess
    } else if contains_pattern(map, "Halmos: partial proof") {
        Classification::PartialSuccess
    } else if contains_pattern(map, "Compilation failed.")
        || contains_pattern(map, "Tests failed.")
        || contains_pattern(map, "Halmos counterexample found.")
    {
        Classification::Failure
    } else {
        Classification::Default
    }
}

// ==========================================
// Output builders
// ==========================================

/// Build output instructing the hook to stop execution.
fn stop_output(reason: &str) -> PostToolUseHookOutput {
    let mut extra = HashMap::new();
    extra.insert(
        "hookEventName".into(),
        serde_json::Value::String("PostToolUse".into()),
    );
    extra.insert(
        "additionalContext".into(),
        serde_json::Value::String(reason.into()),
    );

    PostToolUseHookOutput {
        cont: Some(false),
        stop_reason: None,
        suppress_output: None,
        system_message: None,
        reason: Some(reason.into()),
        hook_specific_output: Some(extra),
        decision: Some(HookDecision::Block),
    }
}

/// Build output instructing the hook to continue.
fn continue_output() -> PostToolUseHookOutput {
    PostToolUseHookOutput {
        cont: Some(true),
        stop_reason: None,
        suppress_output: None,
        system_message: None,
        reason: None,
        hook_specific_output: None,
        decision: None,
    }
}

/// Evaluate a synthesis tool response and decide whether to continue.
fn evaluate(map: &HashMap<String, serde_json::Value>) -> PostToolUseHookOutput {
    match classify(map) {
        Classification::FullSuccess => stop_output("The job is finished; you can stop now."),
        Classification::PartialSuccess => {
            stop_output("The result is good enough; you can stop now.")
        }
        Classification::Failure | Classification::Default => continue_output(),
    }
}

// ==========================================
// Hook handler
// ==========================================

struct SynthesisStopPlugin;

impl HookHandler for SynthesisStopPlugin {
    fn execute(&self, hook: &mut Hook) -> Result<HookOutput, String> {
        let input = match hook.0.as_post_tool_use() {
            Some(i) => i,
            None => {
                return Ok(HookOutput::PostTool(PostToolUseHookOutput {
                    cont: Some(true),
                    stop_reason: None,
                    suppress_output: None,
                    system_message: None,
                    reason: None,
                    hook_specific_output: None,
                    decision: None,
                }));
            }
        };

        let output = evaluate(&input.tool_response);
        Ok(HookOutput::PostTool(output))
    }
}

// ==========================================
// Entry point
// ==========================================

fn main() {
    let plugin = SynthesisStopPlugin;

    // Reads a CommandRequest from stdin, parses it into a Hook.
    let h = Hook::new(HookEventName::PostToolUse, HookType::Command);

    // Runs the handler and prints the JSON output to stdout.
    HookEngine::run_hook(plugin, h);

    // Always exit 0 — blocking decisions are communicated via JSON.
    process::exit(0);
}

// ==========================================
// Tests
// ==========================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn str_response(s: &str) -> HashMap<String, serde_json::Value> {
        let mut map = HashMap::new();
        map.insert("content".into(), serde_json::Value::String(s.into()));
        map
    }

    // -- Full success

    #[test]
    fn test_full_success() {
        let resp = str_response("Halmos: all invariants proven (1.2s)");
        let out = evaluate(&resp);
        assert_eq!(out.cont, Some(false));
        assert_eq!(
            out.reason.as_deref(),
            Some("The job is finished; you can stop now.")
        );
        assert_eq!(out.decision, Some(HookDecision::Block));
        let extra = out.hook_specific_output.as_ref().unwrap();
        let ctx = extra.get("additionalContext").and_then(|v| v.as_str());
        assert_eq!(ctx, Some("The job is finished; you can stop now."));
    }

    // -- Partial success

    #[test]
    fn test_partial_success() {
        let resp = str_response("Halmos: partial proof (some unproven)");
        let out = evaluate(&resp);
        assert_eq!(out.cont, Some(false));
        assert_eq!(
            out.reason.as_deref(),
            Some("The result is good enough; you can stop now.")
        );
        assert_eq!(out.decision, Some(HookDecision::Block));
    }

    // -- Failure markers

    #[test]
    fn test_compilation_failed() {
        let resp = str_response("Compilation failed.\nerrors during build");
        let out = evaluate(&resp);
        assert_eq!(out.cont, Some(true));
        assert_eq!(out.reason, None);
        assert_eq!(out.decision, None);
    }

    #[test]
    fn test_tests_failed() {
        let resp = str_response("Tests failed.\n2 assertions violated");
        let out = evaluate(&resp);
        assert_eq!(out.cont, Some(true));
    }

    #[test]
    fn test_counterexample() {
        let resp = str_response("Halmos counterexample found.\n  a = 42");
        let out = evaluate(&resp);
        assert_eq!(out.cont, Some(true));
    }

    // -- Default (no match)

    #[test]
    fn test_default() {
        let resp = str_response("forge build passed.\nAll OK.");
        let out = evaluate(&resp);
        assert_eq!(out.cont, Some(true));
        assert_eq!(out.reason, None);
    }

    // -- Nested structures

    #[test]
    fn test_nested_object() {
        let mut resp = HashMap::new();
        resp.insert(
            "report".into(),
            serde_json::json!({
                "status": "Halmos: all invariants proven (1.2s)",
                "metrics": { "gas": 123456 }
            }),
        );
        let out = evaluate(&resp);
        assert_eq!(out.cont, Some(false));
    }

    #[test]
    fn test_nested_array() {
        let mut resp = HashMap::new();
        resp.insert(
            "logs".into(),
            serde_json::json!([
                "some output",
                "Halmos: all invariants proven",
                "more output"
            ]),
        );
        let out = evaluate(&resp);
        assert_eq!(out.cont, Some(false));
    }

    // -- Priority: full beats partial

    #[test]
    fn test_priority_full_over_partial() {
        let mut resp = HashMap::new();
        resp.insert(
            "halmos".into(),
            serde_json::Value::String("Halmos: partial proof".into()),
        );
        resp.insert(
            "summary".into(),
            serde_json::Value::String("Halmos: all invariants proven".into()),
        );
        let out = evaluate(&resp);
        assert_eq!(out.cont, Some(false));
        assert_eq!(
            out.reason.as_deref(),
            Some("The job is finished; you can stop now.")
        );
    }

    // -- Edge cases

    #[test]
    fn test_empty_response() {
        let resp = HashMap::new();
        let out = evaluate(&resp);
        assert_eq!(out.cont, Some(true));
        assert_eq!(out.reason, None);
    }

    #[test]
    fn test_deeply_nested() {
        let resp: HashMap<String, serde_json::Value> = serde_json::from_value(
            serde_json::json!({
                "level1": {
                    "level2": { "level3": "Halmos: all invariants proven" }
                }
            }),
        )
        .unwrap();
        let out = evaluate(&resp);
        assert_eq!(out.cont, Some(false));
    }
}
