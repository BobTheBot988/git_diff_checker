use git2::Repository;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Represents a parsed hunk with its line ranges
#[derive(Debug, Clone)]
pub struct HunkInfo {
    pub content: String,
    pub original_start: usize, // Starting line number in original file
    pub original_count: usize, // Number of lines in original file
    pub new_start: usize,      // Starting line number in new file
    pub new_count: usize,      // Number of lines in new file
}

/// Result of reverting modifications in a single file
#[derive(Debug, Clone)]
pub struct RevertDetail {
    pub filename: String,
    /// Number of dirty hunks that were reverted
    pub reverted_hunks: usize,
    /// Original committed lines that had been modified and were restored
    pub reverted_lines: Vec<String>,
    /// New lines added by the model that were preserved (pure additions)
    pub preserved_lines: Vec<String>,
}

/// Discover the git repository root from a given path
pub fn get_git_root(repo_path: &str) -> Result<PathBuf, String> {
    let repo = Repository::discover(repo_path)
        .map_err(|e| format!("Failed to discover repository: {}", e))?;

    let repo_root = repo
        .workdir()
        .ok_or("Repository has no workdir (bare repo?")?;

    Ok(repo_root.to_path_buf())
}

/// Check if the file in the test repository has been modified since the git commit
///
/// Returns:
/// - Ok(true) if modifications were detected to original line content
/// - Ok(false) if no modifications to original content detected
///   (new lines, whitespace changes, and indentation changes are ignored)
/// - Err(String) if an error occurred
pub fn check_file_modified(repo_path: &str, filename: &str) -> Result<bool, String> {
    // Get original file content to determine line count
    let original_content = get_original_file_content(repo_path, filename)?;
    let original_line_count = original_content.lines().count();

    // Get diff hunks with line range info
    let hunks = get_diff_hunks_with_ranges(repo_path, filename)?;

    if hunks.is_empty() {
        return Ok(false); // No modifications detected
    }

    // Check if any hunk affects original line content
    // (deletions or modifications, not just additions)
    for hunk in &hunks {
        if hunk_affects_original_content(hunk, original_line_count) {
            return Ok(true);
        }
    }

    Ok(false) // Only new lines or whitespace changes, no original content modified
}

/// Parse hunk header line like "@@ -1,3 +1,5 @@"
/// Returns (original_start, original_count, new_start, new_count)
fn parse_hunk_header(header: &str) -> Option<(usize, usize, usize, usize)> {
    // Format: @@ -line,count +line,count @@ (optional context)
    if !header.starts_with("@@") {
        return None;
    }

    let parts: Vec<&str> = header.split(' ').collect();
    if parts.len() < 3 {
        return None;
    }

    let orig_range = parts[1]; // e.g., "-1,3"
    let new_range = parts[2]; // e.g., "+1,5"

    let orig = parse_range(orig_range);
    let new = parse_range(new_range);

    match (orig, new) {
        (Some((o_start, o_count)), Some((n_start, n_count))) => {
            Some((o_start, o_count, n_start, n_count))
        }
        _ => None,
    }
}

fn parse_range(range: &str) -> Option<(usize, usize)> {
    // Format: "-start,count" or "+start,count" or "-start" (single line)
    if !range.starts_with(['-', '+']) {
        return None;
    }

    let rest = &range[1..]; // Remove prefix
    let parts: Vec<&str> = rest.split(',').collect();

    match parts.as_slice() {
        [start_str] => {
            let start = start_str.parse::<usize>().ok()?;
            Some((start, 1))
        }
        [start_str, count_str] => {
            let start = start_str.parse::<usize>().ok()?;
            let count = count_str.parse::<usize>().ok()?;
            Some((start, count))
        }
        _ => None,
    }
}

