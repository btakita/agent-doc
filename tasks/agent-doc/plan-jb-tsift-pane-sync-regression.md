# Plan: JetBrains navigation and Sync Tmux Layout do not surface tsift pane

**Pending:** `#jb-tsift-pane-sync`
**Trigger:** `#agent-doc-bug` report from `tasks/software/tsift.md`
**Related:** `#autosync`, `#tmuxfocus`, `#stash-pane-promote-on-focus`, `#jb-load-tmux-window-verify`

## Problem

When JetBrains navigated to `tasks/software/tsift.md`, the corresponding
agent-doc tmux pane did not appear immediately. Running the manual JetBrains
`Sync Tmux Layout` action also did not bring that pane into the visible
`agent-doc` tmux window.

This is stronger than the older automatic-navigation latency class because the
explicit manual repair action failed as well. The expected operator model is:

- normal editor selection should make an existing pane visible or focused
  quickly
- if automatic selection cannot safely mutate layout, the manual sync action
  should either make the target pane visible or return a precise deferred /
  blocked reason
- a visible failure must name whether the pane was absent, stashed, protected by
  an open closeout, blocked by the local sync guard, or rejected by ownership
  proof

### New symptom (2026-06-01): cross-document pane/owner mismatch

Operator report while switching from `agent-doc-bugs2.md` to
`tasks/software/tsift.md`: **the pane surfaced for `tsift.md` was actually
running `agent-doc-bugs2.md`** ("the tsift.md session was running this
document"). The bugs2 session pane was the one present in the tmux layout when
the editor switched to `tsift.md`, and a manual JB `Sync Tmux Layout` appears to
have left agent-doc in an inconsistent pane↔document binding state rather than
either surfacing a correct `tsift.md` owner or cleanly autostarting one.

This is a different (and worse) failure mode than "the target pane is not
visible": here a pane *is* surfaced/focused for the navigated file, but it is
bound to the **wrong document**. Likely culprits:

- focus/sync selected or reused the currently-visible pane (the bugs2 owner) for
  `tsift.md` instead of resolving `tsift.md`'s own registered owner;
- a navigation/sync autostart attached a new `tsift.md` session onto a pane that
  was already a live owner of `agent-doc-bugs2.md`, aliasing two documents onto
  one pane (sibling of the `cmdline_owns_other_document` claim-guard class,
  `#j0k8`);
- `Sync Tmux Layout` reconciled layout by position/visibility without
  re-validating each surfaced pane's owned-file against the editor's intended
  file, so the stale bugs2 owner was promoted as the "tsift" slot.

Operator ask, restated as contract: navigating to a document, and running manual
`Sync Tmux Layout`, must **cleanly autostart or surface the owner for that exact
document** — never bind/reuse/alias a pane that owns a different document. If the
correct owner cannot be surfaced safely, fail closed with the specific blocker
instead of presenting a wrong-document pane.

### New symptom (2026-06-01): navigation lands at 3 panes for a 2-column editor

Operator report: navigating from `agent-doc-bugs2.md` to `tsift.md` left the
`agent-doc` tmux window with **3 panes** while the editor only had a **2-column
(2-editor-pane) layout**. **Logging is sufficient** and already captures the
cause — no new instrumentation needed beyond the `[layout-detect]` editor-side
line added in JB `0.2.146` (and the editor side turned out to be correct here).

Evidence from `/tmp/agent-doc-sync.log`:

- A clean transition uses the reconciler's atomic SWAP and lands at 2 panes:
  - `columns=[tsift.md, monsterrodholders.md]` (editor genuinely had 2 columns)
  - `pre-reconcile panes: ["%57", "%61"]` → `SWAP: %62 ↔ %57 (atomic)` →
    `post-reconcile panes: ["%62", "%61"]` (2 panes) ✓
- The failing transition rescues a stashed pane via the **focus path** and ends
  at 3 panes:
  - `pre-reconcile panes: ["%62", "%61"]`, `desired=["%57", "%61"]`
  - `promote_on_focus_action=join-pane src=%57 dst=%61` /
    `promote_on_focus_result=join-pane ok=true` (rescues %57 into the window)
  - `post-reconcile panes: ["%57", "%61", "%62"]` (3 panes) ✗ — the displaced
    previously-visible pane (`%62`) was never stashed.

Root cause: `promote_pane_to_agent_doc_window` (`sync.rs`, called from the focus
path `focus.rs:147`, `#stash-pane-promote-on-focus`) reparents the focused
file's stashed live-owner pane into the `agent-doc` window with `join-pane`,
**adding** a pane. On a 1-in/1-out tab switch this races/duplicates the sync
reconciler's job: the focus-path rescue runs independently of (and concurrently
with) the debounced `sync --no-autostart --exact-visible` reconcile, so the
reconcile operates on a stale pre-reconcile snapshot and does not stash the
displaced pane. This is exactly the "sync rescues a stashed pane before
reconcile" anti-pattern the reconciler's SWAP fast path (`sync.rs` doc comment)
was meant to eliminate — the SWAP path lands at 2 panes, the focus-path rescue
lands at 3.

