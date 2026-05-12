use clap::Parser;
use git_diff_checker::{check_file_modified, get_diff_hunks_with_ranges, selective_revert};
use std::path::Path;

#[derive(Parser, Debug)]
#[command(name = "git_diff_checker")]
#[command(about = "Check if files have been modified since git commit and selectively revert")]
struct Args {
    /// Path to the git repository (absolute or relative)
    #[arg(short, long, default_value = "test/test1")]
    repo_path: String,

    /// Path to the file to check (absolute or relative to repo_path)
    #[arg(short, long, default_value = "src/hello_world.c")]
    filename: String,
}

fn main() {
    let args = Args::parse();

    println!(
        "CHECK FIRST COMMIT WITH A DIFF COMMAND TO SEE IF THE OG LINES HAVE NOT BEEN MODIFIED"
    );
    
    // Canonicalize paths for guaranteed absolute paths
    let repo_path = Path::new(&args.repo_path).canonicalize()
        .unwrap_or_else(|_| args.repo_path.clone().into());
    let filename = Path::new(&args.filename).canonicalize()
        .unwrap_or_else(|_| args.filename.clone().into());
    
    println!("Repository: {:?}", repo_path);
    println!("File: {:?}", filename);

    let repo_path_str = repo_path.to_string_lossy().to_string();
    let filename_str = filename.to_string_lossy().to_string();

    // Check if file has been modified
    match check_file_modified(&repo_path_str, &filename_str) {
        Ok(false) => {
            println!("\nNo modifications detected.");
            println!("Do nothing and let the model proceed its current task");
        }
        Ok(true) => {
            println!("\nMODIFICATIONS DETECTED!");

            // Get diff hunks to show what would be affected
            match get_diff_hunks_with_ranges(&repo_path_str, &filename_str) {
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
                        match selective_revert(&repo_path_str, &filename_str) {
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
