# Plan — JB Clear Exchange clears the exchange but content comes back from stale HEAD

Bug id: `#clearexchstale`
Tag: `[clean-session]` (live write path / disk-vs-buffer convergence; must not run under the live route-owned supervisor)

## Operator-stated symptom (2026-06-13)

JB `Clear Exchange` cleared the exchange, but the content **came back** — from a
stale HEAD? Earlier in the same session the operator also reported a related
revival: a typed prompt was removed, re-typed, and then the deleted prompt came
back.

## Relationship to existing items

- `#fcc0` (`plan-zero-file-cache-conflict.md`) — converge all document-mutating
  disk writes through one `converge_document_or_disk()` behind the
  listener-active gate. The stale-HEAD revival after Clear Exchange is the same
  disk-vs-buffer convergence family: the clear writes the cleared buffer, but a
  fallback/replay path re-applies a stale committed (HEAD) or snapshot copy over
  it.
- `#stuckhandoff2` / `#ipcdrift` — wedged Preparing controllers re-writing a
  stale buffer over `ipc.sock` can resurrect content after a clear.
- Existing clear plans: `plan-jb-clear-session-busy-actions.md`,
  `plan-session-clear-blocked-by-own-drafted-command.md`,
  `plan-post-clear-auto-queue-suppress.md`.

## Hypotheses to verify (clean session)

- **Clear writes the cleared exchange, then a redeliver/replay path re-applies
  HEAD/snapshot.** Identify which path re-materializes content after
  `session clear` (idle-watch auto-redeliver, normalization fallback,
  CRDT replay, or a wedged Preparing controller). Cross-ref the redeliver guards
  added in `#dvre`/`#eprf` (fresh editor-buffer proof before full-document
  redelivery).
- **Clear does not advance/clear the durable merge base**, so the next
  preflight/finalize diff treats HEAD as authoritative and the clear looks like
  drift to be "recovered."

## Code anchors (re-baseline first — submodule tree is dirty)

- `session_actor_cmd.rs` clear path (clear + clear-cooldown write).
- Redeliver/replay: `redeliver_ipc_dedupe_to_editor`,
  `redeliver_normalization_fallback_to_editor`, idle-watch redeliver.
- Convergence gate: the `converge_document_or_disk()` design in
  `plan-zero-file-cache-conflict.md`.

## Acceptance

- After JB `Clear Exchange`, the cleared exchange stays cleared; no HEAD/snapshot
  replay re-materializes the prior content (no revival of a deleted prompt).
- A clear advances the merge base so the next preflight reports `no_changes`
  rather than treating the clear as recoverable drift.
- SimWorld coverage: clear-then-idle and clear-then-retype, asserting no revival;
  operator live JB verification of zero revival.

DO NOT IMPLEMENT under the live supervisor — clean session required.