/// Get the diff hunks for a modified file with line range information
pub fn get_diff_hunks_with_ranges(
    repo_path: &str,
    filename: &str,
) -> Result<Vec<HunkInfo>, String> {
    // Open the repository
    let repo = Repository::discover(repo_path)
        .map_err(|e| format!("Failed to discover repository: {}", e))?;

    // Get the working directory (actual repo root)
    let repo_root = repo
        .workdir()
        .ok_or("Repository has no workdir (bare repo?")?;

    // Get HEAD tree
    let head = repo
        .head()
        .map_err(|e| format!("Failed to get HEAD: {}", e))?;
    let _tree = head
        .peel_to_tree()
        .map_err(|e| format!("Failed to get tree: {}", e))?;

    // Determine the relative path from repo root to filename
    let relative_filename = if Path::new(filename).is_absolute() {
        Path::new(filename)
            .strip_prefix(repo_root)
            .map_err(|_| {
                format!(
                    "Filename {} is not within repo {}",
                    filename,
                    repo_root.to_string_lossy()
                )
            })?
            .to_path_buf()
    } else {
        Path::new(filename).to_path_buf()
    };

    // Get the full diff output
    let output = Command::new("git")
        .args(["diff", "HEAD", &relative_filename.to_string_lossy()])
        .current_dir(repo_root)
        .output()
        .map_err(|e| format!("Failed to execute git diff: {}", e))?;

    let diff_output = String::from_utf8_lossy(&output.stdout);

    // Parse the full diff into hunks with line range information
    let mut hunks = Vec::new();
    let mut current_hunk = String::new();
    let mut in_hunk = false;
    let mut found_diff_header = false;
    let mut current_range_info: Option<(usize, usize, usize, usize)> = None;
    let mut after_plusplus = false; // Track if we've seen +++ line

    for line in diff_output.lines() {
        if line.starts_with("diff --git") {
            // Start of a new file diff - save previous hunk if any
            if in_hunk && !current_hunk.is_empty() {
                if let Some((orig_start, orig_count, new_start, new_count)) = current_range_info {
                    hunks.push(HunkInfo {
                        content: current_hunk,
                        original_start: orig_start,
                        original_count: orig_count,
                        new_start,
                        new_count,
                    });
                }
            }
            current_hunk = format!("{}\n", line);
            found_diff_header = true;
            in_hunk = false;
            after_plusplus = false;
            current_range_info = None;
        } else if line.starts_with("---") {
            current_hunk.push_str(line);
            current_hunk.push('\n');
        } else if line.starts_with("+++") {
            current_hunk.push_str(line);
            current_hunk.push('\n');
            after_plusplus = true;
        } else if line.starts_with("@@") {
            // Hunk header - parse line ranges
            if let Some((orig_start, orig_count, new_start, new_count)) = parse_hunk_header(line) {
                if !found_diff_header {
                    current_hunk = format!(
                        "diff --git a/{} b/{}\n--- a/{}\n+++ b/{}\n",
                        filename, filename, filename, filename
                    );
                    found_diff_header = true;
                }
                current_hunk.push_str(line);
                current_hunk.push('\n');
                in_hunk = true;
                after_plusplus = false;
                current_range_info = Some((orig_start, orig_count, new_start, new_count));
            }
        } else if in_hunk || (after_plusplus && !line.starts_with("index")) {
            // Collect content after +++ and during hunk
            current_hunk.push_str(line);
            current_hunk.push('\n');
        }
    }

    // Push the last hunk if exists
    if in_hunk && !current_hunk.is_empty() {
        if let Some((orig_start, orig_count, new_start, new_count)) = current_range_info {
            hunks.push(HunkInfo {
                content: current_hunk,
                original_start: orig_start,
                original_count: orig_count,
                new_start,
                new_count,
            });
        }
    }

    Ok(hunks)
}

