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
            FileCommand::Tree => compact_tree(raw_output, self.tree_max_lines),
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
/// For long listings (`ls -l`-style), strips the "total" line and compacts
/// entries. For simple listings, limits to N items with a count summary.
fn compact_ls(raw_output: &str, max_entries: usize, max_items: usize) -> String {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return "(empty directory)".to_string();
    }

    let lines: Vec<&str> = trimmed.lines().collect();

    // Detect long-format output (starts with permissions like drwxr-xr-x or -)
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

/// Compact `tree` output: limit depth/entries.
fn compact_tree(raw_output: &str, max_lines: usize) -> String {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return "(empty)".to_string();
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    let total = lines.len();

    if total <= max_lines {
        return trimmed.to_string();
    }

    // Keep the beginning (tree structure) and the summary line at the end
    let mut result: Vec<&str> = lines[..max_lines - 1].to_vec();

    // The last line of tree output is typically a summary like "N directories, M files"
    if let Some(last) = lines.last()
        && (last.contains("director") || last.contains("file"))
    {
        result.push("");
        result.push(last);
        let summary = format!("...({} lines omitted)", total - max_lines);
        let mut output = result.join("\n");
        output.push_str(&format!("\n{summary}"));
        return output;
    }

    result.push("");
    let summary = format!("...+{} more lines ({} total)", total - max_lines + 1, total);
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

    #[test]
    fn compact_tree_empty() {
        assert_eq!(compact_tree("", 60), "(empty)");
    }

    #[test]
    fn compact_tree_short_passthrough() {
        let input = ".\n├── src\n│   └── main.rs\n└── Cargo.toml\n\n1 directory, 2 files";
        assert_eq!(compact_tree(input, 60), input);
    }

    #[test]
    fn compact_tree_truncates_with_summary() {
        let mut lines: Vec<String> = (0..100).map(|i| format!("├── file{i}.txt")).collect();
        lines.push("100 directories, 200 files".to_string());
        let input = lines.join("\n");
        let result = compact_tree(&input, 60);
        assert!(result.contains("100 directories, 200 files"));
        assert!(result.contains("lines omitted"));
    }
}