Contract: a 1-in/1-out navigation must converge to one pane per editor column.
The focus-path promote-on-focus rescue must either go through the same atomic
SWAP as the reconciler, or defer pane-reparenting to the reconciler entirely so
exactly one path owns the in/out transition; the displaced pane must always be
stashed in the same operation that surfaces the incoming pane.

## Working Theory

The likely failure is at the boundary between JetBrains request scheduling and
the binary sync/focus handoff:

1. Automatic navigation issues an immediate `agent-doc focus <file>` plus a
   guarded `agent-doc sync --no-autostart --exact-visible ...`.
2. A stashed or hidden but live owner may require focus promotion or safe-passive
   attach behavior that the fast path does not complete.
3. Manual `Sync Tmux Layout` shares the same local FFI sync guard as background
   autosync. If a background sync is still running or retrying, the manual action
   can skip instead of becoming a foreground "latest request wins" repair.
4. The operator then sees no pane movement and may receive no actionable
   diagnostic distinguishing "deferred" from "not attempted".

There is also documentation/spec drift to settle: some editor docs describe
navigation as passive and non-autostart, while newer sync behavior allows safe
cold-start/reuse on the passive path. The live behavior and operator-facing
actions need one clear contract.

## Implementation Plan

### Phase 1 - capture the failing command path

- Add a live trace recipe for this exact repro:
  - selected editor file: `tasks/software/tsift.md`
  - visible markdown files and detected editor layout columns
  - immediate focus command, automatic sync command, and manual sync command
  - working directory chosen by JetBrains
  - sync guard state and generation
  - tmux window/pane state before and after each command
  - relevant `.agent-doc/logs/ops.log` sync/focus lines
- Make the JetBrains logs include whether manual sync skipped due to the local
  sync guard, completed with preserved-layout output, timed out, or applied focus.

### Phase 2 - make manual Sync Tmux Layout authoritative

- Treat manual `Sync Tmux Layout` as a foreground repair request:
  - it must supersede queued automatic sync requests for the same project
  - if another sync is running, either wait briefly and replay the manual request
    as the latest generation or surface "still syncing" with the target file
  - it must not silently drop the operator's explicit repair attempt
- Ensure manual sync always reports the post-condition for the focused file:
  focused/visible, deferred with protected pane details, missing owner, ownership
  conflict, or command failure.

### Phase 3 - close the focus/sync handoff gap

- Audit the `agent-doc focus` fast path for `tsift.md`:
  - if the live owner is in `stash`, promotion to `agent-doc` must be attempted
  - if registry points at a stale pane but a newer live owner exists, focus must
    select the newer owner or defer with proof
- Audit `sync --no-autostart --exact-visible` and full `sync`:
  - a hidden/stashed target pane must be attached when safe
  - preserve-layout exits must still reselect an already-visible target pane
  - exact-visible dedup must not mark a request applied unless the focused pane
    is actually active

### Phase 3b - cross-document owner guard on focus/sync/autostart

Targets the 2026-06-01 symptom (wrong document surfaced in the navigated pane).

- Before focus/sync surfaces or reuses a pane for file `F`, validate the pane's
  owned-file equals `F`. A pane that owns a different live document must never be
  presented as `F`'s pane — reuse `cmdline_owns_other_document`-style ownership
  checks (`focus.rs`, `sync.rs`, `route.rs`) so the guard is shared with claim
  (`#j0k8`).
