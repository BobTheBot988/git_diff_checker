use enum_as_inner::EnumAsInner;
use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Write},
    path::PathBuf,
    process,
};
use strum::{AsRefStr, EnumDiscriminants};
use strum_macros::Display;

// ==========================================
// 1. Macros
// ==========================================

macro_rules! impl_check_valid_type {
    ($target_struct:ident, $event_name:expr) => {
        impl $target_struct {
            fn check_correctness(req: &CommandRequest) -> bool {
                // Check both direct hook_event_name and extra_fields (for flattened case)
                req.hook_event_name
                    .as_ref()
                    .map(|n| n.eq_ignore_ascii_case($event_name))
                    .unwrap_or(false)
                    || req
                        .extra_fields
                        .get("hook_event_name")
                        .and_then(|v| v.as_str())
                        .map(|n| n.eq_ignore_ascii_case($event_name))
                        .unwrap_or(false)
            }
        }
    };
}

macro_rules! impl_try_from_request {
    ($target_struct:ident, $event_name:expr) => {
        impl TryFrom<CommandRequest> for $target_struct {
            type Error = String;

            fn try_from(req: CommandRequest) -> Result<Self, Self::Error> {
                if !$target_struct::check_correctness(&req) {
                    return Err(format!("Not a {} event", $event_name));
                }

                // Start with extra_fields (which contains permission_mode, tool_name, tool_use_id, etc.)
                let mut obj: serde_json::Map<String, serde_json::Value> =
                    req.extra_fields.into_iter().collect();

                // Add tool_input from req if available (it's a direct field in CommandRequest)
                if let Some(tool_input) = req.tool_input.as_ref() {
                    obj.insert(
                        "tool_input".to_string(),
                        serde_json::Value::Object(tool_input.clone().into_iter().collect()),
                    );
                } else {
                    obj.insert(
                        "tool_input".to_string(),
                        serde_json::Value::Object(serde_json::Map::new()),
                    );
                }

                serde_json::from_value(serde_json::Value::Object(obj)).map_err(|e| e.to_string())
            }
        }
    };
}

macro_rules! impl_hook_output_methods {
    ($struct_name:ident) => {
        impl $struct_name {
            #[allow(dead_code)]
            pub fn is_blocking_decision(&self) -> bool {
                match &self.decision {
                    Some(HookDecision::Block) | Some(HookDecision::Deny) => true,
                    _ => false,
                }
            }

            pub fn should_stop_execution(&self) -> bool {
                self.cont.is_some_and(|c| !c)
            }

            pub fn get_effective_reason(&self) -> String {
                self.stop_reason
                    .as_ref()
                    .or(self.reason.as_ref())
                    .cloned()
                    .unwrap_or_else(|| "No reason provided".to_string())
            }

            pub fn get_blocking_error(&self) -> (bool, String) {
                if self.is_blocking_decision() {
                    (true, self.get_effective_reason())
                } else {
                    (false, "".to_string())
                }
            }

            pub fn should_clear_context() -> bool {
                false
            }

            pub fn get_additional_context(&self) -> Option<String> {
                self.hook_specific_output
                    .as_ref()
                    .and_then(|map| map.get("additionalContext"))
                    .map(|val| val.to_string().replace("<", "&lt;").replace(">", "&gt;"))
            }
        }
    };
}

// ==========================================
// 2. Shared Enums & Primitives
// ==========================================

#[derive(Debug, Display, Serialize, Deserialize, Clone, Copy)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "kebab-case")]
pub enum PermissionMode {
    Default,
    Plan,
    AutoEdit,
    Yolo,
    #[serde(rename = "bypassPermissions")]
    BypassPermissions,
}

#[derive(Debug, Display, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum HookType {
    Command,
    Http,
    Function,
}

#[derive(Debug, Display, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum HookDecision {
    Ask,
    Block,
    Deny,
    Approve,
    Allow,
}

#[derive(Debug, Display, Deserialize, Serialize, Clone)]
#[serde(rename_all = "PascalCase")]
#[strum(serialize_all = "PascalCase")]
pub enum HookEventName {
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    Notification,
    UserPromptSubmit,
    SessionStart,
    Stop,
    SubAgentStart,
    SubAgentStop,
    PreCompact,
    PostCompact,
    SessionEnd,
    PermissionRequest,
    StopFailure,
}

