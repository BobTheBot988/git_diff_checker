# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`git_diff_checker` is a Rust tool that detects modifications to original committed lines in a git-tracked file and **selectively reverts** only those changes, preserving new lines appended by an LLM code agent. It integrates into Claude Code / Qwen Code via a **two-phase hook system** (pre-tool-use and post-tool-use) that prevents unauthorized edits and blocks the agent when violations occur.

The repository contains **two Rust builds**: the root `git_diff_checker` crate (core engine + CLI) and a `hooks/` Cargo workspace (hook protocol infrastructure).

## Build & Test Commands

```bash
# Build the main crate
cargo build
cargo build --release

# Build the hooks workspace
cargo build --manifest-path hooks/Cargo.toml
cargo build --manifest-path hooks/Cargo.toml --release

# Run all tests (main crate only)
cargo test -- --test--threads 1

# Run a single test by name
cargo test test_parse_hunk_header

# Run hook tests
cargo test --manifest-path hooks/Cargo.toml

# Run shell integration test suite
bash test/test.sh

# Lint checking
cargo clippy
cargo clippy --manifest-path hooks/Cargo.toml
```

## Architecture

### Core Crate (`src/`)

- **`src/lib.rs`** — Core library exporting:
  - `HunkInfo` struct: parsed git diff hunk with line ranges
  - `check_file_modified()`: checks if original content was modified
  - `get_diff_hunks_with_ranges()`: parses `git diff HEAD` output into hunks
  - `selective_revert()`: builds a patch of only original-line-affecting hunks, applies in reverse via `git apply -R`
  - `get_all_modified_files()`: lists all files modified since HEAD via `git diff --name-only HEAD`
  - `selective_revert_all()`: batch revert across all modified files, continues on per-file errors
  - Internal helpers: `parse_hunk_header`, `hunk_affects_original_content`, `build_selective_patch`
- **`src/main.rs`** — CLI binary using `clap::Parser`. Arguments: `--repo-path` (default: `test/test1`), `--filename` (default: `src/hello_world.c`), `--all` (check all modified files)
- **`tests/integration_test.rs`** — Integration tests exercising `check_file_modified` against the `test/test1` fixture

### Hooks Workspace (`hooks/`)

A separate Cargo workspace with three crates:

- **`hooks/common/`** — Shared library defining the hook protocol:
  - Input models: `CommandRequest`, `PreToolUseInput`, `PostToolUseInput`
  - Output models: `PreToolUseHookOutput`, `PostToolUseHookOutput`
  - Core framework: `Hook` struct, `HookHandler` trait, `HookEngine::run_hook()` static method
  - `recv_hook_input()` reads JSON from stdin; outputs are serialized to stdout
  - Writes debug info to `/tmp/debug.json`

- **`hooks/pre/`** — PreToolUse hook binary:
  - Enforces directory whitelist for Write/Edit operations (default: `src/`, configurable via `HOOK_ALLOWED_DIRS` env var)
  - Parses Bash commands to detect file write operations (`sed -i`, `>`, `>>`, `tee`, `cp`, `mv`, `dd of=`) and checks against whitelist
  - Validates paths with `shell-sanitize-rules` crate (blocks path traversal, control chars)
  - Allows read operations anywhere
  - Denies writes outside whitelisted dirs with descriptive reason

- **`hooks/post/`** — PostToolUse hook binary:
  - Runs `git_diff_checker` as a subprocess after every tool call
  - Parses output for "MODIFICATIONS DETECTED" / "Successfully reverted"
  - Returns `cont: false` (Block) if original lines were modified

### Selective Revert Strategy

The engine:

1. Gets original file content from HEAD via `git2` (libgit2 bindings)
2. Runs `git diff HEAD` via CLI and parses hunks with line ranges
3. For each hunk, determines if it modifies pre-existing content (not just appends or whitespace changes)
4. Builds a composite patch of only offending hunks
5. Applies in reverse via `git apply -p1 -R --ignore-space-change`

Shelling out to `git` CLI for diff/apply is intentional — `git2` lacks equivalents for `-R` (reverse) and `--ignore-space-change`.

### Data Flow

```
Agent modifies file (via Write/Edit or Bash)
  → PreToolUse hook (directory whitelist check + Bash parsing)
    → [if denied] Agent blocked
    → [if allowed] Agent writes file
  → PostToolUse hook (runs git_diff_checker --all)
    → Checks ALL modified files in repo (catches Bash circumvention)
    → [if original lines modified] Selective revert + Block agent
    → [if only new lines] Allow agent to continue
```

### Test Fixtures

- **`test/test1/`** — Git submodule with `src/hello_world.c` (simple C program). Primary test fixture used by unit tests, integration tests, and shell tests.
- **`test/test2/`** — Git submodule with a Foundry Solidity project (not used by Rust tests).

### Coding Style

Strict Clippy lints enforced across all crates:

- `unwrap_used` = deny, `expect_used` = deny
- `question_mark` = deny (no `?` operator)
- `map_flatten` = deny
- `single_match` = deny (force explicit match over `if let`)

All `Result` handling uses explicit `match` statements. Error propagation is manual.
