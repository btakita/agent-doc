---
name: feedback_queue_consume_race_forcedisk_recovery
description: "standard finalize hitting \"queue consume: failed to write document ... retry after typing stops\" → recover with finalize --force-disk"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: b0301d4f-e3f7-40a2-a570-f7ca45bb6990
---

A standard (socket-IPC) `agent-doc finalize ... --done <id>` can reap the backlog item and compute the response merge, then abort with `Error: queue consume: failed to write document ... document changed after the response merge was computed; retry after typing stops`. That is the route-owned supervisor racing the queue-consume write. Symptoms after the abort: the response is **absent from HEAD**, the supervisor commits its own response-less version(s), the worktree holds the new bullets **spliced into the prior `### Re:` section in reversed order** (the cross-cell CRDT splice corruption), and `session-check` shows the cycle stuck `preflight_started` with `live_prompt_drift_after_preflight`.

**Recovery that works:** `git checkout HEAD -- <FILE>` (clean base) immediately followed by `agent-doc finalize <FILE> --force-disk --origin skill --done <id>` carrying the response. `--force-disk` logs `File was modified during response generation. CRDT merging... merge successful` — its CRDT merge absorbs the concurrent supervisor writes that the socket path could not, then reaps + consumes the queue head + commits cleanly (no postcommit-worktree-corruption warning). Verify HEAD has the `### Re:` heading + struck head + worktree clean.

Do NOT retry the plain socket finalize (each retry risks another reversed-splice). Go straight to `--force-disk`. Same family as [[feedback_supervisor_streaming_socket_wedge_forcedisk]] and [[feedback_ipc_crdt_response_loss_forcedisk_recovery]], but the trigger here is the queue-consume concurrent-mod defer, not a socket wedge. This is the live stale-CRDT class that `#lazilycrdtrs`/`#crdtauth` (CRDT-authority model) is being built to root-fix.
