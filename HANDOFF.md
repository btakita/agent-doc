# netadv5 handoff

Branch `netadv5` (based on `batch-0505`). Scope: a timeout is never a verdict;
hot-path round trips are budgeted. Source of the R-ids:
`docs/reference/network-channel-audit.md` §4. Plan:
`agent-loop/tasks/agent-doc/plan-network-adversarial-correctness.md`.

Principle: a timeout yields "unknown / retry later", never "dead / refused /
absent". Destructive action needs positive evidence (pid gone, ECONNREFUSED on
a socket whose owner pid is dead, an explicit refusal message).

Sibling `netadv3` owns TLA ports + one-shot/lost-message F-items. Overlap
note: F1/F2/F3/F6/F19/F20 are the code halves of R1/R2/R3/R5/R8/R9, which the
coordinator assigned here.

## Status

| R | State | Change | Test |
|---|---|---|---|
| R3 | done | `agent-doc-ipc-io`: `probe_listener_for_pid` → `Live/Absent/Unknown`; typed `ConnectTimedOut`; unlink only on refused/ENOENT + owner pid gone; `prune_stale_editor_sockets` uses it | `slow_listener_connect_timeout_never_unlinks_socket`, `connect_failure_classification_separates_slow_from_refused`, `real_connect_watchdog_error_is_typed_timeout` |
| R1 | todo | | |
| R2 | todo | | |
| R5 | todo | | |
| R6 | todo | | |
| R7 | todo | | |
| R8 | todo | | |
| R9 | todo | | |
| RTT budgets | todo | | |

## Resume

`cd /home/brian/work/btakita/agent-doc-wt/netadv5`; continue the first `todo`
row. Do not push/install/release/merge.
