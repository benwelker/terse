/// Configuration schema and defaults for the entire terse system.
///
/// Defines the TOML-serializable configuration structure with all sections:
/// `[general]`, `[fast_path]`, `[smart_path]`, `[output_thresholds]`,
/// `[preprocessing]`, `[router]`, `[passthrough]`, `[logging]`, and
/// `[whitespace]`.
///
/// Every field has a sensible built-in default. Users only need to set the
/// values they want to override.
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Top-level config
// ---------------------------------------------------------------------------

/// Top-level terse configuration.
///
/// Maps directly to the `~/.terse/config.toml` and `.terse.toml` file
/// schemas. All sections and fields are optional — missing values fall back
/// to built-in defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TerseConfig {
    pub general: GeneralConfig,
    pub fast_path: FastPathConfig,
    pub smart_path: SmartPathConfig,
    pub output_thresholds: OutputThresholds,
    pub preprocessing: PreprocessingConfig,
    pub router: RouterConfig,
    pub passthrough: PassthroughConfig,
    pub logging: LoggingConfig,
    pub whitespace: WhitespaceConfig,
    pub optimizers: OptimizersConfig,
}

// ---------------------------------------------------------------------------
// [general]
// ---------------------------------------------------------------------------

/// Operation mode for terse.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Both fast path and smart path enabled (default).
    #[default]
    Hybrid,
    /// Only rule-based optimizers, no LLM.
    FastOnly,
    /// Only LLM smart path, skip rule-based optimizers.
    SmartOnly,
    /// All optimization disabled — log only.
    Passthrough,
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hybrid => write!(f, "hybrid"),
            Self::FastOnly => write!(f, "fast-only"),
            Self::SmartOnly => write!(f, "smart-only"),
            Self::Passthrough => write!(f, "passthrough"),
        }
    }
}

/// Performance profile presets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    /// Prefer fast path, aggressive thresholds, lower LLM timeout.
    Fast,
    /// Default balanced settings.
    #[default]
    Balanced,
    /// Prefer smart path, lower thresholds, higher LLM timeout.
    Quality,
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fast => write!(f, "fast"),
            Self::Balanced => write!(f, "balanced"),
            Self::Quality => write!(f, "quality"),
        }
    }
}

/// General terse settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// Master kill switch — set to `false` to disable all optimization.
    pub enabled: bool,
    /// Operation mode: `hybrid`, `fast-only`, `smart-only`, `passthrough`.
    pub mode: Mode,
    /// Performance profile preset. When set, overrides individual settings
    /// in `fast_path`, `smart_path`, and `output_thresholds` unless those
    /// sections are explicitly specified.
    pub profile: Profile,
    /// Safe mode — disables all optimization, log only.
    /// Can also be set via `TERSE_SAFE_MODE=1`.
    pub safe_mode: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: Mode::default(),
            profile: Profile::default(),
            safe_mode: false,
        }
    }
}

// ---------------------------------------------------------------------------
// [fast_path]
// ---------------------------------------------------------------------------

/// Configuration for optimizer toggles within the fast path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FastPathOptimizers {
    pub git: bool,
    pub file: bool,
    pub build: bool,
    pub docker: bool,
    pub whitespace: bool,
}

impl Default for FastPathOptimizers {
    fn default() -> Self {
        Self {
            git: true,
            file: true,
            build: true,
            docker: true,
            whitespace: true,
        }
    }
}

/// Fast path (rule-based optimizer) settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FastPathConfig {
    /// Whether the fast path is enabled.
    pub enabled: bool,
    /// Maximum time budget for a fast-path optimizer (milliseconds).
    pub timeout_ms: u64,
    /// Per-optimizer enable/disable toggles.
    pub optimizers: FastPathOptimizers,
}

impl Default for FastPathConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            timeout_ms: 100,
            optimizers: FastPathOptimizers::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// [smart_path]
// ---------------------------------------------------------------------------

/// Smart path (LLM via Ollama) settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SmartPathConfig {
    /// Whether the smart path is enabled (default: false, opt-in).
    pub enabled: bool,
    /// Ollama model name.
    pub model: String,
    /// Sampling temperature (0.0 = deterministic).
    pub temperature: f64,
    /// Maximum allowed latency for an LLM request (milliseconds).
    pub max_latency_ms: u64,
    /// Ollama HTTP base URL.
    pub ollama_url: String,
}