// ==========================================
// 2.5. Coverage Models (Halmas coverage.json)
// ==========================================

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct CoverageTestResult {
    pub name: String,
    pub exitcode: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_models: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_paths: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_bounded_loops: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct CoverageJson {
    pub exitcode: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_results: Option<std::collections::HashMap<String, Vec<CoverageTestResult>>>,
}

// ==========================================
// 3. Input Models (Incoming Data)
// ==========================================

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct CommandRequest {
    pub hook_event_name: Option<String>,
    pub cwd: Option<PathBuf>,
    pub tool_input: Option<HashMap<String, serde_json::Value>>,
    #[serde(flatten)]
    pub extra_fields: HashMap<String, serde_json::Value>,
}

#[serde_inline_default]
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct PreToolUseInput {
    #[serde_inline_default(PermissionMode::Default)]
    pub permission_mode: PermissionMode,
    pub tool_name: String,
    pub tool_input: HashMap<String, serde_json::Value>,
    #[serde_inline_default("".to_string())]
    pub tool_use_id: String,
}

#[serde_inline_default]
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct PostToolUseInput {
    #[serde_inline_default(PermissionMode::Default)]
    pub permission_mode: PermissionMode,
    pub tool_name: String,
    pub tool_input: HashMap<String, serde_json::Value>,
    #[serde_inline_default(serde_json::Map::new().into_iter().collect())]
    pub tool_response: HashMap<String, serde_json::Value>,
    pub tool_use_id: String,
}

#[serde_inline_default]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StopInput {
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
    #[serde_inline_default(PermissionMode::Default)]
    pub permission_mode: PermissionMode,
    pub effort: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_input: HashMap<String, serde_json::Value>,
}

impl StopInput {
    #[allow(dead_code)]
    pub fn get_cwd(req: &CommandRequest) -> PathBuf {
        match req.cwd.clone() {
            Some(c) => c,
            None => PathBuf::from("."),
        }
    }
}

impl_check_valid_type!(PreToolUseInput, "PreToolUse");
impl_check_valid_type!(PostToolUseInput, "PostToolUse");
impl_check_valid_type!(StopInput, "Stop");
impl_try_from_request!(PreToolUseInput, "PreToolUse");
impl_try_from_request!(PostToolUseInput, "PostToolUse");
impl_try_from_request!(StopInput, "Stop");

#[derive(Debug, Deserialize, EnumDiscriminants, EnumAsInner)]
#[strum_discriminants(derive(AsRefStr))]
#[serde(rename_all = "snake_case")]
pub enum HookInput {
    PreToolUse(PreToolUseInput),
    PostToolUse(PostToolUseInput),
    Stop(StopInput),
}

impl Clone for HookInput {
    fn clone(&self) -> Self {
        match self {
            Self::PreToolUse(arg0) => Self::PreToolUse(arg0.clone()),
            Self::PostToolUse(arg0) => Self::PostToolUse(arg0.clone()),
            Self::Stop(arg0) => Self::Stop(arg0.clone()),
        }
    }
}

// ==========================================
// 4. Output Models (Outgoing Responses)
// ==========================================

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct PreToolUseHookOutput {
    #[serde(rename = "continue")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cont: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppress_output: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    #[serde(rename = "hookSpecificOutput")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<HookDecision>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct PostToolUseHookOutput {
    #[serde(rename = "continue")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cont: Option<bool>,
    #[serde(rename = "stopReason")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(rename = "suppressOutput")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppress_output: Option<bool>,
    #[serde(rename = "systemMessage")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(rename = "hookSpecificOutput")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<HookDecision>,
}
impl std::fmt::Display for PostToolUseHookOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_string(self).map_err(|_| std::fmt::Error)?
        )
    }
}

impl PreToolUseHookOutput {
    #[allow(dead_code)]
    pub fn is_blocking_decision(&self) -> bool {
        matches!(
            self.decision,
            Some(HookDecision::Block) | Some(HookDecision::Deny)
        )
    }

    pub fn should_stop_execution(&self) -> bool {
        self.cont.is_some_and(|c| !c)
    }

    pub fn get_effective_reason(&self) -> String {
        self.stop_reason
            .as_ref()
            .or(self.reason.as_ref())
            .cloned()
            .unwrap_or_else(|| "No reason provided".to_string())
    }

    pub fn get_blocking_error(&self) -> (bool, String) {
        if self.is_blocking_decision() {
            (true, self.get_effective_reason())
        } else {
            (false, "".to_string())
        }
    }

    pub fn should_clear_context() -> bool {
        false
    }

    pub fn get_additional_context(&self) -> Option<String> {
        self.hook_specific_output
            .as_ref()
            .and_then(|map| map.get("additionalContext"))
            .map(|val| val.to_string().replace("<", "&lt;").replace(">", "&gt;"))
    }
}

impl PostToolUseHookOutput {
    #[allow(dead_code)]
    pub fn is_blocking_decision(&self) -> bool {
        matches!(
            self.decision,
            Some(HookDecision::Block) | Some(HookDecision::Deny)
        )
    }

    pub fn should_stop_execution(&self) -> bool {
        self.cont.is_some_and(|c| !c)
    }

