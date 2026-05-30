# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`git_diff_checker` is a Rust tool for **blockchain code synthesis safety**. It detects modifications to original committed lines in a git-tracked file and **selectively reverts** only those changes, preserving new lines appended by an LLM code agent. Integrates into Claude Code / Qwen Code via a **three-phase hook system** (pre-tool-use, post-tool-use, stop) that prevents unauthorized edits and enforces testing discipline.

The goal: let an LLM agent write Foundry Solidity smart contracts safely — the agent can add new code but **cannot modify original lines** (the "Golden Commit").

The repository contains **three Rust builds**: the root `git_diff_checker` crate (core engine + CLI), a `hooks/` Cargo workspace (hook protocol infrastructure), and the `mcp-synthesizer/` submodule (MCP server for Solidity synthesis).

## Repository Tree

```
./
├── src/                        # Core crate: git_diff_checker
│   ├── lib.rs                  #   Core library (HunkInfo, selective_revert, etc.)
│   └── main.rs                 #   CLI binary (clap)
├── tests/
│   └── integration_test.rs     # Integration tests (currently BROKEN — test1 removed)
├── hooks/                      # Cargo workspace: hook protocol + binaries
│   ├── common/                 #   Shared library: Hook/HookHandler/HookEngine, I/O models
│   │   └── src/lib.rs
│   ├── pre/                    #   PreToolUse hook binary: dir whitelist, forge blocking
│   │   ├── src/main.rs
│   │   ├── INPUT_VALIDATOR_HOOK.md
│   │   └── tests/.hook_tests.rs
│   ├── post/                   #   PostToolUse hook binary: selective revert after each tool call
│   │   ├── src/main.rs
│   │   └── GIT_DIFF_CHECKER.md
│   ├── stop/                   #   Stop hook binary: blocks stop if coverage.info missing
│   │   └── src/main.rs
│   ├── .cargo/config.toml      #   LLD linker config
│   ├── .claude/                #   Claude Code settings (hooks, sandbox, env)
│   ├── .qwen/                  #   Qwen Code settings (permissions)
│   ├── Cargo.toml              #   Workspace manifest (members: pre, post, stop, common)
│   ├── Cargo.lock
│   └── hooks.drawio            #   Architecture diagram
├── mcp-synthesizer/            # Git submodule: MCP server for Solidity synthesis
│   ├── src/                    #   main.rs, db.rs, tools.rs, pipeline.rs
│   ├── Cargo.toml              #   Dependencies: rmcp, tokio, rusqlite, clap (package: mcp_synth)
│   ├── CLAUDE.md
│   ├── justfile                #   Build + install automation
│   └── README.md
├── test/                       # Test fixtures + shell integration tests
│   ├── test2/                  #   Git submodule: Foundry Solidity project (BobTheBot988/test2)
│   ├── test3/                  #   Git submodule: Foundry project (BobTheBot988/test3)
│   ├── test4/                  #   Git submodule: Foundry project (BobTheBot988/test4)
│   ├── test5/                  #   Git submodule: Foundry project (BobTheBot988/sec_proj)
│   ├── test.sh                 #   Shell integration test suite (currently BROKEN — uses test1)
│   ├── prompt.md               #   Expert Solidity Foundry prompt for LLM agent
│   ├── auction_prompt.md       #   Minimal auction prompt for LLM agent
│   └── halmos.toml             #   Halmos coverage/solver config
├── .qwen/                      # Qwen Code IDE settings (root)
├── .cargo/config.toml          # LLD linker config (root)
├── .gitmodules                 # Submodule definitions (6 entries)
├── Cargo.toml                  # Root crate manifest
├── Cargo.lock
├── CLAUDE.md
├── README.md
├── dockerfile                  # Multi-stage Docker: Foundry + Halmos + git
├── justfile                    # Build + install automation
└── workflow.xml                # Decision flowchart (draw.io)
```

## Branches

### Main repo (`git@github.com:BobTheBot988/git_diff_checker.git`)
- **Local:** `master`
- **Remote:** `origin/master`, `origin/dev`, `origin/exp`

### Submodules
| Submodule | Path | Branches |
|-----------|------|----------|
| test2 (BobTheBot988/test2) | test/test2 | master, DeepSeek-WITH-Skill, DeepSeek-Without-Skill, Qwen27BSolidityFinal, Qwen27BSolidityWithSkill, Qwen80B3BActiveWITHOUTSkill, Qwen80B3BActiveWithSkill |
| test3 (BobTheBot988/test3) | test/test3 | master |
| test4 (BobTheBot988/test4) | test/test4 | main |
| test5 (BobTheBot988/sec_proj) | test/test5 | master, remotes/origin/synth |
| mcp-synthesizer (LucaSforza/mcp-synthesizer) | mcp-synthesizer | master, remotes/origin/stable |
| hook (relative `./hook`) | hook/ | (never populated — dead entry) |

