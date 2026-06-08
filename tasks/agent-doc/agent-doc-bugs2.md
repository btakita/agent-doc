---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

 <!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260608-231427.md`*

Compacted content:
- Archived 6 response topic(s): do [#nm1x-codex-clear-parity]; investigate agent-doc churn / IPC drift / File Cache Conflict; investigate agent-doc churn / IPC drift / File Cache Conflict; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
<!-- agent:boundary:36457be2 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- :round_pushpin: [#ipc-early-ack]
- :pushpin: [#pcp-consolidate]
- :round_pushpin: [#ipc-degraded-prefers-file-ipc]
- :round_pushpin: [#ipc-drift-writeprovenance]
- :round_pushpin: Fix duplicate ids that originated from the item content. `[#stale-capture-captured-only-drift] [recommended] [#stale-capture-captured-only-drift]` should be `[#stale-capture-captured-only-drift] [recommended]`.
- :pushpin: Save "Process & race-condition map" content into document. The #mermaid is nice and should be document. Make this evergreen content so updates to the architecture are reflected.
- Fix **R1 — two file-watchers / two writers.** The plugin's WatchService *and* the supervisor's file-watcher both observe the doc; the plugin's `saveDocument` *and* the binary's disk fallback both write it → "File Cache Conflict" + drift. Can we replace the plugin's WatchService with a client for the supervisor's file-watcher? Would the IPC lag cause race conditions? Can the race condition be prevented in the overall system?
- Fix **R2 — EDT/save lag vs ack budget.** The listener dispatches to the EDT, then `saveDocument` + the content sidecar land *after* the EDT runs — lagging the sender's poll budget → false ack-timeout / false degrade vote (partially fixed by `#ipc-ack-timeout-align` + `#ipc-degrade-false-vote`).
- Fix **R3 — disk fallback** manufactures the foreign-write that IntelliJ flags as a cache conflict whenever the session is latched degraded.
- Fix **R4 — mtime-heuristic drift.** `live_buffer_diverges_from_content` infers "foreign disk write vs genuine unsaved edit" from `LIVE_BUFFER_STALE_SKEW_MS` mtime only — no buffer content — so a real concurrent edit fails closed (`live_prompt_drift_after_preflight`).
- Fix **R5 — TOCTOU** between the FFI socket handler and the WatchService for the same patch; today guarded best-effort by EDT re-checks (dedup + generation fence), but a window remains.
- Fix **R6 — supervisor self-race.** A route-owned supervisor in Process 1 races the agent's own finalize writes on the same pane → "could not drain the active closeout" / exit 75.
- `:pushpin:` should not be reprioritized by the `priority` attribute. It should remain exactly where I placed it. If I add an item to a certain position in the queue, it should remain there with a `:pushpin:`.
- The responses are being intermixed with older responses in the document. We are also losing part of the response.
- What part of the stack runs in the Project Control Pane process? What are separate processes? Does the Plugin IPC live in the editor process or can it be moved into the Project Control Pane process? Draw a #mermaid.
---
After "I did not auto-loop: you're actively editing, which takes precedence over the queue drain.", I ran JB `Run Agent Doc` and got an error.
```
[route] target tmux session: 0
[preflight] review: 2 item(s), 2 gated
Error: authoritative actor generation 246 for tasks/agent-doc/agent-doc-bugs2.md owns pane %30 but route could not drain the active closeout before dispatch: captured response baseline no longer matches current document for /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. Rebuild sidecars without clearing session state: `agent-doc reset --from-current --preserve-session /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md`
```
The queue then started auto-draining about 20s later.
---
Fix the root cause of the compounding stale-capture deadlock
```
● The deadlock now also shows a supervisor/route race: "authoritative actor generation 246 owns pane %30 but route could
  not drain the active closeout" — a route-owned supervisor is racing the closeout (a known self-race pattern),
  compounding the stale-capture deadlock. Let me run the binary-recommended reset --from-current --preserve-session to
● Bash(cd /home/brian/work/btakita/agent-loop
      agent-doc reset --from-current --preserve-session tasks/agent-doc/agent-doc-bugs2.md 2>&1 | tail -3…)
  ⎿  Reset sidecars for tasks/agent-doc/agent-doc-bugs2.md from current file while preserving session state
     === cycle phase after reset ===
     phase: preflight_started capture_id: None
● The reset cleared the deadlock — cycle is now clean preflight_started, capture_id: None. Document is quiescent. Let me
  get a fresh baseline and finalize my response in one go:
```
---
- The agent-doc queue should not stop if I'm editing content not related to the current turn.
- On JB `Run Agent Doc`, the `/clear` opt-in should allow for pre-emptively run `/clear` if the threshold is exceeded (e.g. 50%). 50% should be the default threshold. There should be a way to set the opt-in threshold in the agent doc, and a default opt-in threshold in the project config.
- `/agent-doc tasks/agent-doc/agent-doc-bugs2.md` and `/clear` inputs are accumulating as the queue runs. Please ensure the console is idle before sending those messages. Send only 1 command at a time. Do not schedule multiple of the same command. So if there is already a `/agent-doc tasks/agent-doc/agent-doc-bugs2.md` scheduled, to not schedule another `/agent-doc tasks/agent-doc/agent-doc-bugs2.md`. Same with `/clear`.
<!-- /agent:queue -->
###
<!--
`'/clear' is disabled while a task is in progress
---
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added.
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
- This session's console stdout is bleeding in front of the opencode tui. ![img_16.png](img_16.png)
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority --> 
- [ ] [#queue-operator-pin-position-lock-dag] Extend operator-pin position-lock into the auto-DAG ready-set (queue::sort_prompts_by_dag). Today position-lock only applies to sort_prompts_by_priority (the no-deps path); when after=#id edges exist the DAG tie-break still floats operator pins by tier. Anchor operator pins to their slots in the topo order too, keeping the documented blocker-outranks-pin exception. Spec anchor: specs/pending-system.md #queue-auto-dag-priority.
- [ ] [#pcp-consolidate] [recommended] Consolidate document-state authority into the Project Control Pane process: make the supervisor the single disk writer + single file-watcher, demote the plugin WatchService to read-only buffer-state reporting (no autonomous reconcile/write), and reduce the FFI socket listener to a thin EDT apply+ack shim. Serialize supervisor idle-queue/file-watch writes and agent finalize writes through one in-process write queue to kill the supervisor self-race. Depends on/absorbs #ipc-drift-writeprovenance (provenance/generation id) + #nm1x-livebuf-content (buffer content in sidecar) so drift is positively classified instead of inferred from the LIVE_BUFFER_STALE_SKEW_MS mtime heuristic.
- [ ] [#stale-capture-captured-only-drift] [recommended] Evaluate extending the #stale-capture-deadlock-autoretire retire path to Captured-only orphans (write never attempted) whose baseline drifted. Today only WriteApplied captures auto-retire; a Captured-only orphan + drift still fail-closes on validate_replay and needs a manual reset --from-current --preserve-session. Gate on positive superseding-turn evidence before retiring. Lower priority than upstream #ipc-drift-writeprovenance.
- [ ] [#ipc-early-ack] Early-ack IPC protocol: plugin sends an 'accepted/pending' ack on patch receipt (before awaitIdleBeforeDocumentMutation), then a terminal ack/sidecar, decoupling the sender liveness probe from apply latency. FFI-first (ipc_socket protocol + ffi.rs listener contract, then PatchWatcher.handleSocketMessageV2). Plan: tasks/agent-doc/plan-ipc-transport-reliability.md
- [ ] [#ipc-degraded-prefers-file-ipc] When genuinely degraded, route writes through the file-IPC patch queue (plugin still applies via Document API) instead of a raw disk write, so a degraded session never manufactures IDEA File Cache Conflicts. Disk write becomes true last resort. Plan: tasks/agent-doc/plan-ipc-transport-reliability.md
- [ ] [#ipc-drift-writeprovenance] Tag agent-doc's own disk writes with a write-provenance/generation id so guard_visible_write_reconcile / live_buffer_diverges_from_content positively attribute a foreign disk change to the supervisor instead of inferring from the LIVE_BUFFER_STALE_SKEW_MS mtime heuristic. Builds on #op-scoped-drift-* provenance. Plan: tasks/agent-doc/plan-ipc-transport-reliability.md
- [ ] [#nm1x-livebuf-content] [recommended] Capture editor-buffer CONTENT (not just len/hash) in the .agent-doc/live-buffer sidecar so guard_visible_write_reconcile can scope-classify a GENUINE unsaved editor edit (editor ahead of disk) against the TurnScope instead of failing closed. Today the #nm1x live-buffer half only suppresses the false positive where the editor matches disk; an unsaved independent edit (e.g. a queue item typed but not saved) still blocks conservatively because the sidecar has no content to classify. Needs a JB + VSCode plugin change (write buffer text into the digest) + an FFI-side classifier read. Plan: tasks/agent-doc/plan-operation-scoped-drift.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
1. [/] [#ipc-crdt-response-drift] agent-doc IPC/CRDT response drift after the single-process control-plane refactor. TWO LAYERS FIXED + INSTALLED: (1) merge layer c9d276b9 — crdt::merge protects committed ### Re: history from a stale/divergent theirs; (2) visible-buffer layer 51b9ed40 — live_buffer_diverges_from_content suppresses a stale live-buffer sidecar (disk modified after the editor last reported = lagging foreign disk write, not a user edit), killing the false-positive 'visible editor buffer differs' finalize block. Both shipped via cargo install + build --release + agent-doc lib-install (0.34.0). REMAINING (still gated, needs clean-session live repro): reversed-lines/hoisted-above-prior symptom under theirs>base in the IPC node-patch/boundary-append path (ipc_socket.rs, template boundary, stream.rs), and write-back serialization across concurrent supervisors. Plan: tasks/agent-doc/plan-ipc-crdt-response-drift.md
2. [/] [#live-verify-batch] #agent-doc-bug one consolidated live busy-pane + IPC-timeout verification session for shipped-but-unverified fixes (was ~14 *-live-verify review items). Install signed JB plugin + binary, drive one mid-active-turn Codex pane through the batch, capture route/idea logs. Blocked on live IntelliJ/Codex panes.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
