---
name: feedback-agent-doc-supervisor-self-race-finalize-block
description: "Persistent \"visible editor buffer differs\" finalize blocks in a managed agent-doc Claude session are the route-owned supervisor self-racing, not IntelliJ — recover by resetting the supervisor."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 38c3d8e6-c900-4380-98ac-99d9437f4169
---

When a `/agent-doc` Claude Code session is itself **managed by a route-owned supervisor**
(`agent-doc start --route-owned <FILE>`) — i.e. the process tree is
`zsh → agent-doc[supervisor] → claude → zsh(me)` and `echo $TMUX_PANE` matches the pane
`agent-doc session status <FILE>` reports as the authoritative actor — then running
`agent-doc finalize` repeatedly via the Bash tool with an **active queue** can hit a
persistent, recurring block: `File was modified during response generation. CRDT merging…`
followed by `Error: visible editor buffer for <FILE> differs from the expected disk state`
(exit 75), even with `--force-disk` and even after IntelliJ is fully closed.

**Why:** the supervisor owns the IPC listener (`.agent-doc/ipc.sock`) and runs the
idle-queue watch + a file-watcher. Each finalize write to disk triggers the supervisor to
re-apply its (now-divergent) buffer view, racing the finalize — a self-induced feedback
loop. `is_listener_active()` stays true (supervisor socket), so the visible-buffer guard
fires; removing the socket doesn't help (the supervisor re-binds within the finalize
window). This is NOT an IntelliJ editor problem, though an open editor adds a second racer.
It's a live manifestation of the gated `#finalize-visible-buffer-ipc-timeout-race` /
`#ipcfullprompt` class, made worse by hours of churn (restores, failed finalizes, queued
reposition patches accumulating divergence).

**Why it matters:** I burned most of a session blaming IntelliJ and having the user close
it, when the real racer was the session's own supervisor. The diagnostic that pins it:
`echo $TMUX_PANE` + `pstree` of the pane pid showing the supervisor as my ancestor, plus
`agent-doc admin list` showing the doc owned by my own pane.

**How to apply:** Early, when finalize blocks on the visible-buffer guard *repeatedly*,
run `agent-doc session status <FILE>` + `echo $TMUX_PANE` + `pstree`. If I'm the managed
child of the doc's own supervisor, do NOT keep retrying finalize or chase IntelliJ. The
managed child cannot fix it from inside (restarting/killing the supervisor kills this
session). Recover by resetting the supervisor **externally** (from another terminal:
`agent-doc session interrupt-clear <FILE>`, or kill the supervisor PID, then re-invoke
`/agent-doc <FILE>` fresh) — or halt the queue so the idle-queue watch stops. Code work
committed to git/submodule is unaffected; only the session-doc finalize is blocked. See
[[feedback-check-admin-detect-for-pane-contamination-first]].

**Deterministic (non-race) variant + recovery recipe (2026-06-06):** sometimes there is NO
live racer — `admin detect` shows no `cross_document_pane`, `ps` finds no competing
`agent-doc` for the doc — yet `finalize` fails with `IPC proof insufficient …
invariant=live_prompt_drift_after_preflight` (IPC ack content a few bytes longer than
`content_ours`, the editor's `(HEAD)` boundary annotation) and then `Error: queue consume:
snapshot queue state diverged from document queue after completing head prompt`. The queue
region is byte-identical doc-vs-snapshot; the divergence is whole-document (snapshot saved
from `content_ours` ≠ disk with the IPC-applied response/boundary). This is the gated
`#finalize-divergence-orphans-committed-head` bug live. The response is already on disk and
`write --commit` reports `AlreadyApplied` but still bails on consume. **Recovery that
works:** `agent-doc reset --from-current --preserve-session <FILE>` (rebuild sidecars so
snapshot == disk-with-response, session preserved), then `agent-doc write --commit
--force-disk <FILE>` — force-disk bypasses the editor IPC drift, the queue head consumes
cleanly, and the cycle reaches `committed`. Then confirm with `session-check`.
