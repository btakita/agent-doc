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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260601-004032.md`*

Compacted content:
- Archived 9 response topic(s): lazily-rs for caching project_root_for_doc; lazily-rs dependency graph vision; Two Context lifetimes (CLI vs Actor); 6 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d2...
❯ Do we have a way to invalidate Slots and Cells in agent-doc if the user changes the document mid-run?
### Re: Slot/Cell invalidation mid-run — glm-5.1

Slots and Cells belong to the `lazily-rs` crate, which is **not imported by agent-doc** — there are zero `use lazily` or `Cargo.toml` dependencies connecting them. So the short answer is: agent-doc doesn't use Slots or Cells, and therefore has no invalidation path for them.

Agent-doc's actual mid-run change detection is a layered system that doesn't use a reactive dependency graph:

1. **Snapshot/file SHA-256 hashing** at each `CyclePhase` transition (`cycle_state.rs:72-73`) — tamper evidence and drift detection
2. **Debounce system** (`debounce.rs`) — cross-process typing indicators, live buffer divergence checks, editor buffer state from FFI plugins
3. **Actor generation CAS** (`session_actor.rs:216-233`) — rejects stale concurrent transitions
4. **Cycle phase state machine** — monotonic progression prevents regression (`cycle_state.rs:44-50`)
5. **Watch daemon** (`watch.rs`) — file-system watcher with convergence detection and agent-triggered-loop prevention

If the intent is to wire `lazily-rs` into agent-doc so that a document change mid-run reactively clears cached computation, `lazily-rs` already provides the transitive clearing infrastructure (`SlotHandle::clear()` and `CellHandle::clear_dependents()`). The integration gap would be connecting agent-doc's debounce/snapshot-hash signals to `Context::set_cell()` or `SlotHandle::clear()` calls at the appropriate cycle boundaries.
Use `CellHandle::set(&ctx)` instead of `Context::set_cell()`.

### Re: CellHandle::set vs Context::set_cell — glm-5.1

Acknowledged. Currently `set` lives on `Context::set_cell()` (`context.rs`) and `CellHandle` has no `set` method — only `clear_dependents()`. The correction points to a better ergonomic direction: the mutation should be a method on the handle (`CellHandle::set(&ctx, value)`) rather than on the container, mirroring how `SlotHandle::clear(&ctx)` already works. Updated phrasing: the integration gap would be connecting agent-doc's debounce/snapshot-hash signals to `CellHandle::set(&ctx, value)` calls at the appropriate cycle boundaries.
<!-- agent:boundary:a3b24b03 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue -->
<!-- /agent:queue -->

<!-- agent:backlog -->
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-05-31 [#pbc1] Commit-boundary reliability for Codex/IPC sessions: fail closed when fresh-start route trigger never fires, and auto-resume commit when write/snapshot succeeded but commit boundary was interrupted. Plan: `tasks/agent-doc/plans/commit-boundary-reliability-codex-ipc.md`
- 2026-05-31 [#dtmc] scan_text_source parallelized via std::thread::scope in diagnose_cycle
- 2026-05-31 [#editor-buffer-state-ffi-debounce] [recommended] Replace `wait_for_stable_content`'s last-added-line/truncation heuristic with editor-authoritative buffer stability when an IDE plugin is attached. JetBrains and VS Code plugins should report normalized editor state through the FFI bridge to the Session Actor: canonical doc path/session, monotonic editor document version, dirty flag, last edit timestamp, optional save timestamp, optional buffer hash, and ideally current buffer content. Run requests should include the editor version/hash observed at click time so the Session Actor can detect edits after invocation, debounce until the version/hash is stable, and compute the diff against the exact stable editor buffer when available. If no editor state exists, fall back to whole-file content + mtime stability polling. This removes reliance on `extract_last_added_line`, which misses edits above the last inserted line, and reduces disk/snapshot/editor-buffer race classes seen in live IPC/JB/VSCode flows.
<!-- /agent:done -->