/// Get the original file content from git history
fn get_original_file_content(repo_path: &str, filename: &str) -> Result<String, String> {
    // Discover the repository root from the given path
    let repo = Repository::discover(repo_path)
        .map_err(|e| format!("Failed to discover repository: {}", e))?;

    // Get the working directory (actual repo root), not the .git path
    let repo_root = repo
        .workdir()
        .ok_or("Repository has no workdir (bare repo?")?;

    // Parse the HEAD reference
    let head = repo
        .head()
        .map_err(|e| format!("Failed to get HEAD: {}", e))?;

    // Get the tree entry for the file
    let tree = head
        .peel_to_tree()
        .map_err(|e| format!("Failed to get tree: {}", e))?;

    // Determine the relative path from repo root to the given repo_path
    // This is needed when repo_path is a subdirectory of the actual repo root
    // Need to make both paths absolute to compare them correctly
    let repo_path_abs = Path::new(repo_path)
        .canonicalize()
        .unwrap_or_else(|_| Path::new(repo_path).to_path_buf());
    let repo_root_abs = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());

    let path_from_root = repo_path_abs
        .strip_prefix(&repo_root_abs)
        .ok()
        .map(|p| p.to_path_buf());

    // Construct the full path relative to repo root
    let relative_filename = if Path::new(filename).is_absolute() {
        // For absolute filenames, strip the repo_root prefix
        Path::new(filename)
            .strip_prefix(&repo_root_abs)
            .map_err(|_| {
                format!(
                    "Filename {} is not within repo {}",
                    filename,
                    repo_root.to_string_lossy()
                )
            })?
            .to_path_buf()
    } else if let Some(prefix) = &path_from_root {
        // When repo_path is a subdirectory, prepend that path to filename
        prefix.join(filename)
    } else {
        Path::new(filename).to_path_buf()
    };

    // Get the blob for the file
    let tree_entry = tree
        .get_path(relative_filename.as_path())
        .map_err(|e| format!("Failed to get file from tree: {}", e))?;

    let blob = tree_entry
        .to_object(&repo)
        .map_err(|e| format!("Failed to get blob: {:?}", e))?
        .into_blob()
        .map_err(|e| format!("Failed to convert to blob: {:?}", e))?;

    String::from_utf8(blob.content().to_vec())
        .map_err(|e| format!("Failed to convert content to UTF-8: {}", e))
}

/// Check if two lines differ only by whitespace and brace repositioning.
///
/// A formatting change is when the non-whitespace content is the same, or differs
/// only by one `{` or `}` at a boundary (captures brace-split like `fun(){}` → `fun() {`).
fn is_formatting_change(orig: &str, new: &str) -> bool {
    let compact_orig: String = orig.chars().filter(|c| !c.is_whitespace()).collect();
    let compact_new: String = new.chars().filter(|c| !c.is_whitespace()).collect();

    if compact_orig == compact_new {
        return true;
    }

    // After stripping one boundary brace, check equality.
    // Handles brace-split like `fun(){}` → `fun() {`:
    //   compacted: `fun(){}` vs `fun(){`
    //   strip one `}` from first → `fun()` == `fun()`
    fn strip_one_boundary_brace(s: &str) -> &str {
        fn is_brace(c: char) -> bool {
            c == '{' || c == '}'
        }
        s.trim_start_matches(is_brace).trim_end_matches(is_brace)
    }
    strip_one_boundary_brace(&compact_orig) == strip_one_boundary_brace(&compact_new)
}

/// Check if a hunk actually modifies original line content (not just adds at end or whitespace changes)
///
/// Returns true only if:
/// - Original lines are deleted (not just replaced)
/// - Original line content actually changes (not just whitespace/indentation)
/// - Changes are not formatting-only (brace expansion, whitespace)
fn hunk_affects_original_content(hunk: &HunkInfo, original_line_count: usize) -> bool {
    // Parse the hunk content to see if it modifies existing lines
    let mut has_deletions = false;
    let mut has_non_whitespace_changes = false;

    // Track original lines (lines starting with -) and their new versions
    let mut original_lines: Vec<String> = Vec::new();
    let mut new_lines: Vec<String> = Vec::new();

    for line in hunk.content.lines() {
        if line.starts_with('-') && !line.starts_with("---") {
            has_deletions = true;
            original_lines.push(line[1..].to_string()); // Remove leading '-'
        } else if line.starts_with('+') && !line.starts_with("+++") {
            new_lines.push(line[1..].to_string()); // Remove leading '+'
        }
    }

    // If there are deletions but no corresponding additions, it's a real deletion
    if has_deletions && new_lines.is_empty() {
        return true;
    }

    // Check if original lines were actually modified (not just whitespace)
    // We compare line by line where possible
    let min_len = original_lines.len().min(new_lines.len());
    for i in 0..min_len {
        let orig = original_lines[i].trim();
        let new = new_lines[i].trim();
        // Only count as modification if original line had real content (not just whitespace)
        if !orig.is_empty() && orig != new && !is_formatting_change(orig, new) {
            // The non-whitespace content differs - this is a real modification
            has_non_whitespace_changes = true;
            break;
        }
    }

    // If there are more original lines than new lines, some were deleted
    if original_lines.len() > new_lines.len() {
        return true;
    }

    // If content actually changed (different non-whitespace text), it's a modification
    if has_non_whitespace_changes {
        return true;
    }

    // If there are only additions beyond original content, check if it's appending
    let original_end = hunk
        .original_start
        .saturating_add(hunk.original_count)
        .saturating_sub(1);
    let new_end = hunk
        .new_start
        .saturating_add(hunk.new_count)
        .saturating_sub(1);

    // If the hunk ends exactly at the original file boundary and new file is longer,
    // it's just appending, not modifying original content
    if original_end == original_line_count && new_end > original_end {
        return false; // Just appending lines
    }

    // Default: check if it affects lines within the original file bounds
    // If we get here, there were no actual modifications detected
    false
}

