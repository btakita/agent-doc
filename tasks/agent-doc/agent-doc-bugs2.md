---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

 <!-- agent:status patch=replace -->

#jbcompactcrdt CODE-COMPLETE (concurrent-edit convergence coverage landed agent-doc 3083b821; #w42v compact editor-IPC path shipped). Operator-drive remainder: lib-install + live Compact Exchange verify. #rtwbcast remains queued (separate multi-editor item).
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260614-073904.md`*

Compacted content:
- Archived 8 response topic(s): do [#anw0]; monsterrodholders.md; stale-supervisor restart failed + already-captured churn reports; 5 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: stale-supervisor churn (red/green deploy, backlog→queue, post-compact attr wipe) — opus-4-8

> **Queue prompt:**
>
> JB `Run Agent Doc` should red/green deploy the new supervisor and switch over to the new supervisor automatically...in Claude Code, Codex, Opencode
> ```
> resync: pruned 8 stale session(s)
> [route] target tmux session: 0
> Error: project controller command `dispatch` failed: dispatch blocked for tasks/software/lazily-rs.md: failed_stage=queue_paused reason=stale route-owned supervisor (pid 968752) replaying already-answered/archived #advance-review queue item with go-mode; halting churn until supervisor recycled receipt_id=3455
> ```


All three queued reports share one root cause, and this session is sitting on top of it: the route-owned host **supervisor (pid 1562136) is stale**. It started 02:32 today, *before* the currently-installed binary, and `~/.cargo/bin/agent-doc` + cdylib + `target/release` themselves **predate source commit `e9cf3b74`**. That doubly-stale supervisor is what produces the File Cache Conflict dialogs, the `live_prompt_drift` / IPC churn, the `dispatch blocked … halting churn until supervisor recycled` errors you pasted (lazily-rs / efs), and the compaction that couldn't commit. I cleared the compaction-commit wedge this cycle (`reset --from-current --preserve-session` → HEAD now matches the post-compact disk).

Per-report status — **all three are already captured as `[clean-session]` backlog items**, so nothing is lost:

1. **JB `Run Agent Doc` should red/green-deploy + auto-switch the supervisor** (CC/Codex/OpenCode) → `#jbrestale`. This is exactly the self-healing that would have prevented this whole session: detect a stale/churn-stopped supervisor, drain-and-supersede to a fresh one, switch over automatically instead of failing closed. The lazily-rs `failed_stage=queue_paused … stale route-owned supervisor (pid 968752)` error is the same class.
2. **backlog `priority queue` items not flowing into `agent:queue`** → tracked, and preflight reports the go-mode path *is* now appending fresh backlog `queue`-attr ids (`#backlog-queue-attr-populates-in-go-mode`); the batch you listed maps to `#4myd`, `#pushpinaccum` (`:pushpin:`/`:round_pushpin:` dedup), `#fcc0`, `#rtwbcast`, `#recyclerestart`, and `#0kjc` (markdown-AST tags-in-strings/code-spans-are-content).
3. **Post-compaction wiped all `agent:queue` attributes** (`preset`, `priority`) in monsterrodholders.md → `#compactqattr`.

