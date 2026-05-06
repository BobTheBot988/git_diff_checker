use regex::Regex;
use serde_inline_default::serde_inline_default;
use std::{collections::HashMap, process};
use strum_macros::Display;

use serde::{Deserialize, Serialize};

use std::io::{self, Read};

#[derive(Debug, Deserialize)]
pub enum HookInput {
    PreToolUse(PreToolUseInput),
    PostToolUse(PostToolUseInput),
}

#[derive(Debug, Display, Deserialize)]
#[strum(serialize_all = "snake_case")]
pub enum PermissionMode {
    Default,
    Plan,
    AutoEdit,
    Yolo,
}

#[serde_inline_default]
#[derive(Debug, Deserialize)]
pub struct PreToolUseInput {
    #[serde_inline_default(PermissionMode::Default)]
    pub permission_mode: PermissionMode,
    pub tool_name: String,
    pub tool_input: HashMap<String, serde_json::Value>,
    #[serde_inline_default("".to_string())]
    pub tool_use_id: String, // Unique identifier for this tool use instance
}

impl PreToolUseInput {
    fn new(
        permission_mode: PermissionMode,
        tool_name: String,
        tool_input: HashMap<String, serde_json::Value>,
        tool_use_id: String,
    ) -> Self {
        Self {
            permission_mode,
            tool_name,
            tool_input,
            tool_use_id,
        }
    }
}
#[derive(Debug, Deserialize)]
pub struct PostToolUseInput {
    pub permission_mode: PermissionMode,
    pub tool_name: String,
    pub tool_input: HashMap<String, serde_json::Value>,
    pub tool_response: HashMap<String, serde_json::Value>,
    pub tool_use_id: String, // Unique identifier for this tool use instance
}

impl PostToolUseInput {
    fn new(
        permission_mode: PermissionMode,
        tool_name: String,
        tool_input: HashMap<String, serde_json::Value>,
        tool_response: HashMap<String, serde_json::Value>,
        tool_use_id: String,
    ) -> Self {
        Self {
            permission_mode,
            tool_name,
            tool_input,
            tool_response,
            tool_use_id,
        }
    }
}

#[derive(Debug, Display, Serialize, Deserialize)]
#[strum(serialize_all = "snake_case")]
pub enum HookType {
    Command,
    Http,
    Function,
}

#[derive(Debug, Display, Deserialize, Serialize)]
#[strum(serialize_all = "snake_case")]
pub enum HookDecision {
    Ask,
    Block,
    Deny,
    Approve,
    Allow,
}

#[derive(Debug, Display, Deserialize, Serialize)]
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
#[derive(Debug, Display)]
#[strum(serialize_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    Deny,
    Ask,
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
pub struct PostToolUseHookOutput {
    pub cont: Option<bool>,
    pub stop_reason: Option<String>,
    pub suppress_output: Option<bool>,
    pub system_message: Option<String>,
    pub reason: Option<String>,
    pub hook_specific_output: Option<HashMap<String, serde_json::Value>>,
    pub decision: HookDecision,
}

pub struct Hook(HookInput, HookOutput, HookEventName, HookType);

fn recv_hook_input(he: &HookEventName, h: &HookType) -> HookInput {
    let input_str = match h {
        HookType::Command => {
            let mut input_str = String::new();
            if let Err(e) = io::stdin().read_to_string(&mut input_str) {
                eprintln!("Error reading stdin: {}", e);
                process::exit(2);
            }
            input_str
        }
        HookType::Http => todo!(),
        _ => todo!(),
    };

    match he {
        //TODO: implement macro to make this more scalable
        HookEventName::PreToolUse => {
            let input: PreToolUseInput = match serde_json::from_str(&input_str) {
                Ok(i) => i,
                Err(e) => {
                    eprintln!("Error parsing JSON: {}", e);
                    process::exit(2);
                }
            };
            HookInput::PreToolUse(input)
        }

        HookEventName::PostToolUse => {
            let input: PostToolUseInput = match serde_json::from_str(&input_str) {
                Ok(i) => i,
                Err(e) => {
                    eprintln!("Error parsing JSON: {}", e);
                    process::exit(2);
                }
            };
            HookInput::PostToolUse(input)
        }
        _ => todo!(),
    }
}

impl Hook {
    pub fn new(
        hook_event_name: HookEventName,
        hook_type: HookType,
        f: fn(&HookInput) -> HookOutput,
    ) -> Self {
        let input_data: HookInput = recv_hook_input(&hook_event_name, &hook_type);
        let output_data: HookOutput = f(&input_data);

        Self(input_data, output_data, hook_event_name, hook_type)
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
#[derive(Debug, Serialize)]
pub enum HookOutput {
    PreTool(PreToolUseHookOutput),
    PostTool(PostToolUseHookOutput),
}
