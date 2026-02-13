use serde::{Deserialize, Serialize};

/// Hook request received from Claude Code on stdin.
///
/// Claude Code sends this JSON when a PreToolUse event fires for a Bash tool call.
/// Contains the tool name and the command Claude wants to execute.
#[derive(Debug, Deserialize)]
pub struct HookRequest {
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: ToolInput,
}

impl HookRequest {
    pub fn is_bash(&self) -> bool {
        self.tool_name.eq_ignore_ascii_case("bash")
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct ToolInput {
    pub command: Option<String>,
}

/// Hook response written to stdout for Claude Code.
///
/// Two variants:
/// - **Passthrough**: empty JSON `{}` — tells Claude Code to proceed unchanged.
/// - **Rewrite**: JSON with `hookSpecificOutput` containing `updatedInput` — tells
///   Claude Code to execute the rewritten command instead.
///
/// See: <https://code.claude.com/docs/en/hooks#pretooluse-decision-control>
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    hook_specific_output: Option<HookSpecificOutput>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookSpecificOutput {
    hook_event_name: String,
    permission_decision: String,
    permission_decision_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_input: Option<UpdatedInput>,
}

#[derive(Debug, Serialize)]
pub struct UpdatedInput {
    command: String,
}

impl HookResponse {
    /// Return empty JSON `{}` — Claude Code proceeds with the original command.
    pub fn passthrough() -> Self {
        Self {
            hook_specific_output: None,
        }
    }

    /// Return JSON that rewrites the Bash command via `updatedInput`.
    ///
    /// Claude Code will execute the rewritten command instead of the original.
    /// `permissionDecision: "allow"` bypasses the permission prompt so the
    /// rewrite is transparent to the user.
    pub fn rewrite(rewritten_command: &str) -> Self {
        Self {
            hook_specific_output: Some(HookSpecificOutput {
                hook_event_name: "PreToolUse".to_string(),
                permission_decision: "allow".to_string(),
                permission_decision_reason: "terse command rewrite".to_string(),
                updated_input: Some(UpdatedInput {
                    command: rewritten_command.to_string(),
                }),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_serializes_to_empty_json() {
        let response = HookResponse::passthrough();
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(json, "{}");
    }

    #[test]
    fn rewrite_serializes_with_updated_input() {
        let response = HookResponse::rewrite("terse run \"git status\"");
        let json = serde_json::to_string_pretty(&response).unwrap();

        assert!(json.contains("hookSpecificOutput"));
        assert!(json.contains("hookEventName"));
        assert!(json.contains("PreToolUse"));
        assert!(json.contains("permissionDecision"));
        assert!(json.contains("allow"));
        assert!(json.contains("updatedInput"));
        assert!(json.contains("terse run \\\"git status\\\""));
    }

    #[test]
    fn deserializes_bash_hook_request() {
        let input = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
        let request: HookRequest = serde_json::from_str(input).unwrap();

        assert!(request.is_bash());
        assert_eq!(request.tool_input.command.as_deref(), Some("git status"));
    }

    #[test]
    fn deserializes_non_bash_hook_request() {
        let input = r#"{"tool_name":"Edit","tool_input":{}}"#;
        let request: HookRequest = serde_json::from_str(input).unwrap();

        assert!(!request.is_bash());
        assert_eq!(request.tool_input.command, None);
    }
}
