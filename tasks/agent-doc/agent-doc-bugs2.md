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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260609-050025.md`*

Compacted content:
- Archived 1 response topic(s): do [#ipc-early-ack]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: do [#ipc-degraded-prefers-file-ipc] — opus-4-8

> **Queue prompt:**
>
> :round_pushpin: do [#ipc-degraded-prefers-file-ipc]


Done — Phase 3 of `plan-ipc-transport-reliability.md`. A degraded session now routes writes through the file-IPC patch queue instead of a raw disk write, so it never manufactures an IDEA "File Cache Conflict".

**Root cause:** a latched-degraded socket only means the plugin's *socket* listener is wedged. The file-IPC patch queue uses a *separate* plugin file watcher that is almost always still alive. The old code conflated the two — on degrade it returned straight to a raw foreign disk write (the exact thing IntelliJ flags as a cache conflict on the open buffer).

**Fix (all in `write.rs`):**
- `try_ipc` no longer early-returns to disk on degrade. It skips only the wedged socket attempt and falls through to the file-IPC patch queue (`write_ipc_and_poll`), where the plugin applies via the Document API.
- The socket-ack-timeout arm that *newly* latches degraded also falls through to file IPC instead of returning.
- `run_stream` always calls `try_ipc` when the plugin is installed (dropped the `!ipc_direct_disk_degraded` gate + its dead direct-disk branch), so the stream/finalize path inherits the routing.
- Raw disk write stays the **true last resort** — reached only on the existing IPC-timeout (`exit 75`) path when file IPC *also* fails to deliver. The fire-and-forget `reposition_boundary` signal keeps its direct-disk-skip (no response body).

**Verification:** `make check` green (clippy clean). New tests: `try_ipc_prefers_file_ipc_when_socket_degraded` (no plugin → routes through file IPC, times out, disk last resort) and `try_ipc_degraded_succeeds_via_file_ipc_when_plugin_consumes` (live file watcher → degraded write succeeds via plugin, no disk write). FlowCore `reason=` budget bumped 29→30 for the audited prefer-file-ipc diagnostic. Spec: `process-topology.md` R3 ✅.

**Shipped:** `cargo install` + `cargo build --release` both rebuilt 0.34.0. Submodule pushed (`0b931d04`); superproject pointer + plan advanced. CI in progress for the pushed commit — local suite is green.

Note: this is the file-IPC-routing half of review item `#pcp5`; the early-ack Kotlin half (`#ipc-early-ack`, Phase 2) is still its gated remainder and needs live IntelliJ verification.
<!-- agent:boundary:e98b6ccc -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~:round_pushpin: do [#ipc-degraded-prefers-file-ipc]~~
- :round_pushpin: do [#loop-guard-exchange-node-granularity]
- :round_pushpin: do [#queue-completed-items-escape-below-component]
- :round_pushpin: do [#queue-operator-pin-position-lock-dag]
- :round_pushpin: do [#stale-capture-captured-only-drift]
- :round_pushpin: do [#queue-contamination-guard-false-positive]
- :round_pushpin: do [#nm1x-livebuf-content]
- :round_pushpin: do [#clear-opt-in-threshold]
- :round_pushpin: do [#console-input-accumulation]
- :round_pushpin: do [#vbk7]
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

<!-- agent:backlog priority queue --> 
- [ ] [#loop-guard-exchange-node-granularity] [recommended] The #queue-no-stop-unrelated-edit loop-guard fix scopes turn-affectedness at whole-component exchange granularity (TurnScope write_set holds exchange:0), so an edit to an OLD/unrelated response block inside exchange still classifies as turn-affecting and preempts the drain. Narrow the exchange address in TurnScope to the active turn response node(s) so only edits to the in-scope exchange tail preempt. Overlaps op-scoped-drift node granularity.
- [ ] [#vbk7] #pcpc4b plugin WatchService read-only demotion (realizes #pcp7) — change editors/jetbrains WatchService to a read-only buffer reporter (no independent reconcile), so the single controller-owned watcher (#pcpc4) is the only event source. Needs live IntelliJ verification; pairs with #pcpc5 live-tmux/editor cutover. Plan: tasks/agent-doc/plan-supervisor-in-process.md
- [ ] [#clear-opt-in-threshold] JB Run Agent Doc /clear opt-in should pre-emptively run /clear when the context threshold is exceeded; default 50%, configurable per-doc (frontmatter) and via a project-config default (user request, relocated from queue).
- [ ] [#console-input-accumulation] /agent-doc + /clear console inputs accumulate as the queue runs; ensure the console is idle before sending, send one command at a time, and never schedule a duplicate /agent-doc or /clear when one is already pending (user request, relocated from queue).
- [ ] [#queue-contamination-guard-false-positive] session-check #jb-run-agent-doc-response-queue-contamination false-positives on a legit user queue prompt that mentions slash commands (/agent-doc, /clear) because it resembles response prose; the heuristic must not flag genuine user prompts as contamination.
- [ ] [#queue-completed-items-escape-below-component] Completed/struck queue items can be written BELOW the `<!-- /agent:queue -->` closing marker, landing inside the adjacent HTML-comment "parking lot" instead of staying inside the `agent:queue` component (struck, until reaped) or being reaped into `agent:done`. Repro signature: after a closeout, the queue component renders empty while N `- ~~:pushpin: [...]~~` lines appear in the comment block beneath it (invisible in rendered markdown = orphaned residue). Observed 2026-06-08 as 12 struck lines in the UNCOMMITTED working tree only — committed HEAD was clean — so this is post-commit closeout drift, not the deterministic strike path (`preflight.rs:4660` `q.replace_content` writes inside the component). Most likely a CRDT/boundary merge displacing the queue's struck content past the component's closing boundary into the neighboring comment; same family as #ipc-crdt-response-drift / R1–R6. Fix: queue strike/reap must keep struck items within the queue component span and reaped items in `agent:done`; add a boundary-displacement guard (reject/repair queue content emitted after `<!-- /agent:queue -->`) + SimWorld coverage. Mechanism not yet pinned to an exact code path — root-cause before fixing.
- [ ] [#queue-operator-pin-position-lock-dag] Extend operator-pin position-lock into the auto-DAG ready-set (queue::sort_prompts_by_dag). Today position-lock only applies to sort_prompts_by_priority (the no-deps path); when after=#id edges exist the DAG tie-break still floats operator pins by tier. Anchor operator pins to their slots in the topo order too, keeping the documented blocker-outranks-pin exception. Spec anchor: specs/pending-system.md #queue-auto-dag-priority.
- [ ] [#stale-capture-captured-only-drift] [recommended] Evaluate extending the #stale-capture-deadlock-autoretire retire path to Captured-only orphans (write never attempted) whose baseline drifted. Today only WriteApplied captures auto-retire; a Captured-only orphan + drift still fail-closes on validate_replay and needs a manual reset --from-current --preserve-session. Gate on positive superseding-turn evidence before retiring. Lower priority than upstream #ipc-drift-writeprovenance.
- [ ] [#ipc-degraded-prefers-file-ipc] When genuinely degraded, route writes through the file-IPC patch queue (plugin still applies via Document API) instead of a raw disk write, so a degraded session never manufactures IDEA File Cache Conflicts. Disk write becomes true last resort. Plan: tasks/agent-doc/plan-ipc-transport-reliability.md
- [ ] [#nm1x-livebuf-content] [recommended] Capture editor-buffer CONTENT (not just len/hash) in the .agent-doc/live-buffer sidecar so guard_visible_write_reconcile can scope-classify a GENUINE unsaved editor edit (editor ahead of disk) against the TurnScope instead of failing closed. Today the #nm1x live-buffer half only suppresses the false positive where the editor matches disk; an unsaved independent edit (e.g. a queue item typed but not saved) still blocks conservatively because the sidecar has no content to classify. Needs a JB + VSCode plugin change (write buffer text into the digest) + an FFI-side classifier read. Plan: tasks/agent-doc/plan-operation-scoped-drift.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
1. [/] [#ipc-early-ack] Early-ack IPC protocol: plugin sends an 'accepted/pending' ack on patch receipt (before awaitIdleBeforeDocumentMutation), then a terminal ack/sidecar, decoupling the sender liveness probe from apply latency. FFI-first (ipc_socket protocol + ffi.rs listener contract, then PatchWatcher.handleSocketMessageV2). Plan: tasks/agent-doc/plan-ipc-transport-reliability.md
2. [/] [#pcpc5] PCP-consol phase 5 — 08b migration cutover (shadow -> dual-write -> read-switch -> authority -> removal of the out-of-process supervisor + direct-disk writers). HARD-BLOCKED from any normal agent-doc response cycle by spec 08b (specs/08b-single-process-control-plane.md, Document write/watch authority): "Behavior changes here must never be landed in the same cycle that persists a live session response through the write path being changed" — every agent-doc turn persists its response through that exact write path. Run in a fresh, non-saturated session (this one keeps hitting #ipc-crdt-response-drift on closeout) with live-tmux + CLI + SimWorld + live IntelliJ proof, decomposed one actionable id per gate (pcpc5a..e). Pairs with #vbk7 and review #pcp7. Plan: tasks/agent-doc/plan-supervisor-in-process.md
3. [/] [#pcp6] PCP phase 6 — Capture editor-buffer CONTENT in the live-buffer sidecar (absorbs #nm1x-livebuf-content) so a genuine unsaved edit is scope-classified vs failing closed (R4). JB+VSCode plugin + FFI classifier. Requires live editor verification. Plan: tasks/agent-doc/plan-pcp-consolidate.md
4. [/] [#pcp7] PCP phase 7 — Demote plugin WatchService to read-only buffer reporting + reduce FFI socket listener to a thin EDT apply+ack shim (closes R1/R2/R5 TOCTOU). BLOCKED on supervisor-in-process consolidation (#pcp-consol) AND requires live IntelliJ/Codex verification; pairs with #live-verify-batch. Plan: tasks/agent-doc/plan-pcp-consolidate.md
5. [/] [#pcp5] PCP phase 5 — Early-ack IPC + degraded-prefers-file-IPC (absorbs #ipc-early-ack, #ipc-degraded-prefers-file-ipc). FFI-first: accepted/pending ack on receipt + terminal ack; degraded routes through file-IPC, never raw disk. GATED: the early-ack emit is a Kotlin PatchWatcher change needing live IntelliJ/Codex verification, and the IPC degrade routing is delicate state-machine code best done in a fresh focused session (this session context is saturated and its closeouts keep fighting the IPC drift). Plan: tasks/agent-doc/plan-pcp-consolidate.md
6. [/] [#pcp4] PCP phase 4 — Single controller-owned filesystem watcher replacing per-process notify watchers (kills R1 two-watchers). BLOCKED on supervisor-in-process consolidation (#pcp-consol); deferred per operator decision to ship unblocked wins first. Plan: tasks/agent-doc/plan-pcp-consolidate.md
7. [/] [#pcp3] PCP phase 3b — In-process ordered write queue in the session actor; serialize supervisor idle/file-watch writes + agent finalize writes. BLOCKED on supervisor-in-process consolidation (supervisor and agent are separate processes today, flock-only). Gated until the single-process control-plane consolidation lands. Plan: tasks/agent-doc/plan-pcp-consolidate.md
8. [/] [#ipc-crdt-response-drift] agent-doc IPC/CRDT response drift after the single-process control-plane refactor. TWO LAYERS FIXED + INSTALLED: (1) merge layer c9d276b9 — crdt::merge protects committed ### Re: history from a stale/divergent theirs; (2) visible-buffer layer 51b9ed40 — live_buffer_diverges_from_content suppresses a stale live-buffer sidecar (disk modified after the editor last reported = lagging foreign disk write, not a user edit), killing the false-positive 'visible editor buffer differs' finalize block. Both shipped via cargo install + build --release + agent-doc lib-install (0.34.0). REMAINING (still gated, needs clean-session live repro): reversed-lines/hoisted-above-prior symptom under theirs>base in the IPC node-patch/boundary-append path (ipc_socket.rs, template boundary, stream.rs), and write-back serialization across concurrent supervisors. Plan: tasks/agent-doc/plan-ipc-crdt-response-drift.md
9. [/] [#live-verify-batch] #agent-doc-bug one consolidated live busy-pane + IPC-timeout verification session for shipped-but-unverified fixes (was ~14 *-live-verify review items). Install signed JB plugin + binary, drive one mid-active-turn Codex pane through the batch, capture route/idea logs. Blocked on live IntelliJ/Codex panes.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