- Define the autostart-on-navigation contract: when navigation/sync finds no
  live owner for `F`, autostart must provision a **new** owner pane for `F`
  (never alias onto another document's owner). When a stale/hidden/stashed owner
  for `F` exists, surface/promote that owner instead of cold-starting a duplicate.
- `Sync Tmux Layout` must reconcile by owned-file identity, not by tmux position
  or visibility alone: each surfaced slot's pane owned-file must match the editor
  file it is mapped to, or the slot fails closed with `wrong_owner` proof.
- Emit a precise diagnostic when the guard trips: `focused file=F surfaced
  pane=%N owns=G (mismatch)` so JB logs name the contaminating document.

### Phase 3c - execution-context contamination guard (run/write/stream/preflight)

Targets the 2026-06-02 confirmation that Phase 3b's *display* guard is not
enough. The binary logs `cross_document_execution_context` (origin=run/write/
stream, `current_pane=%N pane_owns=<other document>`) proving that agent-doc
cycles for document `F` physically EXECUTE inside a tmux pane owned by a
different live document: `agent-doc-bugs2.md` cycles ran inside `lazily-rs.md`'s
`%71`/`%73`, and separately inside `monsterrodholders.md`'s pane. Phase 3b
guards which pane is *surfaced/focused*; this phase guards where a cycle *runs*.

- At the `run` / `write` / `stream` / `preflight` entry, when the executing tmux
  pane's registered owned-file differs from the cycle's target document, the
  binary must not silently proceed. Promote the existing best-effort
  `cross_document_execution_context` LOG into an actionable guard: either
  re-home the cycle to the target document's own owner pane, or fail closed with
  `executing_pane_owns_other_document file=F pane=%N owns=G` plus the recovery
  command — never commit a cycle for `F` from inside `G`'s pane.
- Caching constraint for the not-yet-implemented lazily-rs `#lr-actor-10`
  (GlobalConfigSlot + SessionRegistrySlot on the long-lived ActorContext): a
  cached session registry MUST invalidate on registry mtime/epoch change.
  `RunContext` is per-invocation (per-document) today and is NOT the current
  cause, but a process-global SessionRegistrySlot without invalidation would
  cache stale pane↔document bindings and could *introduce* this contamination
  class — so that slot must ship with an explicit `invalidate` hook keyed to
  registry change, mirroring the existing `RunContext::invalidate_*` pattern.

### Phase 4 - regression coverage and specs

- Add JetBrains tests for:
  - manual sync while an automatic sync is already running
  - manual sync replaying the latest selected file after guard contention
  - preserved-layout output remaining deferred unless focus proof is present
- Add Rust sync/focus regressions for:
  - focused `tsift.md` pane hidden in stash
  - focused pane visible but not active after preserve-layout
  - manual full sync attaching a hidden target pane with protected unrelated panes
  - cross-document guard: focus/sync for `F` must NOT surface a pane that owns a
    different live document (`agent-doc-bugs2.md`); it must surface `F`'s own
    owner, autostart a new owner, or fail closed with `wrong_owner` proof
  - navigation autostart with no live owner provisions a new pane (never aliases
    onto another document's owner pane)
- Align `editors/SPEC.md`, `editors/jetbrains/SPEC.md`, and
  `specs/07-session-tmux-commands.md` on the final contract for navigation,
  Load Tmux Window, passive sync, and manual Sync Tmux Layout.

## Acceptance Criteria

- Navigating to `tasks/software/tsift.md` focuses or surfaces an existing live
  pane without requiring manual tmux navigation.
- Manual JetBrains `Sync Tmux Layout` either makes the `tsift.md` pane visible
  or reports one specific actionable blocker.
- A manual sync request cannot be lost behind a background autosync guard.
- Logs identify the exact command path and post-condition for the focused file.
- Navigating to `F` or running manual `Sync Tmux Layout` never surfaces a pane
  bound to a different document; the navigated pane always owns `F`, autostarts a
  new owner for `F`, or fails closed naming the contaminating document.
- An agent-doc `run`/`write`/`stream`/`preflight` cycle for `F` never commits
  from inside a tmux pane owned by a different live document; it re-homes to
  `F`'s owner or fails closed with `executing_pane_owns_other_document` proof.
  This closes the execution-context facet of `#jb-tsift-pane-sync`, not just the
  display/surfacing facet.
