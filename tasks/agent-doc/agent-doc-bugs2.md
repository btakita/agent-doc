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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260609-024137.md`*

Compacted content:
- Archived 14 response topic(s): #pcp-consolidate; #pcp1; #pcp2; 11 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: R1–R6 + remaining queue items — opus-4-8

Addressing the remaining queue prompts together — all are now resolved or tracked, so I cleared them from the queue (nothing dropped silently; here's the disposition of each):

- **R1 — two file-watchers / two writers / can we replace the plugin WatchService / would IPC lag race / can it be prevented:** answered in `specs/process-topology.md` (the "Can the Plugin IPC move into the PCP process?" section) — the plugin stays in the editor process but is demoted to a read-only reporter + apply/ack shim; the single controller-owned watcher (`#pcpc4`) + WatchService demotion (`#pcp7`) remove the two-watchers race; IPC lag stops mattering because the editor is no longer an independent writer.
- **R2 — EDT/save lag vs ack budget:** `#pcp5` early-ack IPC (partially done via `#ipc-ack-timeout-align` + `#ipc-degrade-false-vote`).
- **R3 — disk fallback manufactures the cache conflict when degraded:** `#pcp5` degraded-prefers-file-IPC.
- **R4 — mtime-heuristic drift / `live_buffer_diverges_from_content` fails closed:** **fixed** — `#pcp2` (write-provenance) + `#pcp6` (editor-buffer content); your 0.2.155 install stopped the drift.
- **R5 — TOCTOU between FFI socket handler and WatchService:** `#pcp7` thin apply+ack shim (closes the window).
- **R6 — supervisor self-race / "could not drain the active closeout" / exit 75:** **mitigated** by `#pcp3a` (drain race-hardening, shipped); rooted out by `#pcpc3` (in-process write queue).
- **`:pushpin:` should not be reprioritized by `priority`:** already implemented — operator-pin position-lock applies to the priority sort; extending it to the auto-DAG ready-set is tracked as `#queue-operator-pin-position-lock-dag`.
- **responses intermixed / losing part of the response:** that was `#ipc-crdt-response-drift` — fixed by `#pcp2`+`#pcp6` (kept in `agent:review` for the live-repro signoff).
- **What runs in the PCP process / draw a #mermaid:** the full answer + current and target mermaids are in `specs/process-topology.md`.

The full R1–R6 race→fix table and both topology diagrams live in the evergreen `specs/process-topology.md`. With these dispositioned, the queue is drained — the only substantial open work is `#pcpc1`–`#pcpc5` (the consolidation, fresh session).

### Re: do [#pcpc1] — Session-actor message runtime — opus-4-8

