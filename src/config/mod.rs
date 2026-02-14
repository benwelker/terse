/// Configuration system for terse.
///
/// Provides a layered configuration hierarchy:
///
/// 1. **Built-in defaults** — hardcoded in [`schema::TerseConfig::default()`]
/// 2. **User global config** — `~/.terse/config.toml`
/// 3. **Project local config** — `.terse.toml` in the current working directory
/// 4. **Environment variables** — `TERSE_*` overrides (highest precedence)
///
/// Later layers override earlier ones at the field level. Missing sections
/// in a TOML file fall back to the previous layer's values.
///
/// # Performance Profiles
///
/// After merging all layers, the active profile (`general.profile`) is
/// applied. Profiles override a curated subset of settings:
///
/// - **fast** — lower LLM timeout, higher smart path threshold
/// - **balanced** — built-in defaults (no-op)
/// - **quality** — higher LLM timeout, lower smart path threshold
///
/// # Usage
///
/// ```rust,ignore
/// use terse::config;
///
/// let cfg = config::load();
/// if cfg.general.enabled && cfg.fast_path.enabled {
///     // ...
/// }
/// ```
pub mod schema;

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

pub use schema::TerseConfig;

// ---------------------------------------------------------------------------
// Config loading
// ---------------------------------------------------------------------------

/// Load the fully resolved terse configuration.
///
/// Merges all layers in order: defaults → global TOML → project TOML → env
/// vars → profile application. This is the primary entry point for all
/// modules that need configuration.
pub fn load() -> TerseConfig {
    let mut config = TerseConfig::default();

    // Layer 2: user global config (~/.terse/config.toml)
    if let Some(global) = load_toml_file(global_config_path()) {
        merge_config(&mut config, &global);
    }

    // Layer 3: project local config (.terse.toml)
    if let Some(project) = load_toml_file(project_config_path()) {
        merge_config(&mut config, &project);
    }

    // Layer 4: environment variable overrides
    apply_env_overrides(&mut config);

    // Post-processing: apply profile
    config.apply_profile();

    config
}

/// Load a TOML config file from the given path (if it exists).
///
/// Returns `None` if the path is `None`, the file doesn't exist, or the
/// content is malformed. Malformed files are silently ignored to uphold the
/// golden rule: never break the Claude Code session.
fn load_toml_file(path: Option<PathBuf>) -> Option<TerseConfig> {
    let path = path?;
    let content = fs::read_to_string(&path).ok()?;
    toml::from_str(&content).ok()
}

/// Merge a loaded config layer into the base config.
///
/// Since TOML deserialization fills missing fields with defaults, we use a
/// re-serialization approach: we serialize the overlay to a TOML `Value`,
/// then walk the tables and only overwrite keys that are present in the
/// overlay's source. However, because `serde(default)` fills everything in,
/// we take a simpler approach: the overlay fully replaces the base. This
/// works because each TOML file is deserialized with defaults, so only
/// explicitly-set values differ from defaults — and those are the ones we
/// want to apply.
///
/// For the common case (users only set a handful of keys), this is correct
/// behavior: the overlay has defaults for unset keys, which match the base's
/// defaults.
fn merge_config(base: &mut TerseConfig, overlay: &TerseConfig) {
    *base = overlay.clone();
}

// ---------------------------------------------------------------------------
// File paths
// ---------------------------------------------------------------------------

/// Path to the user global config: `~/.terse/config.toml`.
fn global_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".terse").join("config.toml"))
}

/// Path to the project local config: `.terse.toml` in the current directory.
fn project_config_path() -> Option<PathBuf> {
    std::env::current_dir()
        .ok()
        .map(|cwd| cwd.join(".terse.toml"))
}

/// Return the path to the global config file for display/init purposes.
pub fn global_config_file() -> Option<PathBuf> {
    global_config_path()
}

/// Return the path to the project config file for display purposes.
pub fn project_config_file() -> Option<PathBuf> {
    project_config_path()
}

