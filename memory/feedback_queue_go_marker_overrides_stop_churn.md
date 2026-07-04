---
name: feedback_queue_go_marker_overrides_stop_churn
description: "A queue marker with `priority go` force-resets queue:start every preflight, overriding queue:stop — the real churn root cause"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 4f0c363c-493a-415d-a9d0-a2c8dea9d6b5
---

When a `go`-mode queue keeps re-activating across repeated `/agent-doc` invocations (continuation_required flips back to true, drainable heads reappear) even after you set `queue: stop`:

**Why:** the `<!-- agent:queue ... priority go -->` MARKER's `go` attribute makes preflight's convergence force `queue: start` every cycle ("go-mode active queue ... converged live editor buffer"), overriding the frontmatter `queue: stop`. So `queue: stop` alone does NOT hold.

**How to apply:** to actually stop the in-session drain loop, remove ` priority go` from the queue marker line (keep the preset + heads) AND set `queue: stop`. With `go` gone, `queue: stop` holds → `queue_active=null, continuation=false, no_drainable_work`. Do this via Edit + `reset --from-current` + `commit` (not finalize) when IPC is degraded.

**Also (session-check confirmed):** `agent-doc admin queue pause` suppresses ONLY the unattended supervisor idle-watch auto-injection — it does **NOT** stop the attended in-session loop. Do not rely on pause to gate per-port operator-gated work; use `queue: stop` / `--- stop` fence (or strike/tag the heads).

**And:** a concurrent session (e.g. a codex companion on another doc) contending the shared supervisor socket wedges `finalize` (socket+file IPC `no_ack` timeout, `recovery=retry_without_disk_write`). Recover the response via the git path: `git checkout HEAD -- FILE` → Edit in the `### Re:` section + strike the head → move `.agent-doc/{captures,crdt,state/cycles,patches}/<hash>` aside → `reset --from-current` → `commit` → `session-check`. The real fix is a session restart. Relates to [[feedback_supervisor_streaming_socket_wedge_forcedisk]] and [[feedback_degraded_ipc_write_retry_duplicates_response]].
