use enum_as_inner::EnumAsInner;
use regex::Regex;
use serde_inline_default::serde_inline_default;
use std::{collections::HashMap, path::PathBuf, process};
use strum::{AsRefStr, EnumDiscriminants};
use strum_macros::Display;

use serde::{Deserialize, Serialize};

use std::io;
use std::ops::{Deref, DerefMut};
// I'm constraining the HookFunction Trait only or the HookWrapper
#[derive(Debug)]
pub struct HookWrapper(pub Hook);

impl Deref for HookWrapper {
    type Target = Hook;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for HookWrapper {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
mod private {
    pub trait Sealed {}
}

impl private::Sealed for HookWrapper {}

pub trait HookFunction: private::Sealed {
    fn execute(&self) -> Result<HookOutput, ()>;
}

impl<T: private::Sealed + ?Sized> HookFunction for T {
    fn execute(&self) -> Result<HookOutput, ()> {
        todo!()
    }
}

// HookWrapper Is needed to circumvent the orphan rule WHY RUST WHY!?!?!?
// Maybe it's just a skill issue
#[derive(Debug, Deserialize, EnumDiscriminants, EnumAsInner)]
#[strum_discriminants(derive(AsRefStr))]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Display, Serialize, Deserialize, Clone, Copy)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    Default,
    Plan,
    AutoEdit,
    Yolo,
}
// 1. Update CommandRequest to capture extra fields for conversion
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct CommandRequest {
    pub hook_event_name: Option<String>,
    pub cwd: Option<PathBuf>,
    // Flatten captures all other JSON fields into this map
    #[serde(flatten)]
    pub extra_fields: HashMap<String, serde_json::Value>,
}

macro_rules! impl_check_valid_type {
    ($target_struct:ident, $event_name:expr) => {
        impl $target_struct {
            fn check_correctness(req: &CommandRequest) -> bool {
                req.hook_event_name
                    .as_ref()
                    .map(|n| n.eq_ignore_ascii_case($event_name))
                    .unwrap_or(false)
            }
        }
    };
}

impl_check_valid_type!(PreToolUseInput, "PreToolUse");
impl_check_valid_type!(PostToolUseInput, "PostToolUse");

macro_rules! impl_try_from_request {
    ($target_struct:ident, $event_name:expr) => {
        impl TryFrom<CommandRequest> for $target_struct {
            type Error = String;

            fn try_from(req: CommandRequest) -> Result<Self, Self::Error> {
                // Check if the event name exists and matches (case-insensitive)

                if !$target_struct::check_correctness(&req) {
                    return Err(format!("Not a {} event", $event_name));
                }

                // Map the flattened fields into the actual struct
                serde_json::from_value(serde_json::Value::Object(
                    req.extra_fields.into_iter().collect(),
                ))
                .map_err(|e| e.to_string())
            }
        }
    };
}

// Apply the macro for your types
impl_try_from_request!(PreToolUseInput, "PreToolUse");
impl_try_from_request!(PostToolUseInput, "PostToolUse");
#[serde_inline_default]
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct PreToolUseInput {
    #[serde_inline_default(PermissionMode::Default)]
    pub permission_mode: PermissionMode,
    pub tool_name: String,
    pub tool_input: HashMap<String, serde_json::Value>,
    #[serde_inline_default("".to_string())]
    pub tool_use_id: String, // Unique identifier for this tool use instance
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct PostToolUseInput {
    pub permission_mode: PermissionMode,
    pub tool_name: String,
    pub tool_input: HashMap<String, serde_json::Value>,
    pub tool_response: HashMap<String, serde_json::Value>,
    pub tool_use_id: String, // Unique identifier for this tool use instance
}

#[derive(Debug, Display, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum HookType {
    Command,
    Http,
    Function,
}

#[derive(Debug, Display, Deserialize, Serialize, Clone)]
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
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
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
#[derive(Debug, Display, Deserialize, Serialize, Clone)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    Deny,
    Ask,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct PreToolUseHookOutput {
    #[serde(rename = "continue")]
    pub cont: Option<bool>,
    pub stop_reason: Option<String>,
    pub suppress_output: Option<bool>,
    pub system_message: Option<String>,
    pub reason: Option<String>,
    pub hook_specific_output: Option<HashMap<String, serde_json::Value>>,
    pub decision: HookDecision,
}

impl PreToolUseHookOutput {
    pub fn make_pre_tool_output(decision: HookDecision, cont: bool, reason: String) -> HookOutput {
        let mut map = std::collections::HashMap::new();
        map.insert(
            "hookEventName".to_string(),
            serde_json::Value::from("preToolUse"),
        );
        map.insert(
            "permissionDecision".to_string(),
            serde_json::Value::from(decision.to_string()),
        );
        map.insert(
            "permissionDecisionReason".to_string(),
            serde_json::Value::from(reason),
        );

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
                if let Some(reason) = &self.stop_reason {
                    return reason.clone();
                }
                if let Some(reason) = &self.reason {
                    return reason.clone();
                }
                "No reason provided".to_string()
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
                if self
                    .hook_specific_output
                    .as_ref()
                    .is_some_and(|h| h.contains_key("additionalContext"))
                {
                    let mut context = self
                        .hook_specific_output
                        .as_ref() // 1. Don't move the map, just look at it
                        .and_then(|map| map.get("additionalContext")) // 2. Get the key if map exists
                        .unwrap()
                        .to_string(); // 3. Try to treat it as a String

                    // Sanitize by escaping < and > to prevent tag injection
                    let relt = Regex::new("<").unwrap();
                    let regt = Regex::new(">").unwrap();
                    context = relt.replace_all(context.as_str(), "&lt;").to_string();
                    context = regt.replace_all(context.as_str(), "&gt;").to_string();
                    return Some(context);
                }
                return None;
            }
        }
    };
}

