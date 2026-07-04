---
name: feedback_stale_binary_supervisor_causes_live_crdt_edit_bugs
description: "operator's live queue-edit bugs (deletes reappear / attributes vanish / typing duplicates) can be a stale-BINARY supervisor, not a code bug — diagnose via ps lstart vs binary mtime, fix with admin recycle"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: e6c21fed-7fee-479d-80fc-aa04fab4b989
---

When the operator reports live editor/queue CRDT-edit bugs — "when I delete queue items they reappear", "when I add attributes to an element they disappear", "when I type into the queue an old duplicate appears above where I'm typing" (the `#qeditdup`/`#qftloss` symptom class) — and the *installed* binary already has the `#queuestatemachine` idempotent-convergence + Live<Struck lattice fixes, suspect a **stale-BINARY route-owned supervisor**, not a code regression.

**Diagnose:** compare the running supervisor's start time to the installed binary's mtime:
```
pgrep -f 'start --route-owned <FILE>'         # get pid
ps -o pid,lstart -p <pid>                      # supervisor START time
stat -c '%y' ~/.cargo/bin/agent-doc            # installed binary mtime
```
If the binary mtime is AFTER the supervisor start time, the running supervisor holds the OLD in-process CRDT code — the fixes never got promoted into the live process. A stale-binary supervisor does NOT self-heal by drain-lease expiry (`#supselfheal`); nothing replaces its binary on its own.

**Why:** the supervisor runs in-process CRDT (08b single-process control plane). A binary reinstall doesn't touch the already-running process's code; it keeps serving stale convergence logic that reverts/duplicates operator edits.

**Fix (`#staleloop-recycle-restart`):** `agent-doc admin recycle <FILE>` → "controller … marked to recycle at next idle boundary". Ending the turn lets it land; verify a NEW supervisor pid with a start time matching the recycle (`ps -o lstart`), and ops.log `controller_recycle_requested reason=operator_request`. This is non-disruptive (in-place re-exec) and safe even though the supervisor is the session parent — unlike `restart-supervisor`, which kills the parent.

**Caveat — recycle DROPS the in-memory queue pause.** After the fresh supervisor comes up it forgets any prior `admin queue pause`, so its idle-watch resumes re-dispatching `/agent-doc` on any un-drainable residue heads (a `#qflood`). Re-issue the pause on the fresh supervisor: `agent-doc admin queue pause <FILE> --observed-generation <N> --reason …` — it rejects `missing_observed_generation` and prints `current_generation=<N>`; retry with that N. Also: the recorded `queue_pause_reason` string is a persisted field, NOT live state — it keeps showing the OLD stale-CRDT text even after a clean recycle; don't read it as current. After recycle, tell the operator to RE-TEST the edits; if they still misbehave on the fresh binary it's the genuine deeper editor-edit-during-cycle guard. Do NOT `cargo install` mid-session to "fix" it — that just makes the finalize client newer than the running supervisor (see [[feedback_no_mid_session_install_against_live_supervisor]]). Related: [[feedback_supervisor_autoinstall_drives_install_not_manual]].
