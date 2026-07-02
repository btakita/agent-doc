# Plan — Sidecars are backup, not hot-path authority (finalize authoritative on live-doc + capture ledger only)

**Status:** design captured 2026-07-02 (operator-directed), from dogfooding
`tasks/recruit/aboobakkar-abdul-nashir.md`. Two point-fixes already shipped
(`#fmdrop`, `#stale-already-applied`); this plan is the systemic follow-up.

## Origin

Dogfooding aboobakkar, the operator hit three distinct hot-path corruptions in
one session, then asked the questions that name the root pattern:

1. *"Why is the hotpath using snapshot? I thought the snapshot was only for
   crash recovery."*
2. *"If the snapshot is corrupted in the hotpath, why not just regenerate the
   snapshot in the background? The snapshot should never interfere with the
   hotpath."*
3. *"Is the sidecar another source of state that can disrupt the hot path?"*

The answer to (3) is **yes** — and it generalizes. The finalize/commit hot path
today *reconciles across* multiple out-of-band state sidecars, treating each as
**authority to merge/commit against**. Any one of them being stale, phantom, or
diverged can corrupt the committed document or wedge the cycle.

## The trap (what actually went wrong this session)

| Sidecar | Current hot-path role | How it corrupted/wedged aboobakkar |
|---|---|---|
| **snapshot** (`.agent-doc/snapshots/…`) | backup **and** the commit-staging image (`git::commit` stages the snapshot, not the working tree) | a `no_liveness_signals` synthetic auto-reap / stale-base CRDT merge produced a snapshot with 6 frontmatter keys dropped + emptied exchange; commit persisted it to HEAD, then preflight re-collapsed the snapshot to that corrupt HEAD every cycle → `suprecyclespin` `cycle_never_closed` (`#fmdrop`) |
| **ack-content sidecar** (`.agent-doc/ack-content/<id>.md`) | the `current` buffer the `already_applied` repair merges the response *into* | a socket `already_applied` ack from a stale/dead endpoint drove a merge into a phantom/oscillating buffer → scramble + infinite defer ("retry after typing stops" with nothing typing) → wedged `response_captured` cycle (`#stale-already-applied`) |
| **cycle-state** (`.agent-doc/state/cycles/<hash>.json`) | phase tracking gating session-check | a stuck `response_captured` cycle blocked a correct manual closeout until moved aside |
| **live-buffer digest** (editor epoch) | divergence detection | another surface that can report drift/in-flight incorrectly |
| **capture ledger** (`.agent-doc/captures/…`) | durable response body | **the one source that stayed correct and enabled recovery both times** |

The pattern: the hot path treats **backup/audit sidecars as merge/commit
authority**. That contradicts the contract already written in `AGENTS.md`
("Operator-visible document text is authoritative … Snapshots are backup/audit
state, not hot-path authority; fail closed or retry through the editor
instead") — the code has drifted from its own stated invariant.

## The invariant this plan enforces

**The finalize/commit hot path is authoritative on exactly two sources:**

1. **The operator-visible live document** (editor buffer / working-tree file) —
   for everything the operator can see and edit (frontmatter, prompts, queue,
   backlog, and — via the editor transport — the merged result).
2. **The durable capture ledger** — for the agent response body (already the
   `#response-replay` contract: "final parsed responses must be durably
   persisted before write/hook emission").

**Every other sidecar (snapshot, ack-content, cycle-state, live-buffer digest)
is regenerable backup.** It may *inform* a decision, but it must never *drive* a
merge that can corrupt the document, and a stale/divergent sidecar must **fail
safe** — regenerate from the authoritative pair, fall back to the editor/file
transport, or fail closed — never scramble or wedge.

Corollary (operator's Q2): when a sidecar is found corrupt/divergent on the hot
path, **regenerate it from the authoritative pair in the background** and
proceed; do not block or fail the closeout on sidecar reconciliation.

## Phases

- **Phase 0 — audit & instrument (no behavior change).** Enumerate every
  hot-path read of a sidecar in `git.rs::commit_with_outcome`, `write/ipc.rs`,
  `write/ipc/transport.rs`, `capture.rs`, `cycle_state.rs`. For each, classify:
  does it treat the sidecar as *authority* (drives a merge/commit/visible write)
  or *advisory* (informs a decision, has a fail-safe)? Emit a greppable
  `sidecar_authority_read source=… role=authority|advisory` op-log at each site.
  Deliverable: a table of authority-reads to eliminate.

- **Phase 1 — snapshot demotion (extends `#fmdrop`).** The commit-staging image
  is derived from **live-doc frontmatter + capture-ledger response body**, not a
  raw snapshot blob. The snapshot stays as a *recovery* artifact, regenerated
  after each commit. `#fmdrop`'s frontmatter overlay is the first slice; the full
  version stages the response from the capture ledger onto the live document's
  operator-visible state, so a corrupt snapshot can never reach HEAD.

- **Phase 2 — ack-content demotion (extends `#stale-already-applied`).** The
  `already_applied` repair merges into `current` **only** when `current`
  provably reflects a live editor buffer (idle + editor-liveness proven).
  Otherwise the ack is treated as unproven: regenerate from (live doc + capture)
  and route through the editor/file transport. Never merge a response into a
  sidecar-sourced `current` that cannot be proven live. (Note: editor-liveness is
  *not* `is_listener_active` alone — that is false even when a listener just
  acked; use idle-guard + ack provenance.)

- **Phase 3 — background regeneration.** A single `regenerate_sidecars_from_authority(doc)`
  that rebuilds snapshot + CRDT + cycle-state from (committed HEAD + live doc +
  capture ledger), invoked best-effort after any hot-path sidecar-divergence is
  detected (and by `reset --from-current`). Closeout never blocks on it.

- **Phase 4 — regression surface.** Property-style tests that inject a
  stale/divergent value into each sidecar and assert the closeout either
  self-heals or fails safe — never corrupts HEAD, never wedges the cycle, never
  scrambles the visible document. Seed cases: the three aboobakkar failures
  (`#fmdrop`, `#stale-already-applied`, stuck `response_captured`).

## Guardrails (do not regress)

- Selective commit still leaves operator working-tree edits uncommitted — the
  authoritative "commit image" is (last-answered live-doc state + this cycle's
  captured response), not the whole dirty working tree.
- Fail **open to the editor buffer**, never fail closed to a held lock or a
  stale-snapshot discard (same trap as [`plan-editor-sync-barrier.md`](plan-editor-sync-barrier.md)).
- Never write a session-document disk blob that drops operator-visible text to
  satisfy a sidecar.

## Related

- Shipped: `#fmdrop` (`commit_integrity.rs`), `#stale-already-applied`
  (`write/ipc.rs`), commit `3863dd99`.
- [`plan-editor-sync-barrier.md`](plan-editor-sync-barrier.md) — editor-buffer-as-truth + sync barrier (same north star).
- [`plan-ipc-corruption-and-duplicate-during-typing.md`](plan-ipc-corruption-and-duplicate-during-typing.md) — the `already_applied` origin.
- `AGENTS.md` — "Operator-visible document text is authoritative … snapshots are backup/audit state, not hot-path authority."