/// Flush a contiguous change block: separate modifications from pure additions.
///
/// Within a `-`/`+` block:
/// - ALL `-` lines (original content) are restored and tracked as reverted
/// - The first `min(del, add)` `+` lines are the replacement side of modifications (dropped)
/// - Remaining `+` lines are pure additions (preserved)
fn flush_change_block(
    pending_del: &mut Vec<String>,
    pending_add: &mut Vec<String>,
    result: &mut Vec<String>,
    reverted_out: &mut Vec<String>,
    preserved_out: &mut Vec<String>,
) {
    if pending_del.is_empty() && pending_add.is_empty() {
        return;
    }

    let del_len = pending_del.len();
    let add_len = pending_add.len();
    let replacements = del_len.min(add_len);

    // Restore ALL original lines and track as reverted
    for line in pending_del.iter() {
        reverted_out.push(line.clone());
        result.push(line.clone());
    }

    // Keep only PURE additions (beyond the replacement count)
    let mut i = replacements;
    while i < add_len {
        let line = pending_add[i].clone();
        preserved_out.push(line.clone());
        result.push(line);
        i += 1;
    }

    pending_del.clear();
    pending_add.clear();
}

/// Build a "clean" version of a hunk's affected region.
///
/// Walks the hunk body, splitting on context lines to identify change blocks.
/// For each change block, modifications are reverted and pure additions are kept.
///
/// Returns (clean_lines, reverted_lines, preserved_lines).
fn build_clean_region(hunk: &HunkInfo) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut result: Vec<String> = Vec::new();
    let mut reverted: Vec<String> = Vec::new();
    let mut preserved: Vec<String> = Vec::new();
    let mut pending_del: Vec<String> = Vec::new();
    let mut pending_add: Vec<String> = Vec::new();

    let lines: Vec<&str> = hunk.content.lines().collect();
    for line in &lines {
        // Skip hunk headers and unified diff preamble
        if line.starts_with("@@") || line.starts_with("diff --git") {
            continue;
        }
        // Skip file header markers (--- a/file, +++ b/file) — 4th char is space
        if (line.starts_with("---") && line.len() > 3 && line.as_bytes()[3] == b' ')
            || (line.starts_with("+++") && line.len() > 3 && line.as_bytes()[3] == b' ')
        {
            continue;
        }

        if line.starts_with('-') {
            // Deletion of an original line
            pending_del.push((&line[1..]).to_string());
        } else if line.starts_with('+') {
            // Addition of a new line
            pending_add.push((&line[1..]).to_string());
        } else {
            // Context line (starts with space in unified diff)
            // Flush any pending change block first
            flush_change_block(
                &mut pending_del,
                &mut pending_add,
                &mut result,
                &mut reverted,
                &mut preserved,
            );
            // Emit context line, stripping the leading space
            result.push((&line[1..]).to_string());
        }
    }

    // Flush any remaining change block at end of hunk
    flush_change_block(
        &mut pending_del,
        &mut pending_add,
        &mut result,
        &mut reverted,
        &mut preserved,
    );

    (result, reverted, preserved)
}