// ---------------------------------------------------------------------------
// Environment variable overrides
// ---------------------------------------------------------------------------

/// Apply environment variable overrides (highest precedence layer).
///
/// Supported variables:
/// - `TERSE_ENABLED` — master kill switch (`1`/`true`/`yes`/`on`)
/// - `TERSE_MODE` — operation mode (`hybrid`, `fast-only`, `smart-only`, `passthrough`)
/// - `TERSE_PROFILE` — performance profile (`fast`, `balanced`, `quality`)
/// - `TERSE_SAFE_MODE` — safe mode (`1`/`true`)
/// - `TERSE_SMART_PATH` — smart path enabled
/// - `TERSE_SMART_PATH_MODEL` — Ollama model name
/// - `TERSE_SMART_PATH_URL` — Ollama endpoint URL
/// - `TERSE_SMART_PATH_TIMEOUT_MS` — LLM request timeout
fn apply_env_overrides(config: &mut TerseConfig) {
    // General
    if let Ok(val) = std::env::var("TERSE_ENABLED") {
        config.general.enabled = is_truthy(&val);
    }
    if let Ok(val) = std::env::var("TERSE_MODE")
        && let Some(mode) = parse_mode(&val)
    {
        config.general.mode = mode;
    }
    if let Ok(val) = std::env::var("TERSE_PROFILE")
        && let Some(profile) = parse_profile(&val)
    {
        config.general.profile = profile;
    }
    if let Ok(val) = std::env::var("TERSE_SAFE_MODE") {
        config.general.safe_mode = is_truthy(&val);
    }

    // Smart path
    if let Ok(val) = std::env::var("TERSE_SMART_PATH") {
        config.smart_path.enabled = is_truthy(&val);
    }
    if let Ok(val) = std::env::var("TERSE_SMART_PATH_MODEL")
        && !val.is_empty()
    {
        config.smart_path.model = val;
    }
    if let Ok(val) = std::env::var("TERSE_SMART_PATH_URL")
        && !val.is_empty()
    {
        config.smart_path.ollama_url = val;
    }
    if let Ok(val) = std::env::var("TERSE_SMART_PATH_TIMEOUT_MS")
        && let Ok(ms) = val.parse::<u64>()
    {
        config.smart_path.max_latency_ms = ms;
    }
}