## Submodule Details

### mcp-synthesizer (`git@github.com:LucaSforza/mcp-synthesizer.git`)
Separate MCP server for Solidity contract synthesis using `rmcp` SDK (package: `mcp_synth`). Exposes 4 MCP tools:
- `forge_install`, `forge_build`, `forge_test` — wrap Foundry commands
- `run_synthesis` — full pipeline: forge build → forge test → halmos verification
- SQLite persistence for trial results
- `justfile` for build/install to `~/.local/bin/`

### test/test2 (`BobTheBot988/test2`)
Foundry Solidity project with auction contract. Has its own submodules, `foundry.toml`, `halmos.toml`, `justfile`. Multiple experiment branches for different LLM agent configurations.

### test/test3, test/test4, test/test5
Foundry Solidity projects with `src/`, `test/`, `lib/`, `foundry.toml`. test5 also includes Echidna fuzzing config (`echidna.yaml`), Crytic export, and docs.

### test/test1 — REMOVED
Deleted in commit `b4ba856`. Was the primary C-language test fixture (`src/hello_world.c`). All unit tests, integration tests, and shell tests reference it — they are now **broken** until fixtures are updated.

## Build & Test Commands

```bash
# Build the main crate
cargo build
cargo build --release

# Build the hooks workspace
cargo build --manifest-path hooks/Cargo.toml
cargo build --manifest-path hooks/Cargo.toml --release

# Build mcp-synthesizer
cargo build --manifest-path mcp-synthesizer/Cargo.toml

# Run all tests (main crate only)
cargo test -- --test-threads 1

# Run a single test by name
cargo test test_parse_hunk_header

# Run hook tests
cargo test --manifest-path hooks/Cargo.toml

# Run mcp-synthesizer tests
cargo test --manifest-path mcp-synthesizer/Cargo.toml

# Run shell integration test suite
bash test/test.sh

# Lint checking
cargo clippy
cargo clippy --manifest-path hooks/Cargo.toml
cargo clippy --manifest-path mcp-synthesizer/Cargo.toml

# Build + install hooks to ~/.local/bin
just install

# Docker build (Foundry + Halmos environment)
docker build -t synth-env -f dockerfile .
```

## Architecture

### Core Crate (`src/`)

- **`src/lib.rs`** — Core library exporting:
  - `HunkInfo` struct: parsed git diff hunk with line ranges
  - `RevertDetail` struct: `filename`, `reverted_hunks`, `reverted_lines`, `preserved_lines`
  - `check_file_modified()`: checks if original content was modified
  - `get_diff_hunks_with_ranges()`: parses `git diff HEAD` output into hunks
  - `selective_revert()`: builds a patch of only original-line-affecting hunks, applies in reverse via `git apply -R`
  - `get_all_modified_files()`: lists all files modified since HEAD via `git diff --name-only HEAD`
  - `selective_revert_all()`: batch revert across all modified files, continues on per-file errors
  - `get_git_root()`: discovers git repo root via libgit2
  - Internal helpers: `parse_hunk_header`, `hunk_affects_original_content`, `build_selective_patch`, `is_formatting_change`, `flush_change_block`, `build_clean_region`
- **`src/main.rs`** — CLI binary using `clap::Parser`. Arguments: `--repo-path` (default: `test/test1` — **OUTDATED**, test1 removed), `--filename` (default: `src/hello_world.c`), `--all` (check all modified files). Two modes: single file or all-mode.
- **`tests/integration_test.rs`** — Integration tests currently **fail** because they reference deleted `test/test1` fixture.

### Hooks Workspace (`hooks/`)

A Cargo workspace with **four** crates:

#### `hooks/common/` — Shared library (edition 2024)
Defines the entire hook protocol:
- **Input models:** `CommandRequest`, `PreToolUseInput`, `PostToolUseInput`, `StopInput`
- **Output models:** `PreToolUseHookOutput`, `PostToolUseHookOutput`, `StopHookOutput`
- **Enums:** `HookEventName` (14 variants incl. Stop, PermissionRequest), `HookDecision` (Ask/Block/Deny/Approve/Allow), `PermissionMode`, `HookType`
- **Core framework:** `Hook` struct (5-tuple), `HookHandler` trait, `HookEngine::run_hook()` static method
- `recv_hook_input()` reads JSON stream from stdin; outputs serialized to stdout
- Writes debug info to `/tmp/debug.json`
- Uses macros (`impl_try_from_request!`, `impl_hook_output_methods!`) for boilerplate

