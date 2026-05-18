# PreToolUse Hook: Input Validator & Directory Whitelist Enforcer

## Purpose

This hook prevents the model from writing to or modifying files outside a whitelist of allowed directories. It is the **first line of defense** against unauthorized file system modifications.

The whitelist is configured via the `HOOK_ALLOWED_DIRS` environment variable (comma-separated, defaults to `"src"`). Read operations (`read_file`, `glob`, `grep_search`, `list_directory`) are allowed on any path — only write/edit operations are restricted.

## Post-conditions

| Condition | Decision | Behaviour |
|---|---|---|
| Tool is a read operation (`read_file`, `glob`, `grep_search`, `list_directory`) | `Allow` | Passes through — no restrictions on reading |
| Tool is `Write` or `Edit` and `file_path` is inside an allowed directory | `Allow` | Passes through — file can be written |
| Tool is `Write` or `Edit` and `file_path` is **outside** an allowed directory | `Deny` | Request is blocked with a descriptive reason citing which path is outside the whitelist |
| Tool is `Bash` and no write patterns are detected by the heuristic | `Allow` | Passes through — the **PostToolUse hook** is the safety net that catches Bash-level writes |
| Tool is `Bash` and write patterns (`>`, `sed -i`, `tee`, `cp`, `mv`, etc.) target paths inside allowed dirs | `Allow` | Passes through |
| Tool is `Bash` and write patterns target paths **outside** allowed dirs | `Deny` | Request is blocked with the offending path and the full Bash command cited |
| Tool is `Bash` and extracted paths contain suspicious patterns (path traversal, control chars) | `Deny` | Blocked — `shell-sanitize-rules` validation failed |
| Tool is unknown / not in the restricted set | `Allow` | Passes through — not a recognized write mechanism |

## Outputs

### `permissionDecision` — always present, one of `"allow"` or `"deny"`

- **`allow`**: The tool call proceeds. Output includes the reason (e.g. `"File 'src/main.rs' is inside whitelisted directory"`).
- **`deny`**: The tool call is blocked. Output includes a specific reason (e.g. `"Only files inside whitelisted directories can be modified. 'config/settings.json' is outside."`).

### `additionalContext` — always present

Contains a human-readable explanation of the decision. This is injected into the model's context so it can understand *why* a request was denied and adjust its behaviour.

### `hookEventName` — always `"PreToolUse"`

Identifies which hook stage produced the output.

## Design Notes

- **Heuristic, not exhaustive.** The Bash command parser uses `shlex` tokenization and pattern matching for common write operations (`>`, `>>`, `sed -i`, `tee`, `cp`, `mv`, `install`, `dd of=`). It intentionally misses edge cases because the PostToolUse hook runs `git diff HEAD` on every tool call and catches anything that gets through.
- **Canonical path comparison.** Paths are resolved via `canonicalize()` before comparison, preventing symlink-based bypasses and relative-path tricks like `../../etc/passwd`.
- **Suspicious path rejection.** The `shell-sanitize-rules` crate validates extracted paths for control characters, null bytes, and path traversal patterns before the whitelist check.
