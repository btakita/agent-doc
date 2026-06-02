# Plan — Codex upgrade/restart loses focused session and surfaces a foreign document's pane

**Trigger:** `#agent-doc-bug` operator report on `tasks/agent-doc/agent-doc-bugs2.md` (2026-06-02)
**Related:** `#jb-tsift-pane-sync` (cross-document pane↔owner mismatch), `#rbret` (editor-return session rebind), `#bugs2tmux` (bugs2 tmux session crash/orphan), `#stash-pane-promote-on-focus`

## Problem

Operator report:

> I lost my existing session to this document. I saw Codex ask for an upgrade. I
> upgraded and restarted. Then I found out that the Codex session for
> `equityfundingsource.md` had tmux focus when I focused the editor on this
> document (`agent-doc-bugs2.md`).

Two intertwined failures:

1. **Session loss across a Codex supervisor upgrade/restart.** The live
   agent-doc/Codex session bound to `agent-doc-bugs2.md` did not survive the
   Codex upgrade + restart. The user perceived the session as lost rather than
   restored/rebound. This is adjacent to `#rbret`/`#bugs2tmux` but the trigger
   here is an explicit supervisor upgrade restart, not editor-tab churn.

2. **Cross-document tmux focus mis-binding after restart.** Once the editor was
   focused on `agent-doc-bugs2.md`, the pane that received tmux focus was the
   one running `equityfundingsource.md`. Editor-focus → pane-focus routing
   aliased onto a *different document's* owner pane. Same defect class as
   `#jb-tsift-pane-sync`'s "cross-document pane/owner mismatch", but observed
   immediately after a Codex restart re-enumerated panes/owners.

## Likely cause

- On Codex restart the session/pane registry is re-enumerated. If owner→document
  bindings are reconstructed from live pane cmdlines or a stale routed-state
  marker rather than from the authoritative per-document session record, a
  restart can leave the registry pointing the focused document at whichever pane
  Codex happened to bring up first (here, `equityfundingsource.md`).
- Editor focus emits `agent-doc sync ... --focus <file>`; if focus-sync resolves
  the target pane by "currently focused / first live agent-doc pane" instead of
  strictly keying on the focused document path, it surfaces/focuses the foreign
  pane.

## Desired behavior

1. A Codex supervisor upgrade/restart must rebind each document to its own
   authoritative session record (document path → session UUID → pane), not to a
   first-available or cmdline-inferred pane. If the pre-restart pane is gone, the
   focused document must re-provision its **own** pane, never alias onto another
   document's owner.
2. Editor focus-sync must resolve the target pane strictly by the focused
   document path. If no pane owns that exact document, fail closed (re-provision
   or report a precise blocker) instead of focusing a pane owning a different
   document.
3. If the focused document's prior session genuinely cannot be restored, surface
   that as session-loss with recovery (claim/restart) rather than silently
   handing focus to an unrelated document's session.

## Investigation steps

1. Reproduce: open two documents (`agent-doc-bugs2.md`, `equityfundingsource.md`)
   with live Codex panes, trigger a Codex upgrade/restart, then focus the editor
   on `agent-doc-bugs2.md`. Capture `agent-doc` route/sync logs and the
   per-document session logs across the restart boundary.
2. Confirm how owner→document bindings are reconstructed on restart in
   `src/tmux-router` (registry rebuild vs. authoritative session record) and how
   `sync --focus <file>` selects its target pane.
3. Determine whether the session-loss is real (no recoverable record) or a
   rebind/aliasing artifact (record exists but registry pointed elsewhere).

## Verification

- Deterministic SimWorld coverage for: restart re-enumeration must bind each
  document to its own session record; `sync --focus <doc>` must never focus a
  pane owning a different document (fail closed otherwise).
- Live-verify the two-document upgrade/restart repro once core fix lands.

## Notes

Coordinate with `#jb-tsift-pane-sync` — the focus-by-exact-document-path
invariant is shared. This plan adds the Codex-upgrade-restart re-enumeration and
session-loss facet on top of that invariant.
