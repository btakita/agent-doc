# Plan: agent-doc IPC/CRDT response drift (control-plane refactor)

**Bug id:** `#ipc-crdt-response-drift`
**Source:** `tasks/agent-doc/agent-doc-bugs2.md` queue report (2026-06-08) — "monsterrodholders.md has drift issues. We recently refactored to a control pane process. Please fix. Add Claude Code SimWorld tests."

## Symptom (reported)

When a turn's response is written back, the IPC/CRDT path garbles it: response lines land **reversed**, the `### Re:` **header is missing**, and response fragments are inserted **above** the prior HEAD response instead of appended at the tail. The agent then has to reconstruct the Exchange by hand and reset sidecars.

## Firsthand reproduction (this very turn, agent-doc-bugs2.md)

While closing out `#codex-exchange-prompt-no-dispatch` I hit the same failure mode directly — strong corroborating evidence:

- `finalize` logged `[write] File was modified during response generation. CRDT merging...` with `base_len=5797 ours_len=7895 theirs_len=6386`. The on-disk file had grown (`theirs` > `base`) **during** response generation.
- A **second supervisor** (a `monsterrodholders` agent-doc session) committed to the same superproject concurrently (`d4182b0e1 agent-doc(monsterrodholders): ... (submodule pointer)`) interleaved with this session's commits — i.e. multiple control-plane/supervisor processes are writing back to the same git repo at once.
- CRDT reported `merge successful — no conflicts possible`, yet the visible-editor-buffer guard then fired (`visible editor buffer ... differs from the expected disk state`) and the response **did not land** on the first `finalize`; it had to be recovered via `write --commit`.
- Preflight separately reported a `stuck_captured_cycle`: a cycle reached `committed` but the captured response body was **not present in HEAD**.

This matches the reported "reversed lines / missing header / inserted above" exactly: a concurrent foreign write changes `theirs` mid-generation, the CRDT merge base selection produces mis-ordered content, and the commit/visible-buffer boundary then disagrees.

## Reliable repro — `live_prompt_drift_after_preflight` (2026-06-09)

A second, **more specific and reliably reproducible** manifestation surfaced repeatedly during `agent-doc-bugs2.md` closeouts on 2026-06-09 (hit ~4× across the session, twice in one cycle). This is the near-term-actionable slice — distinct from the deeper CRDT-ordering root cause above.

**Exact signature** (from `finalize --stream`):
```
[write] socket IPC patch delivered
[write] IPC proof insufficient ... invariant=live_prompt_drift_after_preflight
        recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar
        candidate_len=<visible> content_ours_len=<snapshot, LARGER>
[write] snapshot from content_ours (<larger> bytes)
Error: refusing commit ... snapshot is <N> bytes but the visible file is <M<N> bytes,
       which looks like a manual cleanup with stale snapshot/CRDT state.
```

**What happens:** the pending mutations (`--done`/`--pending-gate`/`--pending-add`, queue strike) **land**, but the `### Re:` response **body fragments out of `exchange`** (only partial bullets reach the file, or none). The `content_ours` snapshot (baseline + full response) is **larger** than the visible file, so the next commit fails closed as "manual cleanup". The response is **not lost** — it is intact in the `ack_content_sidecar` / `content_ours` snapshot.

**Trigger condition:** correlates with an **active IPC listener** during closeout — every failing finalize logged `[commit] skipping working-tree boundary reposition — IPC listener active`. A live editor/supervisor holds the buffer and the post-preflight live state drifts from the baseline preflight captured, so the IPC write proof fails closed. This is the supervisor/finalize **self-race** (root-fixed by `#pcpconsol` single ordered write queue), surfacing on the IPC/CRDT closeout path.

**Proven manual recovery** (works every time this session — keep this exact order; do NOT run bare `reset --from-current`):
1. `git checkout HEAD -- <FILE>` — discard the fragmented/partial visible state.
2. `agent-doc reset --from-current --preserve-session <FILE>` — clear the wedged cycle sidecars, keep session.
3. `agent-doc preflight <FILE>` — re-baseline (auto-reaps already-done mirror items, auto-strikes resolved queue heads).
4. `agent-doc finalize <FILE> --force-disk [--done <id>] ` — re-land the response via the disk path, bypassing the live-buffer IPC drift. If a `--done <id>` was already reaped by the failed attempt, drop the flag (`id not found in backlog/icebox`) and finalize the response alone.

