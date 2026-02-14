//! JSON API handlers for the web dashboard.
//!
//! Each handler corresponds to an API endpoint and returns a
//! `Response<Cursor<Vec<u8>>>` with JSON content.

use std::io::Cursor;

use anyhow::{Context, Result};
use serde::Serialize;
use tiny_http::{Response, StatusCode};

use crate::analytics::reporter;
use crate::config;
use crate::utils::process;

use super::content_type_json;

// ---------------------------------------------------------------------------
// JSON response types
// ---------------------------------------------------------------------------

/// Stats API response — mirrors `reporter::Stats` but is Serialize.
#[derive(Serialize)]
struct StatsResponse {
    total_commands: usize,
    total_original_tokens: usize,
    total_optimized_tokens: usize,
    total_savings_pct: f64,
    path_distribution: PathDistributionResponse,
    command_stats: Vec<CommandStatResponse>,
}

#[derive(Serialize)]
struct PathDistributionResponse {
    fast: usize,
    smart: usize,
    passthrough: usize,
    fast_pct: f64,
    smart_pct: f64,
    passthrough_pct: f64,
}

#[derive(Serialize)]
struct CommandStatResponse {
    command: String,
    count: usize,
    total_original_tokens: usize,
    total_optimized_tokens: usize,
    avg_savings_pct: f64,
    primary_optimizer: String,
}

/// Trend API response.
#[derive(Serialize)]
struct TrendResponse {
    days: u32,
    entries: Vec<TrendEntryResponse>,
}

#[derive(Serialize)]
struct TrendEntryResponse {
    date: String,
    commands: usize,
    tokens_saved: usize,
    avg_savings_pct: f64,
}

/// Discovery API response.
#[derive(Serialize)]
struct DiscoverResponse {
    candidates: Vec<DiscoveryCandidateResponse>,
}

#[derive(Serialize)]
struct DiscoveryCandidateResponse {
    command: String,
    count: usize,
    total_tokens: usize,
    avg_tokens: usize,
    current_path: String,
}

/// Config API response — the full config as a JSON value + the raw TOML.
#[derive(Serialize)]
struct ConfigResponse {
    config: config::schema::TerseConfig,
    toml_text: String,
}

/// Config update request — a list of key-value pairs.
#[derive(serde::Deserialize)]
struct ConfigUpdateRequest {
    updates: Vec<ConfigKeyValue>,
}

#[derive(serde::Deserialize)]
struct ConfigKeyValue {
    key: String,
    value: String,
}

/// Health API response.
#[derive(Serialize)]
struct HealthResponse {
    platform: String,
    shell: String,
    binary_name: String,
    git_available: bool,
    ollama_available: bool,
    config_exists: bool,
    log_exists: bool,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a JSON success response.
fn json_response<T: Serialize>(data: &T) -> Result<Response<Cursor<Vec<u8>>>> {
    let body = serde_json::to_string(data).context("failed to serialize JSON response")?;
    Ok(Response::from_data(body.into_bytes())
        .with_header(content_type_json())
        .with_status_code(StatusCode(200)))
}

/// Parse the `?days=N` query parameter from a URL.
fn parse_days_param(url: &str) -> Option<u32> {
    url.split('?').nth(1)?.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        if k == "days" { v.parse().ok() } else { None }
    })
}

// ---------------------------------------------------------------------------
// API Handlers
// ---------------------------------------------------------------------------

/// `GET /api/stats?days=N` — token savings statistics.
pub fn get_stats(url: &str) -> Result<Response<Cursor<Vec<u8>>>> {
    let days = parse_days_param(url);
    let stats = reporter::compute_stats(days);

    let resp = StatsResponse {
        total_commands: stats.total_commands,
        total_original_tokens: stats.total_original_tokens,
        total_optimized_tokens: stats.total_optimized_tokens,
        total_savings_pct: stats.total_savings_pct,
        path_distribution: PathDistributionResponse {
            fast: stats.path_distribution.fast,
            smart: stats.path_distribution.smart,
            passthrough: stats.path_distribution.passthrough,
            fast_pct: stats.path_distribution.pct(stats.path_distribution.fast),
            smart_pct: stats.path_distribution.pct(stats.path_distribution.smart),
            passthrough_pct: stats
                .path_distribution
                .pct(stats.path_distribution.passthrough),
        },
        command_stats: stats
            .command_stats
            .into_iter()
            .map(|cs| CommandStatResponse {
                command: cs.command,
                count: cs.count,
                total_original_tokens: cs.total_original_tokens,
                total_optimized_tokens: cs.total_optimized_tokens,
                avg_savings_pct: cs.avg_savings_pct,
                primary_optimizer: cs.primary_optimizer,
            })
            .collect(),
    };

    json_response(&resp)
}

/// `GET /api/trends?days=N` — daily trend data.
pub fn get_trends(url: &str) -> Result<Response<Cursor<Vec<u8>>>> {
    let days = parse_days_param(url).unwrap_or(30);
    let trends = reporter::compute_trends(days);

    let resp = TrendResponse {
        days,
        entries: trends
            .into_iter()
            .map(|t| TrendEntryResponse {
                date: t.date,
                commands: t.commands,
                tokens_saved: t.tokens_saved,
                avg_savings_pct: t.avg_savings_pct,
            })
            .collect(),
    };

    json_response(&resp)
}