#### `hooks/pre/` — PreToolUse hook binary
First line of defense against unauthorized filesystem modifications:
- Enforces directory whitelist for Write/Edit operations (default: `src/`, configurable via `HOOK_ALLOWED_DIRS` env var)
- Supports blacklist via `HOOK_BLOCKED_PATHS` env var
- Parses Bash commands with `shlex` to detect write operations (`sed -i`, `>`, `>>`, `tee`, `cp`, `mv`, `install`, `dd of=`)
- **Blocks `forge` commands** outright — agent must use Forge MCP server instead
- Validates paths with `shell-sanitize-rules` crate (blocks path traversal, control chars)
- Allows read operations anywhere
- Denies writes outside whitelisted dirs with descriptive reason
- 34 unit tests

#### `hooks/post/` — PostToolUse hook binary
Second line of defense, runs **after every tool call**:
- Imports `git_diff_checker` as a **library crate** (not subprocess)
- Calls `selective_revert_all()` to detect and revert unauthorized modifications
- Injects detailed `additionalContext` with line-level revert/preserve info + corrective guidelines
- **Always allows continuation** (`cont: true`) — never blocks, but educates the model
- Exits with code 0 (JSON output handles decisions)

#### `hooks/stop/` — Stop hook binary
Enforces testing discipline before session stop:
- Checks for `coverage.info` in CWD
- If missing: **blocks** stop with message to use `mcp_synth` tool for Halmos testing
- If exists: allows stop (no decision)
- 3 unit tests

#### Hook configuration (`hooks/.claude/.settings.local.json`)
- Claude Code configured for local Qwen3-Coder model at `127.0.0.1:8080` (OpenAI-compatible API)
- PreToolUse hook: `$HOME/.local/bin/pre_hook` (30s timeout)
- PostToolUse hook: `$HOME/.local/bin/post_hook` (60s timeout)
- Stop hook: NOT registered in current config
- Sandbox: enabled, allowWrite only `./src/`, allowRead everywhere

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
  → PreToolUse hook (directory whitelist check + Bash parsing + forge blocking)
    → [if denied] Agent blocked with reason
    → [if allowed] Agent writes file
  → PostToolUse hook (calls git_diff_checker::selective_revert_all as library)
    → Checks ALL modified files in repo (catches Bash circumvention)
    → [if original lines modified] Selective revert + inject corrective context (cont: true)
    → [if only new lines] Allow agent to continue (cont: true)
  → Stop hook (on session stop)
    → [if coverage.info missing] Block stop, instruct to test with mcp_synth/halmos
    → [if coverage.info exists] Allow stop
```

### Test Fixtures

- **`test/test1/`** — **REMOVED** (commit `b4ba856`). Was `src/hello_world.c` C fixture. All unit tests, integration tests, and shell tests currently **broken**.
- **`test/test2/`** — Foundry Solidity project (auction contract). Has own submodules. Used for agent synthesis experiments across multiple branches.
- **`test/test3/`** — Foundry Solidity project.
- **`test/test4/`** — Foundry Solidity project.
- **`test/test5/`** — Foundry Solidity project with Echidna fuzzing config and Crytic export.
- **`halmos.toml`** (at `test/` level): Halmos config with Z3 solver, loop=10, coverage output to `/tmp/coverage.info`.

### Configuration Files

- **`.cargo/config.toml`** — LLD linker for faster Rust compilation
- **`hooks/.cargo/config.toml`** — Same LLD linker config for hooks workspace
- **`.gitignore`** — Ignores `target`, `Cargo.lock` (both root and hooks/)
- **`justfile`** — `just build` / `just b` (builds root + hooks with LLD), `just install` / `just i` (copies binaries to `~/.local/bin/`)
- **`dockerfile`** — Multi-stage: Foundry CLI tools + Alpine + Python/Halmos + git
- **`.qwen/settings.json`** — Qwen Code permissions: pre-approves `cargo build *`, read access to `src/` and `.qwen/`
- **`workflow.xml`** — draw.io flowchart of the revert-or-proceed decision logic

### Coding Style

Strict Clippy lints enforced across all crates (root and hooks/):

- `unwrap_used` = deny, `expect_used` = deny
- `question_mark` = deny (no `?` operator)
- `map_flatten` = deny
- `single_match` = deny (force explicit match over `if let`)

All `Result` handling uses explicit `match` statements. Error propagation is manual.

### Known Issues

1. **`test/test1` deleted** — Default CLI arg (`--repo-path test/test1`) and all tests (unit, integration, shell) reference removed fixture. PRs `b4ba856` ("Removed test1") and `cd3d303` suggest active migration.
2. **`hooks/stop/` not registered** — Stop hook binary exists but not wired into `hooks/.claude/.settings.local.json`.
3. **Tests fail** — `cargo test` produces 0 passed, 3 failed (all integration tests) due to missing test1.
4. **`./hook` submodule** — Listed in `.gitmodules` with relative URL `./hook`, never populated.