**Near-term fix (this plan's actionable slice — `#exch-intermix`):** when `finalize` detects `invariant=live_prompt_drift_after_preflight` with the response safe in `content_ours`/`ack_content_sidecar`, **auto-run the recovery above** instead of erroring out and stranding the response. Guard rails (must be airtight — this is automatic data mutation):
- Only auto-recover when the captured `content_ours` snapshot hash matches the expected baseline+response (the response is provably safe), and the visible-file drift is the known benign boundary/IPC pattern — never when the drift could be real user content.
- Prefer `reset --from-current --preserve-session` + `finalize --force-disk` without `git checkout` when the visible file already contains the prior committed state plus only benign boundary markers; fall back to `git checkout HEAD` only when the visible file is fragmented.
- Emit a `ops.log` marker (e.g. `live_prompt_drift_auto_recovered`) so the recovery is observable and testable.
- SimWorld coverage: drive a closeout where an IPC-listener-active live buffer drifts post-preflight → assert the response lands intact (single clean `### Re:`), pending mutations apply once (no dup), and no `manual cleanup` wedge. Include the negative case: genuine user content drift must NOT be auto-recovered (fail closed).

**Files (near-term slice):** `agent-doc-orchestration/src/write.rs` (IPC proof / `live_prompt_drift_after_preflight` branch), `git.rs` (the `manual cleanup` commit refusal), `reset.rs`, plus SimWorld harness.

## Suspected area — recent "single-process control plane" refactor

Recent commits that touch the implicated paths (run `git log` in `src/agent-doc`):

- `84b2d690 docs: specify single-process control plane`
- `ac213a76 controller: make actor state memory authoritative`
- `767832e8 controller: persist session actor closeout`
- `3978a949 controller: return durable dispatch receipts`
- `5fd64b26 Use overlay CRDT as merge base`  ← prime suspect for ordering
- `a45121b0 Apply IPC node patches in editors`
- `b76a3099 fix: de-wedge slow ipc listener`
- `cf0a3f21 controller: make sidecars compatibility projections`

The "single-process control plane" intent is that one process owns actor state — but the superproject git write-back is still being performed by multiple supervisor processes concurrently (observed above). The drift is the seam between (a) overlay-CRDT-as-merge-base ordering and (b) concurrent foreign writes to the same file during generation.

## Investigation steps

1. Reproduce deterministically: a write-back where `theirs` (disk) is mutated by a foreign writer mid-generation, then assert the merged Exchange preserves append order and the `### Re:` header. Drive it through the SimWorld harness so no live tmux is needed.
2. Inspect `merge.rs` / `crdt.rs` overlay-as-merge-base selection (`5fd64b26`): confirm whether the overlay base can reorder appended response nodes when the disk file grew from a foreign append.
3. Inspect `ipc_socket.rs` / editor IPC node-patch application (`a45121b0`) and the visible-buffer guard: why does `merge successful` still disagree with the visible buffer, and why did the captured response not reach HEAD (`stuck_captured_cycle`).
4. Confirm whether the single-process control plane is actually single-writer for git write-back, or whether multiple supervisors still race the superproject (the monsterrodholders interleave suggests the latter). If multi-writer, add a write-back serialization point owned by the control plane.

## Fix approach (refine after investigation)

- Make CRDT merge-base selection order-stable for the append case so a concurrent foreign append cannot reverse or hoist this session's response above the prior HEAD response.
- Serialize superproject git write-back through the single control-plane owner (or a per-repo lock) so two supervisors do not interleave commits mid-generation.
- Reconcile the visible-buffer guard with a successful CRDT merge so a clean merge does not then fail closed and strand the captured response outside HEAD.

## Test coverage (Claude Code SimWorld)

- New SimWorld scenario: concurrent foreign append to the session doc during response generation → assert merged Exchange keeps append order + `### Re:` header intact (no reversal, no hoist).
- SimWorld scenario: two supervisors writing back to the same superproject → assert serialized, no interleaved partial commits, no `stuck_captured_cycle`.
- SimWorld scenario: clean CRDT merge must not then trip the visible-buffer guard / must land the captured response in HEAD.

## Files

- `agent-doc-orchestration/src/merge.rs`, `crdt.rs` — overlay-as-merge-base ordering
- `agent-doc-orchestration/src/ipc_socket.rs`, `write.rs`, `stream.rs` — IPC node-patch application + visible-buffer guard
- `agent-doc-orchestration/src/project_controller.rs`, `session_actor.rs` — single-process control plane / write-back ownership
- SimWorld harness (existing `sim_world` tests) for deterministic coverage

## Note

This needs to be implemented in a **clean session**, not inside a document that is actively drifting — editing the CRDT/IPC write path while the session doc using it is mid-corruption risks compounding the garbling. Drive the repro and fix from a fresh, quiescent session (ideally with other agent-doc supervisors idle).
