use anyhow::Result;

use crate::config::schema::FileOptimizerConfig;
use crate::optimizers::{CommandContext, OptimizedOutput, Optimizer};
use crate::utils::token_counter::estimate_tokens;

// ---------------------------------------------------------------------------
// Subcommand classification
// ---------------------------------------------------------------------------

/// Recognized file commands that TERSE can optimize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileCommand {
    /// ls / dir — directory listing
    Ls,
    /// find — file search
    Find,
    /// cat / head / tail — file content display
    CatHeadTail,
    /// wc — word/line/byte count
    Wc,
    /// tree — directory tree listing
    Tree,
}

/// Classify the core command into a [`FileCommand`].
fn classify(lower: &str) -> Option<FileCommand> {
    let first = lower.split_whitespace().next()?;
    match first {
        "ls" | "dir" | "gci" | "get-childitem" => Some(FileCommand::Ls),
        "find" => Some(FileCommand::Find),
        "cat" | "head" | "tail" | "type" | "get-content" | "gc" => Some(FileCommand::CatHeadTail),
        "wc" => Some(FileCommand::Wc),
        "tree" => Some(FileCommand::Tree),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Flag helpers
// ---------------------------------------------------------------------------

/// Check if the command already contains flags that produce compact output.
fn has_compact_flag(lower: &str, cmd: FileCommand) -> bool {
    match cmd {
        FileCommand::Ls => {
            // Skip if user already has a custom format or single-column flag
            has_any_flag(lower, &["-1", "--format", "-C", "-m", "-x"])
        }
        _ => false,
    }
}

/// Check if any whitespace-delimited token matches a flag exactly.
fn has_any_flag(text: &str, flags: &[&str]) -> bool {
    text.split_whitespace()
        .any(|word| flags.contains(&word))
}

// ---------------------------------------------------------------------------
// Optimizer
// ---------------------------------------------------------------------------

pub struct FileOptimizer {
    ls_max_entries: usize,
    ls_max_items: usize,
    find_max_results: usize,
    cat_max_lines: usize,
    cat_head_lines: usize,
    cat_tail_lines: usize,
    wc_max_lines: usize,
    tree_max_lines: usize,
    tree_noise_dirs: Vec<String>,
}

impl Default for FileOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

impl FileOptimizer {
    pub fn new() -> Self {
        Self::from_config(&FileOptimizerConfig::default())
    }

    /// Create a `FileOptimizer` from the configuration.
    pub fn from_config(cfg: &FileOptimizerConfig) -> Self {
        Self {
            ls_max_entries: cfg.ls_max_entries,
            ls_max_items: cfg.ls_max_items,
            find_max_results: cfg.find_max_results,
            cat_max_lines: cfg.cat_max_lines,
            cat_head_lines: cfg.cat_head_lines,
            cat_tail_lines: cfg.cat_tail_lines,
            wc_max_lines: cfg.wc_max_lines,
            tree_max_lines: cfg.tree_max_lines,
            tree_noise_dirs: cfg.tree_noise_dirs.clone(),
        }
    }
}

impl Optimizer for FileOptimizer {
    fn name(&self) -> &'static str {
        "file"
    }

    fn can_handle(&self, ctx: &CommandContext) -> bool {
        let lower = ctx.core.to_ascii_lowercase();
        let Some(cmd) = classify(&lower) else {
            return false;
        };

        match cmd {
            FileCommand::Ls => !has_compact_flag(&lower, cmd),
            _ => true,
        }
    }

    fn optimize_output(&self, _ctx: &CommandContext, raw_output: &str) -> Result<OptimizedOutput> {
        let lower = _ctx.core.to_ascii_lowercase();
        let cmd = classify(&lower).unwrap_or(FileCommand::Ls);

        let optimized = match cmd {
            FileCommand::Ls => compact_ls(raw_output, self.ls_max_entries, self.ls_max_items),
            FileCommand::Find => compact_find(raw_output, self.find_max_results),
            FileCommand::CatHeadTail => compact_cat(raw_output, self.cat_max_lines, self.cat_head_lines, self.cat_tail_lines),
            FileCommand::Wc => compact_wc(raw_output, self.wc_max_lines),
            FileCommand::Tree => compact_tree(raw_output, self.tree_max_lines, &self.tree_noise_dirs),
        };

        Ok(OptimizedOutput {
            optimized_tokens: estimate_tokens(&optimized),
            output: optimized,
            optimizer_used: self.name().to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// ls / dir — compact directory listing
// ---------------------------------------------------------------------------

/// Compact `ls` output: limit items, strip verbose metadata.
///
/// Handles three formats:
/// - **Windows PowerShell** (`Get-ChildItem`): `Mode LastWriteTime Length Name`
///   columns → extracts just names with a type marker (`[D]`/`[F]`).
/// - **Unix long-format** (`ls -l`): permission strings → strips `total` line,
///   limits entries.
/// - **Simple listing**: just file names → limits item count.
fn compact_ls(raw_output: &str, max_entries: usize, max_items: usize) -> String {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return "(empty directory)".to_string();
    }

    let lines: Vec<&str> = trimmed.lines().collect();

    // Detect Windows PowerShell Get-ChildItem format:
    // "Mode  LastWriteTime  Length Name" header followed by a "----" separator
    if is_powershell_format(&lines) {
        return compact_ls_powershell(&lines, max_entries);
    }

    // Detect Unix long-format output (starts with permissions like drwxr-xr-x or -)
    let is_long_format = lines.iter().any(|line| {
        let l = line.trim();
        l.starts_with("total ")
            || l.starts_with('d')
                && l.len() > 10
                && l.as_bytes().get(1).is_some_and(|&b| b == b'r' || b == b'-')
            || l.starts_with('-')
                && l.len() > 10
                && l.as_bytes().get(1).is_some_and(|&b| b == b'r' || b == b'-')
            || l.starts_with('l')
                && l.len() > 10
                && l.as_bytes().get(1).is_some_and(|&b| b == b'r' || b == b'-')
    });

    if is_long_format {
        compact_ls_long(&lines, max_entries)
    } else {
        compact_ls_simple(&lines, max_items)
    }
}

/// Detect PowerShell `Get-ChildItem` tabular format.
///
/// Looks for a header line containing "Mode" and "Name" and a separator line
/// consisting mostly of dashes.
fn is_powershell_format(lines: &[&str]) -> bool {
    // Need at least a header, separator, and one data line
    if lines.len() < 3 {
        return false;
    }

    let has_header = lines.iter().take(5).any(|l| {
        let t = l.trim();
        t.contains("Mode") && t.contains("Name")
    });

    let has_separator = lines.iter().take(6).any(|l| {
        let t = l.trim();
        !t.is_empty() && t.chars().all(|c| c == '-' || c.is_whitespace())
    });

    has_header && has_separator
}

/// Compact PowerShell `Get-ChildItem` output into a clean name list.
///
/// Extracts just the entry name with a type prefix:
/// - `[D] dirname` for directories (mode starts with `d`)
/// - `    filename  (1.2 KB)` for files with human-readable size
///
/// Strips the header, separator, "Directory:" line, and all date/time
/// metadata that is noise for an AI assistant.
fn compact_ls_powershell(lines: &[&str], max_entries: usize) -> String {
    let mut entries: Vec<String> = Vec::new();
    let mut dir_count = 0usize;
    let mut file_count = 0usize;

    // Find the name column position from the header line
    let name_col = lines
        .iter()
        .take(5)
        .find_map(|l| l.find("Name"))
        .unwrap_or(0);

    for line in lines {
        let trimmed = line.trim();

        // Skip metadata lines
        if trimmed.is_empty()
            || trimmed.starts_with("Directory:")
            || trimmed.starts_with("Mode")
            || trimmed.chars().all(|c| c == '-' || c.is_whitespace())
        {
            continue;
        }

        // Parse mode (first token) and name (at column position)
        let mode = trimmed.split_whitespace().next().unwrap_or("");
        let is_dir = mode.starts_with('d');

        // Extract name from the column position (handles names with spaces)
        let name = if name_col > 0 && line.len() > name_col {
            line[name_col..].trim()
        } else {
            // Fallback: last whitespace-delimited token
            trimmed.split_whitespace().last().unwrap_or(trimmed)
        };

        if name.is_empty() || name == "Name" {
            continue;
        }

        if is_dir {
            dir_count += 1;
            entries.push(format!("[D] {name}"));
        } else {
            file_count += 1;
            // Try to extract file size from the Length column
            let size_str = extract_ps_file_size(trimmed);
            if let Some(size) = size_str {
                entries.push(format!("    {name}  ({size})"));
            } else {
                entries.push(format!("    {name}"));
            }
        }
    }

    if entries.is_empty() {
        return "(empty directory)".to_string();
    }

    let total = entries.len();
    let mut result = Vec::with_capacity(max_entries + 2);

    // Summary header
    result.push(format!("{dir_count} directories, {file_count} files"));

    for entry in entries.iter().take(max_entries) {
        result.push(entry.clone());
    }

    if total > max_entries {
        result.push(format!("...+{} more ({total} total)", total - max_entries));
    }

    result.join("\n")
}

/// Extract a human-readable file size from a PowerShell entry line.
///
/// Looks for the numeric `Length` field and converts to human units.
fn extract_ps_file_size(line: &str) -> Option<String> {
    // PowerShell lines look like:
    // -a---   2/12/2026  5:11 PM   21497 Achieve.sln
    // The Length column is the token before the filename — find the last
    // purely numeric token.
    let tokens: Vec<&str> = line.split_whitespace().collect();
    for token in tokens.iter().rev().skip(1) {
        if let Ok(bytes) = token.parse::<u64>() {
            return Some(human_size(bytes));
        }
    }
    None
}

/// Format a byte count as a human-readable size string.
fn human_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Compact long-format listing: strip "total" line, limit to N entries.
fn compact_ls_long(lines: &[&str], max_entries: usize) -> String {
    let mut result = Vec::new();
    let mut count = 0usize;

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("total ") {
            continue;
        }
        count += 1;
        if count <= max_entries {
            result.push(trimmed);
        }
    }

    let mut output = result.join("\n");
    if count > max_entries {
        output.push_str(&format!("\n...+{} more entries ({} total)", count - max_entries, count));
    }
    output
}

/// Compact simple listing: show items, limit count.
fn compact_ls_simple(lines: &[&str], max_items: usize) -> String {
    let items: Vec<&str> = lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    if items.is_empty() {
        return "(empty directory)".to_string();
    }

    let total = items.len();
    if total <= max_items {
        return items.join("\n");
    }

    let mut output: Vec<&str> = items[..max_items].to_vec();
    output.push("");
    let summary = format!("...+{} more ({} total)", total - max_items, total);
    let mut result = output.join("\n");
    result.push_str(&summary);
    result
}

// ---------------------------------------------------------------------------
// find — compact file search results
// ---------------------------------------------------------------------------

/// Compact `find` output: limit results, group by directory.
fn compact_find(raw_output: &str, max_results: usize) -> String {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return "No files found".to_string();
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    let total = lines.len();

    if total <= max_results {
        return trimmed.to_string();
    }

    let mut result: Vec<&str> = lines[..max_results].to_vec();
    result.push("");
    let summary = format!("...+{} more ({} total)", total - max_results, total);
    let mut output = result.join("\n");
    output.push_str(&summary);
    output
}

// ---------------------------------------------------------------------------
// cat / head / tail — compact file content
// ---------------------------------------------------------------------------

/// Compact `cat`/`head`/`tail` output: truncate long files with context.
fn compact_cat(raw_output: &str, max_lines: usize, head_count: usize, tail_count: usize) -> String {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return "(empty file)".to_string();
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    let total = lines.len();

    if total <= max_lines {
        return trimmed.to_string();
    }

    let mut result = Vec::with_capacity(head_count + tail_count + 1);
    for line in lines.iter().take(head_count) {
        result.push(*line);
    }
    result.push("");
    let gap_msg = format!(
        "... ({} lines omitted, {} total) ...",
        total - head_count - tail_count,
        total
    );
    // We need to handle the gap message lifetime carefully
    let mut output = result.join("\n");
    output.push_str(&gap_msg);
    output.push('\n');
    for line in lines.iter().skip(total - tail_count) {
        output.push_str(line);
        output.push('\n');
    }

    output.trim_end().to_string()
}

// ---------------------------------------------------------------------------
// wc — single-line result
// ---------------------------------------------------------------------------

/// Compact `wc` output: already compact, just trim whitespace.
fn compact_wc(raw_output: &str, max_lines: usize) -> String {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return "0".to_string();
    }

    // wc output is typically already compact (line count word count byte count filename)
    // Just normalize whitespace and limit lines
    let lines: Vec<&str> = trimmed.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();

    if lines.len() <= max_lines {
        return lines.join("\n");
    }

    // Keep first N and the "total" line (usually last)
    let mut result: Vec<&str> = lines[..max_lines - 1].to_vec();
    if let Some(last) = lines.last() {
        result.push(last);
    }
    result.push("");
    let summary = format!("...{} files total", lines.len());
    let mut output = result.join("\n");
    output.push_str(&summary);
    output
}

// ---------------------------------------------------------------------------
// tree — compact directory tree
// ---------------------------------------------------------------------------

/// Compact `tree` output: prune noise subtrees, then limit entries.
///
/// Two-phase approach:
/// 1. **Subtree pruning** — when a tree line's name matches a known noise
///    directory (`.idea`, `node_modules`, `bin`, `obj`, …), collapse the
///    entire subtree into a single `[pruned]` marker.
/// 2. **Line truncation** — if the pruned output still exceeds `max_lines`,
///    keep the top lines plus the summary line.
fn compact_tree(raw_output: &str, max_lines: usize, noise_dirs: &[String]) -> String {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return "(empty)".to_string();
    }

    let lines: Vec<&str> = trimmed.lines().collect();

    // Phase 1: Prune noise subtrees
    let pruned = prune_tree_noise(&lines, noise_dirs);

    let total_original = lines.len();
    let total_pruned = pruned.len();

    if total_pruned <= max_lines {
        return pruned.join("\n");
    }

    // Phase 2: Truncate
    let mut result: Vec<&str> = pruned[..max_lines - 1].iter().map(|s| s.as_str()).collect();

    // Preserve the summary line ("N directories, M files") if present
    if let Some(last) = pruned.last()
        && (last.contains("director") || last.contains("file"))
    {
        result.push("");
        result.push(last);
        let pruned_note = if total_pruned < total_original {
            format!(
                " ({} noise lines pruned)",
                total_original - total_pruned
            )
        } else {
            String::new()
        };
        let summary = format!(
            "...({} lines omitted){pruned_note}",
            total_pruned - max_lines
        );
        let mut output = result.join("\n");
        output.push_str(&format!("\n{summary}"));
        return output;
    }

    result.push("");
    let summary = format!(
        "...+{} more lines ({} total)",
        total_pruned - max_lines + 1,
        total_original
    );
    let mut output = result.join("\n");
    output.push_str(&summary);
    output
}

/// Extract the indentation depth of a tree-drawing line.
///
/// Counts the number of tree-drawing characters and spaces before the entry
/// name. Returns a depth measure that can be compared: if line B has a
/// *greater* depth than line A, B is a child of A (or a sibling's child).
fn tree_indent_depth(line: &str) -> usize {
    let mut depth = 0;
    for ch in line.chars() {
        match ch {
            ' ' | '│' | '├' | '└' | '─' | '|' | '+' | '`' | '-' | '\t' => depth += 1,
            _ => break,
        }
    }
    depth
}

/// Extract the entry name from a tree-drawing line.
///
/// Strips tree-drawing prefixes (`├── `, `│   `, `└── `, `+---`) and
/// returns the remaining text (the directory or file name).
fn tree_entry_name(line: &str) -> &str {
    let s = line.trim_start_matches(|c: char| {
        matches!(c, ' ' | '│' | '├' | '└' | '─' | '|' | '+' | '`' | '-' | '\t')
    });
    s.trim()
}

/// Prune noise subtrees from parsed tree lines.
///
/// When a line's entry name matches a noise directory, the line is replaced
/// with `<prefix><name>/ [contents hidden]` and all subsequent lines at a
/// deeper indentation level are dropped.
fn prune_tree_noise(lines: &[&str], noise_dirs: &[String]) -> Vec<String> {
    let mut result: Vec<String> = Vec::with_capacity(lines.len());
    let mut skip_depth: Option<usize> = None;

    for &line in lines {
        let depth = tree_indent_depth(line);

        // If we're inside a pruned subtree, skip until we reach the same or
        // shallower depth.
        if let Some(prune_at) = skip_depth {
            if depth > prune_at {
                continue; // still inside pruned subtree
            }
            // We've exited the subtree
            skip_depth = None;
        }

        let name = tree_entry_name(line);

        // Check if this entry matches a noise directory
        let is_noise = noise_dirs.iter().any(|noise_dir| {
            name.eq_ignore_ascii_case(noise_dir)
                // Handle trailing slashes or decorations: "node_modules/"
                || name.strip_suffix('/').is_some_and(|n| n.eq_ignore_ascii_case(noise_dir))
        });

        if is_noise {
            // Replace with a summary marker, keeping the tree-drawing prefix
            let prefix_end = line.len() - line.trim_start_matches(|c: char| {
                matches!(c, ' ' | '│' | '├' | '└' | '─' | '|' | '+' | '`' | '-' | '\t')
            }).len();
            let prefix = &line[..prefix_end];
            result.push(format!("{prefix}{name}/ [contents hidden]"));
            skip_depth = Some(depth);
        } else {
            result.push(line.to_string());
        }
    }

    result
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
    fn classifies_file_commands() {
        assert_eq!(classify("ls -la"), Some(FileCommand::Ls));
        assert_eq!(classify("ls"), Some(FileCommand::Ls));
        assert_eq!(classify("dir"), Some(FileCommand::Ls));
        assert_eq!(classify("find . -name '*.rs'"), Some(FileCommand::Find));
        assert_eq!(classify("cat README.md"), Some(FileCommand::CatHeadTail));
        assert_eq!(classify("head -n 20 file.txt"), Some(FileCommand::CatHeadTail));
        assert_eq!(classify("tail -f log.txt"), Some(FileCommand::CatHeadTail));
        assert_eq!(classify("wc -l file.txt"), Some(FileCommand::Wc));
        assert_eq!(classify("tree"), Some(FileCommand::Tree));
        assert_eq!(classify("tree src/"), Some(FileCommand::Tree));
        assert_eq!(classify("git status"), None);
        assert_eq!(classify("cargo build"), None);
    }

    // can_handle ---------------------------------------------------------

    #[test]
    fn handles_ls_commands() {
        let opt = FileOptimizer::new();
        assert!(opt.can_handle(&CommandContext::new("ls -la")));
        assert!(opt.can_handle(&CommandContext::new("ls")));
        assert!(opt.can_handle(&CommandContext::new("cd /repo && ls -la")));
        // Skip if already compact
        assert!(!opt.can_handle(&CommandContext::new("ls -1")));
    }

    #[test]
    fn handles_find_commands() {
        let opt = FileOptimizer::new();
        assert!(opt.can_handle(&CommandContext::new("find . -name '*.rs'")));
        assert!(opt.can_handle(&CommandContext::new("cd /repo && find . -type f")));
    }

    #[test]
    fn handles_cat_commands() {
        let opt = FileOptimizer::new();
        assert!(opt.can_handle(&CommandContext::new("cat README.md")));
        assert!(opt.can_handle(&CommandContext::new("head -n 20 file.txt")));
        assert!(opt.can_handle(&CommandContext::new("tail -100 app.log")));
    }

    #[test]
    fn handles_wc_commands() {
        let opt = FileOptimizer::new();
        assert!(opt.can_handle(&CommandContext::new("wc -l *.txt")));
    }

    #[test]
    fn handles_tree_commands() {
        let opt = FileOptimizer::new();
        assert!(opt.can_handle(&CommandContext::new("tree")));
        assert!(opt.can_handle(&CommandContext::new("tree src/")));
    }

    // compact_ls ---------------------------------------------------------

    #[test]
    fn compact_ls_empty() {
        assert_eq!(compact_ls("", 50, 60), "(empty directory)");
        assert_eq!(compact_ls("  \n  ", 50, 60), "(empty directory)");
    }

    #[test]
    fn compact_ls_simple_short() {
        let input = "file1.txt\nfile2.rs\nsrc\ntarget";
        assert_eq!(compact_ls(input, 50, 60), "file1.txt\nfile2.rs\nsrc\ntarget");
    }

    #[test]
    fn compact_ls_simple_truncates() {
        let lines: Vec<String> = (0..100).map(|i| format!("file{i}.txt")).collect();
        let input = lines.join("\n");
        let result = compact_ls(&input, 50, 60);
        assert!(result.contains("...+40 more (100 total)"));
    }

    #[test]
    fn compact_ls_long_strips_total() {
        let input = "total 32\ndrwxr-xr-x  2 user group  4096 Jan  1 00:00 src\n-rw-r--r--  1 user group  1234 Jan  1 00:00 Cargo.toml";
        let result = compact_ls(input, 50, 60);
        assert!(!result.contains("total 32"));
        assert!(result.contains("src"));
        assert!(result.contains("Cargo.toml"));
    }

    // compact_find -------------------------------------------------------

    #[test]
    fn compact_find_empty() {
        assert_eq!(compact_find("", 40), "No files found");
    }

    #[test]
    fn compact_find_short_passthrough() {
        let input = "./src/main.rs\n./src/lib.rs";
        assert_eq!(compact_find(input, 40), input);
    }

    #[test]
    fn compact_find_truncates() {
        let lines: Vec<String> = (0..80).map(|i| format!("./src/file{i}.rs")).collect();
        let input = lines.join("\n");
        let result = compact_find(&input, 40);
        assert!(result.contains("...+40 more (80 total)"));
    }

    // compact_cat --------------------------------------------------------

    #[test]
    fn compact_cat_empty() {
        assert_eq!(compact_cat("", 100, 60, 30), "(empty file)");
    }

    #[test]
    fn compact_cat_short_passthrough() {
        let input = "line 1\nline 2\nline 3";
        assert_eq!(compact_cat(input, 100, 60, 30), input);
    }

    #[test]
    fn compact_cat_truncates_long_file() {
        let lines: Vec<String> = (0..200).map(|i| format!("line {i}")).collect();
        let input = lines.join("\n");
        let result = compact_cat(&input, 100, 60, 30);
        assert!(result.contains("lines omitted"));
        assert!(result.contains("line 0"));   // head preserved
        assert!(result.contains("line 199")); // tail preserved
    }

    // compact_wc ---------------------------------------------------------

    #[test]
    fn compact_wc_empty() {
        assert_eq!(compact_wc("", 30), "0");
    }

    #[test]
    fn compact_wc_single_line() {
        assert_eq!(compact_wc("  42 README.md", 30), "42 README.md");
    }

    // compact_tree -------------------------------------------------------

    /// Helper: default noise dirs for tests.
    fn test_noise_dirs() -> Vec<String> {
        crate::config::schema::FileOptimizerConfig::default().tree_noise_dirs
    }

    #[test]
    fn compact_tree_empty() {
        assert_eq!(compact_tree("", 60, &test_noise_dirs()), "(empty)");
    }

    #[test]
    fn compact_tree_short_passthrough() {
        let input = ".\n├── src\n│   └── main.rs\n└── Cargo.toml\n\n1 directory, 2 files";
        assert_eq!(compact_tree(input, 60, &test_noise_dirs()), input);
    }

    #[test]
    fn compact_tree_truncates_with_summary() {
        let mut lines: Vec<String> = (0..100).map(|i| format!("├── file{i}.txt")).collect();
        lines.push("100 directories, 200 files".to_string());
        let input = lines.join("\n");
        let result = compact_tree(&input, 60, &test_noise_dirs());
        assert!(result.contains("100 directories, 200 files"));
        assert!(result.contains("lines omitted"));
    }

    // compact_tree pruning -----------------------------------------------

    #[test]
    fn compact_tree_prunes_noise_subtrees() {
        let input = "\
.
├── src
│   └── main.rs
├── .idea
│   ├── workspace.xml
│   ├── modules.xml
│   └── shelf
│       └── Changes
├── node_modules
│   ├── lodash
│   │   └── lodash.js
│   └── express
│       └── index.js
└── Cargo.toml

5 directories, 7 files";
        let result = compact_tree(input, 60, &test_noise_dirs());

        // Noise subtrees should be collapsed
        assert!(result.contains(".idea/ [contents hidden]"));
        assert!(result.contains("node_modules/ [contents hidden]"));

        // Children should be removed
        assert!(!result.contains("workspace.xml"));
        assert!(!result.contains("modules.xml"));
        assert!(!result.contains("lodash"));
        assert!(!result.contains("express"));

        // Non-noise entries preserved
        assert!(result.contains("src"));
        assert!(result.contains("main.rs"));
        assert!(result.contains("Cargo.toml"));
    }

    #[test]
    fn compact_tree_prunes_windows_style_tree() {
        // Windows `tree /F` uses +--- and |   prefixes
        let input = "\
C:\\PROJECT
+---.idea
|   +---shelf
|   |   +---Changes
|   +---modules.xml
+---src
|   +---main.rs
+---Cargo.toml";
        let result = compact_tree(input, 60, &test_noise_dirs());

        assert!(result.contains(".idea/ [contents hidden]"));
        assert!(!result.contains("shelf"));
        assert!(!result.contains("modules.xml"));
        assert!(result.contains("src"));
        assert!(result.contains("main.rs"));
    }

    #[test]
    fn compact_tree_prunes_bin_obj_subtrees() {
        let input = "\
.
├── MyApp
│   ├── Program.cs
│   ├── bin
│   │   └── Debug
│   │       └── net8.0
│   │           └── MyApp.dll
│   └── obj
│       └── Debug
│           └── net8.0
│               └── MyApp.pdb
└── MyApp.sln";
        let result = compact_tree(input, 60, &test_noise_dirs());

        assert!(result.contains("bin/ [contents hidden]"));
        assert!(result.contains("obj/ [contents hidden]"));
        assert!(!result.contains("MyApp.dll"));
        assert!(!result.contains("MyApp.pdb"));
        assert!(result.contains("Program.cs"));
        assert!(result.contains("MyApp.sln"));
    }

    #[test]
    fn compact_tree_custom_noise_dirs() {
        let input = "\
.
├── src
│   └── main.rs
├── custom_junk
│   ├── file1
│   └── file2
└── Cargo.toml";
        let noise = vec!["custom_junk".to_string()];
        let result = compact_tree(input, 60, &noise);
        assert!(result.contains("custom_junk/ [contents hidden]"));
        assert!(!result.contains("file1"));
        assert!(!result.contains("file2"));
        assert!(result.contains("main.rs"));
    }

    #[test]
    fn compact_tree_empty_noise_list_preserves_all() {
        let input = "\
.
├── node_modules
│   └── lodash.js
└── src";
        let result = compact_tree(input, 60, &[]);
        assert!(result.contains("lodash.js"));
        assert!(!result.contains("[contents hidden]"));
    }

    // compact_ls PowerShell ----------------------------------------------

    #[test]
    fn compact_ls_powershell_format() {
        let input = [
            "",
            "    Directory: C:\\source\\repos\\MyProject",
            "",
            "Mode                 LastWriteTime         Length Name",
            "----                 -------------         ------ ----",
            "d----          2/12/2026   5:11 PM                .ai-tss",
            "d----          2/12/2026   3:45 PM                src",
            "d----          2/12/2026   3:45 PM                tests",
            "-a---          2/12/2026   5:11 PM          21497 MyProject.sln",
            "-a---           1/2/2026  10:30 AM           1234 README.md",
        ]
        .join("\n");

        let result = compact_ls(&input, 50, 60);

        // Should extract names with type markers
        assert!(result.contains("[D] .ai-tss"));
        assert!(result.contains("[D] src"));
        assert!(result.contains("[D] tests"));
        assert!(result.contains("MyProject.sln"));
        assert!(result.contains("README.md"));

        // Should strip the header, separator, and Directory: line
        assert!(!result.contains("Mode"));
        assert!(!result.contains("LastWriteTime"));
        assert!(!result.contains("----"));
        assert!(!result.contains("Directory:"));

        // Should have a summary header
        assert!(result.contains("3 directories, 2 files"));
    }

    #[test]
    fn compact_ls_powershell_shows_sizes() {
        let input = "\
Mode                 LastWriteTime         Length Name
----                 -------------         ------ ----
-a---          2/12/2026   5:11 PM          21497 bigfile.zip
-a---           1/2/2026  10:30 AM            512 small.txt";

        let result = compact_ls(input, 50, 60);
        assert!(result.contains("21.0 KB"));
        assert!(result.contains("512 B"));
    }

    #[test]
    fn compact_ls_powershell_truncates() {
        let mut lines = vec![
            "Mode                 LastWriteTime         Length Name".to_string(),
            "----                 -------------         ------ ----".to_string(),
        ];
        for i in 0..100 {
            lines.push(format!("-a---          1/1/2026  12:00 PM          {i:>5} file{i}.txt"));
        }
        let input = lines.join("\n");
        let result = compact_ls(&input, 10, 60);
        assert!(result.contains("...+90 more (100 total)"));
    }

    // human_size ---------------------------------------------------------

    #[test]
    fn human_size_formats() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(1_048_576), "1.0 MB");
        assert_eq!(human_size(1_073_741_824), "1.0 GB");
    }

    // tree helper functions ----------------------------------------------

    #[test]
    fn tree_indent_depth_works() {
        assert_eq!(tree_indent_depth("root"), 0);
        assert_eq!(tree_indent_depth("├── src"), 4);
        assert_eq!(tree_indent_depth("│   └── main.rs"), 8);
        assert_eq!(tree_indent_depth("+---node_modules"), 4);
        assert_eq!(tree_indent_depth("|   +---lodash"), 8);
    }

    #[test]
    fn tree_entry_name_works() {
        assert_eq!(tree_entry_name("├── src"), "src");
        assert_eq!(tree_entry_name("│   └── main.rs"), "main.rs");
        assert_eq!(tree_entry_name("+---.idea"), ".idea");
        assert_eq!(tree_entry_name("|   +---shelf"), "shelf");
        assert_eq!(tree_entry_name("root"), "root");
    }
}
