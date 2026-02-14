use anyhow::Result;

use crate::config::schema::DockerOptimizerConfig;
use crate::optimizers::{CommandContext, OptimizedOutput, Optimizer};
use crate::utils::token_counter::estimate_tokens;

// ---------------------------------------------------------------------------
// Subcommand classification
// ---------------------------------------------------------------------------

/// Recognized docker commands that TERSE can optimize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DockerCommand {
    /// docker ps — running containers
    Ps,
    /// docker images — image listing
    Images,
    /// docker logs — container logs
    Logs,
    /// docker compose ps / docker-compose ps
    ComposePs,
    /// docker inspect — container/image details
    Inspect,
    /// docker build / docker compose build
    Build,
    /// docker pull / docker push
    PullPush,
    /// docker network ls / docker volume ls
    ListResource,
}

/// Classify the core command into a [`DockerCommand`].
fn classify(lower: &str) -> Option<DockerCommand> {
    // docker compose / docker-compose commands
    if lower.starts_with("docker compose ps") || lower.starts_with("docker-compose ps") {
        return Some(DockerCommand::ComposePs);
    }
    if lower.starts_with("docker compose build") || lower.starts_with("docker-compose build") {
        return Some(DockerCommand::Build);
    }

    // Regular docker commands
    if lower.starts_with("docker ps") {
        return Some(DockerCommand::Ps);
    }
    if lower.starts_with("docker images") || lower.starts_with("docker image ls") {
        return Some(DockerCommand::Images);
    }
    if lower.starts_with("docker logs") {
        return Some(DockerCommand::Logs);
    }
    if lower.starts_with("docker inspect") {
        return Some(DockerCommand::Inspect);
    }
    if lower.starts_with("docker build") {
        return Some(DockerCommand::Build);
    }
    if lower.starts_with("docker pull") || lower.starts_with("docker push") {
        return Some(DockerCommand::PullPush);
    }
    if lower.starts_with("docker network ls")
        || lower.starts_with("docker network list")
        || lower.starts_with("docker volume ls")
        || lower.starts_with("docker volume list")
    {
        return Some(DockerCommand::ListResource);
    }

    None
}

// ---------------------------------------------------------------------------
// Flag helpers
// ---------------------------------------------------------------------------

/// Check if the command already has a format flag.
fn has_format_flag(lower: &str) -> bool {
    lower
        .split_whitespace()
        .any(|w| w == "--format" || w.starts_with("--format=") || w == "-f")
}

// ---------------------------------------------------------------------------
// Optimizer
// ---------------------------------------------------------------------------

pub struct DockerOptimizer {
    ps_max_rows: usize,
    images_max_rows: usize,
    logs_max_tail: usize,
    logs_max_errors: usize,
    inspect_max_lines: usize,
    compose_max_rows: usize,
    resource_max_rows: usize,
}

impl Default for DockerOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

impl DockerOptimizer {
    pub fn new() -> Self {
        Self::from_config(&DockerOptimizerConfig::default())
    }

    /// Create a `DockerOptimizer` from the configuration.
    pub fn from_config(cfg: &DockerOptimizerConfig) -> Self {
        Self {
            ps_max_rows: cfg.ps_max_rows,
            images_max_rows: cfg.images_max_rows,
            logs_max_tail: cfg.logs_max_tail,
            logs_max_errors: cfg.logs_max_errors,
            inspect_max_lines: cfg.inspect_max_lines,
            compose_max_rows: cfg.compose_max_rows,
            resource_max_rows: cfg.resource_max_rows,
        }
    }
}

