---
name: feedback_wedged_controller_and_jb_ide_spin_on_corrupt_crdt_response_in_head
description: "A corrupt CRDT response buffer makes BOTH the shared controller and the JB IDE spin at 100%+ CPU; response is already clean in HEAD — restart controller, checkout HEAD, purge sidecar, user reopens IDE; do NOT build a concurrent session's uncommitted controller WIP."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: ef8e1c10-0ff0-43ff-802a-7cc1bf4d1f52
---

Preflight looped on `state_backbone_response_captured` (`ipc_proof_insufficient`, `recovery=retry_without_disk_write`) for `tasks/recruit/haiven.md`. Every preflight replayed a "captured CRDT patchback" that merged into a **scrambled, duplicated** doc (`repaired_len` 30000–39155 vs clean HEAD 21936; numbered lists reversed, bullets bottom-to-top) — classic failed-IPC append scramble of content **already committed clean in HEAD**.

**Root topology (this variant):** the shared project controller (`controller serve --launch-mode lazy`, PID at **100%+ CPU** for 11+ min) AND the operator's **JetBrains IDE** (`idea`, **113% CPU for 6.5 HOURS**) were BOTH spin-wedged trying to converge the same corrupt haiven CRDT buffer. The scramble lived in the IDE's in-memory CRDT (`socket already_applied` = "response already in live buffer"), NOT in `.agent-doc/state.db` (`document_cycles` had zero haiven rows). Restarting the controller alone did NOT help: a fresh lazy controller reloaded/re-synced the corrupt buffer from the IDE and re-wedged to ~100% within minutes.

**Why:** the phantom "captured response" is just the DIFF between the corrupt IDE buffer (35000) and the clean snapshot (21936); preflight keeps persisting it. A JB close/reopen does NOT reliably clear stale-cdylib CRDT corruption (see [[feedback_jb_restart_reopen_dont_fix_stale_cdylib_corruption]]) — but killing the 6.5hr-spinning `idea` process via reopen DID end that spin here.

**How to apply (order matters):**
1. Verify the real deliverable is safe: `git show HEAD:<file>` — the well-formed response was already committed (it even carried a prior recovery note). This is a **false blocker**; git is authoritative ([[feedback_forcedisk_committed_but_false_closeout_error_stop_retrying]]).
2. `git checkout HEAD -- <file>` to discard the scrambled disk copy (confirm the diff is ONLY scrambled duplicates of HEAD content — no new operator prompt/queue item — before discarding).
3. Kill the spin-wedged **shared controller** (verify by `%CPU`/`cputime`, not lstart; it is NOT your ancestor — your `start --route-owned <file>` supervisor IS your ancestor and `admin kill-supervisor` refuses it). It relaunches lazily.
4. Purge disk-side poison: `mv .agent-doc/crdt/<dochash>.yrs` aside, `rm` the stale `.yrs.lock` (0-byte, no `fuser` holder), `mv .agent-doc/state/cycles/<dochash>.json` aside. `<dochash>` here was `e715e181…`.
5. `agent-doc reset <file> --from-current --force-disk` (NO `--preserve-session`) rebuilds CRDT from clean disk. It contends on the sidecar lock held by the live editor.
6. The IDE re-asserts as editor authority. Ask the operator to **close/reopen the doc tab (or restart the IDE)** — this is their call (don't kill their IDE unilaterally). Disk is clean so it reloads clean; it also ends any multi-hour IDE spin.
7. **STOP running preflight** — each run re-materializes the scramble and re-wedges a controller. `admin inspect` is a safe lightweight check; preflight's repair step is not.

**Do NOT `make install` to "fix" it here:** the agent-doc submodule had **uncommitted** `agent-doc-controller-io/src/project_controller/rpc.rs` + `main.rs` changes owned by a **concurrent codex `agent-doc-bugs2` session** actively fixing this exact controller-RPC/convergence bug. Building another session's WIP is the anti-pattern in [[feedback_concurrent_agent_doc_refactor_session_build_coordination]]. The real fix ships when that session commits + installs. Keep the concurrent `siemens` session untouched throughout (separate `document_id`/supervisor).
