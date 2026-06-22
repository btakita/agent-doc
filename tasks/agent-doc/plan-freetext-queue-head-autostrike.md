# Plan: free-text queue heads must auto-strike after a proven closeout (`#qheadstrike`)

**Status:** P1 + P3 shipped previously; **P2 implemented 2026-06-21** (agent-doc submodule `84a939f7`): the strike now runs at the commit seam (`commit_with_outcome`, before staging) sourcing the answered response from the durable capture ledger, so the `agent-doc commit` / `reset --from-current` → commit / auto-recovery paths converge on the same strike the finalize path does. `make check` green (4950); test `commit_seam_strikes_answered_free_text_head_from_capture`. Remaining: the LIVE recovery-path proof (operator-verify — needs the supervisor on the new binary), tracked as `#qheadstrikeverify`.

## Symptom (operator-reported 2026-06-19, equityfundingsource.md)

A single agent-doc cycle drained an operator queue of free-text heads (no `#id`):

```
- :pushpin: Remove LENDING_GROUP_EMAIL_NOTIFICATIONS_ENABLED gate. It should be on by default.
- Adding btak.dev+lender2@gmail.com had an error "D1_ERROR: UNIQUE constraint failed ...". Make the error friendlier. Add resend invite button.
- deploy
- Remove LENDING_GROUP_EMAIL_NOTIFICATIONS_ENABLED gate. It should be on by default. Send email to btak.dev+lender2@gmail.com. deploy
```

All four heads were fully completed + deployed and the cycle reached a **proven closeout** (cycle `committed`, `session-check` ok), yet **none of the heads were struck**. Post-commit `preflight` still reported `queue_drainable_head_count: 4` and `queue_continuation_required: true`, so the loop would churn no-op `#qchurn` cycles and the operator had to clear the heads by hand. Operator: "The free text heads should have been struck."

## Root causes (three, compounding)

1. **One-head-per-finalize strike heuristic.** `main.rs` `QueueAction::Consume` documents that "the strike heuristic only consumes one head per finalize." A cycle that legitimately answers *multiple* free-text heads leaves the stragglers unstruck unless the operator/agent runs `agent-doc queue consume --count N` manually. There is no automatic "strike every head this response answered."

2. **Recovery/commit paths bypass the strike entirely.** The normal free-text strike runs inside the IPC `finalize` path. When `finalize` cannot prove the editor write (wedged stale-binary supervisor → `recovery=retry_without_disk_write`) and the cycle is closed out via the documented fallback (`finalize --force-disk`, or `reset --from-current --preserve-session` → `agent-doc commit`), the strike step does not run at all, so **zero** heads are struck even though the response was committed. The strike is coupled to the happy-path finalize instead of to "a response cycle reached `committed`."

3. **Exact head-matching is brittle to live-buffer rendering drift.** `agent-doc queue consume` failed in this session with `queue consume: snapshot head prompts [...] do not match document head prompts [...]` because the live editor buffer had mutated the head between preflight and closeout: it rendered a `:pushpin:` (📌) prefix and split one operator prompt into several items. Exact snapshot-vs-document head equality rejects the strike instead of normalizing (emoji/pushpin prefix, leading/trailing whitespace, one-prompt-split-into-N) and striking the matched answered heads.

## Fix direction (to design + implement in a focused cycle)

- **P1 — strike all answered free-text heads, not one.** At closeout, correlate the heads answered by this cycle's response(s) against the leading free-text heads and strike *all* of them. Source the "answered set" from durable cycle state: capture the addressed free-text head identities (normalized text + queue `node_key`) at `preflight`/`plan` time and persist them in the cycle capture, so the strike is driven by what the cycle actually addressed rather than a positional one-shot guess.

- **P2 — make the strike a property of `committed`, not of the IPC finalize path.** Run the free-text head reconciliation at the point the cycle reaches `committed` (so `finalize`, `finalize --force-disk`, and the `reset --from-current` → `commit` recovery path all converge on the same strike). `git.rs`/`commit` and `cycle_state.rs` are the natural seam: when a captured cycle is marked committed and it recorded answered free-text heads, strike them as part of the same commit (or a deterministic follow-up the recovery path also triggers).