/// Selectively revert only modified lines, preserving pure additions.
///
/// Reads current file content into memory, processes dirty hunks bottom-to-top,
/// splices clean regions (original lines restored, pure additions kept), writes result.
pub fn selective_revert(repo_path: &str, filename: &str) -> Result<RevertDetail, String> {
    // Get original file content to determine line count
    let original_content = get_original_file_content(repo_path, filename)?;
    let original_line_count = original_content.lines().count();

    // Get diff hunks with line range info
    let hunks = get_diff_hunks_with_ranges(repo_path, filename)?;

    if hunks.is_empty() {
        return Ok(RevertDetail {
            filename: filename.to_string(),
            reverted_hunks: 0,
            reverted_lines: Vec::new(),
            preserved_lines: Vec::new(),
        });
    }

    // Check if any dirty hunks exist
    let mut has_dirty = false;
    for hunk in &hunks {
        if hunk_affects_original_content(hunk, original_line_count) {
            has_dirty = true;
            break;
        }
    }

    if !has_dirty {
        return Ok(RevertDetail {
            filename: filename.to_string(),
            reverted_hunks: 0,
            reverted_lines: Vec::new(),
            preserved_lines: Vec::new(),
        });
    }

    // Open the repository and get repo root
    let repo = match Repository::discover(repo_path) {
        Ok(r) => r,
        Err(e) => return Err(format!("Failed to discover repository: {}", e)),
    };
    let repo_root = match repo.workdir() {
        Some(d) => d.to_path_buf(),
        None => return Err("Repository has no workdir".to_string()),
    };

    // Resolve current file path
    let current_path = if Path::new(filename).is_absolute() {
        Path::new(filename).to_path_buf()
    } else {
        repo_root.join(filename)
    };

    // Read current file content
    let current_content = match std::fs::read_to_string(&current_path) {
        Ok(c) => c,
        Err(e) => return Err(format!("Failed to read current file: {}", e)),
    };
    let has_trailing_newline = current_content.ends_with('\n');
    let mut current_lines: Vec<String> = current_content.lines().map(|s| s.to_string()).collect();

    let mut total_reverted_lines: Vec<String> = Vec::new();
    let mut total_preserved_lines: Vec<String> = Vec::new();
    let mut reverted_count: usize = 0;

    // Process dirty hunks bottom-to-top so earlier splices don't shift later positions
    let mut idx = hunks.len();
    while idx > 0 {
        idx -= 1;
        let hunk = &hunks[idx];
        if !hunk_affects_original_content(hunk, original_line_count) {
            continue;
        }

        reverted_count += 1;
        let (clean_lines, reverted_lines, preserved_lines) = build_clean_region(hunk);

        for rl in &reverted_lines {
            total_reverted_lines.push(rl.clone());
        }
        for pl in &preserved_lines {
            total_preserved_lines.push(pl.clone());
        }

        let start = hunk.new_start - 1;
        let end = start + hunk.new_count;
        let _ = current_lines.splice(start..end, clean_lines);
    }

    // Reconstruct file content
    let mut new_content = current_lines.join("\n");
    if has_trailing_newline {
        new_content.push('\n');
    }

    match std::fs::write(&current_path, &new_content) {
        Ok(_) => {}
        Err(e) => return Err(format!("Failed to write file: {}", e)),
    }

    Ok(RevertDetail {
        filename: filename.to_string(),
        reverted_hunks: reverted_count,
        reverted_lines: total_reverted_lines,
        preserved_lines: total_preserved_lines,
    })
}

/// Get a list of all files modified since HEAD in the repository
///
/// Runs `git diff --name-only HEAD` and returns relative paths.
pub fn get_all_modified_files(repo_path: &str) -> Result<Vec<String>, String> {
    let repo_root = get_git_root(repo_path)?;

    let output = match Command::new("git")
        .args(["diff", "--name-only", "HEAD"])
        .current_dir(repo_root)
        .output()
    {
        Ok(o) => o,
        Err(e) => return Err(format!("Failed to execute git diff --name-only: {}", e)),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git diff --name-only failed: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut files: Vec<String> = Vec::new();

    for line in stdout.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            files.push(trimmed.to_string());
        }
    }

    Ok(files)
}

