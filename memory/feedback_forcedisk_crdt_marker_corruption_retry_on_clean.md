---
name: feedback_forcedisk_crdt_marker_corruption_retry_on_clean
description: "force-disk CRDT merge can inject |-run corruption into a component close marker; retry force-disk after the doc self-heals to clean, don't manual-repair"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: a2486637-9090-4dd8-96c0-ec65adb9e347
---

When the route-owned supervisor's write socket is wedged (`IPC ack status error` then file-IPC `no_ack` / `recovery=retry_without_disk_write`), `finalize --force-disk` is the recovery — BUT its CRDT merge against the wedged supervisor's stale live buffer can corrupt a component close marker, injecting a long run of `|` into it (e.g. `<|||…|||!-- /agent:queue -->`), which then fails the post-merge structure check (`unclosed component: <!-- agent:queue --> without matching close`) and aborts WITHOUT committing the response (cycle stuck `response_captured`).

**Do NOT manual-Edit the corrupted marker or `git checkout HEAD` (loses the user's new queued prompt).** The wedged editor applies its queued file-IPC patch asynchronously a moment later and self-heals the marker back to the correct `<!-- /agent:queue -->`. Once the on-disk doc is clean+parseable again (verify: `grep -c '||||||' FILE` == 0 and the close marker is intact), **re-run `agent-doc finalize --force-disk`** with the same body — the merge now has a clean base and succeeds (consumes the queue head, commits, drains). The actual code work (build/test/deploy) should already be done+pushed before this; only the session-doc closeout is what's churning.

**Why:** the `|`-corruption originates from merging against a stale/wedged supervisor buffer, not from your patch; it's transient and clears when the editor finishes applying its own queued patch. Fighting it with manual repair races the live IPC listener and risks worse corruption.

**How to apply:** socket-wedge finalize → try `--force-disk`; if it aborts on a structural `unclosed component` after a `|`-run marker, wait/verify the doc settles clean, then retry `--force-disk` once. Related: [[feedback_supervisor_streaming_socket_wedge_forcedisk]], [[feedback_supervisor_crdt_replay_reset_from_current]], [[feedback_ipc_drift_wedged_preparing_controllers]].
