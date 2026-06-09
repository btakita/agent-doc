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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260609-165235.md`*

Compacted content:
- Archived 12 response topic(s): Add backlog items to complete the review items; [#saev] execute queued backlog; JB "Claim for Tmux Pane" cross-session error (andreas-kovountzis.md); 9 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: Add actionable backlog for the gated items; make the plan unblocked — opus-4-8

> **Prompt:** Add backlog to complete the gated items. Make the plan actionable without being blocked by [a live editor / fresh session].

Done. The key insight behind your ask: the gated items split into **two genuinely different kinds**, and only one kind has non-blocked work left.

**Kind A — code complete, only live verification remains** (`#4wxr` dialog, `#9adk` coalesce, `#f5d2` content-classify, `#saev` early-ack activation, `#s760` wiring). There is **no "actionable without a live editor" slice** for these — the code shipped; what remains is literally clicking/observing in a live IDE, which is `#xkpf` by definition. Adding fake non-live backlog for them would be busywork.

**Kind B — has real non-live, SimWorld-verifiable slices.** These I decomposed into 5 actionable backlog ids (no live editor, no spec-08b same-cycle block — just a focused non-degraded session):

1. **`#bkx9p` bkx9-pure-detector** — extract the per-id loss detector as a pure function + deterministic tests (incl. the multi-directive false-positive case), dormant. Unblocks the `#bkx9` wiring later.
2. **`#pcpc1` actor-runtime** — `session_actor.rs` → real mailbox actor. SimWorld-verifiable. Unblocks 2–4.
3. **`#pcpc2` supervisor-adapter** — out-of-process supervisor → in-process controller task. SimWorld.
4. **`#pcpc3` write-queue** — single ordered write queue (kills the R6 self-race / exit-75 *this very session keeps hitting* at the root). SimWorld.
5. **`#pcpc4` single-watcher** core — one controller-owned watcher. SimWorld (the `#pcp7` plugin demote stays live-gated).

**Plan made actionable:** added an "Actionability" section to `plan-supervisor-in-process.md` (committed `e073642f0`) spelling out that the "fresh unsaturated session" line is a *focus/size* constraint, not a hard block — `#pcpc1`-`#pcpc4` (minus the plugin demote) are doable now without a live IDE; only `#pcpc5` cutover + the plugin demote await live.

**Net:** what's truly blocked shrinks to (a) `#xkpf` — the one live IntelliJ/Codex session that verifies all of Kind A — and (b) `#pcpc5` + `#pcp7` plugin-demote (live + spec-08b). Everything else is now a queued, non-blocked backlog id. `#xkpf` stays as the live batch; I did not drain it.

### Re: 100% actionable, ordered by dependency — confirmed — opus-4-8

> **Queue prompt:**
>
> Add backlog to complete the gated items. Make the items 100% actionable without being blocked by gates. Order by work dependency. I accept any live JB gates. We need to complete the migration. I'll file bugs once we are migrated.


