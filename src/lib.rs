use std::process::Command;

/// Represents a parsed hunk with its line ranges
#[derive(Debug, Clone)]
pub struct HunkInfo {
    pub content: String,
    pub original_start: usize,  // Starting line number in original file
    pub original_count: usize,  // Number of lines in original file
    pub new_start: usize,       // Starting line number in new file
    pub new_count: usize,       // Number of lines in new file
}

/// Check if the file in the test repository has been modified since the git commit
///
/// Returns:
/// - Ok(true) if modifications were detected
/// - Ok(false) if no modifications detected
/// - Err(String) if an error occurred
pub fn check_file_modified(repo_path: &str, filename: &str) -> Result<bool, String> {
    // Get the original file content from git history
    let output = Command::new("git")
        .args(["show", &format!("HEAD:{}", filename)])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("Failed to execute git show: {}", e))?;

    let original_content = String::from_utf8_lossy(&output.stdout);

    // Get the current working tree file content
    let current_path = std::path::Path::new(repo_path).join(filename);
    let current_content = std::fs::read_to_string(&current_path)
        .map_err(|e| format!("Failed to read current file: {}", e))?;

    Ok(original_content != current_content)
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
    let new_range = parts[2];  // e.g., "+1,5"

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
pub fn get_diff_hunks_with_ranges(repo_path: &str, filename: &str) -> Result<Vec<HunkInfo>, String> {
    // Get the full diff output
    let output = Command::new("git")
        .args(["diff", "HEAD", filename])
        .current_dir(repo_path)
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
                    current_hunk = format!("diff --git a/{} b/{}\n--- a/{}\n+++ b/{}\n", filename, filename, filename, filename);
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
    let output = Command::new("git")
        .args(["show", &format!("HEAD:{}", filename)])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("Failed to execute git show: {}", e))?;

    String::from_utf8(output.stdout).map_err(|e| format!("Failed to convert output to UTF-8: {}", e))
}

/// Check if a hunk actually modifies original line content (not just adds at end)
fn hunk_affects_original_content(hunk: &HunkInfo, original_line_count: usize) -> bool {
    // Parse the hunk content to see if it modifies existing lines
    let mut has_deletions = false;

    for line in hunk.content.lines() {
        if line.starts_with('-') && !line.starts_with("---") {
            has_deletions = true;
        }
    }

    // If there are deletions (lines starting with -), original content is being modified
    if has_deletions {
        return true;
    }

    // If there are only additions, check if it's appending beyond original file
    let original_end = hunk.original_start.saturating_add(hunk.original_count).saturating_sub(1);
    let new_end = hunk.new_start.saturating_add(hunk.new_count).saturating_sub(1);

    // If the hunk ends exactly at the original file boundary and new file is longer,
    // it's just appending, not modifying original content
    if original_end == original_line_count && new_end > original_end && !has_deletions {
        return false; // Just appending lines
    }

    // Default: check if it affects lines within the original file bounds
    hunk.original_start <= original_line_count
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

    // Write patch to temp file
    let patch_path = std::path::Path::new(".temp_selective_patch");
    std::fs::write(patch_path, &patch_content)
        .map_err(|e| format!("Failed to write temp patch: {}", e))?;

    // Apply the selective patch in reverse
    let output = Command::new("git")
        .args([
            "apply",
            "-p1",
            "--directory",
            repo_path,
            "-R",
            "--ignore-space-change",
            patch_path.to_str().unwrap(),
        ])
        .output()
        .map_err(|e| format!("Failed to apply selective patch: {}", e))?;

    // Clean up temp patch
    let _ = std::fs::remove_file(patch_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to apply selective patch: {}", stderr));
    }

    // Count how many original-line hunks were reverted
    let reverted_count = hunks.iter()
        .filter(|h| hunk_affects_original_content(h, original_line_count))
        .count();

    Ok(reverted_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_unmodified_file() {
        let result = check_file_modified("test/test1", "hello_world.c");
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_get_diff_hunks_unmodified() {
        let result = get_diff_hunks_with_ranges("test/test1", "hello_world.c");
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
}
