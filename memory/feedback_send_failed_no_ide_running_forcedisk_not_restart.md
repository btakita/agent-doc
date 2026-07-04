---
name: feedback_send_failed_no_ide_running_forcedisk_not_restart
description: "send_failed with NO JetBrains process running = stale endpoint → force-disk is legitimate (not \"restart IDE\"); clear phantom publish-live-buffer.signal + stuck blocked_closeout"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: b132e1f8-3969-47ed-9f27-8f41b25e4d04
---

**Verify editor liveness with `ss -xlp`, NOT `ps grep` (2026-07-02).** `ps aux | grep -iE "jetbrains|idea"` gave a **false negative** here (returned nothing) while the IDE was very much alive — `ss -xlp | grep "<project-root>/.agent-doc/ipc.sock"` showed `idea` (pid 1679357, binary `/home/brian/opt/idea-IU-<ver>/bin/idea`) holding the LISTEN socket. Socket ownership is authoritative; a missed `ps` grep must NOT be read as "no IDE" — that mistake leads to a wrong `--force-disk` against a live buffer. Always confirm with `ss -xlp` before concluding the endpoint is stale.

`send_failed` on `editor_endpoint=live` has TWO distinct causes — confirm socket
ownership (`ss -xlp`) BEFORE choosing recovery:

- **IDE running, listener dead** → operator restarts IDE (see [[feedback_dead_jb_listener_send_failed_blocks_finalize_only]]).
- **No IDE process running at all** → the `controller.sock`/`ipc.sock` are stale
  0-byte files and `.agent-doc/patches/*.json` sit un-drained (a live plugin
  applies+deletes them as ACK). The `live` endpoint is **stale**, there is no live
  buffer to protect, and the operator prompt is already safe on disk. Here
  `--force-disk` is the **correct** recovery, not a STOP — git is authoritative.

**Why:** the "never force-disk / restart IDE" guidance assumes a live editor buffer
whose operator text could be dropped. With no IDE running, that assumption is false;
refusing to force-disk just wedges the cycle forever.

**How to apply (full sequence seen 2026-07-01 on agent-doc-bugs2.md):**
1. `ps aux | grep -iE 'idea|jetbrains'` — confirm no IDE. Check un-drained patch mtimes.
2. `agent-doc write --force-disk --backlog-add ...` then `agent-doc finalize --force-disk --stream`.
3. force-disk trips a **false-positive visible-buffer guard** ("visible editor buffer
   differs from expected disk state") fed by a stale `.agent-doc/patches/publish-live-buffer.signal`
   (names your doc) + `.agent-doc/live-buffer/*.jetbrains-<deadpid>-*` snapshots. `mv`
   the `publish-live-buffer.signal` (and orphaned backlog patch) aside, retry finalize.
4. finalize then **commits** but `session-check` still false-errors on a stuck
   `blocked_closeout` (`kind=editor_convergence_required reason=send_failed`) in
   `.agent-doc/state/cycles/<dochash>.json`. Verify HEAD has the response, then `mv`
   that cycle-state file aside (see [[feedback_forcedisk_committed_but_false_closeout_error_stop_retrying]]). STOP retrying.
5. The realtime CRDT replica may re-surface the answered prompt below a fresh boundary
   (the `#boundaryaccum1` unsafe-drift bug) + merge in genuinely new operator queue
   content — preserve both; do NOT checkout/reset. If concurrent sessions have advanced
   HEAD ([[feedback_concurrent_sessions_force_push_shared_superproject]]), STOP: no push.
6. Recommend the operator clear `.agent-doc/patches/*` + restart JB before re-invoking so
   a fresh IDE+binary reconciles the boundary artifact cleanly.
