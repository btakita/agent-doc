# Plan: Orchestrate subprocess fixes

Two compounding bugs prevent `agent-doc orchestrate` from successfully spawning agent subprocesses.

## Bug 1: Frontmatter args not forwarded to subprocesses

**Root cause:** `run_ordered_task_step` in `orchestrate.rs` spawns fresh Claude/Codex subprocesses via `CliAgentRunner::send_fresh_streaming` → `agent::resolve_streaming_for_file` → `build_backend_command` → `default_base_args()`. Those defaults use `--permission-mode acceptEdits`, ignoring the document's frontmatter `claude_args` / `codex_args`.

**Fix:**
1. Thread the parsed frontmatter `claude_args` / `codex_args` into `build_backend_command`
2. Override or merge with `default_base_args()` so the subprocess inherits the document's permission mode
3. Add tests: spawn with frontmatter `--dangerously-skip-permissions`, verify the subprocess command line includes it

## Bug 2: Stderr silently discarded

**Root cause:** `send_streaming` in `agent/claude.rs` pipes stderr (`Stdio::piped()`) but never reads it. `StreamIterator` only reads stdout. When the subprocess fails, the actual error (permission denied, crash, etc.) is lost. `stream_step_response` in `orchestrate.rs` only sees empty stdout and reports "empty response from streaming orchestrate step".

**Fix:**
1. Spawn a thread (or async task) in `send_streaming` to drain stderr into a buffer
2. On subprocess exit with non-zero status or empty stdout, include stderr content in the error message
3. Surface stderr in `stream_step_response` error reporting so the orchestrate caller gets actionable diagnostics
4. Add tests: subprocess that writes to stderr only, verify error message includes stderr content

## Verification

- `cargo test` (all orchestrate + agent tests pass)
- `cargo build --release`
- `cargo install --path .`
- Manual test: document with `claude_args: --dangerously-skip-permissions` runs `orchestrate` without permission stall
