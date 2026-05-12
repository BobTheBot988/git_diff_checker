use git_diff_checker::check_file_modified;
use std::fs;

/// Clean up any stale git lock file
fn cleanup_git_lock(repo_path: &str) {
    let lock_path = std::path::Path::new(repo_path)
        .join(".git")
        .join("index.lock");
    let _ = fs::remove_file(&lock_path);
}

/// Test that detects modifications when the file is modified
#[test]
fn test_detects_modifications() {
    let repo_path = "test/test1";
    let filename = "src/hello_world.c";
    
    // Clean up any stale lock file
    cleanup_git_lock(repo_path);

    // Reset the file to original state first to ensure clean test
    std::process::Command::new("git")
        .args(["checkout", "HEAD", "--", filename])
        .current_dir(repo_path)
        .output()
        .expect("Failed to reset file");

    // Modify the file temporarily
    let current_path = std::path::Path::new(repo_path).join(filename);
    let original_content =
        std::fs::read_to_string(&current_path).expect("Failed to read current file");

    // Modify an existing line (not just appending)
    // Change "Hello" to "Hi" in the printf statement
    let modified_content = original_content.replace("Hello", "Hi");
    std::fs::write(&current_path, modified_content).expect("Failed to modify file");

    // Check that modification is detected
    let result = check_file_modified(repo_path, filename);
    assert!(result.is_ok(), "Modification check should succeed");
    assert!(result.unwrap(), "Modification should be detected");

    // Restore original file
    std::fs::write(&current_path, original_content).expect("Failed to restore original file");
}

/// Test that no modifications are detected for an unmodified file
#[test]
fn test_no_modifications_detected() {
    let repo_path = "test/test1";
    let filename = "src/hello_world.c";
    
    // Clean up any stale lock file
    cleanup_git_lock(repo_path);

    // Reset the file to original state first to ensure clean test
    let status = std::process::Command::new("git")
        .args(["checkout", "HEAD", "--", filename])
        .current_dir(repo_path)
        .status()
        .expect("Failed to run git");
    
    if !status.success() {
        panic!("Git checkout failed: {}", status);
    }

    // First check initial state is unmodified
    let result = check_file_modified(repo_path, filename);
    let result = match result {
        Ok(res) => res,
        Err(e) => panic!("Result: {}", e),
    };
    assert!(!result, "No modifications should be detected");

    // Modify and restore to leave file unchanged
    let current_path = std::path::Path::new(repo_path).join(filename);
    let original_content = std::fs::read_to_string(&current_path).expect("Failed to read file");

    // Modify an existing line (not just appending)
    // Change "World" to "World!" in the printf statement
    let modified_content = original_content.replace("World", "World!");
    std::fs::write(&current_path, modified_content).expect("Failed to modify");
    let result = check_file_modified(repo_path, filename);
    assert!(result.unwrap(), "Modification should be detected");

    // Restore original file
    std::fs::write(&current_path, original_content).expect("Failed to restore");
}
