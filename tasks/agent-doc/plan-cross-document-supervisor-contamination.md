# Plan: Cross-document supervisor contamination (`#xdocsuper`)

## Operator ask

> Please fix the cross document supervisor contamination. This was from the
> session that started off running corky.md, but somehow run agent-doc-bugs2.md.

Live repro this session (2026-06-25): one Claude Code pane (`%73`, session
`b26b9957`) ran `/agent-doc` on `equityfundingsource.md` → `corky.md` →
`agent-doc-bugs2.md` in sequence (SessionStart index `#S9003`/`#S9009`). The
route-owned supervisor started for the first document persisted as the pane's
parent and was reused for each later document.

## Root cause

- The **authoritative actor record** is keyed per *file*
  (`route/authoritative_actor.rs::load_authoritative_actor_binding(file, …)`) and
  has a robust **harness**-mismatch guard
  (`route_authoritative_actor_harness_mismatch_{stale,deferred}`) — but **no
  document-identity guard**.
- Under the 08b supervisor-in-process model, `start --route-owned` is **one
  supervisor process per pane**, hosting cycles in-process. When the same pane
  switches documents, the single supervisor keeps serving the new document while
  still holding the **prior document's per-document in-memory state** (CRDT
  overlay, queue model, drain lease, cycle journal).
- That stale in-memory state replayed against the new document is precisely the
  recurring churn class: `live_prompt_drift_after_preflight`, post-commit
  `postcommit_worktree_check match=false`, and answered-head residue re-injection
  (`feedback_parent_supervisor_stale_crdt_reset_nopreserve_then_restart`). The
  per-`reset --from-current` and `git checkout HEAD` recoveries we keep applying
  are treating the symptom; this is the structural source.

## The claim-guard gap (operator: "how was the supervisor even allowed to claim a document another supervisor claimed?")

The claim binding invariant is **pane/session-keyed, not document-supervisor-keyed**:

- `claim.rs::cross_session_decision(pane_session, configured, configured_alive, force)`
  returns `Accept` (pane session matches), **`AcceptStale` (configured/prior
  session not alive → auto-force)**, `AcceptForce` (`--force`), else `Reject`.
- Its invariant is "never commandeer another **document's** pane" (SPEC §8.5) —
  it has **no** "is another **live supervisor** already hosting this document?"
  check. When the harness relaunches the agent into a fresh pane and the prior
  owner pane/session is classified stale (the `● Three agent-doc panes exist …
  %73 is the stale prior owner` contamination block in this doc is the live
  trace), `AcceptStale` auto-grants the reclaim.
- A document-level `supervisor_lease` **does** exist
  (`project_controller.rs::load_supervisor_lease_from_db` /
  `supervisor_lease_is_fresh_or_alive`, reconciled in
  `reconcile_supervisor_leases_after_restart`), but **`claim.rs` never consults
  it**. So two supervisors (old + relaunched) can both believe they own the
  document across the relaunch window — the contamination.

**Re the operator's FFI↔JB IPC-state-machine hypothesis:** partially right. The
merge-control IPC SM (`merge_control_state_machine.rs`) governs *write-ownership*,
not *claim*, so it is not the direct trigger. But the same-era supervisor-in-
process (08b) migration is what lets a single supervisor process host multiple
documents in-process; combined with claim being pane/session-keyed (never
checking the supervisor lease), that is the contamination surface. The instinct
points at the right neighborhood; the exact hole is the claim guard.

## Fix approach (focused cycle)

0. **`#xdocsuper0` — claim consults the document supervisor lease.** Before
   `AcceptStale` (and on the route reclaim path), check
   `supervisor_lease_is_fresh_or_alive` for the document: if a FRESH lease is
   held by a live supervisor, `Reject` (or require `--force`) even when the prior
   pane/session looks stale. This closes the "two supervisors own one document"
   window directly.


1. **`#xdocsuper1` — document-switch detection + state-reset guard.** When a
   route-owned supervisor is asked to host/dispatch a document whose path-hash
   differs from the one it last hosted on that pane, drop that supervisor's stale
   per-document in-memory state (CRDT overlay, queue projection, drain lease,
   cycle cache) and reload from disk before the first cycle — i.e. treat a
   document switch like a fresh `reset --from-current` for the incoming document.
   Mirror the existing harness-mismatch guard's structure in
   `route/authoritative_actor.rs` + the supervisor host loop
   (`project_controller/rpc.rs`, `supervisor/`).
2. **`#xdocsuper2` — SimWorld coverage.** Deterministic scenario: a route-owned
   supervisor hosts doc A, then is handed doc B; assert B's first cycle reads B's
   on-disk state (no A overlay), and A's state is untouched. Add a contamination
   regression asserting no `live_prompt_drift` from a sibling document's stale
   overlay.
3. **`#xdocsuper3` — model the binding as state (optional).** Fold the
   pane→document host binding into the state backbone (`state_backbone.rs`) so a
   document switch is an explicit transition, not implicit reuse — same direction
   as `#queuestatemachine` / `#mergestatemachine` / `#waitmachine`.

## Why a fresh focused cycle (not inline)

This is supervisor-in-process-critical (`project_controller`, `route`,
`supervisor`) and needs new SimWorld infra. It must NOT be implemented in a
session that is itself a contamination victim (this `agent-doc-bugs2.md` session
on a degraded route-owned supervisor) — run it from a fresh session/pane on a
clean supervisor. Same rationale as `#agentreloadrestart` Phase 1b
(`feedback_deep_supervisor_refactor_fresh_focused_cycle`).

## Relationship to other work

The structural sibling of `#queuestatemachine` (per-item dup-impossibility),
`#mergestatemachine` (write-ownership), and `#waitmachine` (bounded waits): the
operator's "represent X as a state machine so the bad state is impossible"
direction. Here X is the pane→document supervisor host binding.