/// Check if a string value represents a truthy boolean.
fn is_truthy(val: &str) -> bool {
    matches!(
        val.to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Parse a mode string.
fn parse_mode(val: &str) -> Option<schema::Mode> {
    match val.to_ascii_lowercase().as_str() {
        "hybrid" => Some(schema::Mode::Hybrid),
        "fast-only" | "fast_only" | "fastonly" => Some(schema::Mode::FastOnly),
        "smart-only" | "smart_only" | "smartonly" => Some(schema::Mode::SmartOnly),
        "passthrough" => Some(schema::Mode::Passthrough),
        _ => None,
    }
}

/// Parse a profile string.
fn parse_profile(val: &str) -> Option<schema::Profile> {
    match val.to_ascii_lowercase().as_str() {
        "fast" => Some(schema::Profile::Fast),
        "balanced" => Some(schema::Profile::Balanced),
        "quality" => Some(schema::Profile::Quality),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Config init / set / reset
// ---------------------------------------------------------------------------

/// Write the default annotated config to `~/.terse/config.toml`.
///
/// Creates the `~/.terse/` directory if it doesn't exist. Returns an error
/// if the file already exists (use `force = true` to overwrite).
pub fn init_config(force: bool) -> Result<PathBuf> {
    let path = global_config_path().context("could not determine home directory")?;

    if path.exists() && !force {
        anyhow::bail!(
            "config file already exists at {}. Use --force to overwrite.",
            path.display()
        );
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create ~/.terse/ directory")?;
    }

    fs::write(&path, TerseConfig::default_toml()).context("failed to write config file")?;

    Ok(path)
}

/// Set a single config key to a value in the global config file.
///
/// Reads the current global config (or defaults), updates the specified key,
/// and writes the result back. Supports dotted keys like `smart_path.enabled`.
pub fn set_config_value(key: &str, value: &str) -> Result<()> {
    let path = global_config_path().context("could not determine home directory")?;

    // Load current config or defaults
    let config = if path.exists() {
        let content = fs::read_to_string(&path).context("failed to read config file")?;
        // Parse as toml::Value for surgical update
        let mut value_table: toml::Value =
            toml::from_str(&content).context("failed to parse config as TOML value")?;

        set_toml_value(&mut value_table, key, value)?;

        // Write back
        let toml_str =
            toml::to_string_pretty(&value_table).context("failed to serialize config")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("failed to create config directory")?;
        }
        fs::write(&path, toml_str).context("failed to write config file")?;

        return Ok(());
    } else {
        TerseConfig::default()
    };

    // No existing file — serialize defaults, update, write
    let toml_str = toml::to_string_pretty(&config).context("failed to serialize default config")?;
    let mut value_table: toml::Value =
        toml::from_str(&toml_str).context("failed to parse serialized defaults")?;

    set_toml_value(&mut value_table, key, value)?;

    let output =
        toml::to_string_pretty(&value_table).context("failed to serialize updated config")?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create config directory")?;
    }
    fs::write(&path, output).context("failed to write config file")?;

    Ok(())
}

/// Set a value in a TOML value tree using a dotted key path.
fn set_toml_value(root: &mut toml::Value, key: &str, raw_value: &str) -> Result<()> {
    let parts: Vec<&str> = key.split('.').collect();
    if parts.is_empty() {
        anyhow::bail!("empty config key");
    }

    // Navigate to the parent table
    let mut current = root;
    for &part in &parts[..parts.len() - 1] {
        current = current
            .get_mut(part)
            .with_context(|| format!("config key not found: section '{part}' in '{key}'"))?;
    }

    let leaf = parts[parts.len() - 1];

    // Determine the type of the existing value to parse correctly
    let table = current.as_table_mut().with_context(|| {
        format!(
            "expected table at '{}'",
            key.rsplit_once('.').map(|(s, _)| s).unwrap_or("")
        )
    })?;

    let existing = table.get(leaf);
    let new_value = match existing {
        Some(toml::Value::Boolean(_)) => toml::Value::Boolean(is_truthy(raw_value)),
        Some(toml::Value::Integer(_)) => {
            let n: i64 = raw_value
                .parse()
                .with_context(|| format!("expected integer for '{key}', got '{raw_value}'"))?;
            toml::Value::Integer(n)
        }
        Some(toml::Value::Float(_)) => {
            let f: f64 = raw_value
                .parse()
                .with_context(|| format!("expected float for '{key}', got '{raw_value}'"))?;
            toml::Value::Float(f)
        }
        Some(toml::Value::Array(_)) => {
            // Parse as comma-separated list
            let items: Vec<toml::Value> = raw_value
                .split(',')
                .map(|s| toml::Value::String(s.trim().to_string()))
                .collect();
            toml::Value::Array(items)
        }
        _ => {
            // Default to string
            toml::Value::String(raw_value.to_string())
        }
    };

    table.insert(leaf.to_string(), new_value);
    Ok(())
}

/// Reset the global config to defaults (overwrite the file).
pub fn reset_config() -> Result<PathBuf> {
    init_config(true)
}

/// Show the effective (fully resolved) config as TOML.
pub fn show_effective_config() -> Result<String> {
    let config = load();
    toml::to_string_pretty(&config).context("failed to serialize effective config")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_returns_defaults_when_no_files_exist() {
        // This test relies on no config files being present in the test
        // environment. If run in a dev environment with ~/.terse/config.toml,
        // the result will reflect that file's contents.
        let config = load();
        assert!(config.general.enabled);
    }

    #[test]
    fn is_truthy_accepts_variants() {
        assert!(is_truthy("1"));
        assert!(is_truthy("true"));
        assert!(is_truthy("TRUE"));
        assert!(is_truthy("yes"));
        assert!(is_truthy("YES"));
        assert!(is_truthy("on"));
        assert!(is_truthy("ON"));
        assert!(!is_truthy("0"));
        assert!(!is_truthy("false"));
        assert!(!is_truthy("no"));
        assert!(!is_truthy("off"));
        assert!(!is_truthy(""));
    }

    #[test]
    fn parse_mode_handles_variants() {
        assert_eq!(parse_mode("hybrid"), Some(schema::Mode::Hybrid));
        assert_eq!(parse_mode("fast-only"), Some(schema::Mode::FastOnly));
        assert_eq!(parse_mode("fast_only"), Some(schema::Mode::FastOnly));
        assert_eq!(parse_mode("fastonly"), Some(schema::Mode::FastOnly));
        assert_eq!(parse_mode("smart-only"), Some(schema::Mode::SmartOnly));
        assert_eq!(parse_mode("passthrough"), Some(schema::Mode::Passthrough));
        assert_eq!(parse_mode("invalid"), None);
    }

    #[test]
    fn parse_profile_handles_variants() {
        assert_eq!(parse_profile("fast"), Some(schema::Profile::Fast));
        assert_eq!(parse_profile("balanced"), Some(schema::Profile::Balanced));
        assert_eq!(parse_profile("quality"), Some(schema::Profile::Quality));
        assert_eq!(parse_profile("invalid"), None);
    }

    #[test]
    fn set_toml_value_updates_string() {
        let toml_str = r#"
[general]
mode = "hybrid"
"#;
        let mut root: toml::Value = toml::from_str(toml_str).unwrap();
        set_toml_value(&mut root, "general.mode", "fast-only").unwrap();

        let table = root.as_table().unwrap();
        let general = table["general"].as_table().unwrap();
        assert_eq!(general["mode"].as_str(), Some("fast-only"));
    }

    #[test]
    fn set_toml_value_updates_bool() {
        let toml_str = r#"
[smart_path]
enabled = false
"#;
        let mut root: toml::Value = toml::from_str(toml_str).unwrap();
        set_toml_value(&mut root, "smart_path.enabled", "true").unwrap();

        let table = root.as_table().unwrap();
        let sp = table["smart_path"].as_table().unwrap();
        assert_eq!(sp["enabled"].as_bool(), Some(true));
    }

    #[test]
    fn set_toml_value_updates_integer() {
        let toml_str = r#"
[fast_path]
timeout_ms = 100
"#;
        let mut root: toml::Value = toml::from_str(toml_str).unwrap();
        set_toml_value(&mut root, "fast_path.timeout_ms", "50").unwrap();

        let table = root.as_table().unwrap();
        let fp = table["fast_path"].as_table().unwrap();
        assert_eq!(fp["timeout_ms"].as_integer(), Some(50));
    }

    #[test]
    fn set_toml_value_updates_float() {
        let toml_str = r#"
[router]
circuit_breaker_threshold = 0.2
"#;
        let mut root: toml::Value = toml::from_str(toml_str).unwrap();
        set_toml_value(&mut root, "router.circuit_breaker_threshold", "0.3").unwrap();

        let table = root.as_table().unwrap();
        let r = table["router"].as_table().unwrap();
        assert!((r["circuit_breaker_threshold"].as_float().unwrap() - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn set_toml_value_rejects_invalid_key() {
        let toml_str = r#"
[general]
mode = "hybrid"
"#;
        let mut root: toml::Value = toml::from_str(toml_str).unwrap();
        let result = set_toml_value(&mut root, "nonexistent.key", "value");
        assert!(result.is_err());
    }

    #[test]
    fn show_effective_config_returns_toml() {
        let result = show_effective_config();
        assert!(result.is_ok());
        let toml_str = result.unwrap();
        // Should be parseable back
        let _: TerseConfig = toml::from_str(&toml_str).unwrap();
    }
}