> **Queue prompt:**
>
> do [#pcpc1]


Done — `#pcpc1` (PCP-consol phase 1) is implemented and shipped in both binaries.

**What landed** (`agent-doc-orchestration/src/session_actor.rs`): turned the session actor from a pure data/CAS struct into a real in-process actor runtime.

- `DocumentActor::spawn` — a per-document mailbox (`mpsc` channel) drained on a single owning thread, named `session-actor:<doc-id>`. All document-scoped ops for one document now run in one serialized order.
- `submit` / `enqueue` — typed `submit::<R>(kind, job)` runs a closure on the owner thread and returns its result; `enqueue` is the non-blocking pipeline variant. `SessionOpKind` (`WriteSubmit`, `Closeout`, `QueueHead`, `Lifecycle`) carries the `08b` op taxonomy and is surfaced via `ActorContext::last_op()` as a #pcpc3 branch hook.
- `SessionActorRuntime` + `runtime()` singleton + `document_actor_in(base_dir, file)` — a process-wide registry that funnels every caller for one canonical document id into the same mailbox, so serialization holds across call sites, not just within one. Clean `Drop` drops the sender then joins the owner thread.
- SQLite CAS stays the durable cross-process backstop; the mailbox serializes the same-process writers it could not order before.

**Scope:** phase 1 is the runtime only — existing call sites are unchanged so it lands independently shippable. `#pcpc3` (already queued) routes the concrete `write`/`stream`/`finalize`/repair + supervisor idle/file-watch disk writes through `DocumentActor::submit`.

**SimWorld determinism (3 new tests, all green):**
- `..._serializes_single_producer_in_fifo_order` — N pipelined ops drain in FIFO submission order; `processed` counter strictly increases.
- `..._serializes_concurrent_producers_without_overlap` — 32 producer threads hit one actor at a barrier; a re-entrancy guard proves no two ops overlap and each runs exactly once.
- `..._registry_funnels_one_document_to_one_mailbox` — same doc id → one shared actor (`Arc::ptr_eq`), distinct docs → distinct actors, ordered ops via either handle.

**Verification:** `make check` green (clippy `-D warnings` + full test suite); `cargo build --release` + `cargo install --path .` both refreshed; latest agent-doc CI run green pre-push.
<!-- agent:boundary:5b65157e -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~do [#pcpc1]~~
- do [#pcpc2]
- do [#pcpc3]
- do [#pcpc4]
- do [#pcpc5]
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
- [ ] [#pcpc2] PCP-consol phase 2 — In-process supervisor adapter: host the harness child PTY/stdin/restart/heartbeat as a controller-managed task instead of a separate OS process; preserve the Unix-socket external boundary for CLI/editor callers. Plan: tasks/agent-doc/plan-supervisor-in-process.md
- [ ] [#pcpc3] PCP-consol phase 3 — In-process ordered write queue (realizes #pcp3b): route all write/stream/finalize/repair + supervisor idle/file-watch writes through the session actor queue; flock becomes cross-process backstop only. Kills R6/exit-75 at the root. SimWorld. Plan: tasks/agent-doc/plan-supervisor-in-process.md
- [ ] [#pcpc4] PCP-consol phase 4 — Single controller-owned filesystem watcher (realizes #pcp4) + demote plugin WatchService to read-only (realizes #pcp7; needs live verification). Plan: tasks/agent-doc/plan-supervisor-in-process.md
- [ ] [#pcpc5] PCP-consol phase 5 — Migration cutover via the 08b gate ladder (shadow -> dual-write -> read-switch -> authority -> removal), each with a rollback flag; remove the out-of-process supervisor + direct-disk writers only after SimWorld/CLI/live-tmux proof. Plan: tasks/agent-doc/plan-supervisor-in-process.md
- [ ] [#queue-no-stop-unrelated-edit] Queue should not stop when the user edits content unrelated to the current turn (user request, relocated from queue). Don't halt the drain on independent edits; only a real user prompt mid-loop should preempt.
- [ ] [#clear-opt-in-threshold] JB Run Agent Doc /clear opt-in should pre-emptively run /clear when the context threshold is exceeded; default 50%, configurable per-doc (frontmatter) and via a project-config default (user request, relocated from queue).
- [ ] [#console-input-accumulation] /agent-doc + /clear console inputs accumulate as the queue runs; ensure the console is idle before sending, send one command at a time, and never schedule a duplicate /agent-doc or /clear when one is already pending (user request, relocated from queue).
- [ ] [#queue-contamination-guard-false-positive] session-check #jb-run-agent-doc-response-queue-contamination false-positives on a legit user queue prompt that mentions slash commands (/agent-doc, /clear) because it resembles response prose; the heuristic must not flag genuine user prompts as contamination.
- [ ] [#queue-completed-items-escape-below-component] Completed/struck queue items can be written BELOW the `<!-- /agent:queue -->` closing marker, landing inside the adjacent HTML-comment "parking lot" instead of staying inside the `agent:queue` component (struck, until reaped) or being reaped into `agent:done`. Repro signature: after a closeout, the queue component renders empty while N `- ~~:pushpin: [...]~~` lines appear in the comment block beneath it (invisible in rendered markdown = orphaned residue). Observed 2026-06-08 as 12 struck lines in the UNCOMMITTED working tree only — committed HEAD was clean — so this is post-commit closeout drift, not the deterministic strike path (`preflight.rs:4660` `q.replace_content` writes inside the component). Most likely a CRDT/boundary merge displacing the queue's struck content past the component's closing boundary into the neighboring comment; same family as #ipc-crdt-response-drift / R1–R6. Fix: queue strike/reap must keep struck items within the queue component span and reaped items in `agent:done`; add a boundary-displacement guard (reject/repair queue content emitted after `<!-- /agent:queue -->`) + SimWorld coverage. Mechanism not yet pinned to an exact code path — root-cause before fixing.
- [ ] [#queue-operator-pin-position-lock-dag] Extend operator-pin position-lock into the auto-DAG ready-set (queue::sort_prompts_by_dag). Today position-lock only applies to sort_prompts_by_priority (the no-deps path); when after=#id edges exist the DAG tie-break still floats operator pins by tier. Anchor operator pins to their slots in the topo order too, keeping the documented blocker-outranks-pin exception. Spec anchor: specs/pending-system.md #queue-auto-dag-priority.
- [ ] [#stale-capture-captured-only-drift] [recommended] Evaluate extending the #stale-capture-deadlock-autoretire retire path to Captured-only orphans (write never attempted) whose baseline drifted. Today only WriteApplied captures auto-retire; a Captured-only orphan + drift still fail-closes on validate_replay and needs a manual reset --from-current --preserve-session. Gate on positive superseding-turn evidence before retiring. Lower priority than upstream #ipc-drift-writeprovenance.
- [ ] [#ipc-early-ack] Early-ack IPC protocol: plugin sends an 'accepted/pending' ack on patch receipt (before awaitIdleBeforeDocumentMutation), then a terminal ack/sidecar, decoupling the sender liveness probe from apply latency. FFI-first (ipc_socket protocol + ffi.rs listener contract, then PatchWatcher.handleSocketMessageV2). Plan: tasks/agent-doc/plan-ipc-transport-reliability.md
- [ ] [#ipc-degraded-prefers-file-ipc] When genuinely degraded, route writes through the file-IPC patch queue (plugin still applies via Document API) instead of a raw disk write, so a degraded session never manufactures IDEA File Cache Conflicts. Disk write becomes true last resort. Plan: tasks/agent-doc/plan-ipc-transport-reliability.md
- [ ] [#ipc-drift-writeprovenance] Tag agent-doc's own disk writes with a write-provenance/generation id so guard_visible_write_reconcile / live_buffer_diverges_from_content positively attribute a foreign disk change to the supervisor instead of inferring from the LIVE_BUFFER_STALE_SKEW_MS mtime heuristic. Builds on #op-scoped-drift-* provenance. Plan: tasks/agent-doc/plan-ipc-transport-reliability.md
- [ ] [#nm1x-livebuf-content] [recommended] Capture editor-buffer CONTENT (not just len/hash) in the .agent-doc/live-buffer sidecar so guard_visible_write_reconcile can scope-classify a GENUINE unsaved editor edit (editor ahead of disk) against the TurnScope instead of failing closed. Today the #nm1x live-buffer half only suppresses the false positive where the editor matches disk; an unsaved independent edit (e.g. a queue item typed but not saved) still blocks conservatively because the sidecar has no content to classify. Needs a JB + VSCode plugin change (write buffer text into the digest) + an FFI-side classifier read. Plan: tasks/agent-doc/plan-operation-scoped-drift.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
1. [/] [#pcp6] PCP phase 6 — Capture editor-buffer CONTENT in the live-buffer sidecar (absorbs #nm1x-livebuf-content) so a genuine unsaved edit is scope-classified vs failing closed (R4). JB+VSCode plugin + FFI classifier. Requires live editor verification. Plan: tasks/agent-doc/plan-pcp-consolidate.md
2. [/] [#pcp7] PCP phase 7 — Demote plugin WatchService to read-only buffer reporting + reduce FFI socket listener to a thin EDT apply+ack shim (closes R1/R2/R5 TOCTOU). BLOCKED on supervisor-in-process consolidation (#pcp-consol) AND requires live IntelliJ/Codex verification; pairs with #live-verify-batch. Plan: tasks/agent-doc/plan-pcp-consolidate.md
3. [/] [#pcp5] PCP phase 5 — Early-ack IPC + degraded-prefers-file-IPC (absorbs #ipc-early-ack, #ipc-degraded-prefers-file-ipc). FFI-first: accepted/pending ack on receipt + terminal ack; degraded routes through file-IPC, never raw disk. GATED: the early-ack emit is a Kotlin PatchWatcher change needing live IntelliJ/Codex verification, and the IPC degrade routing is delicate state-machine code best done in a fresh focused session (this session context is saturated and its closeouts keep fighting the IPC drift). Plan: tasks/agent-doc/plan-pcp-consolidate.md
4. [/] [#pcp4] PCP phase 4 — Single controller-owned filesystem watcher replacing per-process notify watchers (kills R1 two-watchers). BLOCKED on supervisor-in-process consolidation (#pcp-consol); deferred per operator decision to ship unblocked wins first. Plan: tasks/agent-doc/plan-pcp-consolidate.md
5. [/] [#pcp3] PCP phase 3b — In-process ordered write queue in the session actor; serialize supervisor idle/file-watch writes + agent finalize writes. BLOCKED on supervisor-in-process consolidation (supervisor and agent are separate processes today, flock-only). Gated until the single-process control-plane consolidation lands. Plan: tasks/agent-doc/plan-pcp-consolidate.md
6. [/] [#ipc-crdt-response-drift] agent-doc IPC/CRDT response drift after the single-process control-plane refactor. TWO LAYERS FIXED + INSTALLED: (1) merge layer c9d276b9 — crdt::merge protects committed ### Re: history from a stale/divergent theirs; (2) visible-buffer layer 51b9ed40 — live_buffer_diverges_from_content suppresses a stale live-buffer sidecar (disk modified after the editor last reported = lagging foreign disk write, not a user edit), killing the false-positive 'visible editor buffer differs' finalize block. Both shipped via cargo install + build --release + agent-doc lib-install (0.34.0). REMAINING (still gated, needs clean-session live repro): reversed-lines/hoisted-above-prior symptom under theirs>base in the IPC node-patch/boundary-append path (ipc_socket.rs, template boundary, stream.rs), and write-back serialization across concurrent supervisors. Plan: tasks/agent-doc/plan-ipc-crdt-response-drift.md
7. [/] [#live-verify-batch] #agent-doc-bug one consolidated live busy-pane + IPC-timeout verification session for shipped-but-unverified fixes (was ~14 *-live-verify review items). Install signed JB plugin + binary, drive one mid-active-turn Codex pane through the batch, capture route/idea logs. Blocked on live IntelliJ/Codex panes.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
