use git2::Repository;
use std::path::Path;
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

/// Check if a hunk actually modifies original line content (not just adds at end or whitespace changes)
///
/// Returns true only if:
/// - Original lines are deleted (not just replaced)
/// - Original line content actually changes (not just whitespace/indentation)
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
        if orig != new {
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

/// Build a selective patch containing only hunks that affect original lines
fn build_selective_patch(hunks: &[HunkInfo], original_line_count: usize) -> String {
    let mut patch = String::new();

    for hunk in hunks {
        if hunk_affects_original_content(hunk, original_line_count) {
            patch.push_str(&hunk.content);
        }
    }

    patch
}

/// Selectively revert only modifications to original lines, preserving model-added lines
pub fn selective_revert(repo_path: &str, filename: &str) -> Result<usize, String> {
    // Get original file content to determine line count
    let original_content = get_original_file_content(repo_path, filename)?;
    let original_line_count = original_content.lines().count();

    // Get diff hunks with line range info
    let hunks = get_diff_hunks_with_ranges(repo_path, filename)?;

    if hunks.is_empty() {
        return Ok(0); // No modifications
    }

    // Build selective patch with only original-line modifications
    let patch_content = build_selective_patch(&hunks, original_line_count);

    if patch_content.is_empty() {
        return Ok(0); // No original lines were modified
    }

    // Open the repository and get repo root
    let repo = Repository::discover(repo_path)
        .map_err(|e| format!("Failed to discover repository: {}", e))?;
    let repo_root = repo
        .workdir()
        .ok_or("Repository has no workdir")?;

    // Write patch to temp file in the repo directory
    let patch_path = std::path::Path::new(repo_path).join(".temp_selective_patch");
    std::fs::write(&patch_path, &patch_content)
        .map_err(|e| format!("Failed to write temp patch: {}", e))?;

    // Apply the selective patch in reverse using git apply
    // git2 doesn't have a direct equivalent for -R (reverse) + --ignore-space-change
    // So we use git CLI for the actual patch application
    let output = Command::new("git")
        .args([
            "apply",
            "-p1",
            "-R",
            "--ignore-space-change",
            ".temp_selective_patch",
        ])
        .current_dir(&repo_root)
        .output()
        .map_err(|e| format!("Failed to apply selective patch: {}", e))?;

    // Clean up temp patch
    let _ = std::fs::remove_file(patch_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to apply selective patch: {}", stderr));
    }

    // Count how many original-line hunks were reverted
    let reverted_count = hunks
        .iter()
        .filter(|h| hunk_affects_original_content(h, original_line_count))
        .count();

    Ok(reverted_count)
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
}
