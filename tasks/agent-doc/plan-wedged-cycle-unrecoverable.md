# Plan — Wedged Cycle Unrecoverable Without Destructive Compact

**Status:** plan only — DO NOT IMPLEMENT
**Filed:** 2026-05-19
**Source:** incident on `src/session-share/tasks/planning-agent-pipeline.md`

## Symptom

During a `#spec-test-news-commit-push` turn, the agent-doc cycle wedged and could
not be closed by any documented recovery command. With every recovery path
failing, the agent ran `agent-doc compact --commit` as the only apparent escape
— which the user had **not** requested and which destructively rewrote the
session document (537 exchange lines archived). The document had to be restored
by a manual `git revert` of the compact commit.

## Incident chain

1. `agent-doc finalize` failed its pre-write `pending_capture_guard` gate — the
   prompt's imperative verbs ("Please update", "Move") tripped the heuristic and
   no `--pending-add` had been supplied. The finalize aborted **before** writing.
2. The failed finalize nonetheless left cycle `cycle-1779200843460` in a state
   treated as `committed`. Re-running `finalize` (now with `--pending-add`) and
   `agent-doc write --commit` both rejected the response patch:
   `rejecting late fallback patch: cycle cycle-1779200843460 already committed`.
3. `agent-doc repair` found no orphaned response and exited on
   `session-check INTERRUPTED` — it did not re-open the cycle or let the
   response land.
4. `agent-doc reset` cleared the session, but the next `agent-doc preflight`
   failed during snapshot init:
   `refusing to auto-adopt committed historical response patchback … ### Re: Step 1 PRD … (bare prompt target missing ❯: #### #next-steps)`
   — a parser false-positive on a legitimate h4 response sub-heading.
5. `agent-doc write --commit` after the reset hit the same parser refusal plus
   the same `cycle already committed`.
6. Net: every documented recovery (`finalize`, `write --commit`, `repair`,
   `reset`) failed; `compact` was the only command that proceeded — and it is
   destructive.

## Root-cause hypotheses (architectural, not tactical)

Consistent with the standing concern in `agent-doc-bugs2.md` — recurring bugs
that tactical patches do not fix — this incident points at architectural defects
in cycle-state management:

- **A. No single authoritative cycle-state model.** A *failed* `finalize`
  (blocked at the pre-write gate, before any write) still left the cycle marked
  `committed`. Cycle state is mutated across multiple commands/paths with no
  single owner, so a partial/aborted finalize corrupts it. Cycle state needs one
  authoritative state machine with explicit, success-only transitions; a
  finalize that fails its pre-write gate must leave the cycle exactly as it
  found it (open).
- **B. Recovery commands do not converge.** `repair` and `reset` are meant to
  recover a stuck cycle; here neither could. A wedged cycle with an unanswered
  prompt and no response should always be recoverable — by exactly one
  documented command — to "open cycle, prompt awaiting response."
- **C. Response-heading parser false-positive.** `#### #next-steps` — an h4
  sub-heading inside a committed response, whose text begins with `#` — is
  classified as a "bare prompt target" / "out-of-band typed_component_drift."
  Headings at h4+ inside a response body are never prompt targets; the detector
  must not inspect them.
- **D. Compaction is the de-facto escape hatch.** Because A–C leave a wedged
  cycle otherwise unrecoverable, `compact` becomes the only way out — and
  `compact --commit` archives/rewrites the document with no in-tool undo
  (recovery is git-level only). A stuck cycle must never force a destructive
  operation, and `compact` should be opt-in, never a wedge-escape.

## Proposed direction (do not implement yet)

1. Consolidate cycle-state transitions into one module with an explicit state
   machine; every transition is success-gated; a failed pre-write gate is a
   no-op on cycle state.
2. Make `repair` the single guaranteed recovery: from any wedged state, restore
   "open cycle, prompt present, no response" so a normal `finalize` can run.
3. Fix the prompt-target / out-of-band-patchback detector to ignore h4+ headings
   inside committed response bodies; re-evaluate the `❯`-prefix heuristic.
4. Give `compact` a first-class in-tool reversal (`pre-compact-*` tags already
   exist) so compaction is never an irreversible-by-the-tool operation.
5. Add deterministic / SimWorld regression coverage for: failed-finalize-then-retry,
   wedged-cycle `repair`, and the `#### #next-steps` heading class.

## Scope

Plan only. **DO NOT IMPLEMENT.** Tracked as a backlog item in
`tasks/agent-doc/agent-doc-bugs2.md`.
