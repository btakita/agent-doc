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
