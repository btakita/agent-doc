---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent: opencode
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
Prior frontmatter bug fix remains complete. New work this cycle is planning only: added a concrete reliability plan for the “response patchback not committed / no patchback cycle ran” failure family. The current diagnosis is that two bugs can surface as the same symptom: (1) fresh Codex sessions can miss the initial route trigger entirely, so `preflight -> write -> commit` never starts; (2) a write/IPC + snapshot save can succeed, but the cycle can still die before `commit_*`, leaving a visible uncommitted assistant patchback in the working tree.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260531-211807.md`*

Compacted content:
- Archived 3 response topic(s): parallelize scan_text_source; tokio vs std::thread::scope; Is #dtmc complete?
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d2...
❯ do [#editor-buffer-state-ffi-debounce]. #spec-test-build-install-commit-push

### Re: editor-buffer-state-ffi-debounce — glm-5.1

Implemented editor-authoritative buffer stability via the FFI bridge. Three files changed:

**`debounce.rs`** — new `EditorBufferState` struct with fields: `path`, `version` (monotonic), `dirty`, `last_edit_timestamp_ms`, `save_timestamp_ms` (optional), `hash` (optional SHA-256), `content_len` (optional), `session_id` (optional). New functions: `record_editor_buffer_state`, `editor_buffer_state`, `editor_buffer_stable`, `await_editor_buffer_stable`. State persisted to `.agent-doc/editor-state/<hash>` JSON sidecar.

**`diff_io.rs`** — `wait_for_stable_content` now checks for editor buffer state first. When present, uses editor-authoritative stability (version/hash match, dirty flag, debounce elapsed). Falls back to the truncation heuristic (`extract_last_added_line` + `looks_truncated`) when no editor state exists. The editor path returns immediately when the buffer hash matches disk content, or waits up to 6s for stability.

**`ffi.rs`** — three new FFI functions:
- `agent_doc_report_editor_state(file, version, dirty, last_edit_ts, save_ts, hash, len, session_id)` — called by IDE plugins on every document change
- `agent_doc_get_editor_state(file)` → JSON string of current state
- `agent_doc_is_editor_stable(file, debounce_ms)` → bool

10 new tests covering: record/read, stable/not-stable, debounce elapsed, overwrite, editor-state path in `wait_for_stable_content`, fallback without editor state. All 58 debounce+diff_io tests pass. `make check` clean. `cargo install --path .` succeeded. Committed and pushed as `8638f751`.
<!-- agent:boundary:33f61adb -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue -->
<!-- /agent:queue -->

<!-- agent:backlog -->
- [ ] [#editor-buffer-state-ffi-debounce] [recommended] Replace `wait_for_stable_content`'s last-added-line/truncation heuristic with editor-authoritative buffer stability when an IDE plugin is attached. JetBrains and VS Code plugins should report normalized editor state through the FFI bridge to the Session Actor: canonical doc path/session, monotonic editor document version, dirty flag, last edit timestamp, optional save timestamp, optional buffer hash, and ideally current buffer content. Run requests should include the editor version/hash observed at click time so the Session Actor can detect edits after invocation, debounce until the version/hash is stable, and compute the diff against the exact stable editor buffer when available. If no editor state exists, fall back to whole-file content + mtime stability polling. This removes reliance on `extract_last_added_line`, which misses edits above the last inserted line, and reduces disk/snapshot/editor-buffer race classes seen in live IPC/JB/VSCode flows.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-05-31 [#pbc1] Commit-boundary reliability for Codex/IPC sessions: fail closed when fresh-start route trigger never fires, and auto-resume commit when write/snapshot succeeded but commit boundary was interrupted. Plan: `tasks/agent-doc/plans/commit-boundary-reliability-codex-ipc.md`
- 2026-05-31 [#dtmc] scan_text_source parallelized via std::thread::scope in diagnose_cycle
<!-- /agent:done -->