// This will automatically generate the implementation blocks for both!
impl_hook_output_methods!(PreToolUseHookOutput);
impl_hook_output_methods!(PostToolUseHookOutput);

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct PostToolUseHookOutput {
    pub cont: Option<bool>,
    pub stop_reason: Option<String>,
    pub suppress_output: Option<bool>,
    pub system_message: Option<String>,
    pub reason: Option<String>,
    pub hook_specific_output: Option<HashMap<String, serde_json::Value>>,
    pub decision: HookDecision,
}
#[derive(Debug, Clone)]
pub struct Hook(
    pub HookInput,
    pub Option<HookOutput>,
    pub HookEventName,
    pub HookType,
    pub Option<CommandRequest>,
);

fn recv_hook_input(he: &HookEventName, h: &HookType) -> (HookInput, CommandRequest) {
    match h {
        HookType::Command => {
            let stdin = io::stdin();
            let reader = stdin.lock();

            // This iterator handles the {}{} concatenated JSON problem
            let stream =
                serde_json::Deserializer::from_reader(reader).into_iter::<CommandRequest>();

            for item in stream {
                let req = match item {
                    Ok(r) => r,
                    Err(_) => continue, // Skip malformed chunks or trailing data
                };

                match he {
                    HookEventName::PreToolUse => {
                        if let Ok(input) = PreToolUseInput::try_from(req.clone()) {
                            return (HookInput::PreToolUse(input), req);
                        }
                    }
                    HookEventName::PostToolUse => {
                        if let Ok(input) = PostToolUseInput::try_from(req.clone()) {
                            return (HookInput::PostToolUse(input), req);
                        }
                    }
                    _ => todo!(),
                }
            }

            eprintln!(
                "Error: Target event {:?} not found in the input stream.",
                he
            );
            process::exit(2);
        }
        _ => todo!(),
    }
}

impl Hook {
    pub fn new(hook_event_name: HookEventName, hook_type: HookType) -> Self {
        let (input_data, c) = recv_hook_input(&hook_event_name, &hook_type);

        Self(input_data, None, hook_event_name, hook_type, Some(c))
    }

    pub fn send_hook_output(&self) {
        let output_json = serde_json::to_string_pretty(&self.1).unwrap();
        match self.3 {
            HookType::Command => {
                println!("{}", output_json);
            }
            HookType::Http => todo!(),
            _ => todo!(),
        }
    }
}
#[derive(Debug, Serialize, EnumAsInner, Clone)]
#[serde(rename_all = "snake_case")]
pub enum HookOutput {
    PreTool(PreToolUseHookOutput),
    PostTool(PostToolUseHookOutput),
}