impl Default for SmartPathConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: "llama3.2:1b".to_string(),
            temperature: 0.0,
            max_latency_ms: 60000,
            ollama_url: "http://localhost:11434".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// [output_thresholds]
// ---------------------------------------------------------------------------

/// Byte-based thresholds for path routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputThresholds {
    /// Outputs below this size (bytes) are passed through unoptimized.
    pub passthrough_below_bytes: usize,
    /// Outputs at or above this size (bytes) are eligible for the smart path.
    /// Outputs between `passthrough_below_bytes` and this value use fast-path
    /// post-processing only.
    pub smart_path_above_bytes: usize,
}

impl Default for OutputThresholds {
    fn default() -> Self {
        Self {
            passthrough_below_bytes: 2048,     // 2 KB
            smart_path_above_bytes: 10 * 1024, // 10 KB
        }
    }
}

// ---------------------------------------------------------------------------
// [preprocessing]
// ---------------------------------------------------------------------------

/// Preprocessing pipeline settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PreprocessingConfig {
    /// Whether preprocessing is enabled before LLM calls.
    pub enabled: bool,
    /// Maximum output size (bytes) after preprocessing — truncation kicks in
    /// if exceeded.
    pub max_output_bytes: usize,
    /// Enable noise removal (ANSI codes, progress bars, boilerplate).
    pub noise_removal: bool,
    /// Enable path filtering (node_modules, target, etc.).
    pub path_filtering: bool,
    /// Path filter mode: `"summary"` (annotated) or `"remove"` (silent).
    pub path_filter_mode: String,
    /// Enable deduplication of repeated lines/blocks.
    pub deduplication: bool,
    /// Enable truncation with context preservation.
    pub truncation: bool,
    /// Additional boilerplate patterns (appended to built-in list).
    #[serde(default)]
    pub extra_boilerplate: Vec<String>,
    /// Additional directories to filter (appended to built-in list).
    #[serde(default)]
    pub extra_filtered_dirs: Vec<String>,
}

impl Default for PreprocessingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_output_bytes: 128 * 1024, // 128 KB
            noise_removal: true,
            path_filtering: true,
            path_filter_mode: "summary".to_string(),
            deduplication: true,
            truncation: true,
            extra_boilerplate: Vec::new(),
            extra_filtered_dirs: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// [router]
// ---------------------------------------------------------------------------

/// Router and circuit breaker settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RouterConfig {
    /// TTL for the in-memory decision cache (seconds).
    pub decision_cache_ttl_secs: u64,
    /// Failure rate threshold to trip the circuit breaker (0.0–1.0).
    pub circuit_breaker_threshold: f64,
    /// Rolling window size for circuit breaker tracking.
    pub circuit_breaker_window: usize,
    /// Cooldown duration (seconds) after tripping the circuit breaker.
    pub circuit_breaker_cooldown_secs: i64,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            decision_cache_ttl_secs: 300,
            circuit_breaker_threshold: 0.2,
            circuit_breaker_window: 10,
            circuit_breaker_cooldown_secs: 600,
        }
    }
}

// ---------------------------------------------------------------------------
// [passthrough]
// ---------------------------------------------------------------------------

/// Passthrough command list — commands that should never be optimized.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PassthroughConfig {
    /// Commands to always pass through (matched by first word).
    pub commands: Vec<String>,
}

impl Default for PassthroughConfig {
    fn default() -> Self {
        Self {
            commands: vec![
                "code".to_string(),
                "vim".to_string(),
                "vi".to_string(),
                "nano".to_string(),
                "emacs".to_string(),
                "subl".to_string(),
                "notepad".to_string(),
                "rm".to_string(),
                "rmdir".to_string(),
                "del".to_string(),
                "mv".to_string(),
                "move".to_string(),
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// [logging]
// ---------------------------------------------------------------------------

/// Logging settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// Whether command logging is enabled.
    pub enabled: bool,
    /// Path to the command log file. `~` is expanded to the home directory.
    pub path: String,
    /// Log level: `"info"`, `"debug"`, `"warn"`, `"error"`.
    pub level: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: "~/.terse/command-log.jsonl".to_string(),
            level: "info".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// [whitespace]
// ---------------------------------------------------------------------------

/// Whitespace normalization settings (post-processing pass).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WhitespaceConfig {
    /// Whether the whitespace optimizer is enabled.
    pub enabled: bool,
    /// Maximum consecutive blank lines allowed.
    pub max_consecutive_newlines: usize,
    /// Whether to normalize tabs to spaces.
    pub normalize_tabs: bool,
    /// Whether to trim trailing whitespace from each line.
    pub trim_trailing: bool,
}

impl Default for WhitespaceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_consecutive_newlines: 2,
            normalize_tabs: true,
            trim_trailing: true,
        }
    }
}