- **P3 — normalize head matching.** Match answered heads against document heads modulo: leading status glyph/emoji (`:pushpin:`/📌), surrounding whitespace, and one-operator-prompt-rendered-as-N-items (split). Prefer queue `node_key` correlation where available; fall back to normalized-text match. Never reject the whole strike because of a cosmetic render delta; strike the matched subset and log the unmatched remainder.

- **P4 — id-backed heads stay on `--done`.** Keep the existing rule that id-backed (`do #id` / `[#id]`) heads are reaped via `--done`/backlog `agent:done`, not by the free-text strike. This change is scoped to free-text heads only.

## Test coverage (SimWorld, deterministic — no live tmux)

- Multi-head answer: a cycle that answers 3 leading free-text heads strikes all 3; `queue_drainable_head_count` → 0; `queue_continuation_required` → false.
- Recovery-path parity: a cycle closed out via `reset --from-current` → `commit` (and via `--force-disk`) strikes the answered free-text heads identically to the IPC finalize path.
- Render-drift tolerance: a head that gained a `:pushpin:` prefix and/or was split into multiple items between preflight and closeout is still matched and struck (no "do not match document head prompts" rejection).
- Negative: an id-backed head and an *unanswered* free-text head are preserved (not over-struck).

## Acceptance

`make check` + `make tmux-ci` green; new SimWorld cases above pass; on the repro shape a proven closeout leaves `queue_drainable_head_count: 0` with no manual `queue consume`. This is merge/commit/queue-core behavior (the strike moves onto the commit seam) — **not** safe to rush inside an in-session loop; needs a focused operator-driven cycle.

## Implementation seam (located 2026-06-21, agent-doc-bugs2 churn repro)

P1+P3 are **already shipped**: `strike_answered_free_text_queue_heads(file, response_body, force_disk)` in `agent-doc-orchestration/src/write/queue_consume.rs:872` already strikes *all* answered free-text heads regardless of position, is pushpin/fence/baseline-aware (`free_text_head_match_prose` strips code fences; `free_text_head_present_in_baseline` ignores pin/dash cosmetics), and has end-to-end tests (`strike_answered_free_text_heads_strikes_behind_id_head_end_to_end`, queue_consume.rs:3224).

**The remaining gap is purely P2.** The strike is called from exactly one site — `write.rs:2139`, inside the finalize/`run_write` path, gated on `commit_mode != CommitMode::None` and given the in-memory `response_body`. When `finalize` hits `live_prompt_drift_after_preflight` and the cycle is closed out through the **standalone `agent-doc commit` recovery path** (`git::commit` → `commit_with_outcome`, `git.rs:586`), the strike never runs — `commit_with_outcome` has no `response_body` and no strike call. That is exactly why the agent-doc-bugs2 "JB Run Agent Doc stalled without feedback" head stayed unstruck after its answer committed (cycle reached `committed` via `agent-doc commit`, but the strike lives only on the finalize path → phantom-pin churn).

Focused-cycle change:
1. In `commit_with_outcome` (git.rs), at the **post-commit success** point (after the new commit lands, before VCS-refresh signalling), load the freshly-committed cycle's captured `response_body` via `capture::load_by_id` / latest capture for the doc-hash (the capture ledger already persists `response_body`).
2. Call `strike_answered_free_text_queue_heads(file, &response_body, force_disk=false)` there, best-effort (a missed strike must never fail an otherwise-clean commit — mirror the `Ok(0)/Ok(n)/Err` handling at write.rs:2139).
3. Reuse `converge_document_or_disk(.., "free_text_strike")` (queue_consume.rs:924) so the strike write goes through the same IPC/disk path. NOTE: when the live editor IPC is wedged (`ack_mismatch`), this converge can still fail — that is the **operator-verify** portion; the deterministic test below covers the recovery-path wiring itself.
4. SimWorld test: a captured cycle whose `finalize` was driven into `live_prompt_drift` and closed via `agent-doc commit` strikes the answered free-text head identically to the finalize path (the "Recovery-path parity" case above), assertable without live tmux/IPC.

**Precondition for the focused cycle:** run it on a HEALTHY supervisor/editor (recycle first to clear any `ack_mismatch` IPC wedge), so the live strike-write is actually provable rather than blocked.
