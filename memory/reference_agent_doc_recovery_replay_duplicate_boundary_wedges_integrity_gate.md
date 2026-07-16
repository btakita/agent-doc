---
name: reference_agent_doc_recovery_replay_duplicate_boundary_wedges_integrity_gate
description: "delivery_ack_pending recovery replay can duplicate the exchange boundary marker, and the integrity gate then hard-fails preflight AND session-check before any recovery can run"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 55f1cf44-be55-4b97-b15a-86f4efe693bc
---

`finalize` failing with `serialized_atomic_write: ... remains retained after the
foreground editor ACK deadline` (reason=`delivery_ack_pending`) correctly tells you
the binary owns the retry. But the automatic replay can land the response in the
live editor buffer **while inserting a second `<!-- agent:boundary:… -->` marker**.
The integrity gate then fails closed on every subsequent command:

```
[integrity-gate] INTERRUPTED: 2 agent boundary markers in <FILE>;
the exchange must contain at most one standalone boundary marker
```

**Why it does not self-heal:** the gate runs *first* in both `preflight` and
`session-check`, so the supervisor's own recovery paths are blocked by the same
error. "Recovery is scheduled automatically" is true of the ACK retry, not of this
structural state.

**Diagnose (editor authority vs disk):**
```bash
agent-doc read <FILE> | grep -c "agent:boundary"   # live buffer (authority) -> 2
grep -c "agent:boundary" <FILE>                    # disk -> 1
agent-doc read <FILE> | grep -c "<response header>" # response IS in the buffer
```
Divergence is the signature: the response exists in the editor, not on disk/git.

**Do NOT** rerun `finalize`, `write --commit`, repair, or `--force-disk` — the
binary refuses correctly and the response is already captured; forcing risks
dropping operator text. Commit the real (non-session) work to its own repo first,
then report the stuck session-doc bookkeeping and stop. Removing the duplicate
marker is operator-gated because it edits the live authority.

Related: [[reference_agent_doc_stale_plugin_ack_wedge_reload_lib_unwedge]]
(different: that one clears with `admin reload-lib`, this one does not — the gate
blocks before reload matters), [[reference_agent_doc_recovery_index]],
[[reference_concurrent_agentdoc_dev_session_stalls_closeout_recycle]].
