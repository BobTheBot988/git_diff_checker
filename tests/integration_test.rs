use git_diff_checker::check_file_modified;

/// Test that detects modifications when the file is modified
#[test]
fn test_detects_modifications() {
    let repo_path = "test/test1";
    let filename = "hello_world.c";

    // Modify the file temporarily
    let current_path = std::path::Path::new(repo_path).join(filename);
    let original_content = std::fs::read_to_string(&current_path)
        .expect("Failed to read current file");

    // Add a modification
    std::fs::write(&current_path, format!("{}\n// test modification", original_content))
        .expect("Failed to modify file");

    // Check that modification is detected
    let result = check_file_modified(repo_path, filename);
    assert!(result.is_ok(), "Modification check should succeed");
    assert!(result.unwrap(), "Modification should be detected");

    // Restore original file
    std::fs::write(&current_path, original_content)
        .expect("Failed to restore original file");
}

/// Test that no modifications are detected for an unmodified file
#[test]
fn test_no_modifications_detected() {
    let repo_path = "test/test1";
    let filename = "hello_world.c";

    // First check initial state is unmodified
    let result = check_file_modified(repo_path, filename);
    assert!(result.is_ok(), "Check should succeed");
    assert!(!result.unwrap(), "No modifications should be detected");
    
    // Modify and restore to leave file unchanged
    let current_path = std::path::Path::new(repo_path).join(filename);
    let original_content = std::fs::read_to_string(&current_path).expect("Failed to read file");
    std::fs::write(&current_path, format!("{}\n// temp line for testing", original_content))
        .expect("Failed to modify");
    let result = check_file_modified(repo_path, filename);
    assert!(result.unwrap(), "Modification should be detected");
    std::fs::write(&current_path, original_content)
        .expect("Failed to restore");
}
