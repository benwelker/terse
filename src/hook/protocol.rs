use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Supported tool classification
// ---------------------------------------------------------------------------

/// Recognized Claude Code tool categories.
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
    /// Classify a Claude Code tool name into a [`ToolKind`].
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
}
