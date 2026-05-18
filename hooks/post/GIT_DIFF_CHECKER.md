# PostToolUse Hook: Git Diff Checker & Selective Revert

## Purpose

This hook protects the **Golden Commit** — the initial committed state of the codebase established after the analysis and design phase. The model is allowed to **append new lines** to existing files and **add content inside pre-existing block structures**, but must **never modify or delete** lines that were part of the original commit.

After every tool call, this hook runs `git diff HEAD` across all modified files, identifies hunks that touch pre-existing (committed) content, and **selectively reverts only those changes** while preserving any purely additive lines the model introduced.

## Post-conditions

| Condition                                                                                                                                     | Decision                      | Behaviour                                                                                                                                                                                                                                                                                           |
| --------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| No modified files detected, or all changes are purely additive (new lines only, or content added inside pre-existing `()`, `{}`, `[]` blocks) | `Approve`                     | No action taken. `additionalContext` is empty — the model continues without interruption                                                                                                                                                                                                            |
| Original (committed) lines were modified                                                                                                      | `Approve` (with `cont: true`) | **Selective revert applied.** Only the offending original-line changes are reverted; new lines added by the model are preserved. The model receives detailed `additionalContext` listing exactly which lines were reverted and which were kept, plus guidelines on how to avoid triggering the hook |
| `git_diff_checker` encounters an error (e.g. not a git repo, git failure)                                                                     | `Approve` (with `cont: true`) | The error is reported in `additionalContext`. The model is allowed to continue — the error may be transient                                                                                                                                                                                         |

## Allowed Actions

The model **may** add content inside pre-existing structural blocks without triggering a revert. These are changes that respect the Golden Commit:

| Construct                    | Example                                           | Allowed?                                                                  |
| ---------------------------- | ------------------------------------------------- | ------------------------------------------------------------------------- |
| Parentheses `()`             | `function_call(`<br>`arg1,`<br>`arg2`<br>`);`     | No — opening `(` and closing `)` are original lines;                      |
| Braces `{}`                  | `if condition {`<br>`// model-added code`<br>`}`  | Yes — opening `{` and closing `}` are original lines; body is new         |
| Brackets `[]`                | `let arr = [`<br>`1, 2, 3`<br>`];`                | Yes — opening `[` and closing `]` are original lines; elements are new    |
| After a statement (new line) | `let x = 1;`<br>`let y = 2;  // model-added line` | Yes — appending a new line after the last committed line is pure addition |

The model **must not**:

| Action                            | Example                      | Why                                 |
| --------------------------------- | ---------------------------- | ----------------------------------- |
| Modify an existing line's content | `let x = 1;` → `let x = 2;`  | Changes the original committed line |
| Delete an existing line           | Remove `let x = 1;` entirely | Destroys original content           |
| Replace a line inline             | `class Foo` → `class Bar`    | Line content differs from HEAD      |

**Key insight**: If a line existed in the Golden Commit, its text must remain unchanged. New lines can be inserted between original lines or after the last original line. Opening and closing delimiters (`{`, `}`, `(`, `)`, `[`, `]`) are considered original lines — but their **interior** is available for additions.

## Outputs

### When no modifications are detected

```json
{
  "cont": true,
  "reason": "git_diff_checker: No unauthorized changes detected.",
  "hookSpecificOutput": null
}
```

No `additionalContext` is injected — the model continues unaware.

### When modifications are detected and reverted

```json
{
  "cont": true,
  "reason": "git_diff_checker: 2 hunk(s) reverted across 1 file(s).",
  "hookSpecificOutput": {
    "hookEventName": "PostToolUse",
    "additionalContext": "Original committed lines were modified and have been reverted.\nNew code added by the agent has been preserved.\n\nFile: src/main.rs\n  Reverted (restored to original):\n    - fn old_function() { ... }\n  Preserved (model additions kept):\n    + // new function added by agent\n\nGuidelines:\n  DOABLE — Add new lines inside existing constructs\n  NOT DOABLE — Modify or replace content of an original line"
  }
}
```

The `additionalContext` serves as **corrective feedback** — it tells the model exactly what it did wrong and how to fix its approach on the next attempt.

### When an error occurs

```json
{
  "cont": true,
  "reason": "git_diff_checker: <error message>",
  "hookSpecificOutput": {
    "hookEventName": "PostToolUse",
    "additionalContext": "The git diff checker encountered an error: <details>"
  }
}
```

The model continues; the error context may help the model self-correct.

## Selective Revert Strategy

1. Get original file content from HEAD via `git2` (libgit2 bindings)
2. Run `git diff HEAD` and parse hunks with line ranges
3. For each hunk, classify it:
   - **Original-line hunk**: touches lines that existed in HEAD → builds a reverse patch
   - **Pure-addition hunk**: only adds new lines after the last committed line → skipped
4. Apply the composite reverse patch via `git apply -p1 -R --ignore-space-change`

## Design Notes

- **Safety net.** The PreToolUse hook catches obvious violations early (directory whitelist), but Bash commands can write anywhere. The PostToolUse hook is the **unconditional catch-all** — it runs `git diff HEAD` after every tool call and reverts anything that shouldn't have changed.
- **Line-level granularity.** Within a mixed hunk (both original-line modifications and model additions), only the original-line changes are reverted. Pure model additions in the same hunk are preserved.
- **Non-blocking.** The hook always sets `cont: true`. It does not stop the model — it corrects the file state and educates the model via `additionalContext` so the model can adjust its behaviour. The model can see exactly which lines were reverted and why.
