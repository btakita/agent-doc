# Plan: Codex capability-proof failure permanently disables dispatch and is un-recoverable

**Pending:** `#codex-capability-proof-unrecoverable`
**Trigger:** `#agent-doc-bug` — JB error pasted 2026-06-01 (originally surfaced 05:23–05:30):
`managed Codex capability proof failed; prompt dispatch is disabled: codex child network probe timed out after 45s`
**Related:** `#codex-owned-pane-auto-queue-stuck`, the `session interrupt-clear` /
`session clear` recovery path, and the `cross_document_execution_context`
diagnostic (`19d4f966`).

## Problem

The managed Codex capability proof runs a network probe (`prove_managed_session_capabilities`)
in a background supervisor thread (`start.rs` ~1556). On `Err` (e.g. the child
network probe times out after 45s) it sets the dispatch gate to **`Failed`
permanently** (`set_capability_proof_gate(Failed, …)`, `start.rs` ~1579) and
transitions the actor to `Blocked`. From then on `capability_dispatch_blocker()`
(`start.rs` ~2011) returns the error and every dispatch path is refused:

- `auto_trigger_inject_command` (`start.rs` ~1082) — auto-queue / route dispatch
- `IpcMethod::Inject` (`start.rs` ~2120) — injected prompts / operator sends

Two distinct defects:

### 1. No retry — a transient network blip permanently wedges the session
The gate is set to `Failed` on the first probe failure with **no bounded retry
and no re-prove on network recovery**. It is only reset to `Pending` on a fresh
`start` (`start.rs` ~2886/~5073). A 45s network hiccup therefore disables the
whole session's dispatch until a manual restart.

### 2. Recovery is itself blocked — `kill -9` is the only way out
Observed live 2026-06-01: with the gate `Failed`, **both** `agent-doc session clear <FILE>`
and `agent-doc session interrupt-clear <FILE>` fail with the *same*
`prompt dispatch is disabled: … network probe timed out after 45s` error (the
codex child stderr `Reading prompt from stdin…` confirms a managed spawn/proof
ran during the recovery command). So the documented recovery (`agent-doc start`)
and the stop/clear commands all route through the capability gate, leaving no
managed way to stop a network-wedged session — the operator must
`kill`/`tmux kill-pane` the supervisor tree manually (as was done for the
`tsift.md` pane this session).

### Not a defect (secondary symptom)
The `apply_patch verification failed: Failed to find expected lines …` lines in
the same error block are **Codex-internal** (the gpt-5 agent's own apply_patch
tool failing on stale/drifted context for `agent-doc-bugs2.md`, `write.rs`,
`finalize_integration.rs`). They are a downstream symptom of the wedged turn, not
an agent-doc patch bug, and should not be conflated with the gate fix.

## Proposed fix (design — pick scope before implementing)

Two independently-shippable pieces; (B) is the higher-value recovery fix.

### A. Bounded re-prove instead of permanent `Failed`
- In the proof thread's `Err` branch, retry the probe a bounded number of times
  with backoff before committing to `Failed`; OR keep a `Failed` gate but allow a
  background re-prove on an interval so a recovered network self-heals the gate
  back to `Proven` without a manual restart.
- Make the 45s probe timeout and retry budget configurable (frontmatter /
  `.agent-doc/config.toml`), since slow-network environments legitimately need
  longer.
- Risk: changes startup-readiness latency; the probe shells out to the codex
  network check so deterministic unit coverage is limited to the
  decision/back-off logic (extract a pure `proof_retry_decision` helper to test).

### B. Operator stop/clear/recovery must bypass the dispatch gate
- `session clear`, `session interrupt-clear`, and the `agent-doc start` recovery
  path must **not** require or re-run the capability proof — stopping or clearing
  a session is not a dispatch and must succeed when the proof has failed.
- Audit every path these commands take to `capability_dispatch_blocker` /
  `IpcMethod::Inject` / a managed spawn, and route operator stop/interrupt/clear
  through an un-gated control channel (e.g. a dedicated `IpcMethod::Interrupt`/
  `Stop`/`Clear` that is exempt from the gate, and a direct tmux interrupt send
  that never spawns a proof).
- Acceptance: with the gate `Failed`, `session interrupt-clear <FILE>` and
  `session clear <FILE>` succeed and stop/clear the session without `kill -9`.

## Acceptance Criteria

- A transient capability-proof failure no longer permanently disables dispatch
  (auto re-prove or bounded retry; or a documented un-gated recovery).
- `session clear` / `session interrupt-clear` succeed against a capability-proof-
  `Failed` session and never themselves fail with `prompt dispatch is disabled`.
- Pure decision helpers (retry budget, gate-exempt operation classification) have
  deterministic unit coverage; live behavior verified with `make tmux-ci` /
  manual repro against a network-denied codex child.
- `start.rs`, `route.rs`, `session_actor_cmd.rs`, `SPEC.md`, and the
  session/tmux command spec aligned on the gate-exemption + re-prove contract.
