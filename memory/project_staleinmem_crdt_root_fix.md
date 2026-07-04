---
name: project_staleinmem_crdt_root_fix
metadata: 
  node_type: memory
  type: project
  originSessionId: 151a6e18-4d72-47e3-9a21-649f4b32045d
---

The "stale in-memory CRDT re-commits corruption after every clean commit; `git checkout HEAD` won't hold; only a supervisor restart recovers it" class is ROOT-FIXED on agent-doc submodule main (commits `5b4d143f` + `06c84d99`, 2026-06-26), NOT yet installed/pushed/recycled.

**Mechanism found:** `crdt_relay_host::hub_registry` is a process-global `static OnceLock<Mutex<HashMap<String, RelayHub>>>` holding the canonical CRDT replica for the whole supervisor process. Under `MultiReplica` (live editor) "in-memory wins" and `reconcile_disk_projection` is purely additive — so an out-of-band disk correction (`git checkout HEAD`, `reset --from-current`) or a `compact exchange` shrink could never displace stale canonical ops; only a restart (which clears the static) recovered it. The earlier recycle-wedge plan (`plan-midturn-recycle-resume-wedge.md`, Phase A `ed255d81`) WRONGLY dismissed "stale in-memory CRDT" as inexact — it only examined the `start/run.rs` boot path, missing this process-global hub.

**Fix:** gave the MultiReplica commit barrier the baseline-divergence guard the headless path already had (`snapshot::crdt_merge_base_state`). `RelayHub.last_committed_text` + `reconcile_canonical_against_baseline` rebuild the canonical (and reseed editor mirrors) from the corrected disk when disk ≠ last-committed ≠ canonical; `record_committed_baseline_for_file` records post-commit (wired into `write.rs` + `flow/closeout.rs`). Compact coverage: baseline seeded on `recover_from_projection` + first-barrier contact so a compact landing before the first finalize (or after a restart) is still detected. Headless/Detached path byte-for-byte unchanged.

Once installed, the manual recovery dances in [[feedback_supervisor_crdt_replay_reset_from_current]], [[feedback_parent_supervisor_stale_crdt_reset_nopreserve_then_restart]], and [[feedback_compaction_stale_snapshot_reset_not_commit]] should be needed far less for the MultiReplica path.

**Deeper follow-up (operator intent):** make the on-disk file a first-class per-cell syncable endpoint of the reactive Cell Tree (lazily) owned by the in-process control plane, reconciled per-cell via text-diff→ops — `reconcile_canonical_against_baseline` is the seam where the wholesale rebuild becomes a per-cell merge. Blocked on the per-cell Cell Tree being the canonical (see [[project_agent_doc_percell_crdt_merge_intent]], `plan-lazily-agent-doc-reactive-substrate.md`). Note: relay-canonical yrs deltas are still deferred (editors reach the supervisor as markdown patch files, not yrs updates), so the canonical is not yet the live content medium — the barrier guard is supervisor-side detection that works regardless.

Remaining operator-gated steps: install (`make install-full`), push submodule + bump superproject pointer, `agent-doc admin recycle`. Mid-session install against the live route-owned supervisor is the hazard ([[feedback_no_mid_session_install_against_live_supervisor]]).