// ---------------------------------------------------------------------------
// [optimizers] — per-optimizer configurable limits
// ---------------------------------------------------------------------------

/// Per-optimizer limit configurations.
///
/// Each sub-section controls the truncation and filtering limits for a
/// specific optimizer. These limits determine how aggressively output is
/// compacted. All values have sensible built-in defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OptimizersConfig {
    pub git: GitOptimizerConfig,
    pub file: FileOptimizerConfig,
    pub build: BuildOptimizerConfig,
    pub docker: DockerOptimizerConfig,
    pub generic: GenericOptimizerConfig,
}

/// Git optimizer limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GitOptimizerConfig {
    /// Whether the git optimizer is enabled.
    pub enabled: bool,
    /// Maximum log entries to display when user has custom format/limit.
    pub log_max_entries: usize,
    /// Default `-n` limit added when user omits it.
    pub log_default_limit: usize,
    /// Maximum character width for log lines before truncation.
    pub log_line_max_chars: usize,
    /// Maximum changed lines per diff hunk before truncation.
    pub diff_max_hunk_lines: usize,
    /// Maximum total diff lines before the entire diff is truncated.
    pub diff_max_total_lines: usize,
    /// Maximum local branches shown.
    pub branch_max_local: usize,
    /// Maximum remote-only branches shown.
    pub branch_max_remote: usize,
}

impl Default for GitOptimizerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            log_max_entries: 50,
            log_default_limit: 20,
            log_line_max_chars: 120,
            diff_max_hunk_lines: 15,
            diff_max_total_lines: 200,
            branch_max_local: 20,
            branch_max_remote: 10,
        }
    }
}

/// File optimizer limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileOptimizerConfig {
    /// Whether the file optimizer is enabled.
    pub enabled: bool,
    /// Maximum entries for long-format `ls -l` output.
    pub ls_max_entries: usize,
    /// Maximum items for simple `ls` output.
    pub ls_max_items: usize,
    /// Maximum results for `find` output.
    pub find_max_results: usize,
    /// Maximum total lines for `cat`/`head`/`tail` output.
    pub cat_max_lines: usize,
    /// Lines to keep from the head of `cat` output when truncating.
    pub cat_head_lines: usize,
    /// Lines to keep from the tail of `cat` output when truncating.
    pub cat_tail_lines: usize,
    /// Maximum lines for `wc` output.
    pub wc_max_lines: usize,
    /// Maximum lines for `tree` output.
    pub tree_max_lines: usize,
    /// Directory names to prune from `tree` output.
    ///
    /// When a tree entry matches one of these names, the entire subtree is
    /// collapsed into a single `[contents hidden]` marker. Case-insensitive.
    pub tree_noise_dirs: Vec<String>,
}

impl Default for FileOptimizerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ls_max_entries: 50,
            ls_max_items: 60,
            find_max_results: 40,
            cat_max_lines: 100,
            cat_head_lines: 60,
            cat_tail_lines: 30,
            wc_max_lines: 30,
            tree_max_lines: 60,
            tree_noise_dirs: default_tree_noise_dirs(),
        }
    }
}

/// Default set of directory names to prune from tree output.
fn default_tree_noise_dirs() -> Vec<String> {
    [
        "node_modules",
        ".git",
        "__pycache__",
        ".mypy_cache",
        ".pytest_cache",
        ".tox",
        ".next",
        ".nuxt",
        ".cache",
        "coverage",
        ".nyc_output",
        "vendor",
        "Pods",
        ".gradle",
        ".idea",
        ".vs",
        ".vscode",
        "bin",
        "obj",
        "target",
        "dist",
        "build",
        ".angular",
        ".svn",
        ".hg",
        ".terraform",
        ".serverless",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// Build/test/lint optimizer limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildOptimizerConfig {
    /// Whether the build optimizer is enabled.
    pub enabled: bool,
    /// Maximum failure detail lines in test output.
    pub test_max_failure_lines: usize,
    /// Maximum error lines in test output.
    pub test_max_error_lines: usize,
    /// Maximum warnings shown in test output.
    pub test_max_warnings: usize,
    /// Maximum error lines in build output.
    pub build_max_error_lines: usize,
    /// Maximum warnings shown in build output.
    pub build_max_warnings: usize,
    /// Maximum issue lines in lint output.
    pub lint_max_issue_lines: usize,
}

impl Default for BuildOptimizerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            test_max_failure_lines: 80,
            test_max_error_lines: 40,
            test_max_warnings: 10,
            build_max_error_lines: 60,
            build_max_warnings: 10,
            lint_max_issue_lines: 80,
        }
    }
}

