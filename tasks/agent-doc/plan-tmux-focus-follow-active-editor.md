# Plan: Focused tmux pane should follow the active editor document

`#tmuxfocusfollow` · `#agent-doc-feature`

## Symptom

Switching which agent-doc-managed document is focused in the editor (JetBrains,
VS Code) does not sync the focused tmux pane. The operator selects a different
document's editor tab, but the tmux layout keeps the previously-focused pane
active, so the agent pane they see does not match the document they are looking at.

## Desired behavior

When the active editor selection changes to a document that owns a route-associated
tmux pane, the focused/selected tmux pane should follow to that document's owned pane
(within the same tmux session — never move windows across sessions, cf. `#claimnewpane`).
If the newly-focused document has no owned pane, do nothing (fail-closed, no reparenting).

## Approach

1. **Editor selection signal.**
   - JB: `FileEditorManagerListener.selectionChanged` (plugin) → emit a route focus
     signal for the newly-selected document path.
   - VS Code parity: `window.onDidChangeActiveTextEditor` → same signal.
   Debounce rapid tab switches.
2. **Route focus resolution (Rust, `agent-doc-route-io` / tmux integration).**
   - Resolve the selected document's owned pane from the existing route/pane
     association (the same lookup route dispatch uses).
   - If a live owned pane exists, `tmux select-pane`/`select-window` to it in the
     current session. If not, no-op.
3. **Fail-closed guards.**
   - Only act on documents agent-doc actually manages/owns a pane for.
   - Never create, kill, or move panes here — focus only. Layout mutation stays in
     Sync Tmux Layout (`#tmuxsynccrashlog`).

## Verification

- Rust unit/SimWorld: given a doc→pane association map and a selection-change signal,
  the correct `select-pane` target is chosen; missing-association → no-op.
- `make check` / `make tmux-ci` green.
- Live proof `[operator-verify]`: open two agent-doc docs each owning a pane; switching
  editor tabs refocuses the matching tmux pane; a doc with no pane leaves focus unchanged.

## Distinct from

- `#tmuxsynccrashlog` — Sync Tmux Layout crash/repro (layout mutation), not focus-follow.
- `#claimnewpane` — opening a *new* document into a *fresh* pane in the same session,
  not refocusing an existing owned pane on selection change.
