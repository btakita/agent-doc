---
name: reference_agent_doc_doubled_canonical_target_wedges_all_recovery
description: "A retained intent can hold the document concatenated with itself; `duplicate_singleton_component` then blocks EVERY recovery path, including --force-disk. Upgrading the binary and restarting the IDE does not clear it."
metadata:
  node_type: memory
  type: reference
---

Seen 2026-07-30 on `tasks/professional/equityfundingsource.md`, agent-doc **0.35.104**
(fresh `make install`, IDE restarted, **no controller/supervisor process running**).

**Signature — the authority is exactly 2× the disk file:**

```
duplicate_singleton_component:backlog=2,done=2,exchange=2,icebox=2,queue=2,review=2,status=2
[integrity-gate] INTERRUPTED: 2 agent boundary markers in <FILE>
```

Diagnose before touching anything:

```bash
agent-doc read FILE | wc -l          # 686  ← authority
wc -l < FILE                         # 343  ← disk, CORRECT
diff <(agent-doc read FILE | sed -n '344,686p') FILE   # empty ⇒ verbatim duplicate
agent-doc exchange list FILE         # works! 15 nodes, NO duplicate ids/labels
```

`exchange list` succeeding while everything else fails is the tell: the **exchange
component is fine**, and the doubling is introduced when the *document-level* target is
rebuilt from a retained `document_write_deferred` intent. Disk, git HEAD, and
`.agent-doc/snapshots/<hash>.md` are all correct.

**Cause:** several `respond` cycles retained intents while the editor replica was absent
(`editor_replica_reregister=delivery_failed_to_all:1`). A whole-document payload replayed
onto a base that already contains it concatenates instead of rebasing.

**Everything fails closed — do not keep trying:**

| tried | result |
|---|---|
| `preflight` (auto-repair) | refuses: invalid target |
| `reset --from-current` (±`--preserve-session`, ±`--force-disk`) | refuses, or waits forever for convergence |
| `repair --apply-recovery` | withheld; misclassifies as `escaped_template_patch` |
| `cancel` | `no open cycle to reclaim` |
| `admin recycle` | "at next idle boundary" — nothing to recycle |
| `autofix --apply` | its own whitelisted `write --commit` exits 1 |
| `admin repair-projection` | rebuilds *from* the bad state |
| moving `.agent-doc/crdt/<hash>.yrs` aside | no effect (that file was 13 days stale) |

**`--force-disk` does NOT bypass this.** Despite documenting itself as "bypass editor
convergence … intended for explicit recovery", it still validates a target built from the
bad authority.

**Do NOT hand-edit `.agent-doc/state.db`** — ~80 MB, shared by every document in the
project; see [[reference_agent_doc_state_db_header_clobbered_by_tracing_on_controller_recycle]].
Nothing is at risk from waiting: disk and HEAD are complete. Commit the real work to its own
repo, report the stuck bookkeeping, stop.

Plan filed at `tasks/agent-doc/plans/plan-doubled-canonical-target-wedge.md`
(`#doubledtargetwedge`). The fix needs a way to **drop a retained intent** (`cancel` only
handles open cycles), and validation at *capture* time — a payload that fails
`duplicate_singleton_component` can never become writable, so retaining it guarantees a
permanent wedge.

Related: [[reference_agent_doc_recovery_replay_duplicate_boundary_wedges_integrity_gate]]
(same integrity gate, single duplicated marker, older versions),
[[reference_agent_doc_write_applied_no_terminal_patchback_guard_misfire]].