/// `GET /api/discover?days=N` — unoptimized command candidates.
pub fn get_discover(url: &str) -> Result<Response<Cursor<Vec<u8>>>> {
    let days = parse_days_param(url);
    let candidates = reporter::discover_candidates(days);

    let resp = DiscoverResponse {
        candidates: candidates
            .into_iter()
            .map(|c| DiscoveryCandidateResponse {
                command: c.command,
                count: c.count,
                total_tokens: c.total_tokens,
                avg_tokens: c.avg_tokens,
                current_path: c.current_path,
            })
            .collect(),
    };

    json_response(&resp)
}

/// `GET /api/config` — current effective configuration.
pub fn get_config() -> Result<Response<Cursor<Vec<u8>>>> {
    let cfg = config::load();
    let toml_text = toml::to_string_pretty(&cfg).unwrap_or_default();

    let resp = ConfigResponse {
        config: cfg,
        toml_text,
    };

    json_response(&resp)
}

/// `PUT /api/config` — update configuration keys.
///
/// Expects JSON body: `{ "updates": [{ "key": "general.mode", "value": "fast-only" }] }`
pub fn put_config(body: &str) -> Result<Response<Cursor<Vec<u8>>>> {
    let req: ConfigUpdateRequest =
        serde_json::from_str(body).context("invalid JSON in config update request")?;

    let mut errors: Vec<String> = Vec::new();
    let mut applied: Vec<String> = Vec::new();

    for kv in &req.updates {
        match config::set_config_value(&kv.key, &kv.value) {
            Ok(()) => applied.push(format!("{} = {}", kv.key, kv.value)),
            Err(e) => errors.push(format!("{}: {}", kv.key, e)),
        }
    }

    let result = serde_json::json!({
        "applied": applied,
        "errors": errors,
        "success": errors.is_empty(),
    });

    json_response(&result)
}

/// `POST /api/config/reset` — reset config to defaults.
pub fn post_config_reset() -> Result<Response<Cursor<Vec<u8>>>> {
    config::reset_config().context("failed to reset config")?;

    let result = serde_json::json!({
        "success": true,
        "message": "Configuration reset to defaults",
    });

    json_response(&result)
}

/// `GET /api/health` — system health summary.
pub fn get_health() -> Result<Response<Cursor<Vec<u8>>>> {
    let config_exists = config::global_config_file()
        .map(|p| p.exists())
        .unwrap_or(false);

    let log_exists = crate::analytics::logger::command_log_path()
        .map(|p| p.exists())
        .unwrap_or(false);

    let resp = HealthResponse {
        platform: process::platform_name().to_string(),
        shell: process::default_shell().to_string(),
        binary_name: process::terse_binary_name().to_string(),
        git_available: process::is_command_available("git"),
        ollama_available: process::is_ollama_available(),
        config_exists,
        log_exists,
    };

    json_response(&resp)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_days_param_extracts_value() {
        assert_eq!(parse_days_param("/api/stats?days=7"), Some(7));
        assert_eq!(parse_days_param("/api/stats?days=30"), Some(30));
        assert_eq!(parse_days_param("/api/trends?foo=bar&days=14"), Some(14));
    }

    #[test]
    fn parse_days_param_returns_none_for_missing() {
        assert_eq!(parse_days_param("/api/stats"), None);
        assert_eq!(parse_days_param("/api/stats?foo=bar"), None);
    }

    #[test]
    fn parse_days_param_returns_none_for_invalid() {
        assert_eq!(parse_days_param("/api/stats?days=abc"), None);
        assert_eq!(parse_days_param("/api/stats?days="), None);
    }

    #[test]
    fn stats_response_serializes() {
        let resp = StatsResponse {
            total_commands: 100,
            total_original_tokens: 50000,
            total_optimized_tokens: 15000,
            total_savings_pct: 70.0,
            path_distribution: PathDistributionResponse {
                fast: 60,
                smart: 20,
                passthrough: 20,
                fast_pct: 60.0,
                smart_pct: 20.0,
                passthrough_pct: 20.0,
            },
            command_stats: vec![CommandStatResponse {
                command: "git".to_string(),
                count: 50,
                total_original_tokens: 25000,
                total_optimized_tokens: 5000,
                avg_savings_pct: 80.0,
                primary_optimizer: "git".to_string(),
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"total_commands\":100"));
        assert!(json.contains("\"fast\":60"));
    }

    #[test]
    fn config_update_request_deserializes() {
        let json = r#"{"updates": [{"key": "general.mode", "value": "fast-only"}]}"#;
        let req: ConfigUpdateRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.updates.len(), 1);
        assert_eq!(req.updates[0].key, "general.mode");
        assert_eq!(req.updates[0].value, "fast-only");
    }

    #[test]
    fn health_response_serializes() {
        let resp = HealthResponse {
            platform: "windows".to_string(),
            shell: "pwsh".to_string(),
            binary_name: "terse.exe".to_string(),
            git_available: true,
            ollama_available: false,
            config_exists: true,
            log_exists: false,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"platform\":\"windows\""));
    }
}