/// Docker optimizer limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DockerOptimizerConfig {
    /// Whether the docker optimizer is enabled.
    pub enabled: bool,
    /// Maximum rows for `docker ps` output.
    pub ps_max_rows: usize,
    /// Maximum rows for `docker images` output.
    pub images_max_rows: usize,
    /// Maximum tail lines for `docker logs`.
    pub logs_max_tail: usize,
    /// Maximum error lines extracted from `docker logs`.
    pub logs_max_errors: usize,
    /// Maximum lines for `docker inspect` output.
    pub inspect_max_lines: usize,
    /// Maximum rows for `docker compose ps` output.
    pub compose_max_rows: usize,
    /// Maximum rows for `docker network/volume ls`.
    pub resource_max_rows: usize,
}

impl Default for DockerOptimizerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ps_max_rows: 30,
            images_max_rows: 30,
            logs_max_tail: 30,
            logs_max_errors: 20,
            inspect_max_lines: 60,
            compose_max_rows: 30,
            resource_max_rows: 30,
        }
    }
}

/// Generic (fallback) optimizer limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GenericOptimizerConfig {
    /// Whether the generic (fallback) optimizer is enabled.
    pub enabled: bool,
    /// Minimum raw output size (bytes) before optimization kicks in.
    /// Outputs smaller than this are passed through unchanged.
    pub min_size_bytes: usize,
    /// Maximum lines to keep in the output (head/tail preserved).
    pub max_lines: usize,
}

impl Default for GenericOptimizerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_size_bytes: 512,
            max_lines: 200,
        }
    }
}

// ---------------------------------------------------------------------------
// Profile application
// ---------------------------------------------------------------------------

impl TerseConfig {
    /// Apply a performance profile's overrides to this config.
    ///
    /// Profile values only override settings that the user has not explicitly
    /// set. Since we cannot track "was this set explicitly?" in TOML
    /// deserialization, profiles are applied as a post-processing step that
    /// overwrites relevant defaults.
    pub fn apply_profile(&mut self) {
        match self.general.profile {
            Profile::Fast => self.apply_fast_profile(),
            Profile::Balanced => {} // defaults are already balanced
            Profile::Quality => self.apply_quality_profile(),
        }
    }

    fn apply_fast_profile(&mut self) {
        // Prefer fast path, minimize LLM usage
        self.fast_path.timeout_ms = 50;
        self.smart_path.max_latency_ms = 1500;
        self.output_thresholds.passthrough_below_bytes = 1024; // 1 KB
        self.output_thresholds.smart_path_above_bytes = 20 * 1024; // 20 KB
    }

    fn apply_quality_profile(&mut self) {
        // Prefer smart path, lower thresholds, higher LLM timeout
        self.smart_path.max_latency_ms = 5000;
        self.output_thresholds.passthrough_below_bytes = 512;
        self.output_thresholds.smart_path_above_bytes = 4 * 1024; // 4 KB
    }
}

// ---------------------------------------------------------------------------
// Default TOML content
// ---------------------------------------------------------------------------

