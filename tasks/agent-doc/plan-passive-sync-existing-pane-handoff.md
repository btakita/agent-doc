# Plan — Passive sync must reliably hand off to an already-visible tmux pane

**Pending:** `#tmuxfocus`
**Status:** Draft

## Problem

`agent-doc sync --no-autostart` is meant to stay fail-safe when layout reconciliation would need to detach a protected pane or provision around ambiguous ownership. But that safety boundary must not also prevent focus from moving to a pane that is already visible and already registered to the requested document.

The recent repro in `tasks/agent-doc/agent-doc-bugs2.md` narrowed the issue:

- passive sync can hit a preserve-layout fast path because a requested file is missing while another visible pane is still protected by an open closeout cycle
- the selected document may still already have a visible pane in the kept layout
- if the preserve-layout exit returns before the explicit reselect step, tmux appears "stuck" on the old pane even though no replacement or provisioning should be needed

That means the handoff contract is separate from the layout-mutation guard. The regression surface is not "too many passive guards" in general; it is "preserve-layout exits can bypass visible-pane focus handoff."

## Desired Behavior

1. When the requested document already has a visible, alive, registered pane in the target window, passive sync must select it even if layout reconciliation is skipped.
2. Preserve-layout guards must only block attach/detach/provision/reconcile work, not already-visible focus handoff.
3. Plugin retries and dedup must treat a missed handoff separately from a successful sync so automatic replay continues until focus actually applies or a newer request supersedes it.
4. Logs must make it obvious whether passive sync preserved layout, reselected the visible focus pane, or skipped reselect because no visible owner matched.

## Implementation Steps

- [ ] Audit every `SafePassive` early return in `src/agent-doc/src/sync.rs` and ensure the visible-pane reselect helper runs before returning whenever `focus` is supplied.
- [ ] Tighten `select_visible_focus_pane_if_present(...)` call sites so protected-visible and blocked-files preserve-layout branches both log whether reselect succeeded, failed, or found no matching pane.
- [ ] Add tmux-backed regressions covering:
  - protected-pane preserve-layout with the requested focus pane already visible
  - blocked-file preserve-layout with the requested focus pane already visible
  - preserve-layout where the focus file is not visible, proving the command warns without selecting the wrong pane
- [ ] Audit JetBrains automatic replay/dedup expectations so a preserve-layout result without actual pane handoff is still treated as deferred rather than complete.
- [ ] Update the sync/editor specs to separate "preserve layout" from "reselect visible pane" as two explicit obligations of passive sync.

## Files Likely To Change

- `src/agent-doc/src/sync.rs`
- `src/agent-doc/specs/07-session-tmux-commands.md`
- `src/agent-doc/SPEC.md`
- `src/agent-doc/README.md`
- `src/agent-doc/editors/SPEC.md`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt`
- `src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/EditorTabSyncListenerTest.kt`

## Exit Criteria

- Passive `sync --no-autostart` preserves layout when required but still moves focus to an already-visible requested pane.
- Automatic editor sync does not report success when passive sync preserved layout but failed to hand off focus.
- Logs for preserve-layout turns show whether the requested pane was reselected, absent, or failed pane selection.
