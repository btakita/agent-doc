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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260609-034628.md`*

Compacted content:
- Archived 5 response topic(s): R1–R6 + remaining queue items; do [#pcpc1]; do [#pcpc2]; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: do [#vbk7] — blocked on #pcpc5 + live verification, not redundant — opus-4-8

> **Queue prompt:**
>
> do [#vbk7]


**Investigated, not executed — and the "already resolved" flag is a false positive.**

Preflight/plan flagged #vbk7 as possibly already resolved by the completed #pcpc4 (semantic score 1.989). That is wrong. Verified:

- `git show --stat 038f0165` (the #pcpc4 commit) touched **only** `agent-doc-orchestration/src/document_watcher.rs` + `lib.rs`. Its message says verbatim: *"Plugin WatchService read-only demotion (#pcp7) carved out as #pcpc4b (needs live IntelliJ verification)."* #vbk7 **is** #pcpc4b — the carved-out plugin work, still unbuilt.
- The #pcpc4 controller-owned watcher is **unwired**: `document_watcher` appears only as `pub mod document_watcher;` in lib.rs with zero call sites repo-wide. It is not yet an event source; its notify-feed wiring + old-watcher removal is gated in **#pcpc5**.

**Why I did not ship it this turn (correctness, not effort):**

1. The plugin's *only* editor↔disk reconcile authority today is `PromptPoller.kt::mergeOrReload` (VFS `BulkFileListener` → `NativePatching.mergeCrdt` → `doc.setText`). Demoting it to read-only now removes that authority with **no replacement** — #pcpc4's watcher will not feed reconciles until #pcpc5 wires the controller-watcher → IPC patch path. That is a functional gap, independent of verification.
2. #vbk7 needs **live IntelliJ verification**; this is a headless tmux session.
3. This dogfooding session itself runs through the JB plugin's IPC write-back — rebuilding/reinstalling a half-cutover plugin mid-session risks breaking the path this turn depends on.

The plan's Safety section says exactly this: the watcher/plugin-demotion phases "need live IntelliJ/Codex verification" and must not be attempted in a saturated session.

**Action:** gated #vbk7 into `agent:review` with the proven blocker + a hard dependency on #pcpc5 (must sequence after it). Added a low-priority backlog item to discount the semantic-match false positive that sent me chasing this.

### Re: do [#pcpc5] — cutover is spec-fenced from this cycle, gated not dodged — opus-4-8

> **Queue prompt:**
>
> :pushpin: do [#pcpc5]


You re-invoked to keep the queue draining, so I picked up the pinned `do [#pcpc5]`. I did **not** attempt it, and this is a spec constraint, not effort-avoidance.

**The decisive blocker** is in #pcpc5's own contract, `specs/08b-single-process-control-plane.md` (Document write/watch authority):

> "Behavior changes here must never be landed in the same cycle that persists a live session response through the write path being changed."

#pcpc5 *is* the cutover of the write/watch authority. Every agent-doc turn — including this one — persists its response **through** that write path via `finalize`. So landing any #pcpc5 gate in a normal response cycle violates the spec by construction. Three independent reasons reinforce it:

1. **Spec invariant** (above) — hard, code-review-enforced.
2. **Plan Safety section** — "Do NOT attempt in a saturated session"; the watcher/cutover phases "need live IntelliJ/Codex verification." This session is saturated and demonstrably fighting `#ipc-crdt-response-drift` on closeout (last turn's finalize hit the reversed-residue bug; I recovered via `git checkout HEAD` + `reset --from-current`). This very turn I closed out with `--force-disk` to dodge the IPC merge.
3. **Gate ladder** (shadow → dual-write → read-switch → authority → removal) needs SimWorld + CLI + live-tmux proof per gate, and the final gate removes the supervisor + direct-disk writers this session runs on.

**Action:** gated #pcpc5 into `agent:review` with the spec citation and a note to decompose it one actionable id per gate (`pcpc5a..e`) in a fresh session.

**The bigger problem with the queue right now:** the next four items — #pcpc5 (done), then `#ipc-drift-writeprovenance`, `#ipc-early-ack`, `#ipc-degraded-prefers-file-ipc` — are all part of this same IPC/cutover cluster (realized by the already-gated review items `#pcp5`/`#pcp7`), and all want live verification this headless session can't give. The genuinely **shippable-here** bugfixes are stacked *behind* them: `#queue-operator-pin-position-lock-dag`, `#queue-contamination-guard-false-positive`, `#queue-completed-items-escape-below-component`, `#stale-capture-captured-only-drift` — pure-Rust, SimWorld-testable, no live editor needed.

**Your call (pick one and I'll run it):**
- **Reorder** the queue to bring those four headless bugfixes to the front — I'll drain them here and now.
- **Fresh session** for the whole PCP/IPC cutover cluster (#pcpc5, #vbk7, #pcp5/6/7, #live-verify-batch) with IntelliJ open.

I'm holding the auto-loop rather than gating my way down a row of cutover items that all bounce off the same spec fence.
<!-- agent:boundary:05e63de3 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~do [#vbk7]~~
- ~~:pushpin: do [#pcpc5]~~
- do [#queue-no-stop-unrelated-edit]
- do [#queue-completed-items-escape-below-component]
- do [#queue-operator-pin-position-lock-dag]
- do [#ipc-drift-writeprovenance]
- do [#ipc-early-ack]
- do [#ipc-degraded-prefers-file-ipc]
- do [#stale-capture-captured-only-drift]
- do [#queue-contamination-guard-false-positive]
- do [#nm1x-livebuf-content]
- do [#clear-opt-in-threshold]
- do [#console-input-accumulation]
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
- [ ] [#vbk7] #pcpc4b plugin WatchService read-only demotion (realizes #pcp7) — change editors/jetbrains WatchService to a read-only buffer reporter (no independent reconcile), so the single controller-owned watcher (#pcpc4) is the only event source. Needs live IntelliJ verification; pairs with #pcpc5 live-tmux/editor cutover. Plan: tasks/agent-doc/plan-supervisor-in-process.md
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
1. [/] [#pcpc5] PCP-consol phase 5 — 08b migration cutover (shadow -> dual-write -> read-switch -> authority -> removal of the out-of-process supervisor + direct-disk writers). HARD-BLOCKED from any normal agent-doc response cycle by spec 08b (specs/08b-single-process-control-plane.md, Document write/watch authority): "Behavior changes here must never be landed in the same cycle that persists a live session response through the write path being changed" — every agent-doc turn persists its response through that exact write path. Run in a fresh, non-saturated session (this one keeps hitting #ipc-crdt-response-drift on closeout) with live-tmux + CLI + SimWorld + live IntelliJ proof, decomposed one actionable id per gate (pcpc5a..e). Pairs with #vbk7 and review #pcp7. Plan: tasks/agent-doc/plan-supervisor-in-process.md
2. [/] [#pcp6] PCP phase 6 — Capture editor-buffer CONTENT in the live-buffer sidecar (absorbs #nm1x-livebuf-content) so a genuine unsaved edit is scope-classified vs failing closed (R4). JB+VSCode plugin + FFI classifier. Requires live editor verification. Plan: tasks/agent-doc/plan-pcp-consolidate.md
3. [/] [#pcp7] PCP phase 7 — Demote plugin WatchService to read-only buffer reporting + reduce FFI socket listener to a thin EDT apply+ack shim (closes R1/R2/R5 TOCTOU). BLOCKED on supervisor-in-process consolidation (#pcp-consol) AND requires live IntelliJ/Codex verification; pairs with #live-verify-batch. Plan: tasks/agent-doc/plan-pcp-consolidate.md
4. [/] [#pcp5] PCP phase 5 — Early-ack IPC + degraded-prefers-file-IPC (absorbs #ipc-early-ack, #ipc-degraded-prefers-file-ipc). FFI-first: accepted/pending ack on receipt + terminal ack; degraded routes through file-IPC, never raw disk. GATED: the early-ack emit is a Kotlin PatchWatcher change needing live IntelliJ/Codex verification, and the IPC degrade routing is delicate state-machine code best done in a fresh focused session (this session context is saturated and its closeouts keep fighting the IPC drift). Plan: tasks/agent-doc/plan-pcp-consolidate.md
5. [/] [#pcp4] PCP phase 4 — Single controller-owned filesystem watcher replacing per-process notify watchers (kills R1 two-watchers). BLOCKED on supervisor-in-process consolidation (#pcp-consol); deferred per operator decision to ship unblocked wins first. Plan: tasks/agent-doc/plan-pcp-consolidate.md
6. [/] [#pcp3] PCP phase 3b — In-process ordered write queue in the session actor; serialize supervisor idle/file-watch writes + agent finalize writes. BLOCKED on supervisor-in-process consolidation (supervisor and agent are separate processes today, flock-only). Gated until the single-process control-plane consolidation lands. Plan: tasks/agent-doc/plan-pcp-consolidate.md
7. [/] [#ipc-crdt-response-drift] agent-doc IPC/CRDT response drift after the single-process control-plane refactor. TWO LAYERS FIXED + INSTALLED: (1) merge layer c9d276b9 — crdt::merge protects committed ### Re: history from a stale/divergent theirs; (2) visible-buffer layer 51b9ed40 — live_buffer_diverges_from_content suppresses a stale live-buffer sidecar (disk modified after the editor last reported = lagging foreign disk write, not a user edit), killing the false-positive 'visible editor buffer differs' finalize block. Both shipped via cargo install + build --release + agent-doc lib-install (0.34.0). REMAINING (still gated, needs clean-session live repro): reversed-lines/hoisted-above-prior symptom under theirs>base in the IPC node-patch/boundary-append path (ipc_socket.rs, template boundary, stream.rs), and write-back serialization across concurrent supervisors. Plan: tasks/agent-doc/plan-ipc-crdt-response-drift.md
8. [/] [#live-verify-batch] #agent-doc-bug one consolidated live busy-pane + IPC-timeout verification session for shipped-but-unverified fixes (was ~14 *-live-verify review items). Install signed JB plugin + binary, drive one mid-active-turn Codex pane through the batch, capture route/idea logs. Blocked on live IntelliJ/Codex panes.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
