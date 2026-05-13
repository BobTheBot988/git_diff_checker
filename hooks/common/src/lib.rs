use enum_as_inner::EnumAsInner;
use regex::Regex;
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
            pub fn is_blocking_decision(&self) -> bool {
                matches!(self.decision, HookDecision::Block | HookDecision::Deny)
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
                    .map(|val| {
                        let mut context = val.to_string();
                        let relt = Regex::new("<").unwrap();
                        let regt = Regex::new(">").unwrap();
                        context = relt.replace_all(&context, "&lt;").to_string();
                        context = regt.replace_all(&context, "&gt;").to_string();
                        context
                    })
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
}

#[derive(Debug, Display, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum HookType {
    Command,
    Http,
    Function,
}

#[derive(Debug, Display, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookDecision {
    Ask,
    Block,
    Deny,
    Approve,
    Allow,
}

#[derive(Debug, Display, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
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

impl_check_valid_type!(PreToolUseInput, "PreToolUse");
impl_check_valid_type!(PostToolUseInput, "PostToolUse");
impl_try_from_request!(PreToolUseInput, "PreToolUse");
impl_try_from_request!(PostToolUseInput, "PostToolUse");

#[derive(Debug, Deserialize, EnumDiscriminants, EnumAsInner)]
#[strum_discriminants(derive(AsRefStr))]
#[serde(rename_all = "snake_case")]
#[serde(untagged)]
pub enum HookInput {
    PreToolUse(PreToolUseInput),
    PostToolUse(PostToolUseInput),
}

impl Clone for HookInput {
    fn clone(&self) -> Self {
        match self {
            Self::PreToolUse(arg0) => Self::PreToolUse(arg0.clone()),
            Self::PostToolUse(arg0) => Self::PostToolUse(arg0.clone()),
        }
    }
}

// ==========================================
// 4. Output Models (Outgoing Responses)
// ==========================================

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
#[serde_inline_default]
pub struct PreToolUseHookOutput {
    #[serde(rename = "continue")]
    pub cont: Option<bool>,
    pub stop_reason: Option<String>,
    pub suppress_output: Option<bool>,
    pub system_message: Option<String>,
    pub reason: Option<String>,
    pub hook_specific_output: Option<HashMap<String, serde_json::Value>>,
    #[serde_inline_default(HookDecision::Deny)]
    pub decision: HookDecision,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
#[serde_inline_default]
pub struct PostToolUseHookOutput {
    #[serde(rename = "continue")]
    pub cont: Option<bool>,
    pub stop_reason: Option<String>,
    pub suppress_output: Option<bool>,
    pub system_message: Option<String>,
    pub reason: Option<String>,
    pub hook_specific_output: Option<HashMap<String, serde_json::Value>>,
    #[serde_inline_default(HookDecision::Deny)]
    pub decision: HookDecision,
}
impl ToString for PostToolUseHookOutput {
    fn to_string(&self) -> String {
        format!(
            "{}",
            serde_json::to_string(self).expect("Error converting to string")
        )
    }
}

impl_hook_output_methods!(PreToolUseHookOutput);
impl_hook_output_methods!(PostToolUseHookOutput);

impl PreToolUseHookOutput {
    pub fn make_pre_tool_output(decision: HookDecision, cont: bool, reason: String) -> HookOutput {
        // PreToolUse uses lowercase strings for permissionDecision: deny/allow/ask/defer
        let permission_decision_str = match decision {
            HookDecision::Deny | HookDecision::Block => "deny",
            HookDecision::Allow | HookDecision::Approve => "allow",
            HookDecision::Ask => "ask",
        };

        let mut map = HashMap::new();
        map.insert("hookEventName".into(), "preToolUse".into());
        map.insert("permissionDecision".into(), permission_decision_str.to_string().into());
        map.insert("permissionDecisionReason".into(), reason.into());

        HookOutput::PreTool(PreToolUseHookOutput {
            cont: Some(cont),
            stop_reason: None,
            suppress_output: None,
            system_message: None,
            reason: None,
            hook_specific_output: Some(map),
            decision,
        })
    }
}
impl PostToolUseHookOutput {
    pub fn make_post_tool_output(decision: HookDecision, cont: bool, reason: String) -> HookOutput {
        let mut map = HashMap::new();
        map.insert("hookEventName".into(), "postToolUse".into());
        map.insert("permissionDecision".into(), decision.to_string().into());
        map.insert("permissionDecisionReason".into(), reason.into());

        HookOutput::PostTool(PostToolUseHookOutput {
            cont: Some(cont),
            stop_reason: None,
            suppress_output: None,
            system_message: None,
            reason: None,
            hook_specific_output: Some(map),
            decision,
        })
    }
}

#[derive(Debug, Display, Serialize, EnumAsInner, Clone)]
#[serde(rename_all = "snake_case")]
pub enum HookOutput {
    PreTool(PreToolUseHookOutput),
    PostTool(PostToolUseHookOutput),
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
    fn execute(&self, hook: &mut Hook) -> Result<HookOutput, HookOutput>;
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
                hook.1 = Some(e);
                hook.send_hook_err();
                process::exit(2);
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
        match self.3 {
            HookType::Command => {
                unimplemented!()
            }
            _ => eprintln!("[Hook Log]: {}", message),
        }
    }
    pub fn send_hook_output(&self) {
        let output_json = serde_json::to_string_pretty(&self.1).unwrap();
        match self.3 {
            HookType::Command => {
                std::io::stdout().flush().unwrap();
                println!("{}", output_json)
            }
            _ => todo!(),
        }
    }
    pub fn send_hook_err(&self) {
        let output_json = serde_json::to_string_pretty(&self.1).unwrap();
        match self.3 {
            HookType::Command => {
                std::io::stderr().flush().unwrap();
                eprintln!("{}", output_json)
            }
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
                    Ok(_) => eprintln!(
                        "DEBUG recv_hook_input: written to debug_file:{}",
                        &serde_json::to_string_pretty(&req).unwrap()
                    ),
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
                    _ => todo!(),
                }
            }
            eprintln!("DEBUG recv_hook_input: no valid input found");
            process::exit(2);
        }
        _ => todo!(),
    }
}
