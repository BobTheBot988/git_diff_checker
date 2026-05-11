use git_diff_checker::{check_file_modified, get_diff_hunks_with_ranges, selective_revert};

fn main() {
    println!(
        "CHECK FIRST COMMIT WITH A DIFF COMMAND TO SEE IF THE OG LINES HAVE NOT BEEN MODIFIED"
    );

    //TODO make these 2 vars arguments from cli using clap
    let repo_path = "test/test1";
    let filename = "src/hello_world.c";

    // Check if file has been modified
    match check_file_modified(repo_path, filename) {
        Ok(false) => {
            println!("\nNo modifications detected.");
            println!("Do nothing and let the model proceed its current task");
        }
        Ok(true) => {
            println!("\nMODIFICATIONS DETECTED!");

            // Get diff hunks to show what would be affected
            match get_diff_hunks_with_ranges(repo_path, filename) {
                Ok(hunks) => {
                    println!("\nFound {} hunk(s) in the diff:", hunks.len());
                    for (i, hunk) in hunks.iter().enumerate() {
                        println!("\n--- Hunk {} ---", i + 1);
                        println!(
                            "  Original lines: {}-{}",
                            hunk.original_start,
                            hunk.original_start + hunk.original_count - 1
                        );
                        println!(
                            "  New lines: {}-{}",
                            hunk.new_start,
                            hunk.new_start + hunk.new_count - 1
                        );
                        println!("  Affects original lines: {}", hunk_affects_original(hunk));
                        println!("\nContent:");
                        for line in hunk.content.lines() {
                            println!("    {}", line);
                        }
                    }

                    // Count how many hunks affect original lines
                    let original_hunk_count =
                        hunks.iter().filter(|h| hunk_affects_original(h)).count();

                    if original_hunk_count > 0 {
                        println!(
                            "\n{} hunk(s) affect original lines - will be reverted.",
                            original_hunk_count
                        );

                        // Perform selective revert
                        match selective_revert(repo_path, filename) {
                            Ok(count) => {
                                println!(
                                    "\nSuccessfully reverted {} hunk(s) affecting original lines.",
                                    count
                                );
                                println!("Model-added lines preserved.");
                                println!("Communicate the revert to the LLM.");
                            }
                            Err(e) => {
                                eprintln!("Failed to revert: {}", e);
                                std::process::exit(1);
                            }
                        }
                    } else {
                        println!("\nOnly model-added lines were modified - no reversion needed.");
                        println!("Do nothing and let the model proceed its current task");
                    }
                }
                Err(e) => {
                    eprintln!("Failed to get diff hunks: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("Error checking file: {}", e);
            std::process::exit(1);
        }
    }
}

// Helper function to determine if a hunk affects original lines
fn hunk_affects_original(hunk: &git_diff_checker::HunkInfo) -> bool {
    // A hunk affects original lines if the original start position is within the original file bounds
    hunk.original_start > 0
}
