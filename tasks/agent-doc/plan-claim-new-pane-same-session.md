# Plan: #claimnewpane — Claim a new document into a fresh pane in the same session

## Problem (reported 2026-07-01 in agent-doc-bugs2.md)

When the operator runs JB **Claim for Tmux Pane** on a *new* document while another
pane is already open, there is no way to bring the new document up in its own pane
inside the **same** tmux session. Observed:

- Claim binds the new document to an **existing** pane, so it reads as "Replace the
  current tmux pane" rather than opening a new one.
- The operator reached for a **Move to another Session** action to get a second
  surface; that moved the tmux *window* out to a different session, and there is no
  built-in way to move it back to the original session.

Desired: **Claim should automatically create a new pane for a new document in the
original session** — never replace the current pane, never push the window to
another session.

## Root cause

`ClaimAction.kt` (editors/jetbrains) runs `agent-doc claim <rel> --position <left|right>`
then `SyncLayoutAction.syncLayout(...)`. `claim` **binds** the document to a pane it
infers from editor layout position, falling back to the last active pane. Neither
`claim` nor the follow-on sync has a "this is a new document with no home pane yet →
split a fresh pane in the authoritative session and bind to it" branch. Pane/window
creation primitives already exist in the `tmux-router` sibling crate
(`split_window`, `new_window`, `auto_start` — `auto_start` already creates a new
*window* in an existing session), but the claim path never calls them to allocate a
*new pane* for an unclaimed second document; it only reuses/repositions existing panes.

The cross-session reject dialog (`promptCrossSessionChoice`) offers only **Force
Claim / Switch Project Session / Cancel** — there is no "open in a new pane in this
session" choice, which is the option the operator actually wanted.

## Fix direction

1. **Binary-first (FFI / `claim.rs` + `tmux-router`):** add a claim mode that, when a
   document has no bound pane and the authoritative session already has an occupied
   agent-doc pane, allocates a **new pane in the same session/window** (via
   `tmux-router` `split_window`, respecting the pane-group layout in
   `specs/pane-groups.md`) and binds the new document to it — instead of repositioning
   onto an existing pane. Keep the decision (split vs. reuse vs. cross-session) in the
   binary; the JB plugin stays a thin caller (Shared Foundation pattern).
2. **New-pane as the default for a distinct new document:** claiming a document that
   is not already registered to a pane should default to a fresh pane in the current
   session, so the common case never replaces the operator's current pane.
3. **Add a "New pane in this session" recovery choice** to the claim dialog surface
   for the ambiguous/cross-session case, mapping to the same binary claim-into-new-pane
   path rather than `session set` (move) or force-claim (replace).
4. **Round-trip back to the original session:** provide a supported way to move a
   window/pane that was pushed to another session back to the project's authoritative
   session (reuse the existing `sync`/`session set` migration path rather than relying
   on the raw tmux "Move to another Session" menu).

## Acceptance

- [ ] Claiming a second, previously-unclaimed document opens it in a **new pane in the
      same session** — the operator's current pane is untouched and the window stays in
      the original session.
- [ ] The claim/cross-session dialog exposes an explicit "new pane in this session"
      option; choosing it never runs `session set` (move) or `--force` (replace).
- [ ] A window pushed to another session can be returned to the authoritative session
      through a supported agent-doc command (no manual tmux menu).
- [ ] Decision logic lives in `claim.rs` / `tmux-router`, not in `ClaimAction.kt`;
      the plugin only reports the file + invokes the binary.
- [ ] SimWorld/tmux-router coverage: claim-new-doc-with-occupied-session splits exactly
      one new pane in the same session and binds it; no window migration.
- [ ] `make check` green; `make tmux-ci` green for the new pane-allocation path.

## Notes

- Keep `sync.rs`, `README.md`, `SPEC.md`, `specs/pane-groups.md`,
  `specs/07-session-tmux-commands.md`, and the installed harness surfaces aligned with
  any new claim pane-allocation behavior.
- Relevant existing invariants: fresh route panes stay authoritative; passive sync must
  preserve visible layout; `0:agent-doc` + `1:stash` window shape.
