/// Runtime feature-flag and configuration for the LLM Smart Path.
///
/// The smart path is **disabled by default** and must be explicitly enabled
/// via one of these mechanisms (highest precedence wins):
///
/// 1. **Environment variable**: `TERSE_SMART_PATH=1` (or `true`)
/// 2. **TOML config file**: `~/.terse/config.toml` or `.terse.toml`
///    ```toml
///    [smart_path]
///    enabled = true
///    ```
/// 3. **Legacy JSON config**: `~/.terse/config.json` (deprecated, migrate to TOML)
///
/// The env var overrides the config files, which override the built-in default.
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

/// Default Ollama endpoint.
const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";

/// Default model for the smart path.
const DEFAULT_MODEL: &str = "llama3.2:1b";

/// Default timeout in milliseconds for LLM requests.
///
/// 30 seconds allows time for cold model loading plus inference.
/// Warm inference with GPU should complete in 2-5 s for a 1B model.
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

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
}

impl Default for SmartPathConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: DEFAULT_MODEL.to_string(),
            ollama_url: DEFAULT_OLLAMA_URL.to_string(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
        }
    }
}

impl SmartPathConfig {
    /// Load the smart-path config using the unified configuration system.
    ///
    /// Precedence chain:
    /// built-in defaults → TOML config files → legacy JSON → env vars.
    ///
    /// The unified TOML config (`~/.terse/config.toml`, `.terse.toml`) is the
    /// primary source. The legacy JSON config (`~/.terse/config.json`) is
    /// checked as a fallback for backward compatibility.
    pub fn load() -> Self {
        let unified = crate::config::load();
        let mut config = Self {
            enabled: unified.smart_path.enabled,
            model: unified.smart_path.model,
            ollama_url: unified.smart_path.ollama_url,
            timeout_ms: unified.smart_path.max_latency_ms,
        };

        // Legacy fallback: if no TOML config file exists, check the old JSON
        // config for backward compatibility. The unified loader already applied
        // env vars, so only pull from JSON if the TOML didn't set these.
        if !toml_config_exists() {
            if let Some(file_cfg) = LegacyFileConfig::load()
                && let Some(sp) = file_cfg.smart_path
            {
                sp.apply_to(&mut config);
            }

            // Re-apply env vars (they must win over legacy JSON too)
            Self::apply_env_overrides(&mut config);
        }

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
    }
}

/// Check whether a TOML config file exists at any of the standard locations.
fn toml_config_exists() -> bool {
    let global = dirs::home_dir()
        .map(|h| h.join(".terse").join("config.toml"))
        .is_some_and(|p| p.exists());
    let project = std::env::current_dir()
        .ok()
        .map(|d| d.join(".terse.toml"))
        .is_some_and(|p| p.exists());
    global || project
}

// ---------------------------------------------------------------------------
// Legacy JSON config file schema (deprecated — use TOML)
// ---------------------------------------------------------------------------

/// Top-level legacy JSON config file schema (`~/.terse/config.json`).
#[derive(Debug, Deserialize)]
struct LegacyFileConfig {
    smart_path: Option<LegacyFileSmartPath>,
}

/// Smart-path section inside the legacy JSON config file.
///
/// All fields are optional — only present values override the defaults.
#[derive(Debug, Deserialize)]
struct LegacyFileSmartPath {
    enabled: Option<bool>,
    model: Option<String>,
    ollama_url: Option<String>,
    timeout_ms: Option<u64>,
}

impl LegacyFileSmartPath {
    /// Merge legacy file-level overrides into a [`SmartPathConfig`].
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
    }
}

impl LegacyFileConfig {
    /// Attempt to load the legacy config from `~/.terse/config.json`.
    /// Returns `None` if the file doesn't exist or is malformed.
    fn load() -> Option<Self> {
        let path = legacy_config_file_path()?;
        let contents = fs::read_to_string(path).ok()?;
        serde_json::from_str(&contents).ok()
    }
}

/// Resolve the path to the legacy JSON config file: `~/.terse/config.json`.
fn legacy_config_file_path() -> Option<PathBuf> {
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
        assert_eq!(config.timeout_ms, 30_000);
    }

    #[test]
    fn legacy_file_smart_path_apply_partial_overrides() {
        let mut config = SmartPathConfig::default();
        let file = LegacyFileSmartPath {
            enabled: Some(true),
            model: None,
            ollama_url: Some("http://custom:9999".to_string()),
            timeout_ms: None,
        };

        file.apply_to(&mut config);

        assert!(config.enabled);
        assert_eq!(config.model, "llama3.2:1b"); // unchanged
        assert_eq!(config.ollama_url, "http://custom:9999");
        assert_eq!(config.timeout_ms, 30_000); // unchanged
    }

    #[test]
    fn deserialize_legacy_config_json_full() {
        let json = r#"{
            "smart_path": {
                "enabled": true,
                "model": "qwen2.5:0.5b",
                "ollama_url": "http://localhost:11434",
                "timeout_ms": 3000
            }
        }"#;
        let file_cfg: LegacyFileConfig = serde_json::from_str(json).unwrap();
        let sp = file_cfg.smart_path.unwrap();
        assert_eq!(sp.enabled, Some(true));
        assert_eq!(sp.model.as_deref(), Some("qwen2.5:0.5b"));
        assert_eq!(sp.timeout_ms, Some(3000));
    }

    #[test]
    fn deserialize_legacy_config_json_minimal() {
        let json = r#"{ "smart_path": { "enabled": true } }"#;
        let file_cfg: LegacyFileConfig = serde_json::from_str(json).unwrap();
        let sp = file_cfg.smart_path.unwrap();
        assert_eq!(sp.enabled, Some(true));
        assert!(sp.model.is_none());
    }

    #[test]
    fn deserialize_legacy_config_json_empty() {
        let json = r#"{}"#;
        let file_cfg: LegacyFileConfig = serde_json::from_str(json).unwrap();
        assert!(file_cfg.smart_path.is_none());
    }
}
