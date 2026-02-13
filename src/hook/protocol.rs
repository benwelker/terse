use serde::{Deserialize, Serialize};

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

#[derive(Debug, Default, Serialize)]
pub struct HookResponse {}

impl HookResponse {
    pub fn passthrough() -> Self {
        Self::default()
    }
}
