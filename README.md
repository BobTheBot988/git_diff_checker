# git_diff_checker

Detect and selectively revert modifications to original committed lines in git-tracked files while preserving new lines appended by LLM code agents. Integrates into Claude Code via a two-phase hook system.

## How It Works

An LLM code agent modifies files via Write/Edit tools or Bash commands. The two-phase hook system intercepts each modification:

```
Agent modifies file (via Write/Edit or Bash)
  → PreToolUse hook (directory whitelist + blacklist check)
    → if denied: agent blocked before write
    → if allowed: agent writes file
  → PostToolUse hook (runs git_diff_checker --all)
    → checks ALL modified files in repo
    → if original lines modified: selective revert + detailed feedback to agent
    → if only new lines appended: allow
```

The engine parses `git diff HEAD` output into hunks, determines which hunks modify pre-existing lines (vs pure additions), builds a composite patch of only offending hunks, and applies it in reverse via `git apply -p1 -R --ignore-space-change`.

## Repository Structure

Two Rust builds in one repo:

```
git_diff_checker/
├── src/
│   ├── lib.rs          # Core library: diff parsing, hunk analysis, selective revert
│   └── main.rs         # CLI binary (clap)
├── tests/
│   └── integration_test.rs
├── hooks/
│   ├── Cargo.toml      # Hooks workspace
│   ├── common/         # Shared hook protocol: input/output models, Hook/HookHandler/HookEngine
│   ├── pre/            # PreToolUse hook: directory whitelist + blacklist enforcement
│   └── post/           # PostToolUse hook: runs git_diff_checker after every tool call
└── test/
    ├── test1/          # Git submodule: simple C program fixture
    └── test2/          # Git submodule: Foundry Solidity project
```

## Build & Test

```bash
# Main crate
cargo build
cargo build --release

# Hooks workspace
cargo build --manifest-path hooks/Cargo.toml
cargo build --manifest-path hooks/Cargo.toml --release

# Test
cargo test -- --test-threads 1
cargo test test_parse_hunk_header
cargo test --manifest-path hooks/Cargo.toml
bash test/test.sh

# Lint
cargo clippy
cargo clippy --manifest-path hooks/Cargo.toml
```

## CLI Usage

```bash
# Check a single file
cargo run -- --repo-path test/test1 --filename src/hello_world.c

# Check all modified files
cargo run -- --repo-path test/test1 --all
```

## Core Library (`src/lib.rs`)

Key exports:

- `HunkInfo` — parsed git diff hunk with line ranges
- `check_file_modified()` — detect if original content was modified
- `get_diff_hunks_with_ranges()` — parse `git diff HEAD` into hunks
- `selective_revert()` — revert only original-line-affecting hunks, preserve pure additions
- `selective_revert_all()` — batch revert across all modified files
- `get_all_modified_files()` — list files modified since HEAD

### Selective Revert Strategy

1. Get original file content from HEAD via `git2` (libgit2)
2. Run `git diff HEAD` via CLI, parse hunks with line ranges
3. For each hunk: classify as dirty (modifies pre-existing content) or clean (append/whitespace only)
4. Build composite patch of dirty hunks
5. Apply in reverse via `git apply -p1 -R --ignore-space-change`

Shelling out to `git` CLI for diff/apply is intentional — `git2` lacks equivalents for `-R` (reverse) and `--ignore-space-change`.

Formatting-only changes (brace repositioning, whitespace) are detected and not treated as modifications.

## Hook System (`hooks/`)

### PreToolUse Hook (`hooks/pre/`)

- **Directory whitelist**: restricts Write/Edit/Bash file writes to allowed directories (default: `src/`, configurable via `HOOK_ALLOWED_DIRS` env var)
- **Path blacklist**: blocks specific paths via `HOOK_BLOCKED_PATHS` env var
- **Bash command parsing**: tokenizes commands with `shlex`, detects write patterns (`sed -i`, `>`, `>>`, `tee`, `cp`, `mv`, `dd of=`)
- **Path validation**: blocks path traversal and control characters via `shell-sanitize-rules`
- Read operations (read_file, glob, grep_search) allowed anywhere
- Blocks `forge` commands (must use Forge MCP server)

### PostToolUse Hook (`hooks/post/`)

- Runs after every tool call
- Calls `selective_revert_all()` on the repo
- If original lines were modified: reverts them, injects detailed context to the agent explaining what was reverted vs preserved
- If only new lines: allows continuation

### Hook Protocol (`hooks/common/`)

Shared input/output models:
- `CommandRequest`, `PreToolUseInput`, `PostToolUseInput`
- `PreToolUseHookOutput`, `PostToolUseHookOutput`
- `Hook` struct, `HookHandler` trait, `HookEngine::run_hook()` static method
- Reads JSON from stdin, serializes output to stdout
- Debug info written to `/tmp/debug.json`

## Coding Style

Strict Clippy lints across all crates:
- `unwrap_used` = deny, `expect_used` = deny
- `question_mark` = deny (no `?` operator)
- `map_flatten` = deny
- `single_match` = deny (force explicit `match` over `if let`)

All `Result` handling uses explicit `match`. Error propagation is manual.