/// Selectively revert original-line modifications across all modified files
///
/// Iterates over every file modified since HEAD. For each file, reverts only
/// individual modified lines, preserving pure additions added by the agent.
/// Continues on per-file errors.
///
/// Returns a vector of RevertDetail with per-file results.
pub fn selective_revert_all(repo_path: &str) -> Result<Vec<RevertDetail>, String> {
    let files = get_all_modified_files(repo_path)?;
    let mut results: Vec<RevertDetail> = Vec::new();

    for file in &files {
        let result = selective_revert(repo_path, file);
        match result {
            Ok(detail) => {
                results.push(detail);
            }
            Err(_e) => {
                results.push(RevertDetail {
                    filename: file.clone(),
                    reverted_hunks: 0,
                    reverted_lines: Vec::new(),
                    preserved_lines: Vec::new(),
                });
            }
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_unmodified_file() {
        let result = check_file_modified("test/test1/src", "hello_world.c");
        let a = match result {
            Ok(b) => b,
            Err(e) => {
                // eprintln!("Error: {}", e.as_str());
                panic!("Ciao: {}", e.as_str());
            }
        };
        assert!(!a, "value:{} should be false", a);
    }

    #[test]
    fn test_get_diff_hunks_unmodified() {
        let result = get_diff_hunks_with_ranges("test/test1", "src/hello_world.c");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_parse_hunk_header() {
        // Test parsing of hunk headers - the function parses the range part (without @@ prefix)
        // The actual header format is "@@ -start,count +start,count @@"
        assert_eq!(parse_range("-1,3"), Some((1, 3)));
        assert_eq!(parse_range("+4,4"), Some((4, 4)));
        assert_eq!(parse_range("-1"), Some((1, 1)));
        assert_eq!(parse_range("invalid"), None);
    }

    #[test]
    fn test_is_formatting_change_true_for_brace_expansion() {
        // `fun(){}` → `fun() {` — brace moved to its own line
        assert!(is_formatting_change("fun(){}", "fun() {"));
    }

    #[test]
    fn test_is_formatting_change_true_for_whitespace_only() {
        // Only whitespace difference
        assert!(is_formatting_change("  x = 1;", "x = 1;"));
    }

    #[test]
    fn test_is_formatting_change_false_for_content_change() {
        // Semantic content changed
        assert!(!is_formatting_change("x = 1;", "x = 2;"));
    }

    #[test]
    fn test_is_formatting_change_true_for_brace_and_whitespace() {
        // `{ }` → `{` — closing brace on next line
        assert!(is_formatting_change("{ }", "{"));
    }

    #[test]
    fn test_is_formatting_change_false_for_different_content_with_brace() {
        // Content differs even with same braces
        assert!(!is_formatting_change("a = b; }", "a = c; }"));
    }

    #[test]
    fn test_is_formatting_change_true_for_empty_to_brace() {
        // Empty line with just braces
        assert!(is_formatting_change("{}", "{"));
    }

    #[test]
    fn test_hunk_brace_expansion_not_modified() {
        // A hunk where `fun(){}` becomes `fun() {\n  // code\n}`.
        // The original line `fun(){}` vs new line `fun() {` should be
        // considered formatting-only → hunk should NOT affect original content.
        let hunk = HunkInfo {
            content: "@@ -1,1 +1,3 @@\n-fun(){}\n+fun() {\n+  // code\n+}\n".to_string(),
            original_start: 1,
            original_count: 1,
            new_start: 1,
            new_count: 3,
        };
        assert!(
            !hunk_affects_original_content(&hunk, 10),
            "brace expansion should not count as original content modification"
        );
    }

    #[test]
    fn test_check_modified_file() {
        // Test that check_file_modified detects modifications to original lines
        // This test modifies the file, checks, then restores it
        use std::fs;
        let test_file = "test/test1/src/hello_world.c";
        let original_content = fs::read_to_string(test_file).unwrap();

        // Make a temporary modification
        fs::write(test_file, original_content.replace("World", "Universe")).unwrap();

        let result = check_file_modified("test/test1", "src/hello_world.c");
        fs::write(test_file, &original_content).unwrap(); // Restore original

        assert!(result.is_ok());
        assert!(result.unwrap(), "modifications should be detected");
    }

    #[test]
    fn test_whitespace_only_original_not_modified() {
        // A hunk where the original line is whitespace-only (tab) and new line is code.
        // This should NOT be detected as modifying original content.
        let hunk = HunkInfo {
            content: "@@ -1,1 +1,1 @@\n-\t\n+bids[msg.sender] += amount;\n".to_string(),
            original_start: 1,
            original_count: 1,
            new_start: 1,
            new_count: 1,
        };
        assert!(
            !hunk_affects_original_content(&hunk, 5),
            "whitespace-only original should not count as modification"
        );
    }

    #[test]
    fn test_mixed_hunk_preserves_pure_additions() {
        // Modify an existing line AND add a pure addition in the same hunk.
        // Verify selective_revert restores the original line and keeps the addition.
        use std::fs;
        let test_file = "test/test1/src/hello_world.c";

        // Reset file to HEAD
        let repo = Repository::discover("test/test1").unwrap();
        let repo_root = repo.workdir().unwrap().to_path_buf();
        let _ = std::process::Command::new("git")
            .args(["checkout", "HEAD", "--", "src/hello_world.c"])
            .current_dir(&repo_root)
            .output();

        let original_content = fs::read_to_string(test_file).unwrap();

        // Modify line 3 AND add a new line after it (same hunk region)
        let modified = original_content
            .replace(
                "  printf(\"Hello, World!\\n\");\n",
                "  printf(\"Hello, Universe!\\n\");\n  // model added comment\n",
            );
        fs::write(test_file, &modified).unwrap();

        // Run selective_revert
        let result = selective_revert("test/test1", "src/hello_world.c");
        assert!(result.is_ok(), "selective_revert failed: {:?}", result.as_ref().err());

        let detail = result.unwrap();
        assert_eq!(detail.reverted_hunks, 1,
            "Expected 1 hunk reverted, got {}", detail.reverted_hunks);
        assert!(detail.reverted_lines.contains(&"  printf(\"Hello, World!\\n\");".to_string()),
            "Expected original line in reverted_lines");

        // Verify file on disk: original line restored, pure addition preserved
        let final_content = fs::read_to_string(test_file).unwrap();
        assert!(final_content.contains("Hello, World!"),
            "Original line should be restored");
        assert!(final_content.contains("Hello, Universe!") == false,
            "Modified line should NOT remain");
        assert!(final_content.contains("model added comment"),
            "Pure addition should be preserved");

        // Restore file
        let _ = std::process::Command::new("git")
            .args(["checkout", "HEAD", "--", "src/hello_world.c"])
            .current_dir(&repo_root)
            .output();
    }

    #[test]
    fn test_clean_hunk_no_revert() {
        // Only pure additions — selective_revert should return 0 reverted hunks.
        use std::fs;
        let test_file = "test/test1/src/hello_world.c";

        let repo = Repository::discover("test/test1").unwrap();
        let repo_root = repo.workdir().unwrap().to_path_buf();
        let _ = std::process::Command::new("git")
            .args(["checkout", "HEAD", "--", "src/hello_world.c"])
            .current_dir(&repo_root)
            .output();

        let original_content = fs::read_to_string(test_file).unwrap();
        // Append a new line — pure addition at end, no modifications
        let modified = format!("{}\n  // model added at end\n", original_content.trim_end());
        fs::write(test_file, &modified).unwrap();

        let result = selective_revert("test/test1", "src/hello_world.c");
        assert!(result.is_ok());

        let detail = result.unwrap();
        assert_eq!(detail.reverted_hunks, 0,
            "Should have 0 reverted hunks for clean addition");
        assert!(detail.preserved_lines.contains(&"  // model added at end".to_string()) ||
                detail.reverted_hunks == 0,
            "Pure addition should be noted as preserved or no revert needed");

        // Restore file
        let _ = std::process::Command::new("git")
            .args(["checkout", "HEAD", "--", "src/hello_world.c"])
            .current_dir(&repo_root)
            .output();
    }

    #[test]
    fn test_selective_revert_all_returns_details() {
        // Verify selective_revert_all returns RevertDetail structs correctly.
        use std::fs;
        let test_file = "test/test1/src/hello_world.c";

        let repo = Repository::discover("test/test1").unwrap();
        let repo_root = repo.workdir().unwrap().to_path_buf();
        let _ = std::process::Command::new("git")
            .args(["checkout", "HEAD", "--", "src/hello_world.c"])
            .current_dir(&repo_root)
            .output();

        // Make a modification
        let original_content = fs::read_to_string(test_file).unwrap();
        let modified = original_content.replace("Hello, World!", "Hello, Modified!");
        fs::write(test_file, &modified).unwrap();

        let result = selective_revert_all("test/test1");
        assert!(result.is_ok());

        let details = result.unwrap();
        // At least one file should have been processed
        assert!(details.len() > 0);

        // Find our file
        let mut found = false;
        for d in &details {
            if d.filename.ends_with("hello_world.c") || d.filename.contains("hello_world.c") {
                found = true;
                assert_eq!(d.reverted_hunks, 1);
                break;
            }
        }
        assert!(found, "Should have found hello_world.c in results");

        // Restore file
        let _ = std::process::Command::new("git")
            .args(["checkout", "HEAD", "--", "src/hello_world.c"])
            .current_dir(&repo_root)
            .output();
    }
}
