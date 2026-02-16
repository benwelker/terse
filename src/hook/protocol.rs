use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Supported tool classification
// ---------------------------------------------------------------------------

/// Recognized tool categories for both Claude Code and Copilot hooks.
///
/// Each variant represents a class of tools that terse can intercept.
/// Adding support for a new tool is a two-step process:
/// 1. Add a variant here and update [`ToolKind::from_name`].
/// 2. Add a handler arm in `hook::handle_request`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    /// Shell command execution (`Bash` tool).
    Bash,
    /// Any tool terse does not (yet) handle.
    Unsupported,
}

impl ToolKind {
    /// Classify a tool name into a [`ToolKind`].
    ///
    /// Matching is case-insensitive. Unknown tools map to
    /// [`Unsupported`](ToolKind::Unsupported).
    pub fn from_name(name: &str) -> Self {
        if name.eq_ignore_ascii_case("bash") {
            Self::Bash
        } else {
            Self::Unsupported
        }
    }
}

impl std::fmt::Display for ToolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bash => write!(f, "bash"),
            Self::Unsupported => write!(f, "unsupported"),
        }
    }
}

// ---------------------------------------------------------------------------
// Hook request
// ---------------------------------------------------------------------------

/// Hook request received from Claude Code on stdin.
///
/// Claude Code sends this JSON when a PreToolUse event fires. The payload
/// contains the tool name and tool-specific input fields.
#[derive(Debug, Deserialize)]
pub struct HookRequest {
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: ToolInput,
}

impl HookRequest {
    /// Classify this request's tool into a [`ToolKind`].
    pub fn tool_kind(&self) -> ToolKind {
        ToolKind::from_name(&self.tool_name)
    }
}

/// Tool input fields sent by Claude Code.
///
/// Different tools populate different fields. Unrecognized fields are
/// silently ignored by serde so the struct is forward-compatible.
#[derive(Debug, Default, Deserialize)]
pub struct ToolInput {
    /// Shell command (Bash tool).
    pub command: Option<String>,
    /// File path (Read / Write / Edit tools — reserved for future use).
    #[allow(dead_code)]
    pub file_path: Option<String>,
    /// File content (Write tool — reserved for future use).
    #[allow(dead_code)]
    pub content: Option<String>,
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

// ---------------------------------------------------------------------------
// Copilot hook protocol
// ---------------------------------------------------------------------------

/// Hook request received from GitHub Copilot on stdin.
///
/// Copilot's preToolUse hook sends a JSON payload with camelCase field names.
/// The `toolArgs` field is a JSON-encoded string containing tool arguments.
///
/// See: <https://docs.github.com/en/copilot/reference/hooks-configuration>
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopilotHookRequest {
    /// Unix timestamp in milliseconds.
    #[serde(default)]
    #[allow(dead_code)]
    pub timestamp: u64,
    /// Current working directory.
    #[serde(default)]
    #[allow(dead_code)]
    pub cwd: String,
    /// Name of the tool being invoked (e.g. "bash", "edit", "view", "create").
    #[serde(default)]
    pub tool_name: String,
    /// JSON string containing the tool's arguments.
    #[serde(default)]
    pub tool_args: String,
}

impl CopilotHookRequest {
    /// Classify this request's tool into a [`ToolKind`].
    pub fn tool_kind(&self) -> ToolKind {
        ToolKind::from_name(&self.tool_name)
    }

    /// Parse the `toolArgs` JSON string and extract the `command` field.
    ///
    /// Returns `None` if `toolArgs` is empty, invalid JSON, or lacks `command`.
    pub fn command(&self) -> Option<String> {
        if self.tool_args.is_empty() {
            return None;
        }
        serde_json::from_str::<serde_json::Value>(&self.tool_args)
            .ok()
            .and_then(|v| v.get("command").and_then(|c| c.as_str()).map(String::from))
    }
}

/// Hook response written to stdout for GitHub Copilot.
///
/// Copilot preToolUse hooks return a permission decision:
/// - **Allow**: `{"permissionDecision":"allow"}` — proceed with the tool call.
/// - **Deny**: `{"permissionDecision":"deny","permissionDecisionReason":"..."}` —
///   block the tool call.
/// - **Rewrite**: includes `hookSpecificOutput.updatedInput` to rewrite the
///   command, mirroring the Claude Code protocol. The top-level
///   `permissionDecision: "allow"` provides a safe fallback if the Copilot
///   runtime does not process `hookSpecificOutput`.
///
/// See: <https://docs.github.com/en/copilot/reference/hooks-configuration>
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopilotHookResponse {
    permission_decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    permission_decision_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hook_specific_output: Option<HookSpecificOutput>,
}

impl CopilotHookResponse {
    /// Return a response that allows the tool call to proceed.
    pub fn allow() -> Self {
        Self {
            permission_decision: "allow".to_string(),
            permission_decision_reason: None,
            hook_specific_output: None,
        }
    }

    /// Return a response that denies the tool call with a reason.
    #[allow(dead_code)]
    pub fn deny(reason: &str) -> Self {
        Self {
            permission_decision: "deny".to_string(),
            permission_decision_reason: Some(reason.to_string()),
            hook_specific_output: None,
        }
    }

