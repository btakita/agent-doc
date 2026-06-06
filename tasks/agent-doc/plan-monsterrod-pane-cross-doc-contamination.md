# Plan: #monsterrod-pane-cross-doc-contamination

## Symptom

JB `Run Agent Doc` on `monsterrodholders.md` stalls / refuses while the operator
believes the Codex session is idle. Live evidence (2026-06-05):

- `admin detect` (boost-client project) flags `cross_document_pane`: **pane %1 is
  the live binding of 2 documents**.
- `admin list` shows pane %1 bound to BOTH:
  - `src/boost-client/tasks/astro-listings.md` `[claude-code]` gen=7 (STALE —
    recorded harness is claude-code, but pane %1 actually runs codex pid 31888)
  - `src/boost-client/tasks/monsterrodholders.md` `[codex]` gen=282 (live owner)
- `resync` confirms monsterrodholders legitimately owns pane %1 (alive child),
  and %1 is the only boost-client Codex pane — there is no separate idle
  monsterrodholders Codex session. Pane %1 was measured genuinely busy (a
  progressing Codex turn), so route's busy refusal is technically correct.

The stale `astro-listings [claude-code]` binding on a codex pane is registry
contamination that makes "which session owns/serves this pane" ambiguous and is
the kind of cross-binding that produces wrong-pane routing and operator
confusion about idle vs busy.

## Root cause

Two documents hold a live binding to the same pane. A binding can be recorded
with a harness (`claude-code`) that does not match the pane's live process
(`codex`), so the stale binding is never reaped by liveness checks alone (the
pane *is* alive — just owned by the other document/harness).

## Fix

1. **Fail-closed cross-doc binding guard** in `route.rs` / `claim.rs`: refuse to
   bind a second document to a pane that already has a live binding to a
   different document. The newcomer must get its own pane (or fail closed with an
   explicit "pane already owned by <doc>" diagnostic) rather than co-binding.
2. **Harness-mismatch binding reap** in `resync --fix` / `admin`: a binding whose
   recorded harness (e.g. `claude-code`) does not match the live pane process
   (e.g. `codex`) is stale even though the pane is alive — clear it.
3. **`admin detect` → actionable cleanup**: the existing `cross_document_pane`
   finding should map to a one-shot remediation (drop the non-owning binding).
   Likely depends on the gated `#ipc-admin-api` Phase 2 mutating verbs
   (`rebind` / `stop`).

## Tests

- Unit: cross-doc binding guard rejects a 2nd-document bind to a pane already
  owning another document.
- Unit: resync classifies a harness-mismatch-on-live-pane binding as stale.
- SimWorld: route against a pane cross-bound to another document fails closed
  with the cross-doc diagnostic instead of attributing the other doc's turn.

## Related

- Shipped this session: `#jb-run-agent-doc-busy-active-turn-stall` (b93b3c3c) —
  skip the 60s busy ready-wait on a proven active turn so the refusal is snappy.
  Orthogonal to the contamination but discovered in the same investigation.
- `#jb-busy-reopen-auto-drain-when-idle` — auto-queue a deferred reopen so a busy
  pane drains the reopen when idle without re-clicking.
- `#ipc-admin-api` Phase 2 (gated) — mutating rebind/stop verbs needed for the
  automated cleanup path.
