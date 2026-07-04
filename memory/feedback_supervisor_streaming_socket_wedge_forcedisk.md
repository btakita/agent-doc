---
name: feedback_supervisor_streaming_socket_wedge_forcedisk
description: Route-owned supervisor streaming-socket wedge survives admin recycle + JB restart; force-disk is the recovery
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 849a79db-187a-49ec-9ce6-c5b5e8df176f
---

The route-owned supervisor's streaming write socket (the `agent-doc start --route-owned <FILE>` process) can wedge so every `finalize --stream` prints `socket IPC failed: IPC ack status error {"type":"ack","status":"error"}` → file-IPC `no_ack` / `recovery=retry_without_disk_write`. Observed 2026-06-19 in the EFS `equityfundingsource.md` session: the wedge survived **4 finalize retries + `agent-doc admin recycle` + a full IntelliJ restart** (confirmed by a new `idea` pid, yet the wedge persisted).

**Why editor restarts don't fix it:** the wedge is on the *supervisor's* streaming socket, not the editor. Granular `agent-doc write --commit --pending-edit/--done` and plain `agent-doc commit` keep working *through* the wedge (they mutate the component + git-commit directly, bypassing the streaming IPC; `--pending-only` is even documented as incompatible with `--template/--stream/--ipc`). Only the **exchange prose** needs the `--template`/`--stream` path that the wedge blocks.

**Recovery that worked:** `agent-doc finalize <FILE> --baseline-file <bl> --origin skill --force-disk` (NO `--stream`). It logs `cleaning stale IPC patch file to prevent double-write`, claims the wedged `patch_id` sentinel, CRDT-merges against the on-disk file (absorbs any stray `agent:boundary` markers the failed `--stream` attempts left), consumes the queue head, and commits. Safe when there's **no unsaved editor divergence** — check `ls .agent-doc/live-buffer/<pathhash>` is empty first; IntelliJ's fsnotifier reloads the disk change.

**Why:** the live-binary guidance ([[feedback_operator_drive_items_on_go_queue_churn]]) says retry the editor/CRDT path, not direct disk write — correct as the *first* response. But once editor-path retries + recycle + editor restart are exhausted and the cause is the supervisor socket (not the editor), `--force-disk` is the documented escape hatch ([[feedback_hung_agent_doc_write_stale_live_buffer]], [[feedback_ipc_crdt_response_loss_forcedisk_recovery]]).

**How to apply:** on a recurring `ack status error` streaming wedge — (1) confirm no `.agent-doc/live-buffer/<pathhash>` sidecar, (2) skip repeated `--stream` retries + recycle theater, (3) `finalize --force-disk`, (4) verify `session-check` OK + doc clean. Don't kill pid of `start --route-owned` (it's the session parent — [[feedback_ipc_drift_wedged_preparing_controllers]]).