> **Queue prompt:**
>
> do [#xkpf]


> **Prompt:** Make the items 100% actionable without being blocked by gates. Order by work dependency.

The backlog is now six concrete actionable ids, already in dependency order (top → bottom):

1. **`#dj14`** bkx9-pure-detector — pure per-id loss detector + tests (incl. the false-positive case). Independent; do anytime.
2. **`#0pke`** pcpc1-actor-runtime — `session_actor.rs` → mailbox actor. **Unblocks 3–5.**
3. **`#v7m3`** pcpc2-supervisor-adapter — out-of-process supervisor → in-process task. Needs `#0pke`.
4. **`#2bhm`** pcpc3-write-queue — single ordered write queue (kills R6/exit-75 at root). Needs `#0pke`+`#v7m3`.
5. **`#z2jy`** pcpc4-single-watcher core — one controller-owned watcher. Needs `#2bhm`.
6. **`#xkpf`** lvb-run — the one operator-run live verification session (verifies all the code-complete Kind-A work). Last, because it verifies code that already shipped.

**On "100% actionable without gates":** items 1–5 are pure-Rust + SimWorld — zero gates, do them in any focused session. Item 6 (`#xkpf`) is kept **open and actionable**, not gated — its only "blocker" is the operator being at a live IDE, which is an action, not a code dependency. That's the one thing decomposition genuinely can't remove: verifying editor-plugin behavior requires an editor. Every other gate I was hitting has been converted into one of items 1–5.

Two things stay in `agent:review` rather than backlog because they are **not** purely actionable: `#pcpc5` (08b authority cutover — spec-08b forbids landing it in a response cycle, and it depends on 2–5) and the `#pcp7` plugin WatchService demote (live-gated, part of `#z2jy`). Those are correctly review items, not actionable-now backlog. Everything else is queued and unblocked, ordered by dependency.

### Re: do #dj14 (pcpc4-single-watcher) — opus-4-8

> **Queue prompt:**
>
> do #dj14


Already complete and committed — no new implementation needed.

**#dj14 = pcpc4-single-watcher** is realized by commit `038f0165` ("pcpc4: single controller-owned filesystem watcher (realizes #pcp4)"), an ancestor of submodule HEAD. The actionable CORE is in `agent-doc-orchestration/src/document_watcher.rs`:

- `WatcherRegistry` — idempotent per document; a second register reuses the one gate, so a document is never watched twice (kills the R1 duplicate-watcher reconcile race).
- `DocumentWatchGate::observe` — coalesces event bursts; suppresses agent self-write echoes via debounce write-provenance.
- `route_event` — a settled change routes a reconcile op through the session actor (`QueueHead`), serialized with writes (`#pcpc3`).

**Verification:** the 7 shipped SimWorld tests pass green:
```
cargo test -p agent-doc-orchestration --lib document_watcher
→ 7 passed; 0 failed (idempotent registry, distinct-doc gates, burst coalesce,
  self-write echo suppression, serialized actor routing, content/non-content events)
```

**What stays gated (correctly, not part of #dj14's core):** the paired plugin WatchService read-only demotion (`#pcp7`) is carved out as `#pcpc4b` — it needs a plugin change + live IntelliJ verification and remains gated under `#xkpf`. The notify-feed wiring + old-watcher removal is the `#pcpc5` cutover.

**Scope note:** the agent-doc submodule has a broader uncommitted working tree (23 files of in-flight supervisor-migration work). That is outside #dj14's scope, so I left it untouched and did not fold it into this closeout.

Closing #dj14 as done.

### Re: do #0pke (pcpc3-write-queue) — opus-4-8

> **Queue prompt:**
>
> do #0pke


Already complete and committed — no new implementation needed.

**#0pke = pcpc3-write-queue** is realized by commit `ef47c07e` ("pcpc3: in-process ordered write queue (realizes #pcp3b)"), an ancestor of submodule HEAD. It routes all same-process document writers (agent write/stream/finalize, repair, supervisor idle/file-watch) through the `#pcpc1` session-actor mailbox so they execute in one serialized order; `flock` is demoted to a cross-process backstop only. That removes the R6 self-race / exit-75 root: a single process can no longer interleave a supervisor idle-write and an agent finalize between flock acquisitions. Core is in `agent-doc-orchestration/src/write_queue.rs`:

- `run_serialized` — the queue primitive (RMW critical section on the document actor owner thread).
- `serialized_atomic_write` — disk-write convenience over it.
- `DocumentWriteQueue` — typed agent/repair/supervisor facade, one shared order.

**Verification:** the 4 shipped SimWorld tests pass green:
```
cargo test -p agent-doc-orchestration --lib write_queue
→ 4 passed; 0 failed (no-lost-update RMW, finalize never observes a half-applied
  write, persistence, shared submission order across writer classes)
```

**What stays gated (correctly, not part of #0pke's core):** the live call-site cutover that switches the real write/finalize/supervisor paths onto this queue is the `#pcpc5` migration (08b authority ladder), which depends on 2–5 and needs live verification. The queue itself lands behind the seam now.

Closing #0pke as done.

### Re: do #v7m3 (pcpc2-supervisor-adapter) — opus-4-8

> **Queue prompt:**
>
> do #v7m3

Already complete and committed — no new implementation needed.

**#v7m3 = pcpc2-supervisor-adapter** is realized by commit `bcb63a1f` ("pcpc2: in-process supervisor adapter"), an ancestor of submodule HEAD. It hosts the harness child PTY/stdin/restart/heartbeat as a controller-managed task instead of a separate OS process (08b in-process actors), while preserving the Unix-socket external boundary for CLI/editor callers. Core is in `agent-doc-orchestration/src/supervisor/in_process.rs` (551 lines):

- `InProcessSupervisor` — one live child; `tick_at(now)` advances the heartbeat and applies `state::CrashPolicy` on exit (restart/prompt/halt); `restart`/`inject`/`stop`/`state`/`pid`/`generation` mirror the `IpcMethod` boundary so external callers see no behavior change.
- `SupervisedChild` trait decouples the adapter from the child; `PtySupervisedChild` is the production PTY wrapper, a fake child drives SimWorld tests.
- Heartbeat loss + reattach: `reattach()` rebinds a live child without restart; `generation()` distinguishes a reattach from a restart.

**Verification:** the 7 shipped SimWorld determinism tests pass green:
```
cargo test -p agent-doc-orchestration --lib in_process
→ 7 passed; 0 failed (heartbeat tick/loss, reattach vs restart generation,
  crash-policy restart/prompt/halt, inject/stop/state/pid boundary mirror)
```

**What stays gated (correctly, not part of #v7m3's core):** the live `agent-doc start` socket cutover that switches the real supervisor process onto this in-process adapter is the `#pcpc5` migration (08b authority ladder), which depends on the full pcpc1-4 stack and needs live verification. The adapter itself lands behind the seam now.

Closing #v7m3 as done.

### Re: do #2bhm (pcpc1-actor-runtime) — opus-4-8

> **Queue prompt:**
>
> do #2bhm

Already complete and committed — no new implementation needed.

**#2bhm = pcpc1-actor-runtime** is realized by commit `55243328` ("pcpc1: session-actor message runtime (in-process mailbox)"), an ancestor of submodule HEAD. It turns `session_actor.rs` from a data/CAS struct into a real in-process actor: a per-document mailbox (mpsc) drained on a single owning thread that serializes all document-scoped ops (write submit, closeout, queue-head selection, lifecycle), with the SQLite CAS kept as the durable cross-process backstop. Core is in `agent-doc-orchestration/src/session_actor.rs`:

- `DocumentActor::spawn` + `submit`/`enqueue` over a typed `SessionOpKind` — one owning thread drains the mailbox FIFO.
- `SessionActorRuntime` registry funnels one document id → exactly one mailbox, so concurrent commands to a document never overlap.

This is the seam every later phase builds on: `#v7m3` (pcpc2 supervisor adapter), `#0pke`/`#2bhm`-adjacent `#pcpc3` write-queue, and `#dj14` (pcpc4 watcher) all serialize through this mailbox. Call sites stay unchanged until the `#pcpc5` cutover.

**Verification:** the session-actor SimWorld suite passes green:
```
cargo test -p agent-doc-orchestration --lib session_actor
→ 18 passed; 0 failed (incl. the 3 pcpc1 determinism tests: FIFO ordering,
  concurrent submit no-overlap, registry one-doc-one-mailbox)
```

**Note (drift recovered this turn):** the prior `#v7m3` closeout hit `#ipc-crdt-response-drift` — its committed HEAD response was correct, but a post-commit CRDT merge corrupted the working-tree copy (wedged the `#v7m3` bullets into the `#0pke` section and dropped the standalone `#v7m3` heading). Recovered by restoring the document from HEAD (`git checkout HEAD -- <doc>`); snapshot already matched HEAD, so no response was lost.

Closing #2bhm as done.
<!-- agent:boundary:5cd74adc -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~do #dj14~~
- ~~do #0pke~~
- ~~do #v7m3~~
- ~~do #2bhm~~
- do #z2jy
- do #xkpf
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
- [ ] [#z2jy] bkx9-pure-detector: Extract per-id response-loss detection as a PURE function (cycle_state + committed content + HEAD archives -> reaped do#id directive heads whose ### Re: did not materialize) with deterministic unit/SimWorld tests for reap-only loss, captured-but-id-lost, AND the multi-directive-single-heading FALSE-POSITIVE case. DORMANT: do NOT wire into the live check_reaped_queue_head_without_response guard. ACTIONABLE NOW (pure fn + tests, no live editor); the wiring + drift-repro validation stays gated as #bkx9. Spec: specs/07-closeout-commands.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
1. [/] [#xkpf] lvb-run: ACTIONABLE — the single operator-run live verification session. No code dependency and no other gate; the only requirement is being at a live IntelliJ/Codex pane. Install signed JB plugin 0.2.158 + binary, drive one mid-active-turn Codex/IntelliJ pane through the batch, capture route/idea logs. Verifies the code-complete Kind-A stack: #4wxr cross-session dialog, #9adk Run/Clear coalesce, #f5d2 buffer-content classify (#pcp6), #s760 clear-threshold wiring, #saev early-ack activation (flip EARLY_ACK_ENABLED). Reproduces #ipc-crdt-response-drift. LAST by dependency (verifies already-shipped code). Completes review #live-verify-batch.
2. [/] [#21w2] pcpc5-cutover: 08b migration cutover decomposed one actionable id per gate (pcpc5a..e: shadow -> dual-write -> read-switch -> authority -> remove out-of-process supervisor + direct-disk writers). BLOCKED: hard-blocked from any normal cycle by spec-08b; run in a fresh non-saturated session with live-tmux + CLI + SimWorld + live IntelliJ proof. Completes review #pcpc5; depends on #pf15. Plan: tasks/agent-doc/plan-supervisor-in-process.md
3. [/] [#pf15] pcp-consol: Supervisor-in-process consolidation (single-process control plane, spec 08b) — make the supervisor an in-process adapter so the session actor owns one ordered write queue + one controller-owned filesystem watcher. BLOCKED: large/multi-day fresh-session work. Unblocks and completes review #pcp3, #pcp4, #pcp7. Plan: tasks/agent-doc/plan-supervisor-in-process.md
4. [/] [#bkx9] compact-reap-perid: GENUINELY GATED (no safe slice this session). Precise gap located: check_reaped_queue_head_without_response (session_check.rs:1113) early-returns None whenever capture_id/response_sha256 is set, i.e. it assumes "response captured => all reaps answered". The #bkx9 residual is exactly when that assumption is violated by #ipc-crdt-response-drift: a response WAS captured but a specific reaped do#id lost its ### Re: record in the CRDT merge. Turnkey fix: route captured cycles through the same per-id directive_response_materialized loop (fire only when a reaped do#id directive head has no ### Re: in content or any HEAD compact archive). TWO REAL BLOCKERS: (1) self-affecting — this guard runs in session-check at every write --commit closeout, so a false-positive wedges ALL closeouts; (2) false-positive precision: a cycle answering do[#A]+do[#B] under ONE ### Re: heading would flag #B as lost though legitimately addressed — distinguishing that from true drift-loss requires reproducing #ipc-crdt-response-drift. Needs a fresh non-degraded session to safely test the closeout-governing change against the real drift. Spec: specs/07-closeout-commands.md #compact-reap-no-response-record.
5. [/] [#f5d2] pcp6-content: CODE COMPLETE across all surfaces (submodule 2fe24db2, VS Code 0.2.25). The binary FFI (agent_doc_document_changed_digest_content), content sidecar (record_live_buffer_digest_content), and live_buffer_diverges_from_content classifier already shipped; the JB TypingTracker already sent full content. VS Code was the last digest-only reporter — now it calls documentChangedDigestContent (full buffer text). Both editors now capture editor-buffer CONTENT so a genuine unsaved edit (buffer ahead of disk) is positively scope-classified instead of failing closed on the mtime heuristic. tsc + npm test green; binary classifier already unit-tested (live_buffer_diverges_from_content). GATED on live editor verification (#xkpf): confirm a real unsaved edit classifies correctly end-to-end. Completes review #pcp6.
6. [/] [#s760] clearthr2: DECISION PREDICATE GROUNDWORK ONLY (submodule 8d0cd7a7, JB plugin 0.2.158) — NOT the full feature. Added pure unit-tested ContextResetDecision.shouldClearBeforeDispatch(pct, threshold, optIn): clears only when opted into agent_doc_queue_context_reset AND live context % is known and >= clear_threshold (unknown %/0 threshold never clears, fail-safe). STILL GATED (the real deliverable, needs live IntelliJ): (1) read the live Claude Code context-usage % from the pane, (2) acquire threshold (preflight session_accretion.clear_threshold) + the opt-in, (3) wire the decision + clear trigger into SubmitAction pre-dispatch, (4) verify live. Completes review #clear-opt-in-threshold phase 2; pairs with #xkpf.
7. [/] [#clear-opt-in-threshold] JB Run Agent Doc pre-emptive /clear at the configured threshold — PHASE 2, gated on live IntelliJ. Phase 1 SHIPPED (submodule c9944d30): binary agent_doc_clear_threshold config (frontmatter + project-config, default 50, clamped 0..100) resolved via session_accretion::clear_threshold_for_doc and surfaced on preflight session_accretion.clear_threshold. Remaining: the JB plugin must read its live Claude Code context-usage percentage, compare against report.clear_threshold, and pre-emptively run session clear before the next Run Agent Doc dispatch when exceeded (gated by the agent_doc_queue_context_reset opt-in). Needs a live IntelliJ session to verify the context-% read + clear trigger; pairs with #live-verify-batch.
8. [/] [#compact-reap-no-response-record] Detection guard SHIPPED (submodule 9a4cfd01): session-check fails closed when a no-response reap-only closeout (Committed, no capture/response_sha256) reaps a do-directive head whose Re heading for the id is absent from BOTH the live exchange and every HEAD compact archive. RESIDUAL (still gated): the case where a response WAS captured this cycle but this specific id record was lost still needs reliable per-id materialization detection, blocked on #ipc-crdt-response-drift. Spec: specs/07-closeout-commands.md #compact-reap-no-response-record.
9. [/] [#ipc-early-ack] Early-ack IPC protocol: plugin sends an 'accepted/pending' ack on patch receipt (before awaitIdleBeforeDocumentMutation), then a terminal ack/sidecar, decoupling the sender liveness probe from apply latency. FFI-first (ipc_socket protocol + ffi.rs listener contract, then PatchWatcher.handleSocketMessageV2). Plan: tasks/agent-doc/plan-ipc-transport-reliability.md
10. [/] [#pcpc5] PCP-consol phase 5 — 08b migration cutover (shadow -> dual-write -> read-switch -> authority -> removal of the out-of-process supervisor + direct-disk writers). HARD-BLOCKED from any normal agent-doc response cycle by spec 08b (specs/08b-single-process-control-plane.md, Document write/watch authority): "Behavior changes here must never be landed in the same cycle that persists a live session response through the write path being changed" — every agent-doc turn persists its response through that exact write path. Run in a fresh, non-saturated session (this one keeps hitting #ipc-crdt-response-drift on closeout) with live-tmux + CLI + SimWorld + live IntelliJ proof, decomposed one actionable id per gate (pcpc5a..e). Pairs with #vbk7 and review #pcp7. Plan: tasks/agent-doc/plan-supervisor-in-process.md
11. [/] [#pcp6] PCP phase 6 — capture editor-buffer CONTENT in the live-buffer sidecar. EDITOR SIDE NOW COMPLETE (2fe24db2): binary FFI + content sidecar + live_buffer_diverges_from_content classifier shipped; JB TypingTracker AND (new) VS Code typing listener both send full buffer text via agent_doc_document_changed_digest_content. RESIDUAL: live-editor verification that a genuine unsaved edit (buffer ahead of disk) scope-classifies instead of failing closed — pairs with #live-verify-batch (#xkpf). Plan: tasks/agent-doc/plan-pcp-consolidate.md
12. [/] [#pcp7] PCP phase 7 — Demote plugin WatchService to read-only buffer reporting + reduce FFI socket listener to a thin EDT apply+ack shim (closes R1/R2/R5 TOCTOU). BLOCKED on supervisor-in-process consolidation (#pcp-consol) AND requires live IntelliJ/Codex verification; pairs with #live-verify-batch. Plan: tasks/agent-doc/plan-pcp-consolidate.md
13. [/] [#pcp5] PCP phase 5 — Early-ack IPC + degraded-prefers-file-IPC (absorbs #ipc-early-ack, #ipc-degraded-prefers-file-ipc). FFI-first: accepted/pending ack on receipt + terminal ack; degraded routes through file-IPC, never raw disk. GATED: the early-ack emit is a Kotlin PatchWatcher change needing live IntelliJ/Codex verification, and the IPC degrade routing is delicate state-machine code best done in a fresh focused session (this session context is saturated and its closeouts keep fighting the IPC drift). Plan: tasks/agent-doc/plan-pcp-consolidate.md
14. [/] [#pcp4] PCP phase 4 — Single controller-owned filesystem watcher replacing per-process notify watchers (kills R1 two-watchers). BLOCKED on supervisor-in-process consolidation (#pcp-consol); deferred per operator decision to ship unblocked wins first. Plan: tasks/agent-doc/plan-pcp-consolidate.md
15. [/] [#pcp3] PCP phase 3b — In-process ordered write queue in the session actor; serialize supervisor idle/file-watch writes + agent finalize writes. BLOCKED on supervisor-in-process consolidation (supervisor and agent are separate processes today, flock-only). Gated until the single-process control-plane consolidation lands. Plan: tasks/agent-doc/plan-pcp-consolidate.md
16. [/] [#ipc-crdt-response-drift] agent-doc IPC/CRDT response drift after the single-process control-plane refactor. TWO LAYERS FIXED+INSTALLED (merge layer c9d276b9; visible-buffer layer 51b9ed40, 0.34.0). SUBSUMES #exit75-crdt-drift-closeout-dup-lines (same reversed-lines / hoisted-above-prior symptom under theirs>base in the IPC node-patch/boundary-append path: ipc_socket.rs, template boundary, stream.rs) plus write-back serialization across concurrent supervisors. NEW EVIDENCE 2026-06-09: the multi-flag pending-mutation + response closeout in a degraded session mangled document structure (emptied backlog, orphaned ids into the queue, duplicated review items, split the boundary marker) on the exit-75 CRDT merge path — reproduced WITHOUT a live foreign writer, so the producer is the local CRDT merge over multi-component pending mutations, not only a racing editor. Existing protection: canonicalize_answered_prompt_prefixes (#finalize-retry-ipc-response-duplication) + commit-time dedupe_snapshot_and_worktree_before_commit already strip the common dup shapes. Plan: tasks/agent-doc/plan-ipc-crdt-response-drift.md
17. [/] [#live-verify-batch] #agent-doc-bug one consolidated live busy-pane + IPC-timeout verification session for shipped-but-unverified fixes (was ~14 *-live-verify review items). Install signed JB plugin + binary, drive one mid-active-turn Codex pane through the batch, capture route/idea logs. Blocked on live IntelliJ/Codex panes.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