    pub fn get_effective_reason(&self) -> String {
        self.stop_reason
            .as_ref()
            .or(self.reason.as_ref())
            .cloned()
            .unwrap_or_else(|| "No reason provided".to_string())
    }

    pub fn get_blocking_error(&self) -> (bool, String) {
        if self.is_blocking_decision() {
            (true, self.get_effective_reason())
        } else {
            (false, "".to_string())
        }
    }

    pub fn should_clear_context() -> bool {
        false
    }

    pub fn get_additional_context(&self) -> Option<String> {
        self.hook_specific_output
            .as_ref()
            .and_then(|map| map.get("additionalContext"))
            .map(|val| val.to_string().replace("<", "&lt;").replace(">", "&gt;"))
    }
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct StopHookOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<HookDecision>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(rename = "continue")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cont: Option<bool>,
    #[serde(rename = "stopReason")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(rename = "suppressOutput")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppress_output: Option<bool>,
    #[serde(rename = "systemMessage")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    #[serde(rename = "terminalSequence")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_sequence: Option<String>,
}

impl StopHookOutput {
    #[allow(dead_code)]
    pub fn is_blocking_decision(&self) -> bool {
        matches!(self.decision, Some(HookDecision::Block))
    }

    pub fn should_stop_execution(&self) -> bool {
        self.cont.is_some_and(|c| !c)
    }

    pub fn get_effective_reason(&self) -> String {
        self.stop_reason
            .as_ref()
            .or(self.reason.as_ref())
            .cloned()
            .unwrap_or_else(|| "No reason provided".to_string())
    }

    pub fn get_blocking_error(&self) -> (bool, String) {
        if self.is_blocking_decision() {
            (true, self.get_effective_reason())
        } else {
            (false, "".to_string())
        }
    }

    pub fn should_clear_context() -> bool {
        false
    }

    pub fn get_additional_context(&self) -> Option<String> {
        self.reason.clone()
    }
}

impl PreToolUseHookOutput {
    pub fn make_pre_tool_output(decision: HookDecision, cont: bool, reason: String) -> HookOutput {
        let mut map = HashMap::new();
        map.insert("hookEventName".into(), "PreToolUse".into());
        map.insert("permissionDecision".into(), decision.to_string().into());
        map.insert("permissionDecisionReason".into(), reason.clone().into());
        map.insert("additionalContext".into(), reason.clone().into());

        HookOutput::PreTool(PreToolUseHookOutput {
            cont: Some(cont),
            stop_reason: None,
            suppress_output: None,
            system_message: None,
            reason: Some(reason),
            hook_specific_output: Some(map),
            decision: None,
        })
    }
}
impl PostToolUseHookOutput {
    pub fn make_post_tool_output(decision: HookDecision, cont: bool, reason: String) -> HookOutput {
        let mut map = HashMap::new();
        map.insert("hookEventName".into(), "PostToolUse".into());
        map.insert("additionalContext".into(), reason.clone().into());

        HookOutput::PostTool(PostToolUseHookOutput {
            cont: Some(cont),
            stop_reason: None,
            suppress_output: None,
            system_message: None,
            reason: Some(reason),
            hook_specific_output: Some(map),
            decision: Some(decision),
        })
    }
}

#[derive(Debug, Display, Serialize, EnumAsInner, Clone)]
#[serde(rename_all = "snake_case")]
#[serde(untagged)]
pub enum HookOutput {
    PreTool(PreToolUseHookOutput),
    PostTool(PostToolUseHookOutput),
    Stop(StopHookOutput),
}

// ==========================================
// 5. Core Hook Logic & Handler Paradigm
// ==========================================

#[derive(Debug, Clone)]
pub struct Hook(
    pub HookInput,
    pub Option<HookOutput>,
    pub HookEventName,
    pub HookType,
    pub Option<CommandRequest>,
);

pub trait HookHandler {
    fn execute(&self, hook: &mut Hook) -> Result<HookOutput, String>;
}

pub struct HookEngine;

impl HookEngine {
    pub fn run_hook<H: HookHandler>(handler: H, mut hook: Hook) {
        match handler.execute(&mut hook) {
            Ok(output) => {
                hook.1 = Some(output);
                hook.send_hook_output();
            }
            Err(e) => {
                let error_output = match hook.2 {
                    HookEventName::PreToolUse => {
                        let mut map = HashMap::new();
                        map.insert("hookEventName".into(), "PreToolUse".into());
                        map.insert("permissionDecision".into(), "deny".into());
                        map.insert("permissionDecisionReason".into(), e.clone().into());
                        map.insert("additionalContext".into(), e.clone().into());
                        let output = PreToolUseHookOutput {
                            cont: Some(false),
                            stop_reason: Some(e.clone()),
                            suppress_output: None,
                            system_message: None,
                            reason: Some(e),
                            hook_specific_output: Some(map),
                            decision: None,
                        };
                        HookOutput::PreTool(output)
                    }
                    HookEventName::Stop => {
                        let output = StopHookOutput {
                            decision: Some(HookDecision::Block),
                            reason: Some(e.clone()),
                            cont: Some(false),
                            stop_reason: None,
                            suppress_output: None,
                            system_message: None,
                            terminal_sequence: None,
                        };
                        HookOutput::Stop(output)
                    }
                    _ => {
                        let output = PostToolUseHookOutput {
                            cont: Some(false),
                            stop_reason: Some(e.clone()),
                            suppress_output: None,
                            system_message: None,
                            reason: Some(e),
                            hook_specific_output: None,
                            decision: Some(HookDecision::Block),
                        };
                        HookOutput::PostTool(output)
                    }
                };
                hook.1 = Some(error_output);
                hook.send_hook_output();
            }
        }
    }
}