impl Optimizer for DockerOptimizer {
    fn name(&self) -> &'static str {
        "docker"
    }

    fn can_handle(&self, ctx: &CommandContext) -> bool {
        let lower = ctx.core.to_ascii_lowercase();
        let Some(cmd) = classify(&lower) else {
            return false;
        };

        match cmd {
            // Skip if user already has a custom format
            DockerCommand::Ps | DockerCommand::Images => !has_format_flag(&lower),
            _ => true,
        }
    }

    fn optimize_output(&self, ctx: &CommandContext, raw_output: &str) -> Result<OptimizedOutput> {
        let lower = ctx.core.to_ascii_lowercase();
        let cmd = classify(&lower).unwrap_or(DockerCommand::Ps);

        let optimized = match cmd {
            DockerCommand::Ps => compact_docker_ps(raw_output, self.ps_max_rows),
            DockerCommand::Images => compact_docker_images(raw_output, self.images_max_rows),
            DockerCommand::Logs => {
                compact_docker_logs(raw_output, self.logs_max_tail, self.logs_max_errors)
            }
            DockerCommand::ComposePs => compact_compose_ps(raw_output, self.compose_max_rows),
            DockerCommand::Inspect => compact_docker_inspect(raw_output, self.inspect_max_lines),
            DockerCommand::Build => compact_docker_build(raw_output),
            DockerCommand::PullPush => compact_docker_pull_push(raw_output),
            DockerCommand::ListResource => {
                compact_docker_resource_list(raw_output, self.resource_max_rows)
            }
        };

        Ok(OptimizedOutput {
            optimized_tokens: estimate_tokens(&optimized),
            output: optimized,
            optimizer_used: self.name().to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// docker ps — compact container table
// ---------------------------------------------------------------------------

/// Compact `docker ps` output: name, image, status, ports only.
///
/// Docker ps output is a wide table; we extract only the essential columns.
fn compact_docker_ps(raw_output: &str, max_rows: usize) -> String {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return "No containers running".to_string();
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    if lines.len() <= 1 {
        // Only header or empty
        if lines
            .first()
            .is_some_and(|l| l.to_ascii_lowercase().contains("container"))
        {
            return "No containers running".to_string();
        }
        return trimmed.to_string();
    }

    // Parse the header to find column positions
    let header = lines[0];
    let name_col = find_column_start(header, "NAMES");
    let image_col = find_column_start(header, "IMAGE");
    let status_col = find_column_start(header, "STATUS");
    let ports_col = find_column_start(header, "PORTS");

    // If we can't find columns, fall back to passthrough
    if name_col.is_none() && image_col.is_none() {
        return trim_table_output(trimmed, max_rows);
    }

    let mut result = Vec::new();
    result.push("NAME | IMAGE | STATUS | PORTS".to_string());

    for line in lines.iter().skip(1) {
        if line.trim().is_empty() {
            continue;
        }

        let name = extract_column(line, name_col, None).unwrap_or("-");
        let image = extract_column(line, image_col, status_col).unwrap_or("-");
        let status = extract_column(line, status_col, ports_col).unwrap_or("-");
        let ports = extract_column(line, ports_col, name_col).unwrap_or("-");

        // Truncate long image names
        let image_short = truncate_str(image.trim(), 40);
        let ports_short = truncate_str(ports.trim(), 30);

        result.push(format!(
            "{} | {} | {} | {}",
            name.trim(),
            image_short,
            status.trim(),
            ports_short
        ));
    }

    result.join("\n")
}

// ---------------------------------------------------------------------------
// docker images — compact image listing
// ---------------------------------------------------------------------------

/// Compact `docker images` output: repo, tag, size.
fn compact_docker_images(raw_output: &str, max_images: usize) -> String {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return "No images".to_string();
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    if lines.len() <= 1 {
        return "No images".to_string();
    }

    let header = lines[0];
    let repo_col = find_column_start(header, "REPOSITORY");
    let tag_col = find_column_start(header, "TAG");
    let size_col = find_column_start(header, "SIZE");

    if repo_col.is_none() {
        return trim_table_output(trimmed, max_images);
    }

    let mut result = Vec::new();
    result.push("REPOSITORY:TAG | SIZE".to_string());

    let mut count = 0usize;
    for line in lines.iter().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        count += 1;
        if count > max_images {
            continue;
        }

        let repo = extract_column(line, repo_col, tag_col).unwrap_or("-");
        let tag = extract_column(line, tag_col, size_col).unwrap_or("-");
        let size = if let Some(col) = size_col {
            line.get(col..).unwrap_or("-").trim()
        } else {
            "-"
        };

        // Truncate <none> tags for readability
        let tag_display = if tag.trim() == "<none>" {
            "<none>"
        } else {
            tag.trim()
        };

        result.push(format!("{}:{} | {}", repo.trim(), tag_display, size));
    }

    if count > max_images {
        result.push(format!("...+{} more ({} total)", count - max_images, count));
    }

    result.join("\n")
}

// ---------------------------------------------------------------------------
// docker logs — tail + error extraction
// ---------------------------------------------------------------------------

/// Compact `docker logs` output: keep errors/warnings + last N lines.
fn compact_docker_logs(raw_output: &str, max_tail: usize, max_errors: usize) -> String {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return "No logs".to_string();
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    let total = lines.len();

    if total <= max_tail + max_errors {
        return trimmed.to_string();
    }

    // Extract error/warning lines
    let mut error_lines: Vec<&str> = Vec::new();
    for line in &lines {
        let l = line.to_ascii_lowercase();
        if (l.contains("error")
            || l.contains("fatal")
            || l.contains("panic")
            || l.contains("exception")
            || l.contains("traceback"))
            && error_lines.len() < max_errors
        {
            error_lines.push(line);
        }
    }

    let mut result = Vec::new();

    // Show errors first
    if !error_lines.is_empty() {
        result.push(format!("ERRORS/WARNINGS ({}):", error_lines.len()));
        for line in &error_lines {
            result.push(line.to_string());
        }
        result.push(String::new());
    }

    // Show last N lines
    result.push(format!("TAIL ({} of {} lines):", max_tail, total));
    for line in lines.iter().skip(total - max_tail) {
        result.push(line.to_string());
    }

    result.join("\n")
}

// ---------------------------------------------------------------------------
// docker compose ps
// ---------------------------------------------------------------------------

/// Compact `docker compose ps` output.
fn compact_compose_ps(raw_output: &str, max_rows: usize) -> String {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return "No compose services running".to_string();
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    if lines.len() <= 1 {
        return "No compose services running".to_string();
    }

    // Compose ps usually has NAME, COMMAND, SERVICE, STATUS, PORTS columns
    // Just limit output length
    trim_table_output(trimmed, max_rows)
}

// ---------------------------------------------------------------------------
// docker inspect — compact JSON
// ---------------------------------------------------------------------------

/// Compact `docker inspect` output: extract key fields from JSON.
fn compact_docker_inspect(raw_output: &str, max_lines: usize) -> String {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return "No inspect output".to_string();
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    let total = lines.len();

    if total <= max_lines {
        return trimmed.to_string();
    }

    // For JSON inspect output, keep the beginning (top-level fields)
    // and truncate deep nested structures
    let mut result: Vec<&str> = lines[..max_lines].to_vec();
    result.push("");
    let summary = format!("...({} lines omitted, {} total)", total - max_lines, total);
    let mut output = result.join("\n");
    output.push_str(&summary);
    output
}

// ---------------------------------------------------------------------------
// docker build — success/fail summary
// ---------------------------------------------------------------------------

/// Compact `docker build` output: show final result + errors.
fn compact_docker_build(raw_output: &str) -> String {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return "Build completed (no output)".to_string();
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    let mut errors: Vec<&str> = Vec::new();
    let mut result_lines: Vec<&str> = Vec::new();
    let mut step_count = 0usize;

    for line in &lines {
        let l = line.trim();
        let lower = l.to_ascii_lowercase();

        // Count build steps
        if lower.starts_with("step ") || lower.starts_with("#") && lower.contains("[") {
            step_count += 1;
            continue;
        }

        // Capture errors
        if lower.contains("error") || lower.contains("failed") {
            errors.push(l);
            continue;
        }

        // Capture result lines (successfully built, tagged, etc.)
        if lower.starts_with("successfully")
            || lower.starts_with("writing image")
            || lower.starts_with("naming to")
            || lower.contains("built")
        {
            result_lines.push(l);
        }
    }

    let mut result = Vec::new();

    if step_count > 0 {
        result.push(format!("[{step_count} build steps]"));
    }

    if !errors.is_empty() {
        result.push("ERRORS:".to_string());
        for line in errors.iter().take(20) {
            result.push(line.to_string());
        }
        if errors.len() > 20 {
            result.push(format!("...+{} more error lines", errors.len() - 20));
        }
    }

    for line in &result_lines {
        result.push(line.to_string());
    }

    if result.is_empty() {
        return trim_table_output(trimmed, 30);
    }

    result.join("\n")
}

// ---------------------------------------------------------------------------
// docker pull / push — summary
// ---------------------------------------------------------------------------

/// Compact `docker pull`/`docker push` output.
fn compact_docker_pull_push(raw_output: &str) -> String {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return "completed".to_string();
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    let mut result: Vec<&str> = Vec::new();

    for line in &lines {
        let l = line.trim();
        let lower = l.to_ascii_lowercase();

        // Skip progress/layer lines
        if lower.contains(": pulling")
            || lower.contains(": waiting")
            || lower.contains(": downloading")
            || lower.contains(": extracting")
            || lower.contains(": verifying")
            || lower.contains(": already exists")
            || lower.contains(": pull complete")
            || lower.contains(": pushed")
            || lower.contains(": preparing")
            || lower.contains(": layer already exists")
            || lower.contains(": mounted from")
        {
            continue;
        }

        // Keep digest, status, and error lines
        result.push(l);
    }

    if result.is_empty() {
        // All lines were progress — extract just the last status line
        if let Some(last) = lines.last() {
            return last.trim().to_string();
        }
        return "completed".to_string();
    }

    result.join("\n")
}

// ---------------------------------------------------------------------------
// docker network ls / volume ls — compact table
// ---------------------------------------------------------------------------

/// Compact `docker network ls` / `docker volume ls` output.
fn compact_docker_resource_list(raw_output: &str, max_rows: usize) -> String {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return "No resources".to_string();
    }

    trim_table_output(trimmed, max_rows)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Find the start position of a column by its header name.
fn find_column_start(header: &str, name: &str) -> Option<usize> {
    header.to_ascii_uppercase().find(name)
}

/// Extract a column value from a line given start and optional end positions.
fn extract_column(line: &str, start: Option<usize>, end: Option<usize>) -> Option<&str> {
    let s = start?;
    if s >= line.len() {
        return None;
    }
    let e = end.unwrap_or(line.len()).min(line.len());
    if e <= s {
        return Some(line.get(s..)?.trim());
    }
    Some(line.get(s..e)?.trim())
}

/// Truncate a string to max length with "..." suffix.
fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        // Find a safe char boundary
        let mut end = max.min(s.len());
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

/// Trim table output to max rows.
fn trim_table_output(text: &str, max_rows: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len();

    if total <= max_rows {
        return text.to_string();
    }

    let mut result: Vec<&str> = lines[..max_rows].to_vec();
    result.push("");
    let summary = format!("...+{} more rows ({} total)", total - max_rows, total);
    let mut output = result.join("\n");
    output.push_str(&summary);
    output
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizers::CommandContext;

    // classify -----------------------------------------------------------

    #[test]
    fn classifies_docker_commands() {
        assert_eq!(classify("docker ps"), Some(DockerCommand::Ps));
        assert_eq!(classify("docker ps -a"), Some(DockerCommand::Ps));
        assert_eq!(classify("docker images"), Some(DockerCommand::Images));
        assert_eq!(classify("docker image ls"), Some(DockerCommand::Images));
        assert_eq!(classify("docker logs myapp"), Some(DockerCommand::Logs));
        assert_eq!(
            classify("docker compose ps"),
            Some(DockerCommand::ComposePs)
        );
        assert_eq!(
            classify("docker-compose ps"),
            Some(DockerCommand::ComposePs)
        );
        assert_eq!(
            classify("docker inspect mycontainer"),
            Some(DockerCommand::Inspect)
        );
        assert_eq!(classify("docker build ."), Some(DockerCommand::Build));
        assert_eq!(classify("docker pull nginx"), Some(DockerCommand::PullPush));
        assert_eq!(
            classify("docker push myapp:latest"),
            Some(DockerCommand::PullPush)
        );
        assert_eq!(
            classify("docker network ls"),
            Some(DockerCommand::ListResource)
        );
        assert_eq!(
            classify("docker volume ls"),
            Some(DockerCommand::ListResource)
        );
        assert_eq!(classify("git status"), None);
        assert_eq!(classify("ls -la"), None);
    }

    // can_handle ---------------------------------------------------------

    #[test]
    fn handles_docker_commands() {
        let opt = DockerOptimizer::new();
        assert!(opt.can_handle(&CommandContext::new("docker ps")));
        assert!(opt.can_handle(&CommandContext::new("docker ps -a")));
        assert!(opt.can_handle(&CommandContext::new("cd /app && docker images")));
        assert!(opt.can_handle(&CommandContext::new("docker logs myapp")));
        // Skip if user has custom format
        assert!(!opt.can_handle(&CommandContext::new("docker ps --format '{{.Names}}'")));
    }

    // compact_docker_ps --------------------------------------------------

    #[test]
    fn docker_ps_empty() {
        assert_eq!(compact_docker_ps("", 30), "No containers running");
    }

    #[test]
    fn docker_ps_header_only() {
        let input = "CONTAINER ID   IMAGE   COMMAND   CREATED   STATUS   PORTS   NAMES";
        assert_eq!(compact_docker_ps(input, 30), "No containers running");
    }

    #[test]
    fn docker_ps_with_containers() {
        let input = "\
CONTAINER ID   IMAGE          COMMAND                  CREATED       STATUS       PORTS                    NAMES
abc123def456   nginx:latest   \"nginx -g 'daemon off'\"  2 hours ago   Up 2 hours   0.0.0.0:80->80/tcp       web
def789abc012   redis:7        \"docker-entrypoint.s…\"   3 hours ago   Up 3 hours   0.0.0.0:6379->6379/tcp   cache";

        let result = compact_docker_ps(input, 30);
        assert!(result.contains("NAME | IMAGE | STATUS | PORTS"));
        assert!(result.contains("web"));
        assert!(result.contains("cache"));
    }

    // compact_docker_images ----------------------------------------------

    #[test]
    fn docker_images_empty() {
        assert_eq!(compact_docker_images("", 30), "No images");
    }

    // compact_docker_logs ------------------------------------------------

    #[test]
    fn docker_logs_empty() {
        assert_eq!(compact_docker_logs("", 30, 20), "No logs");
    }

    #[test]
    fn docker_logs_short_passthrough() {
        let input = "Server started on port 3000\nReady to accept connections";
        assert_eq!(compact_docker_logs(input, 30, 20), input);
    }

    #[test]
    fn docker_logs_long_with_errors() {
        let mut lines: Vec<String> = (0..100)
            .map(|i| format!("INFO: Request {} processed", i))
            .collect();
        lines.insert(50, "ERROR: Connection refused to database".to_string());
        let input = lines.join("\n");
        let result = compact_docker_logs(&input, 30, 20);
        assert!(result.contains("ERRORS/WARNINGS"));
        assert!(result.contains("Connection refused"));
        assert!(result.contains("TAIL"));
    }

    // compact_docker_pull_push -------------------------------------------

    #[test]
    fn docker_pull_strips_progress() {
        let input = "\
Using default tag: latest
latest: Pulling from library/nginx
a2abf6c4d29d: Pulling fs layer
a2abf6c4d29d: Downloading
a2abf6c4d29d: Pull complete
Digest: sha256:abc123def456
Status: Downloaded newer image for nginx:latest
docker.io/library/nginx:latest";

        let result = compact_docker_pull_push(input);
        assert!(!result.contains("Pulling fs layer"));
        assert!(!result.contains("Pull complete"));
        assert!(result.contains("Digest"));
        assert!(result.contains("Status"));
    }

    // helpers ------------------------------------------------------------

    #[test]
    fn truncate_str_handles_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn truncate_str_truncates() {
        assert_eq!(truncate_str("hello world", 5), "hello");
    }
}