    /// Return a response that rewrites the Bash command via `updatedInput`.
    ///
    /// Uses the same `hookSpecificOutput.updatedInput` protocol as Claude Code
    /// hooks. The top-level `permissionDecision: "allow"` acts as a safe
    /// fallback — if the Copilot runtime does not process `hookSpecificOutput`,
    /// it will simply allow the original command through (graceful degradation).
    pub fn rewrite(rewritten_command: &str) -> Self {
        Self {
            permission_decision: "allow".to_string(),
            permission_decision_reason: Some("terse command rewrite".to_string()),
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

        assert_eq!(request.tool_kind(), ToolKind::Bash);
        assert_eq!(request.tool_input.command.as_deref(), Some("git status"));
    }

    #[test]
    fn deserializes_non_bash_hook_request() {
        let input = r#"{"tool_name":"Edit","tool_input":{}}"#;
        let request: HookRequest = serde_json::from_str(input).unwrap();

        assert_eq!(request.tool_kind(), ToolKind::Unsupported);
        assert_eq!(request.tool_input.command, None);
    }

    // ToolKind classification -------------------------------------------------

    #[test]
    fn tool_kind_classifies_bash_case_insensitive() {
        assert_eq!(ToolKind::from_name("Bash"), ToolKind::Bash);
        assert_eq!(ToolKind::from_name("bash"), ToolKind::Bash);
        assert_eq!(ToolKind::from_name("BASH"), ToolKind::Bash);
    }

    #[test]
    fn tool_kind_classifies_unknown_as_unsupported() {
        assert_eq!(ToolKind::from_name("Edit"), ToolKind::Unsupported);
        assert_eq!(ToolKind::from_name("Read"), ToolKind::Unsupported);
        assert_eq!(ToolKind::from_name("Write"), ToolKind::Unsupported);
        assert_eq!(ToolKind::from_name(""), ToolKind::Unsupported);
    }

    #[test]
    fn tool_kind_display() {
        assert_eq!(ToolKind::Bash.to_string(), "bash");
        assert_eq!(ToolKind::Unsupported.to_string(), "unsupported");
    }

    // Copilot protocol --------------------------------------------------------

    #[test]
    fn copilot_allow_serializes_correctly() {
        let response = CopilotHookResponse::allow();
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(json, r#"{"permissionDecision":"allow"}"#);
    }

    #[test]
    fn copilot_deny_serializes_with_reason() {
        let response = CopilotHookResponse::deny("Dangerous command detected");
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(r#""permissionDecision":"deny""#));
        assert!(json.contains(r#""permissionDecisionReason":"Dangerous command detected""#));
    }

    #[test]
    fn copilot_rewrite_serializes_with_hook_specific_output() {
        let response = CopilotHookResponse::rewrite("terse run \"git status\"");
        let json = serde_json::to_string_pretty(&response).unwrap();

        // Top-level Copilot fields for safe fallback
        assert!(json.contains(r#""permissionDecision": "allow"#));
        assert!(json.contains(r#""permissionDecisionReason": "terse command rewrite"#));

        // hookSpecificOutput for rewrite
        assert!(json.contains("hookSpecificOutput"));
        assert!(json.contains("hookEventName"));
        assert!(json.contains("PreToolUse"));
        assert!(json.contains("updatedInput"));
        assert!(json.contains("terse run \\\"git status\\\""));
    }

    #[test]
    fn copilot_rewrite_contains_both_layers() {
        let response = CopilotHookResponse::rewrite("terse run \"ls -la\"");
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Top-level permissionDecision (Copilot fallback)
        assert_eq!(parsed["permissionDecision"], "allow");

        // hookSpecificOutput.updatedInput (rewrite protocol)
        assert_eq!(
            parsed["hookSpecificOutput"]["updatedInput"]["command"],
            "terse run \"ls -la\""
        );
        assert_eq!(
            parsed["hookSpecificOutput"]["permissionDecision"],
            "allow"
        );
    }

    #[test]
    fn copilot_request_deserializes_bash() {
        let input = r#"{"timestamp":1704614600000,"cwd":"/tmp","toolName":"bash","toolArgs":"{\"command\":\"git status\"}"}"#;
        let request: CopilotHookRequest = serde_json::from_str(input).unwrap();

        assert_eq!(request.tool_kind(), ToolKind::Bash);
        assert_eq!(request.command().as_deref(), Some("git status"));
        assert_eq!(request.tool_name, "bash");
        assert_eq!(request.cwd, "/tmp");
    }

    #[test]
    fn copilot_request_extracts_command_from_tool_args() {
        let input = r#"{"toolName":"bash","toolArgs":"{\"command\":\"npm test\",\"description\":\"Run tests\"}"}"#;
        let request: CopilotHookRequest = serde_json::from_str(input).unwrap();
        assert_eq!(request.command().as_deref(), Some("npm test"));
    }

    #[test]
    fn copilot_request_no_command_when_args_empty() {
        let input = r#"{"toolName":"view","toolArgs":""}"#;
        let request: CopilotHookRequest = serde_json::from_str(input).unwrap();
        assert_eq!(request.command(), None);
    }

    #[test]
    fn copilot_request_no_command_when_args_lack_command_field() {
        let input = r#"{"toolName":"edit","toolArgs":"{\"path\":\"/tmp/foo.rs\"}"}"#;
        let request: CopilotHookRequest = serde_json::from_str(input).unwrap();
        assert_eq!(request.command(), None);
    }
}