impl TerseConfig {
    /// Generate the annotated default TOML config file content.
    ///
    /// Used by `terse config init` to create a starting config file with
    /// all settings documented.
    pub fn default_toml() -> String {
        r#"# terse Configuration
# Token Efficiency through Refined Stream Engineering
#
# Configuration hierarchy (highest precedence wins):
#   1. Environment variables (TERSE_*)
#   2. Project config (.terse.toml in current directory)
#   3. User global config (~/.terse/config.toml)
#   4. Built-in defaults

[general]
enabled = true
mode = "hybrid"       # hybrid | fast-only | smart-only | passthrough
profile = "balanced"  # fast | balanced | quality
safe_mode = false     # Set true or TERSE_SAFE_MODE=1 to disable all optimization

[fast_path]
enabled = true
timeout_ms = 100

[fast_path.optimizers]
git = true
file = true
build = true
docker = true
whitespace = true

[smart_path]
enabled = false                       # Opt-in: set true or TERSE_SMART_PATH=1
model = "llama3.2:1b"
temperature = 0.0
max_latency_ms = 3000
ollama_url = "http://localhost:11434"

[output_thresholds]
passthrough_below_bytes = 2048        # < 2 KB  -> passthrough
smart_path_above_bytes = 20480        # >= 20 KB -> smart path eligible

[preprocessing]
enabled = true
max_output_bytes = 32768              # Truncate to 32 KB after preprocessing
noise_removal = true
path_filtering = true
path_filter_mode = "summary"          # "summary" (annotated) or "remove" (silent)
deduplication = true
truncation = true
# extra_boilerplate = []              # Additional boilerplate patterns
# extra_filtered_dirs = []            # Additional directories to filter

[router]
decision_cache_ttl_secs = 300
circuit_breaker_threshold = 0.2
circuit_breaker_window = 10
circuit_breaker_cooldown_secs = 600

[passthrough]
commands = ["code", "vim", "vi", "nano", "emacs", "subl", "notepad", "rm", "rmdir", "del", "mv", "move"]

[logging]
enabled = true
path = "~/.terse/command-log.jsonl"
level = "info"

[whitespace]
enabled = true
max_consecutive_newlines = 2
normalize_tabs = true
trim_trailing = true

[optimizers.git]
enabled = true
log_max_entries = 50
log_default_limit = 20
log_line_max_chars = 120
diff_max_hunk_lines = 15
diff_max_total_lines = 200
branch_max_local = 20
branch_max_remote = 10

[optimizers.file]
enabled = true
ls_max_entries = 50
ls_max_items = 60
find_max_results = 40
cat_max_lines = 100
cat_head_lines = 60
cat_tail_lines = 30
wc_max_lines = 30
tree_max_lines = 60

[optimizers.build]
enabled = true
test_max_failure_lines = 80
test_max_error_lines = 40
test_max_warnings = 10
build_max_error_lines = 60
build_max_warnings = 10
lint_max_issue_lines = 80

[optimizers.docker]
enabled = true
ps_max_rows = 30
images_max_rows = 30
logs_max_tail = 30
logs_max_errors = 20
inspect_max_lines = 60
compose_max_rows = 30
resource_max_rows = 30

[optimizers.generic]
enabled = true
min_size_bytes = 512                  # Outputs below this size skip generic cleanup
max_lines = 200                       # Maximum lines before head/tail truncation
"#
        .to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let config = TerseConfig::default();
        assert!(config.general.enabled);
        assert_eq!(config.general.mode, Mode::Hybrid);
        assert_eq!(config.general.profile, Profile::Balanced);
        assert!(!config.general.safe_mode);
        assert!(config.fast_path.enabled);
        assert_eq!(config.fast_path.timeout_ms, 100);
        assert!(!config.smart_path.enabled);
        assert_eq!(config.smart_path.model, "llama3.2:1b");
        assert_eq!(config.output_thresholds.passthrough_below_bytes, 2048);
        assert_eq!(config.output_thresholds.smart_path_above_bytes, 10240);
        assert!(config.preprocessing.enabled);
        assert!(config.logging.enabled);
    }

    #[test]
    fn deserialize_minimal_toml() {
        let toml_str = r#"
[general]
enabled = true
"#;
        let config: TerseConfig = toml::from_str(toml_str).unwrap();
        assert!(config.general.enabled);
        // All other sections fall back to defaults
        assert!(!config.smart_path.enabled);
        assert_eq!(config.fast_path.timeout_ms, 100);
    }

