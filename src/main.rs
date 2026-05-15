use clap::Parser;
use git_diff_checker::{
    check_file_modified, get_all_modified_files, get_diff_hunks_with_ranges, selective_revert,
};
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

    /// Check all modified files in the repository instead of a single file
    #[arg(short, long, default_value_t = false)]
    all: bool,
}

fn main() {
    let args = Args::parse();

    // Canonicalize repo path for guaranteed absolute paths
    let repo_path = Path::new(&args.repo_path)
        .canonicalize()
        .unwrap_or_else(|_| args.repo_path.clone().into());
    let repo_path_str = repo_path.to_string_lossy().to_string();

    if args.all {
        run_all_mode(&repo_path_str);
    } else {
        run_single_file_mode(&repo_path_str, &args);
    }
}

fn run_single_file_mode(repo_path_str: &str, args: &Args) {
    let filename_val = &args.filename;
    let filename_path = Path::new(filename_val);
    let canonical_filename = filename_path
        .canonicalize()
        .unwrap_or_else(|_| filename_path.to_path_buf());
    let filename_str = canonical_filename.to_string_lossy().to_string();

    println!(
        "CHECK FIRST COMMIT WITH A DIFF COMMAND TO SEE IF THE OG LINES HAVE NOT BEEN MODIFIED"
    );
    println!("Repository: {:?}", repo_path_str);
    println!("File: {:?}", filename_str);

    match check_file_modified(repo_path_str, &filename_str) {
        Ok(false) => {
            println!("\nNo modifications detected.");
            println!("Do nothing and let the model proceed its current task");
        }
        Ok(true) => {
            println!("\nMODIFICATIONS DETECTED!");

            match get_diff_hunks_with_ranges(repo_path_str, &filename_str) {
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

                    let original_hunk_count =
                        hunks.iter().filter(|h| hunk_affects_original(h)).count();

                    if original_hunk_count > 0 {
                        println!(
                            "\n{} hunk(s) affect original lines - will be reverted.",
                            original_hunk_count
                        );

                        match selective_revert(repo_path_str, &filename_str) {
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

fn run_all_mode(repo_path_str: &str) {
    println!(
        "CHECK FIRST COMMIT WITH A DIFF COMMAND TO SEE IF THE OG LINES HAVE NOT BEEN MODIFIED"
    );
    println!("Checking all modified files in repository: {:?}", repo_path_str);

    let files = match get_all_modified_files(repo_path_str) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to get modified files: {}", e);
            std::process::exit(1);
        }
    };

    if files.is_empty() {
        println!("\nNo modified files found.");
        println!("Do nothing and let the model proceed its current task");
        return;
    }

    println!("\nFound {} modified file(s):", files.len());

    let mut total_reverted: usize = 0;
    let mut modified_count: usize = 0;

    for file in &files {
        println!("\n--- Checking file: {} ---", file);

        match check_file_modified(repo_path_str, file) {
            Ok(true) => {
                println!("MODIFICATIONS DETECTED");
                modified_count += 1;

                match selective_revert(repo_path_str, file) {
                    Ok(count) => {
                        println!("Successfully reverted {} hunk(s).", count);
                        total_reverted += count;
                    }
                    Err(e) => {
                        eprintln!("Failed to revert: {}", e);
                    }
                }
            }
            Ok(false) => {
                println!("No modifications to original lines.");
            }
            Err(e) => {
                eprintln!("Check failed: {}", e);
            }
        }
    }

    println!(
        "\nSummary: {} file(s) modified, {} hunk(s) reverted across {} file(s).",
        modified_count, total_reverted, files.len()
    );
}

// Helper function to determine if a hunk affects original lines
fn hunk_affects_original(hunk: &git_diff_checker::HunkInfo) -> bool {
    hunk.original_start > 0
}
