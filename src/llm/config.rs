/// Runtime feature-flag and configuration for the LLM Smart Path.
///
/// The smart path is **disabled by default** and must be explicitly enabled
/// via one of two mechanisms (highest precedence wins):
///
/// 1. **Environment variable**: `TERSE_SMART_PATH=1` (or `true`)
/// 2. **JSON config file**: `~/.terse/config.json`
///    ```json
///    { "smart_path": { "enabled": true } }
///    ```
///
/// The env var overrides the JSON file, which overrides the built-in default.
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

/// Default Ollama endpoint.
const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";

/// Default model for the smart path.
const DEFAULT_MODEL: &str = "llama3.2:1b";

/// Default timeout in milliseconds for LLM requests.
const DEFAULT_TIMEOUT_MS: u64 = 5000;

/// Minimum output length (chars) to consider LLM optimization worthwhile.
const DEFAULT_MIN_OUTPUT_CHARS: usize = 200;

// ---------------------------------------------------------------------------
// Public config struct
// ---------------------------------------------------------------------------

/// Fully resolved configuration for the LLM Smart Path.
#[derive(Debug, Clone)]
pub struct SmartPathConfig {
    /// Whether the smart path is enabled (feature flag).
    pub enabled: bool,
    /// Ollama model name (e.g. `"llama3.2:1b"`).
    pub model: String,
    /// Ollama HTTP base URL.
    pub ollama_url: String,
    /// Request timeout in milliseconds.
    pub timeout_ms: u64,
    /// Minimum raw-output character count before LLM optimization is attempted.
    pub min_output_chars: usize,
}

impl Default for SmartPathConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: DEFAULT_MODEL.to_string(),
            ollama_url: DEFAULT_OLLAMA_URL.to_string(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            min_output_chars: DEFAULT_MIN_OUTPUT_CHARS,
        }
    }
}

impl SmartPathConfig {
    /// Load the smart-path config using the precedence chain:
    /// built-in defaults → JSON config file → environment variables.
    pub fn load() -> Self {
        let mut config = Self::default();

        // Layer 2: override from JSON config file
        if let Some(file_cfg) = FileConfig::load()
            && let Some(sp) = file_cfg.smart_path
        {
            sp.apply_to(&mut config);
        }

        // Layer 3: override from environment variables (highest precedence)
        Self::apply_env_overrides(&mut config);

        config
    }

    /// Apply environment-variable overrides.
    fn apply_env_overrides(config: &mut Self) {
        if let Ok(val) = std::env::var("TERSE_SMART_PATH") {
            config.enabled = matches!(
                val.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            );
        }

        if let Ok(val) = std::env::var("TERSE_SMART_PATH_MODEL")
            && !val.is_empty()
        {
            config.model = val;
        }

        if let Ok(val) = std::env::var("TERSE_SMART_PATH_URL")
            && !val.is_empty()
        {
            config.ollama_url = val;
        }

        if let Ok(val) = std::env::var("TERSE_SMART_PATH_TIMEOUT_MS")
            && let Ok(ms) = val.parse::<u64>()
        {
            config.timeout_ms = ms;
        }

        if let Ok(val) = std::env::var("TERSE_SMART_PATH_MIN_CHARS")
            && let Ok(chars) = val.parse::<usize>()
        {
            config.min_output_chars = chars;
        }
    }
}

// ---------------------------------------------------------------------------
// JSON config file schema
// ---------------------------------------------------------------------------

/// Top-level JSON config file schema (`~/.terse/config.json`).
#[derive(Debug, Deserialize)]
struct FileConfig {
    smart_path: Option<FileSmartPath>,
}

/// Smart-path section inside the JSON config file.
///
/// All fields are optional — only present values override the defaults.
#[derive(Debug, Deserialize)]
struct FileSmartPath {
    enabled: Option<bool>,
    model: Option<String>,
    ollama_url: Option<String>,
    timeout_ms: Option<u64>,
    min_output_chars: Option<usize>,
}

impl FileSmartPath {
    /// Merge file-level overrides into a [`SmartPathConfig`].
    fn apply_to(&self, config: &mut SmartPathConfig) {
        if let Some(enabled) = self.enabled {
            config.enabled = enabled;
        }
        if let Some(ref model) = self.model {
            config.model = model.clone();
        }
        if let Some(ref url) = self.ollama_url {
            config.ollama_url = url.clone();
        }
        if let Some(ms) = self.timeout_ms {
            config.timeout_ms = ms;
        }
        if let Some(chars) = self.min_output_chars {
            config.min_output_chars = chars;
        }
    }
}

impl FileConfig {
    /// Attempt to load the config from `~/.terse/config.json`.
    /// Returns `None` if the file doesn't exist or is malformed.
    fn load() -> Option<Self> {
        let path = config_file_path()?;
        let contents = fs::read_to_string(path).ok()?;
        serde_json::from_str(&contents).ok()
    }
}

/// Resolve the path to the JSON config file: `~/.terse/config.json`.
fn config_file_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".terse").join("config.json"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_smart_path_disabled() {
        let config = SmartPathConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.model, "llama3.2:1b");
        assert_eq!(config.ollama_url, "http://localhost:11434");
        assert_eq!(config.timeout_ms, 5000);
        assert_eq!(config.min_output_chars, 200);
    }

    #[test]
    fn file_smart_path_apply_partial_overrides() {
        let mut config = SmartPathConfig::default();
        let file = FileSmartPath {
            enabled: Some(true),
            model: None,
            ollama_url: Some("http://custom:9999".to_string()),
            timeout_ms: None,
            min_output_chars: Some(500),
        };

        file.apply_to(&mut config);

        assert!(config.enabled);
        assert_eq!(config.model, "llama3.2:1b"); // unchanged
        assert_eq!(config.ollama_url, "http://custom:9999");
        assert_eq!(config.timeout_ms, 5000); // unchanged
        assert_eq!(config.min_output_chars, 500);
    }

    #[test]
    fn deserialize_config_json_full() {
        let json = r#"{
            "smart_path": {
                "enabled": true,
                "model": "qwen2.5:0.5b",
                "ollama_url": "http://localhost:11434",
                "timeout_ms": 3000,
                "min_output_chars": 100
            }
        }"#;
        let file_cfg: FileConfig = serde_json::from_str(json).unwrap();
        let sp = file_cfg.smart_path.unwrap();
        assert_eq!(sp.enabled, Some(true));
        assert_eq!(sp.model.as_deref(), Some("qwen2.5:0.5b"));
        assert_eq!(sp.timeout_ms, Some(3000));
    }

    #[test]
    fn deserialize_config_json_minimal() {
        let json = r#"{ "smart_path": { "enabled": true } }"#;
        let file_cfg: FileConfig = serde_json::from_str(json).unwrap();
        let sp = file_cfg.smart_path.unwrap();
        assert_eq!(sp.enabled, Some(true));
        assert!(sp.model.is_none());
    }

    #[test]
    fn deserialize_config_json_empty() {
        let json = r#"{}"#;
        let file_cfg: FileConfig = serde_json::from_str(json).unwrap();
        assert!(file_cfg.smart_path.is_none());
    }
}