Why I did **not** drain them this cycle: they are `[clean-session]` code fixes in `src/agent-doc`, and writing/finalizing those fixes through the live stale supervisor races it and corrupts the document (the churn you're already seeing). They need a fresh supervisor.

**Recovery to unblock a clean drain** (run from a fresh terminal, not through this session):
```
cd src/agent-doc && make install
agent-doc session restart-supervisor tasks/agent-doc/agent-doc-bugs2.md --force
```
`make install` rebuilds the binary + cdylib to `e9cf3b74`+; `restart-supervisor --force` retires pid 1562136 and brings up a fresh supervisor on the new binary so client and supervisor no longer skew. Then re-invoke `/agent-doc tasks/agent-doc/agent-doc-bugs2.md` in a clean session and the `[clean-session]` heads become drainable.

Not auto-looping — the queue heads are agent-undrainable under the live stale supervisor (`#goqueuestall`).
<!-- agent:boundary:c5f4e90c -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
---
JB `Run Agent Doc` should red/green deploy the new supervisor and switch over to the new supervisor automatically...in Claude Code, Codex, Opencode
```
resync: pruned 8 stale session(s)
[route] target tmux session: 0
Error: project controller command `dispatch` failed: dispatch blocked for tasks/software/lazily-rs.md: failed_stage=queue_paused reason=stale route-owned supervisor (pid 968752) replaying already-answered/archived #advance-review queue item with go-mode; halting churn until supervisor recycled receipt_id=3455
```
---
- ~~agent-doc sync .../monsterrodholders.md is not opening the tmux pane for monsterrodholders.md. I previously closed the session and ran `kill-pane` to start a new supervisor version.~~
---
JB `Run Agent Doc` on equityfundingsource.md. This should self-heal. Not obvious why this is blocked as well.
```
resync: pruned 8 stale session(s)
[route] target tmux session: 0
Error: project controller command `dispatch` failed: dispatch blocked for tasks/professional/equityfundingsource.md: failed_stage=queue_paused reason=advance-review preset head is spent (backlog added + both features shipped); pausing so the go-queue does not re-trigger advance-review. Operator can clear the '- #advance-review' line. receipt_id=3457
```
---
:pushpin: JB `Compact Exchange` on this document
```
agent-doc command failed (exit 1): [agent-doc] Tagged pre-compact state as agent-doc/agent-doc-bugs2/pre-compact-23 [compact] CRDT state compacted: 26692 → 26692 bytes [compact] CRDT state refreshed from post-compact content [compact] discarded 1 stale capture sidecar(s) for archived response(s) in tasks/agent-doc/agent-doc-bugs2.md [compact] Archived 148 lines from component 'exchange' to /home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260613-022946.md Error: refusing to auto-adopt committed historical response patchback for tasks/agent-doc/agent-doc-bugs2.md: HEAD contains an out-of-band typed_component_drift mutation with response marker ### Re: do [#rtwbcast] — multi-editor CRDT broadcast — opus-4-8
```
JB `Run Agent Doc` on equityfundingsource.md
```
[route] target tmux session: 0
[route] authoritative actor for tasks/professional/equityfundingsource.md projected busy but the live pane proves a dispatch-ready prompt (generation 66); repairing stale busy projection to ready and dispatching instead of queuing
[route] activated existing inactive agent:queue head "Please create sample borrower documents and sample investor documents that I can upload for testing. Place in the equityfundingsource-dev/tmp directory. Ensure tmp is in .gitignore." for tasks/professional/equityfundingsource.md (already_present=true, activated=true) before dispatch-only reopen
Error: project controller command `dispatch` failed: dispatch coalesced for tasks/professional/equityfundingsource.md: a dispatch for generation 66 is already in flight (#qflood); receipt_id=3310
```
---
- This document has `agent:backlog priority queue`...but the backlog items are not being added to the `agent:queue`.
- :pushpin: :round_pushpin: do [#4myd]
- If there is a `:round_pushpin:` and when upgrading to a `:pushpin:`, drop the `:round_pushpin:` instead of having both. So instead of `:pushpin: :round_pushpin:` we should have `:pushpin:`.
- :pushpin: :round_pushpin: do [#fcc0]
- :pushpin: :round_pushpin: do [#rtwbcast]
- :pushpin: [#recyclerestart]: Stall occurred ![img_23.png](img_23.png)
- :pushpin: Ensure the markdown AST is properly handling strings (quoted text) and code blocks (backtick or triple backtick). Tags within the blocks should be treated as the content, not as part of the document tags.
---
:pushpin: JB ` /home/brian/.cargo/bin/agent-doc start --route-owned tasks/professional/equityfundingsource.md`
```
~/work/btakita/agent-loop main ⇡20 *5 !41 ?11                                                     agent-loop 23:02:05
                                                                                                                      ❯ /home/brian/.cargo/bin/agent-doc start --route-owned tasks/professional/equityfundingsource.md
[start] harness resolved: binary=claude source=frontmatter env=default
                                                                      Registered session 62fe1f41 → pane %33
                                                                                                            Error: project controller command `start_session` failed: controller failed to start session actor for /home/brian/work/btakita/agent-loop/tasks/professional/equityfundingsource.md
```
---
:pushpin: Spurrious post-compaction in monsterrodholders.md deleted all attributes in `agent:queue` including `preset` and `priority`. Too much blast radius.
---
- ~~:pushpin: do [#8rg3]~~
- ~~do [#mb98]~~
<!-- /agent:queue -->
###
<!--
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
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
- [ ] [#anw0verify] [operator-verify] #anw0 redirect live proof: install the new agent-doc binary (cargo build --release + cargo install + lib-install) then drive a real recycle / restart-supervisor race so a dispatcher holding a superseded generation hits the new path. Confirm ops.log shows dispatch_stale_generation_redirect prior_generation=N next_generation=N+1 then dispatch_retry_after_stale_generation and a clean re-dispatch (no terminal generation-N-is-closed). Code+SimWorld landed agent-doc 21a70cdd / superproject 412355fa3. Cannot be agent-done: needs a live editor + real recycle.
- [ ] [#dupcontent] [clean-session] Duplicate-content / malformed-attribute document corruption when finalize adopts a stale-supervisor `content_ours` snapshot. LIVE 2026-06-14 (monsterrodholders.md, stale route-owned supervisor pid1328300 started 02:15:48 < binary mtime 02:28:10, session-check `#fcc0`): `finalize --stream` hit `mid-turn cycle rotation ... #adoc-compact-during-turn-response-loss` then `live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar content_ours_len=40827 > candidate 39216` then `snapshot from content_ours`. The adopted buffer was CORRUPT and written to disk: 273 lines vs 181 in clean HEAD, TWO `agent:queue` blocks (held one at ~88 PLUS a stale re-armed `preset="#spec-test-commit-push" priority go` block at ~181) and a truncated marker `<!-- agent:queue tasks=" -->`. The lint-gate caught `empty-attr-value` only AFTER the corrupt content already landed on disk (interrupt, no commit; HEAD stayed clean). Repair: `git checkout HEAD -- <file>` + `reset --from-current --preserve-session`. ROOT CAUSE (hypothesis): `content_ours`/`ack_content_sidecar` buffer is adopted verbatim as the snapshot base without structural validation, so a bad prior CRDT merge (duplicate singleton component blocks + attribute split across a merge boundary) is taken as truth and written before the lint-gate runs; the known-stale supervisor is still trusted as an IPC content source. FIX: (1) structurally validate before adopting any content_ours snapshot — reject/repair >1 open marker for a singleton component (exchange/status/queue/backlog/review/icebox/done) or a malformed/unterminated attribute, falling back to candidate/HEAD; (2) fail closed when the serving supervisor is already classified `supervisor_binary_stale` (#fcc0) — do not ingest its buffer; (3) run the structural/lint check on the candidate BEFORE the disk write, not after; (4) de-dup singleton blocks on parse instead of concatenating. CLEAN session (snapshot-adoption / converge write path). Related #fcc0/#compactqattr/#adoc-compact-during-turn-response-loss. Plan: tasks/agent-doc/plan-content-ours-duplicate-corruption.md
- [ ] [#mphaseloop] [clean-session] Multi-phase task auto-loop policy (operator directive 2026-06-14): go-mode drain must continuously advance a multi-phase task until it is DONE or legitimately blocked (clean-session / operator-verify / external outage). "Needs review" is NOT a terminal stop — when a phase needs review, move it to agent:review (gated [/]) and KEEP draining the remaining phases/queue; never halt the whole queue just because one phase needs review. Codifies SKILL #drain-no-defer + "complete over gate" into the drain/finalize loop: only done/blocked terminate the drain; a review-routed phase emits its review item and the drain continues to the next drainable head. FIX: in the go-mode drain/finalize path, on a review-routed phase do not treat review as queue-terminal — emit the review item + advance to the next drainable head; proof line drain_continue_after_review next_head=... CLEAN session (drain/queue path).
- [ ] [#compactqattr] [clean-session] Compaction wipes agent:queue inline attributes (preset, priority, likely go) from the opening marker — operator observed in monsterrodholders.md: post-compaction deleted all agent:queue attributes. Too much blast radius. Compaction should only archive/truncate the EXCHANGE component and must leave every other component opening marker byte-identical. HYPOTHESIS: the post-compact CRDT refresh (compact.rs, CRDT state refreshed from post-compact content) re-serializes component opening tags from parsed structure and drops inline attrs. FIX: preserve the verbatim opening marker (all inline attributes) for every non-archived component across compact + the post-compact CRDT refresh; add coverage that compacting a doc whose agent:queue carries preset/priority/go leaves that marker unchanged. CLEAN session (compaction/CRDT path).
- [ ] [#qmisstrike] [clean-session] finalize strike heuristic wrongly strikes the NEXT open queue head. Operator report (tsift.md): after manually deleting struck items, agent-doc struck through the next still-open item. LIVE-REPRODUCED this session: a finalize answering the free-text `:pushpin: JB Run Agent Doc` head consumed it AND additionally struck the next open id-backed head #qchurn (response never addressed it); session-check `#queue-clear-unrun-items` caught it, had to un-strike + reset --from-current + commit. ROOT CAUSE (hypothesis): the strike cursor over-advances by one and strikes an unanswered open head; id-backed heads should only strike via --done/--pending-gate/queue-consume, never the free-text strike heuristic. FIX: strike only the head(s) this turn`s response actually answered (match by topic/text, not position); never side-strike an id-backed head; treat operator strike-deletions as terminal (no forward re-strike); proof queue_strike target=… reason=answered|consume. CLEAN session. Related #qpresetstrike/#pushpinaccum/#qchurn. Plan: tasks/agent-doc/plan-finalize-misstrike-next-head.md
- [ ] [#jbrestale] [clean-session] JB `Run Agent Doc` should auto-restart / red-green-deploy a stale or churn-stopped supervisor and switch over automatically instead of failing closed — across ALL harnesses (Claude Code, Codex, OpenCode). Operator pressed Run Agent Doc expecting the new supervisor to be red/green-deployed and switched over automatically. LIVE-REPRO: 2026-06-13 tsift.md churn-stop, and 2026-06-14 lazily-rs.md (failed_stage=queue_paused reason=stale route-owned supervisor pid968752 replaying already-answered/archived #advance-review queue item with go-mode; halting churn until supervisor recycled, receipt 3455). The churn-stop reason itself prescribes a recycle, but the dispatch path treats it as terminal. ROOT CAUSE: the supervisor self-recycle (supervisor_binary_stale_self_recycled, #supselfheal/#aazp/#4myd) is gated on the idle/next-queue-item boundary, which a churn-stopped queue never reaches. FIX: in the JB Run Agent Doc dispatch handler (all harnesses), classify a churn-stop whose reason is a stale supervisor (supervisor_binary_stale / pre-<ver> / stale route-owned supervisor replaying an archived item) and branch to restart-supervisor / red-green execve switchover (proof route_dispatch_recovery action=restart_supervisor result=restarted|reexec_failed), lift the stale-injected pause, re-dispatch ONCE (no loop), keep operator-verify heads deferred (#goqueuestall), fail closed + keep pane alive on reexec_failed. CLEAN session (route/dispatch + recycle path). Related #recyclerestart/#supselfheal/#aazp/#4myd/#qchurn/#qpresetstrike/#fcc0. Plan: tasks/agent-doc/plan-jb-run-restart-stale-supervisor.md
- [ ] [#0kjc] #mdastquote [clean-session] Markdown AST must treat tags inside quoted strings ("...") and code spans/blocks (backtick / triple-backtick) as CONTENT, not document tags. Operator report: agent:*/#id-looking tokens inside quotes or fenced code are being parsed as real document component markers. Same family as #lintfence (fenced agent:* markers break lint + parser). FIX: tagpath/agent-doc markdown tokenizer must mask quoted-string and code-span/code-fence regions before component/tag extraction. CLEAN session (parser path).
- [ ] [#pushpinaccum] [clean-session] Queue `:pushpin:` annotations accumulate/nest in the wrong place: a head already starting `- :pushpin:` gets another `:pushpin:` prepended -> `:pushpin: - :pushpin: ...` (operator corrected one live 2026-06-13). The queue head-grouping/annotation formatter prepends the pushpin marker without checking for an existing leading pushpin/bullet. FIX: make the queue-head annotation prefix idempotent (single :pushpin:/bullet). CLEAN session (queue formatter path).
- [ ] [#c2b6] [operator-verify] compactdrift live JB Compact Exchange re-verify: with a freshly make-install-ed agent-doc (installed cdylib currently predates aafa6510), run JB Compact Exchange on a doc whose HEAD holds a finalized Re: response and confirm it compacts WITHOUT the "refusing to auto-adopt committed historical response patchback / out-of-band typed_component_drift" bail. Any persistent failure from a NON-wedged session files against stuckhandoff2, not here. Code complete + committed agent-doc bca26cfd; 108 commit_ tests green this cycle.
- [ ] [#aazp] [operator-verify] suprecyclestall live-verify (agent-doc aafa6510, installed+release-built): restart a stale session via session restart-supervisor FILE --force so it runs the new binary, then drive an idle/next-queue-item recycle. Confirm ops.log shows supervisor_binary_stale_self_recycled ... candidates=[...] then EITHER a clean execve hot-reload OR supervisor_reexec_failed fallback=continue_current_binary with per-candidate errno — and in the failure case the pane STAYS ALIVE (no hang) with the restart hint shown. Subset of the broader recyclerestart sync-block work. Cannot be agent-done: needs a live editor + real recycle.
- [ ] [#4myd] [operator-verify] #suprecyclestall live-verify: with the new binary installed (cargo install + cargo build --release done, pushed agent-doc aafa6510), restart a stale session and drive an idle/next-queue-item recycle. Confirm ops.log shows `supervisor_binary_stale_self_recycled ... candidates=[...]` then EITHER a clean execve hot-reload OR `supervisor_reexec_failed fallback=continue_current_binary` with the per-candidate errno — and in the failure case the pane STAYS ALIVE (no hang) with the restart hint shown. Cannot be agent-done: needs a live editor + real recycle.
- [ ] [#recyclerestart] [clean-session] PCP/Supervisor recycle does not restart the agent session (img_21.png); JB Sync Tmux Layout after tmux kill-pane surfaced `Sync deferred: another tmux layout sync is already running`. ROOT CAUSE (code-grounded): the warning fires when agent_doc_sync_try_lock() (ffi.rs:746, a process-wide SYNC_LOCKED AtomicBool shared cross-editor) is already held. SyncLayoutAction.kt holds that FFI lock across the ENTIRE agent-doc sync subprocess (runCommandWithTimeout, line ~299, released only in finally line 331), and that subprocess is bounded only by the 30s SYNC_PROCESS_TIMEOUT_MS and can block on in-pane/binary recovery (the timeout message literally says retry can run binary recovery). So layout GEOMETRY sync is coupled to in-pane readiness, and a second manual sync racing an in-flight one bails immediately (non-blocking compare_exchange) instead of superseding. FIX: decouple tmux geometry sync from in-pane init/recovery; hold SYNC_LOCKED only across the fast geometry reconcile, run pane readiness/recovery separately; make a newer sync generation supersede an in-flight stale one rather than warn-and-skip; prove recycle restarts/clears the harness session. CLEAN session. Plan: tasks/agent-doc/plan-pcp-recycle-no-restart-sync-block.md
- [ ] [#qchurn] [clean-session] go-mode no-op churn: after #goqueuestall tag-defer skips the id-backed [clean-session]/[operator-verify] backlog heads, untagged FREE-TEXT operator bug-report prompts in the ## Queue block still keep queue_active=true and the supervisor idle-watch re-injects queue_trigger=auto each idle boundary -> repeated no-op /agent-doc cycles (no content diff, user_intent_prompt_changes empty, zero drainable work). Halted live 2026-06-13 via admin queue pause (receipt 27, gen 338). FIX: go-mode should treat a queue whose only remaining prompts are non-id free-text reports (no actionable do #id / new user prompt) as not-drainable and stop re-dispatching, same as the undrainable-head skip. Plan: tasks/agent-doc/plan-go-queue-stall.md
- [ ] [#fcc0] [TOP] [clean-session] Zero JB File Cache Conflict dialogs. Route all document-mutating disk writes through one shared converge_document_or_disk() behind the listener-active gate (editor_ipc primary + disk_fallback); resolve the ipcdrift/stuckhandoff2 wedge that forces fallbacks. CLEAN session required (live write path); 0-dialogs proof operator-drive live JB. Plan: tasks/agent-doc/plan-zero-file-cache-conflict.md
- [ ] [#rtwbcast] [clean-session] rtwatch rung 4 — multi-editor CRDT broadcast. Phases rtwbcast-id (editor_id in FFI buffer-report + per-editor live_buffer_snapshot_path), then deliver/merge/verify. CLEAN session required; live two-IDE convergence verify is operator-drive. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#qdedup] [clean-session] Between-queue-turn trigger enqueue must de-duplicate: successive between-turn enqueues concatenated into one garbled trigger (`/clear /agent-doc <FILE>/agent-doc <FILE>`). Operator: stop the drain once a turn completes, insert into the inactive agent between turns, enqueue UNIQUE (one /clear, one /agent-doc <FILE>) landing once in the PCP/Supervisor between-turn stage. Compose next trigger as a deduped set, gate strictly to the inter-turn boundary, proof line between_turn_enqueue deduped=N. Related #qflood2/#qchurn/#clearcontresume. CLEAN session. Plan: tasks/agent-doc/plan-between-turn-trigger-dedup.md
- [/] [#clearexchstale] [clean-session] JB Clear Exchange clears the exchange but the content comes back (from a stale HEAD/snapshot replay); same session, a deleted+retyped prompt also revived. Same disk-vs-buffer convergence family as #fcc0: a redeliver/replay path re-applies HEAD/snapshot over the cleared buffer, and clear does not advance the durable merge base. FIX: route clear through converge_document_or_disk + advance merge base; gate redeliver on fresh editor-buffer proof (#dvre/#eprf). CLEAN session. Plan: tasks/agent-doc/plan-jb-clear-exchange-stale-head-revival.md
- [/] [#qpresetstrike] [clean-session] Preset-token queue head (a #name from prompt_presets, e.g. #advance-review) is unstrikeable by any agent path AND a spent preset head fails closed instead of auto-repairing. `queue consume` rejects it as id-backed (leading #), `--done`/`--pending-gate` reject it as not a backlog id, and `finalize` keeps the head — so it wedges queue_active=true and re-serves every /agent-doc invocation. LIVE-REPRO 2026-06-13 (efs): operator pressed JB Run Agent Doc expecting auto-repair; dispatch returned 'failed_stage=queue_paused reason=advance-review preset head is spent (backlog added + both features shipped); pausing so the go-queue does not re-trigger advance-review. Operator can clear the - #advance-review line' (receipt 3426). The pause itself is correct (do not re-run a spent preset) but it fails CLOSED and prescribes a MANUAL line deletion instead of auto-striking the spent head — same fail-closed-instead-of-auto-repair family as #jbrestale. FIX: (1) classify a #-head against prompt_presets AND backlog/icebox before routing; a preset head should strike like a free-text head via consume + the finalize heuristic, while do[#id] heads stay on the --done path. (2) On a spent-preset queue_paused, JB Run / dispatch must auto-strike the spent preset head and re-dispatch the remaining queue ONCE (or cleanly close if empty), not surface a terminal error requiring manual line deletion. Workaround: strike ~~#name~~ + drop go + reset --from-current --preserve-session + commit. Relates to #qchurn/#goqueuestall/#jbrestale. Plan: tasks/agent-doc/plan-preset-queue-head-unstrikeable.md
- [/] [#qflood2] [operator-verify] qflood regression: JB Run Agent Doc on efs coalesced as exit-1 (dual-drain owner, gen 66 already in flight). Needs live multi-process verify; if it still collides, coalesce should report deduped-success not exit-1. Plan: tasks/agent-doc/plan-queue-dispatch-flood.md
- [/] [#snbc2] [recommended] OPTIONAL cosmetic (not a bugfix): collapse baselines/<hash>.md into a snapshot-store variant so there is one file instead of two, WITHOUT changing merge-base semantics (stays current-at-preflight = resolve_current_doc(newest-of(disk,buffer)), NOT git HEAD). #snbc Slice-1 audit proved the substantive HEAD-as-base consolidation would regress merge correctness (drops the uncommitted new prompt from the common ancestor). Moderate risk, touches the closeout path; gate behind NEW SimWorld coverage for the prompt-bearing finalize-merge class first. Must run in a CLEAN session. Plan: tasks/agent-doc/plan-snapshot-baseline-consolidation.md (Revised slice 2).
- [/] [#efscodexsubmit] JB Run Agent Doc in equityfundingsource.md enters but does not submit the prompt to Codex (operator must press Enter). Same class as #2v7b/#jbcodexsubmit: Codex direct-pane dispatch (route.rs send_command_unchecked) detects prompt_not_submitted (merged text+CR not treated as submit by the Codex TUI) but has no Enter re-submit fallback. FIX: Codex-scoped one-shot Enter re-submit + route_submit_resubmit proof logging + SimWorld coverage (operator-requested). CLEAN session required (touches route.rs; submodule carries dirty #fintol tree). Plan: tasks/agent-doc/plan-jb-codex-submit.md
- [/] [#stuckhandoff2] Stuck-handoff hardening: controllers wedged in --handoff-state preparing survive IDE restart + keep corrupting the worktree over ipc.sock. ROOT CAUSE: two-phase handoff (prepare_handoff->promote_handoff, project_controller.rs:3307/3318) is client-driven; if the client dies/RPC fails between the two, the controller stays Preparing forever. The existing reaper (#kqr6/#sjwm/#stuckhandoff, 45s) is bootstrap-RECORD-scoped (kills only bootstrap.pid), same-project-only, and fires on bind/gc not a timer -> orphaned + cross-project + no-new-invoke wedges survive (operator must pkill -f 'controller serve ... --handoff-state preparing'). FIX (defense-in-depth): M1 controller self-watchdog/suicide-timer on Preparing [strongest], M2 gate worktree/ipc writes on Stable [symptom-killer, overlaps #pcwc], M3 proc-scan reaper for orphans, M4 client handoff drop-guard, M5 cross-project gc sweep. M1+M2 are the durable core; land together with SimWorld coverage. CLEAN session required (submodule carries uncommitted #fintol work). Plan: tasks/agent-doc/plan-stuck-handoff-hardening.md
- [/] [#qt32] #lvbremain Operator live zero-drift PROOF of the 08b supervisor-in-process cutover (cannot be agent-done — needs a live editor). Restart a session, drive one edit-during-finalize cycle, agent reads ops.log for write_authority action=routed + plugin_watch_readonly + supervisor_host_gate hosting=in-process + ZERO drift lines. [recommended]
- [/] [#2v7b] #jbcodexsubmit / #jbclaudesubmit JB Run Agent Doc enters the routed prompt but does not submit it (operator presses Enter). CODE-COMPLETE 2026-06-13 (agent-doc 3b71cfeb, superproject 8fcdc067f): JB dispatch-only routes BOTH Claude and Codex through the same direct-pane send_command_unchecked path (merged text+CR send-keys), and TriggerStillVisible detection already covered both, but the one-shot Enter re-submit was gated codex-only. Generalized direct_pane_needs_codex_resubmit -> direct_pane_needs_enter_resubmit (codex|claude; opencode excluded; requires TimedOut+trigger_visible so no stray Enter into an empty composer); proof line route_submit_resubmit ... action=enter_key result=accepted|still_visible; 3 unit tests; make check 4369 passed 0 FAIL. Built+installed+recycled. GATE now operator-drive ONLY: run JB Run Agent Doc against a Claude pane AND a Codex pane, confirm ops.log shows route_submit_issue issue=prompt_not_submitted then route_submit_resubmit action=enter_key result=accepted with the queue starting. Plan: tasks/agent-doc/plan-jb-codex-submit.md
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
