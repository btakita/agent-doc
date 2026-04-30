# Plan — editor return rebind looks like a tmux pane crash

**Pending:** `#rbret`
**Status:** Draft

## Problem

Switching the editor back to an already-running document can look like "the tmux pane crashed" even when the session log shows no child crash in the bound pane.

Concrete repro from `src/boost-client/tasks/monsterrodholders.md` on April 30, 2026:

- `.agent-doc/logs/a9421282-fd96-4943-9af5-3561ed5cb799.log` records a healthy `codex_start` on pane `%529`, then later:
  - `session_superseded old_pane=%529 new_pane=%533`
  - `session_superseded old_pane=%533 new_pane=%536`
  - `session_superseded old_pane=%536 new_pane=%540`
- those same transitions also append `session_end origin=registry_rebind ...`, which means the registry moved the document session to replacement panes instead of recording a child crash
- the persisted startup-miss marker still pointed at pane `%529` after the document had already been rebound to later panes
- the JetBrains plugin fires `agent-doc sync ... --focus <file>` on Markdown tab selection changes, so returning to the document can trigger fresh layout/session work even though the previous owner was still logically recoverable

So the user-visible symptom is "the pane crashed when I switched back", but the actual failure shape is stale routed/startup-miss state plus selection-driven rebind churn.

## Desired Behavior

1. Returning to a document in the editor must not auto-create a fresh pane era when the existing session is still the canonical owner.
2. If the old pane/session really has been superseded, the stale startup-miss state must be cleared immediately so later returns do not keep reinterpreting the old pane as a current failure.
3. When a replacement pane is intentional, user-facing diagnostics must say "session rebound/replaced" rather than implying an in-pane crash.

## Implementation Steps

- [ ] Reconstruct the exact April 30 flow end-to-end:
  - routed startup-miss retained on `%529`
  - later editor selection change on `monsterrodholders.md`
  - fresh `agent-doc start` / registry rebind to `%533`, `%536`, `%540`
  - why the old ownership state was still considered actionable
- [ ] Audit the editor-selection path that runs on Markdown tab changes:
  - `EditorTabSyncListener.selectionChanged`
  - layout-sync debounce / lock behavior
  - whether `sync --focus` or follow-on route/start paths can start a new pane even when a live owner already exists
- [ ] Audit all rebind-entry paths in the binary:
  - `start.rs` existing-session reuse vs fresh start
  - `route.rs` startup-miss retention / restart / fresh-start logic
  - `sessions.rs` registry overwrite path
  - `startup_miss.rs` stale-marker supersession and clearing
- [ ] Define a single canonical rule for editor-return ownership:
  - if a document already has a live owner or a newer rebound owner, selection-change sync may focus/reuse it but must not spawn another fresh pane
  - if a startup-miss marker points at an older pane than the current registered owner, clear it before any new start/rebind decision
  - if the state is truly stranded, surface that explicitly and fail closed instead of churning through more pane eras
- [ ] Improve provenance for this family:
  - log the editor-triggered source of the reroute/start
  - include old pane, new pane, and stale-marker pane when a rebind happens
  - distinguish `registry_rebind`, `fresh_start_after_startup_miss`, and true child crash in surfaced diagnostics
- [ ] Add regression coverage for:
  - selection-change return with stale startup-miss on an older pane and a newer registered owner already present
  - selection-change return with a live owner where sync should focus/reuse instead of starting fresh
  - intentional rebind path preserving explicit `session_superseded` provenance without leaving a stale marker behind
  - stranded state still failing closed rather than looping through more rebinds

## Files Likely To Change

- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/start.rs`
- `src/agent-doc/src/startup_miss.rs`
- `src/agent-doc/src/sessions.rs`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt`
- `src/agent-doc/specs/07-commands.md`

## Exit Criteria

- Returning to `tasks/monsterrodholders.md` no longer produces fresh replacement-pane churn when a newer registered owner already exists
- stale startup-miss state for `%529` cannot survive after the document has been rebound to `%533`/`%536`/`%540`
- session logs and surfaced errors distinguish rebind/replacement from a real pane crash