    #[test]
    fn deserialize_full_toml() {
        let toml_str = r#"
[general]
enabled = true
mode = "fast-only"
profile = "fast"
safe_mode = false

[fast_path]
enabled = true
timeout_ms = 50

[fast_path.optimizers]
git = true
file = false
build = true
docker = false
whitespace = true

[smart_path]
enabled = true
model = "qwen2.5:0.5b"
temperature = 0.1
max_latency_ms = 2000
ollama_url = "http://custom:9999"

[output_thresholds]
passthrough_below_bytes = 1024
smart_path_above_bytes = 20480

[preprocessing]
enabled = true
max_output_bytes = 16384
noise_removal = true
path_filtering = false
path_filter_mode = "remove"
deduplication = true
truncation = false
extra_boilerplate = ["custom pattern"]
extra_filtered_dirs = [".custom_cache/"]

[router]
decision_cache_ttl_secs = 120
circuit_breaker_threshold = 0.3
circuit_breaker_window = 5
circuit_breaker_cooldown_secs = 300

[passthrough]
commands = ["code", "vim"]

[logging]
enabled = false
path = "/tmp/terse.jsonl"
level = "debug"

[whitespace]
enabled = false
max_consecutive_newlines = 3
normalize_tabs = false
trim_trailing = false
"#;
        let config: TerseConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.general.mode, Mode::FastOnly);
        assert_eq!(config.general.profile, Profile::Fast);
        assert!(config.smart_path.enabled);
        assert_eq!(config.smart_path.model, "qwen2.5:0.5b");
        assert_eq!(config.smart_path.temperature, 0.1);
        assert_eq!(config.output_thresholds.passthrough_below_bytes, 1024);
        assert!(!config.fast_path.optimizers.file);
        assert!(!config.fast_path.optimizers.docker);
        assert_eq!(config.preprocessing.max_output_bytes, 16384);
        assert!(!config.preprocessing.path_filtering);
        assert_eq!(
            config.preprocessing.extra_boilerplate,
            vec!["custom pattern"]
        );
        assert_eq!(config.router.circuit_breaker_window, 5);
        assert_eq!(config.passthrough.commands, vec!["code", "vim"]);
        assert!(!config.logging.enabled);
        assert!(!config.whitespace.enabled);
    }

    #[test]
    fn empty_toml_produces_defaults() {
        let config: TerseConfig = toml::from_str("").unwrap();
        assert!(config.general.enabled);
        assert_eq!(config.general.mode, Mode::Hybrid);
        assert!(!config.smart_path.enabled);
    }

    #[test]
    fn fast_profile_adjusts_thresholds() {
        let mut config = TerseConfig::default();
        config.general.profile = Profile::Fast;
        config.apply_profile();

        assert_eq!(config.fast_path.timeout_ms, 50);
        assert_eq!(config.smart_path.max_latency_ms, 1500);
        assert_eq!(config.output_thresholds.passthrough_below_bytes, 1024);
        assert_eq!(config.output_thresholds.smart_path_above_bytes, 20480);
    }

    #[test]
    fn quality_profile_adjusts_thresholds() {
        let mut config = TerseConfig::default();
        config.general.profile = Profile::Quality;
        config.apply_profile();

        assert_eq!(config.smart_path.max_latency_ms, 5000);
        assert_eq!(config.output_thresholds.passthrough_below_bytes, 512);
        assert_eq!(config.output_thresholds.smart_path_above_bytes, 4096);
    }

    #[test]
    fn balanced_profile_is_noop() {
        let original = TerseConfig::default();
        let mut config = TerseConfig::default();
        config.general.profile = Profile::Balanced;
        config.apply_profile();

        assert_eq!(config.fast_path.timeout_ms, original.fast_path.timeout_ms);
        assert_eq!(
            config.smart_path.max_latency_ms,
            original.smart_path.max_latency_ms
        );
    }

    #[test]
    fn default_toml_parses_back() {
        let toml_str = TerseConfig::default_toml();
        let config: TerseConfig = toml::from_str(&toml_str).unwrap();
        assert!(config.general.enabled);
        assert!(!config.smart_path.enabled);
    }

    #[test]
    fn mode_display() {
        assert_eq!(Mode::Hybrid.to_string(), "hybrid");
        assert_eq!(Mode::FastOnly.to_string(), "fast-only");
        assert_eq!(Mode::SmartOnly.to_string(), "smart-only");
        assert_eq!(Mode::Passthrough.to_string(), "passthrough");
    }

    #[test]
    fn profile_display() {
        assert_eq!(Profile::Fast.to_string(), "fast");
        assert_eq!(Profile::Balanced.to_string(), "balanced");
        assert_eq!(Profile::Quality.to_string(), "quality");
    }

    #[test]
    fn passthrough_config_defaults() {
        let config = PassthroughConfig::default();
        assert!(config.commands.contains(&"rm".to_string()));
        assert!(config.commands.contains(&"code".to_string()));
        assert!(config.commands.contains(&"vim".to_string()));
    }
}