impl Hook {
    pub fn new(hook_event_name: HookEventName, hook_type: HookType) -> Self {
        let (input_data, c) = recv_hook_input(&hook_event_name, &hook_type);
        Self(input_data, None, hook_event_name, hook_type, Some(c))
    }
    pub fn log(&self, message: &str) {
        eprintln!("[Hook Log]: {}", message);
    }
    pub fn send_hook_output(&self) {
        if let Err(e) = std::io::stdout().flush() {
            eprintln!("[Hook Log]: Failed to flush stdout: {}", e);
            return;
        }
        let output_json = match serde_json::to_string_pretty(&self.1) {
            Ok(json) => json,
            Err(e) => {
                eprintln!("[Hook Log]: Failed to serialize output: {}", e);
                return;
            }
        };
        match self.3 {
            HookType::Command => println!("{}", output_json),
            _ => todo!(),
        }
    }
}

fn recv_hook_input(he: &HookEventName, h: &HookType) -> (HookInput, CommandRequest) {
    eprintln!("DEBUG recv_hook_input: he={:?}", he);
    let debug_file = match File::create("/tmp/debug.json") {
        Ok(f) => f,
        Err(e) => panic!("Error in creating /tmp/debug.json:{}", e),
    };
    let mut writer = std::io::BufWriter::new(debug_file);
    match h {
        HookType::Command => {
            let stdin = io::stdin();
            let reader = stdin.lock();
            let stream =
                serde_json::Deserializer::from_reader(reader).into_iter::<CommandRequest>();

            for item in stream {
                let req = match item {
                    Ok(r) => {
                        eprintln!("DEBUG recv_hook_input: parsed CommandRequest");
                        eprintln!("DEBUG extra_fields: {:?}", r.extra_fields);
                        r
                    }
                    Err(e) => {
                        eprintln!("DEBUG recv_hook_input: parse error: {}", e);
                        continue;
                    }
                };
                let r = match &serde_json::to_string_pretty(&req) {
                    Ok(a) => a.clone(),
                    Err(e) => panic!("{}", e),
                };

                match writer.write_all(&(r.into_bytes())) {
                    Ok(_) => match serde_json::to_string_pretty(&req) {
                        Ok(json) => {
                            eprintln!("DEBUG recv_hook_input: written to debug_file:{}", json)
                        }
                        Err(e) => eprintln!("DEBUG recv_hook_input: serialization error: {}", e),
                    },
                    Err(e) => panic!("Error writing to debug file {}", e),
                };
                match he {
                    HookEventName::PreToolUse => match PreToolUseInput::try_from(req.clone()) {
                        Ok(input) => {
                            eprintln!("DEBUG recv_hook_input: converted to PreToolUseInput");
                            return (HookInput::PreToolUse(input), req);
                        }
                        Err(e) => {
                            eprintln!(
                                "DEBUG recv_hook_input: failed to convert to PreToolUseInput: {}",
                                e
                            );
                        }
                    },
                    HookEventName::PostToolUse => match PostToolUseInput::try_from(req.clone()) {
                        Ok(input) => {
                            eprintln!("DEBUG recv_hook_input: converted to PostToolUseInput");
                            return (HookInput::PostToolUse(input), req);
                        }
                        Err(e) => {
                            eprintln!(
                                "DEBUG recv_hook_input: failed to convert to PostToolUseInput: {}",
                                e
                            );
                        }
                    },
                    HookEventName::Stop => match StopInput::try_from(req.clone()) {
                        Ok(input) => {
                            eprintln!("DEBUG recv_hook_input: converted to StopInput");
                            return (HookInput::Stop(input), req);
                        }
                        Err(e) => {
                            eprintln!(
                                "DEBUG recv_hook_input: failed to convert to StopInput: {}",
                                e
                            );
                        }
                    },
                    _ => todo!(),
                }
            }
            eprintln!("DEBUG recv_hook_input: no valid input found");
            process::exit(2);
        }
        _ => todo!(),
    }
}
