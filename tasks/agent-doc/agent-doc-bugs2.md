---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
resume: 70b4d2d9-c397-4c86-8c2b-9effed66f82e
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
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

Live IntelliJ File Cache Conflict Cancel proof captured; JB Cancel residual review items completed.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260531-022911.md`*

Compacted content:
- Archived 5 response topic(s): closeout-repair-churn; closeout repair queue state; JB Run Agent Doc busy-queue reports; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: #spec-test-commit-push-deploy — opus-4-8

Done — queue-item responses now copy the originating prompt. This response is the live demo: the blockquote just above was inserted by the binary, not typed by me.

#### What changed

When a closeout consumes one or more queue prompts, the binary embeds the full consumed prompt text as a `> **Queue prompt:**` blockquote immediately after this cycle's `### Re:` heading. It writes into **both** the document and the snapshot so the selective-commit boundary stays consistent (verified: `HEAD` carries the echo and the working tree is clean after a live finalize).

Why the binary and not the skill: an auto/synthetic queue head is never typed into `agent:exchange`, so a consumed queue turn used to record only the answer with no trace of what it answered. Per the project's "all deterministic behavior in the binary" rule, the echo is a guaranteed, agent-independent document mutation rather than something the agent must remember to do.

- **Location** — `plan_queue_prompt_consumption` (`write.rs`); helpers `format_consumed_prompt_echo` / `embed_consumed_prompt_in_response` / `locate_response_heading_offset`. The response heading is located by the captured response body's first line, falling back to the last non-code `### Re:` heading.
- **Fail-safe** — injection is skipped (content unchanged) when the prompt is empty, the exchange/heading can't be located, the same echo already exists, or the prompt's first line already appears as an exchange line (a user typed it in directly — no double-echo, e.g. the manual `❯ do #fix1` case).
- **Verbatim** — multi-line prompts and fenced code are quoted as-is ("let's try copying the entire prompt"). The echo lands before the trailing boundary marker, so it is committed history and is never re-detected as an unresolved tail prompt.

#### Verify / deploy

- Tests: added `finalize_echoes_consumed_free_text_queue_prompt_into_response`; updated `finalize_consumes_synthetic_queue_prompt_when_response_topic_targets_head_id` and the two `run_*_queue_continues_until_drained` tests to be queue-scoped (prompt drains from the queue, echo appears in the response). Full workspace `cargo test` + `cargo clippy` green.
- Spec: `specs/07-orchestration-commands.md` documents the `#queue-prompt-echo-in-response` invariant.
- Deploy: `cargo build --release` + `cargo install --path .` (both binaries refreshed); committed + pushed in `src/agent-doc` (`c030760e`) with the superproject pointer bump (`6ca3ae899`).

Note: this head carries a trailing `#spec-test-commit-push-deploy` tag, so it consumes as a synthetic/preset head — the response heading had to resolve to exactly that id to strike it, which is existing behavior. Follow-up `[#queue-prompt-echo-summary]` queued for summarizing long prompts.
<!-- agent:boundary:110f809d -->
<!-- /agent:exchange -->

<!-- agent:queue auto -->
---
The response to a queue item should contain the prompt from the queue. If the prompt is long, it may be beneficial to have a summary of the prompt. But for now, let's try copying the entire prompt.
#spec-test-commit-push-deploy
---
JB `Run Agent Doc` successfully started a turn and had an error.
```
[route] debounce OK — file idle for 0.6s and typing indicator idle
[route] target tmux session: 0
[route] authoritative actor generation 241 for tasks/monsterrodholders.md is busy on pane %40; queued pending dispatch "do [#netdemand]. #spec-test-commit-push" in agent:queue auto (appended=true, already_present=false, superseded=false) instead of injecting a duplicate trigger
```
#spec-test-commit-push-deploy
---
preset #spec-test-build-install-commit-push
- do [#clear-stops-running-process]
- do [#stash-pane-promote-on-focus]
- do [#do-id-partial-closeout-state]
- do [#closeout-recovery-state-machine]
- do [#codex-owned-pane-prompt-miss]
- do [#stuck-capture-compact-false-positive]
- do [#codex-final-response-live-verify]
- do [#codex-route-busy-ctrl-g-opens-editor]
- do [#cross-project-stash-pane-condition]
- do [#route-drain-all-surface-live-verify]
- do [#run-agent-doc-stale-busy-replay]
- do [#jb-busy-queue-ux-feedback]
- do [#free-text-queue-owner-consume]
- do [#plugin-cleanup-menu-command]
- do [#stash-session-ttl-prune]
- do [#next-steps-backlog-capture]
- do [#ah0s]
- do [#backlog-priority-attribute]
- do [#backlog-queue-sync-attribute]
- do [#preset-item-id-collision]
<!-- /agent:queue -->
###
<!--
Verify [#ipcfullprompt-recur2]
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
The completed queue item was not removed from the queue. #mermaid
#spec-test-build-install-commit-push
---
equityfundingsource.md had the full-document IPC duplicate corruption. Then a new full-document IPC deletion under the prompt corruption happened. Both several times while editing the prompt.
#spec-test-build-install-commit-push
---
Please review the agent-doc, claude, codex logs. What are #next-steps to fix bugs?
---
When I ran the last turn, #spec-test-build-install-commit-push was removed by agent-doc.
---
As I was typing into the comment below `/agent:exchange`, the full-document IPC corruption happened, then the duplicate line happened, then the full-document IPC corruption happened again.

In addition to diagnosing and fixing the issue, please provide a mermaid showing how the issue occurred and where in the agent-doc lifecycle the full-document IPC corruption can occur. Can we mathematically eliminate all possibility of the full-document IPC corruption from occurring?

#spec-test-build-install-commit-push
-->

## Backlog

<!-- agent:backlog -->
- [ ] [#queue-prompt-echo-summary] [recommended] Summarize long consumed queue prompts in the #queue-prompt-echo-in-response echo instead of copying verbatim. Current behavior embeds the entire prompt as a > **Queue prompt:** blockquote after the response heading; for long prompts a bounded summary plus a pointer to the full queue text would keep the exchange readable. Add a length threshold + summarization path (the user explicitly deferred this: 'for now, let's try copying the entire prompt').
- [ ] [#completed-queue-residue-regression] [recommended] Completed queue items can still remain visible in agent:queue after the turn is finished, despite the archived #drained-done-queue-clear fix. Treat as a regression/uncovered route or IPC path: add a committed-state guard for provable completed queue residue, audit finalize/write/preflight/run continuation drain ordering, and verify live editor/snapshot/HEAD convergence. Plan: tasks/agent-doc/plan-completed-queue-residue-regression.md.
- [ ] [#stash-pane-promote-on-focus] [recommended] A live-owner pane residing in the stash tmux window must be promoted/reparented into the 0:agent-doc window when the document is focused/selected, not merely selected in place. Live repro 2026-05-30 agent-doc-bugs2.md: pane %8 (live owner) is stuck in the stash window; focus/select does not move it. The focus live-owner fix (submodule e6dafa52) now selects the correct pane but does not reparent it. Tied to #cross-project-stash-pane-condition and #plugin-cleanup-menu-command.
- [ ] [#do-id-partial-closeout-state] [recommended] `do [#id]` partial closeouts must rewrite or gate the remaining state instead of leaving a stale open item/queue head. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md`: `do [#shoplabels]. #spec-test-commit-push` completed repo-backed feed-label code/tests/spec/commits/pushes, but deploy + GfW feed sync + live Ads label coverage remained; `#shoplabels` correctly stayed open, but the backlog/queue still looked like the original full task and the user had to ask whether it was done. Require `--pending-edit`, `--pending-gate`, or a typed stays-open marker for partial `do [#id]` closeout. Plan: tasks/agent-doc/plan-do-id-partial-closeout-state.md.
- [ ] [#closeout-recovery-state-machine] [recommended] Remaining #closeout-repair-churn scope: a unified typed closeout recovery state machine shared by finalize / write --commit / repair / commit (states missing_response_body, direct_response_patchback, escaped_template_patch, boundary_only_drift, reaped_item_pending_parent, nested_parent_pointer_stale) that collapses the scattered 'try finalize / write --commit / commit' diagnostic chain into one typed instruction per state. The escaped-component-marker fail-closed guard already shipped (PatchbackShape::EscapedComponentMarkers). Direct-### Re: absorption and atomic parent-pointer repair are already substantially covered by write --commit absorption and complete_required_closeout retry. Plan: tasks/agent-doc/plan-closeout-repair-churn.md.
- [ ] [#free-text-queue-owner-consume] [recommended] Free-text `agent:queue auto` heads can remain live after an owner-pane/direct Codex closeout writes the response but lacks queue-target proof. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md`: `Approve [#shoptiers]. What are #next-steps?` was answered, then reported as not marked done; same-pane `agent-doc <FILE>` refused recursive dispatch and repair finalize logged `[queue] skipped consumption because the active prompt did not target the queue head`. Preserve id-backed queue safety, but record/restore synthetic free-text queue-target proof for recursive-direct repair/finalize paths and add diagnostics/regressions. Plan: tasks/agent-doc/plan-free-text-queue-owner-consume.md.
- [ ] [#preset-item-id-collision] [recommended] Enforce unique active identities across frontmatter `prompt_presets` keys and active `agent:backlog` / `agent:review` / `agent:icebox` item ids. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md`: `#next-steps` exists both as a prompt preset and as a backlog item, making `do #next-steps` and "top backlog item: #next-steps" ambiguous between preset expansion and backlog execution. Fix with an identity registry, mutation-time rejection, preflight/session-check fail-closed diagnostics, and regressions. Plan: tasks/agent-doc/plan-preset-item-id-collision.md.
- [ ] [#codex-final-response-live-verify] [recommended] Opportunistic live re-verification of the #codex-final-response-not-written guard: do a real Codex direct/manual run that commits side-effect work, confirm session-check (via the .codex Stop hook) fails closed with the 'no assistant response body was captured' message + 'agent-doc write --commit' recovery when no ### Re: close-out was written, and that a normal Codex turn with a response stays OK. Guard + deterministic regressions already landed (session_check.rs check_committed_without_response_body_guard).
- [ ] [#stuck-capture-compact-false-positive] [recommended] stuck_captured_cycle false-positive after compaction: a capture recorded as committed (cycle-1780100032048, response 'gate→done for codex guidance') is later archived by compact, so preflight reports committed-but-not-in-HEAD and suggests `write --commit` recovery — which would re-inject an already-archived response (it lives in .agent-doc/archives/<hash>-*.md). Fix: when a captured-committed body is absent from HEAD because it was compacted (present in the matching archive), reconcile the capture instead of flagging it stuck. Repro 2026-05-29 agent-doc-bugs2.md.
- [ ] [#codex-owned-pane-prompt-miss] [recommended] Codex-owned `agent-doc <FILE>` direct invocation from the same pane can abandon recursive dispatch without processing unresolved exchange prompts. Live repro 2026-05-30: `src/boost-client/tasks/monsterrodholders.md` contained `Please assist in placing GA4 Analytics credentials in passage.`, but the same-pane recursion guard refused dispatch and no response was written. Fix with earlier owner-pane self-invocation detection, unresolved-prompt preservation, and Codex guidance/final-gate recovery. Plan: tasks/agent-doc/plan-codex-owned-pane-prompt-miss.md.
- [ ] [#codex-route-busy-ctrl-g-opens-editor] [recommended] route.rs busy-pane reroute interrupt (route.rs ~6409) also sends C-g unconditionally to a busy Codex pane before the Escape+C-c fallback — same root cause as #codex-interrupt-clear-ctrl-g-opens-editor: C-g opens the external editor ($EDITOR/nvim) in the normal Codex composer. Gate it on a shell reverse-i-search/history-search capture like send_operator_interrupt_sequence now does (or skip C-g when the busy blocker is an active turn). Tracked separately from the operator interrupt-clear fix because a hard change here alters reroute semantics. Add a route SimWorld regression for the non-search composer case — the existing ctrl-g test (write_mock_busy_registered_agent_doc_recovers_on_ctrl_g) only models reverse-i-search recovery, which is why this regression slipped through.
- [ ] [#cross-project-stash-pane-condition] [recommended] resync logs 'stash pane %N registered in its own project root — skipping kill' because nested project roots (superproject agent-loop + submodule src/boost-client) auto-detect the SAME tmux session 0 and share one stash window, so superproject sync stashes the submodule's live session pane and resync re-discovers it cross-project. Not a resync logic error (preservation is correct); root cause is the shared-session layout for nested roots. Live evidence + fix options (A nested-root session isolation [recommended], B per-root stash partitioning, C out-of-scope skip, D reap stale agent-doc stash panes) in tasks/agent-doc/plan-cross-project-stash-pane-condition.md. Needs a session-model design decision before implementing; deferred from blind code change.
- [ ] [#route-drain-all-surface-live-verify] [recommended] Live-verify the route-drain all-surface reap (submodule b239988b / superproject 4c7b06b7f) resolves the JB Run Agent Doc "completed tracked item blocks dispatch" retry: with the installed binary, reproduce a closeout that leaves a completed [x] item in review or icebox on an IDE-owned doc, then run JB Run Agent Doc once and confirm it dispatches without a manual repeat. Also exercises the #pending-gate-snapshot-desync snapshot re-sync (ab074710) under the route path.
- [ ] [#run-agent-doc-stale-busy-replay] Follow-up to the #snrun stale-busy-projection fix (submodule 89c99c79): the dispatch-only busy-vs-stale repair is covered by unit tests on the pure decision functions; add a live JB Run Agent Doc replay regression (SimWorld or tmux-ci) exercising a stale busy actor projection + proven ready prompt and asserting dispatch instead of agent:queue auto enqueue. Also still open in #snrun: distinguishing rescued-stash accepted-only proof misses. Plan: tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md.
- [ ] [#jb-busy-queue-ux-feedback] [recommended] When the authoritative actor is genuinely busy, JB Run Agent Doc correctly queues the dispatch in agent:queue auto instead of injecting a duplicate trigger (monsterrodholders.md, equityfundingsource.md reports), but the plugin surfaces the route/resync log dump as an error. The dump also includes benign cross-project resync info such as `resync: stash pane %N is registered in its own project root — skipping kill` (cross-project stash-pane preservation, resync.rs:771/1303/1793 — a stash-window pane that is the live authoritative pane of a different project root is correctly preserved, not reaped), which is expected and not a failure. Present queued-behind-a-busy-turn and benign route/resync info as informational notifications distinct from real failures so expected behavior does not read as a plugin error.
- [ ] [#plugin-cleanup-menu-command] [recommended] Expose session cleanup as agent-doc plugin-menu commands (JetBrains + VS Code): a 'Resync / Fix Sessions' action wrapping agent-doc resync --fix and a 'GC Stale Sessions' action wrapping agent-doc gc, so operators can clear stale routing/registry state and reap orphaned panes from the IDE without dropping to a terminal. Thin event-reporter pattern — the plugin shells the existing CLI/FFI; no cleanup logic in the plugin. Pairs with #stash-session-ttl-prune (auto hygiene) and #jb-busy-queue-ux-feedback (notification classification).
- [ ] [#stash-session-ttl-prune] [recommended] Optional opt-in hygiene to cap stash-session accumulation: reap agent-doc stash panes whose session has been idle in the stash beyond a configurable TTL. Distinct from the existing orphan reap (resync.rs step 8), which only kills unregistered + no-supervisor + no-owned-file panes; live registered/supervisor-backed sessions are correctly never auto-killed, which is why ~9 agent-loop agent-doc panes accumulate in the shared session-0 stash. Must be opt-in (frontmatter/.agent-doc config) + live-verified before enabling (destructive). Relates to #cross-project-stash-pane-condition and #jb-busy-queue-ux-feedback.
- [ ] [#next-steps-backlog-capture] Agent responses containing #next-steps recommendations do not auto-add items to backlog when prompt preset is present. Agent must manually call `--pending-add` but the planning output `pending_mutations: expect_add` is not enforced by session-check `pending_done_guard`.
- [ ] [#ah0s] [recommended] Add explicit-position --pending-add primitives so ordered backlog batches never depend on implicit prepend-vs-argorder semantics: --pending-add-after <id>, --pending-add-before <id>, --pending-add-back/--pending-append, keeping --pending-add as the front-insert default. Backlog is a priority pool with ID-based consumption (not a stack/queue; FIFO lives in agent:queue), so position should be author-set explicitly when it matters. Two agents (open-code.md #adcr waves f94e7851c; corky.md crate-split phases fc8d9b3bc) both reversed args expecting prepend-flip and got reverse-of-intended order. Pairs with #pending-add-append-not-prepend (that item finalizes the default-order decision; this adds the explicit escape hatches). Plan: tasks/agent-doc/plan-pending-add-explicit-position.md.
- [ ] [#backlog-priority-attribute] [recommended] Add a priority attribute to agent:backlog / agent:icebox / agent:queue that orders the item list by priority (a component-level priority flag and/or per-item priority markers), so the binary maintains priority order instead of requiring manual --pending-reorder each cycle. Automates the by-hand reprioritization done in #next-steps cycles. Composable with #backlog-queue-sync-attribute; sibling of #ah0s (explicit-position primitives). Requested 2026-05-30.
- [ ] [#backlog-queue-sync-attribute] [recommended] Add a queue=sync|append|prepend attribute to agent:backlog / agent:icebox: queue=sync replaces agent:queue content with a do [#id] list for every item; queue=append adds item ids not already in agent:queue; queue=prepend prepends them. Composable with #backlog-priority-attribute (prioritize first, then regenerate the queue), automating the manual 'regenerate the queue to the new priority' step that currently has to be done by hand after a reorder. Requested 2026-05-30.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#clear-stops-running-process] [recommended] JB Clear Session Context / Claude Code /clear must stop the running agent-doc child so a following JB Run Agent Doc is not blocked by a stale busy actor. Live repro 2026-05-30 agent-doc-bugs2.md: after /clear, a still-running process blocked Run Agent Doc. Overlaps #cancel-orphans-preflight-cycle and the operator_clear busy-projection path in session_actor_cmd.rs.
- [/] [#jb-run-agent-doc-response-queue-contamination] [recommended] JetBrains `Run Agent Doc` can add assistant response prose to `agent:queue auto` after compact/repair drift. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md`: queue gained `- Yes. I drove the already-authenticated Google Ads browser session...`, copied from `### Re: #gads106demote`, alongside real `do [#nbsearch]`/`#bidstrat`/`#pdpconv` entries. Require queue-source provenance and reject assistant response bodies as queue candidates. Plan: tasks/agent-doc/plan-jb-run-agent-doc-response-queue-contamination.md.
- [/] [#jb-compact-repair-left-uncommitted] [recommended] Compact-output repair can leave the corrected session document dirty instead of committing through a binary-owned repair boundary. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md`: visible exchange correction differs from malformed `HEAD`, creating mixed visible/HEAD/snapshot state for later JetBrains actions. Repair should commit atomically or fail closed with an exact recovery path. Plan: tasks/agent-doc/plan-jb-compact-repair-left-uncommitted.md.
- [/] [#jb-compact-malformed-response-commit] [recommended] JetBrains `Compact Exchange` can commit malformed compact output. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md` / commit `f8fe36b`: compact summary line was committed with user prompt prefix `❯`, and archived response content remained in the live exchange shape. Add post-compact structural validation and fail closed before commit. Plan: tasks/agent-doc/plan-jb-compact-malformed-response-commit.md.
- [/] [#prompt-preempts-auto-queue] [recommended] Unanswered exchange prompts must run before active `agent:queue auto` continuation, even when the prompt was accidentally baselined into the `.agent-doc` snapshot. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md`: `What are #next-steps to complete review items?` was visible after the exchange boundary and duplicated in `agent:queue auto`, but the snapshot already contained the exchange prompt, so the next diff only saw queue bookkeeping and the prompt did not run. Fix: add snapshot-independent unresolved exchange prompt detection, make preflight/direct-run prefer it over queue synthetic diffs, prevent route queue snapshot recovery from absorbing unanswered exchange prompts, and defer Codex/Claude queue continuation while such a prompt exists. Plan: tasks/agent-doc/plan-prompt-preempts-auto-queue.md.
- [/] [#queue-user-edit-overwrite] [recommended] User-authored agent:queue edits can be silently overwritten/deleted by write/reset/commit queue convergence. Live repro 2026-05-30 boost-client monsterrodholders.md: user added do [#gscaccess] at queue head; agent-doc repeatedly removed/reverted it during closeout/snapshot repair. Fix by tracking queue edit provenance/epochs, preserving explicit queue edits unless the current response consumes that exact head, and failing closed on stale IPC/snapshot queue rewrites. Plan: tasks/agent-doc/plan-user-queue-edit-preservation.md.
- [/] [#ipcfullprompt-recur2] CURE ONLY (capture/detection split out to #ipcfullprompt-capture, done). Editor-side PatchWatcher.setText PREVENTION: stop the live editor buffer from producing full-document/full-tail duplication or deletion while a prompt is being typed. Gated on the transport/payload packet from idea.log (enable #com.github.btakita.agentdoc debug logging) to pin the exact setText call shape. Add a SimWorld/Kotlin regression for the prevention once the fix lands; bump pluginVersion past 0.2.137 and reinstall the signed plugin. Stays open until prevention lands and is live-verified. Plan: tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md.
- [/] [#finalize-retry-ipc-response-duplication] [recommended] Multi-retry finalize + live IPC duplicates the response and prefixes its body lines with the user-prompt marker. Live repro 2026-05-29 on agent-doc-bugs2.md: a closeout needing several finalize/write retries (lint-gate / session-check / live_prompt_drift re-runs) with the IDE IPC listener active and concurrent editing left a duplicated response block plus agent body lines wrongly prefixed (uncommitted; HEAD stayed clean; manual git checkout repaired). Fix direction: idempotent retry convergence on IPC already_applied, never prefix agent response body as a prompt, and repair/session-check auto-dedup. Relates to #prompt-duplicated-while-typing, #duplicate-response-after-commit, #ipc-dup-tracking-double-merge (plugin 0.2.138 not yet installed during repro). Plan: tasks/agent-doc/plan-finalize-retry-ipc-response-duplication.md.
- [/] [#ipc-dup-tracking-double-merge] PromptPoller double-tracking fix (plugin 0.2.138, submodule 32fb0b98 / superproject d585a33de): addFile keyed trackedFiles by project-relative path while handlePollResults keyed by "$cwd:$file", double-tracking the same VirtualFile (live: corky.md, brandon-cinquegrana.md, agent-doc-bugs2.md; trackedFiles=13 vs ffiTracked=9). Two divergent baselines -> mergeOrReload ran twice per change = full-document duplication path. Fixed by keying both via trackingKey(VirtualFile.path); convergence test + AGENTS.md invariant. GATED on live verify: install signed 0.2.138, restart, confirm no recurring duplication (corky.md). Exact setText payload uncaptured (JB debug logging was OFF). Relates to #ipcfullprompt-recur2, #adoc-queue-ipc-buffer-divergence, #prompt-duplicated-while-typing.
- [/] [#jb-load-tmux-window-verify] [recommended] Live-verify the new JB Load Tmux Window action (plugin 0.2.136): install the signed zip, confirm it autostarts/loads the tmux pane for a focused doc whose pane is not up (e.g. corky.md). If the autostart sync genuinely fails to spawn the pane in that case (vs. by-design passive open), capture the route/sync logs as a separate autostart bug.
- [/] [#jb-stale-busy-idle-footer] Three JB Run Agent Doc / Clear Session Context reports queue-instead-of-dispatch or block Clear while the live pane footer shows a Claude Code idle composer: monsterrodholders.md (actor gen 233, pane %19), equityfundingsource.md (gen 35, pane %3), tsift.md (pane %21, footer 22.4K (11%) ctrl+p commands). The tsift.md case is the strongest stale-busy candidate — Clear was blocked though the footer indicates idle. Fix: route.rs / session_actor_cmd.rs busy projection should treat the Claude Code idle-composer footer (ctrl+p commands / context % used) as idle direct evidence to repair a stale busy actor/lease projection before queuing or blocking, mirroring the existing OpenCode context % used idle-composer rule. Relates to #run-agent-doc-stale-busy-replay and #snrun (plan tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md).
- [/] [#adoc-queue-ipc-buffer-divergence] Root cause #2 IMPLEMENTED (submodule c483080a; pointer 4703f9788): live editor-buffer convergence after queue halt/drain. New core fn component::converge_queue_auto + agent_doc_converge_queue_auto FFI (C-int ABI for stable JNA); ipc_socket::send_queue_convergence carries queue_auto opening-tag state + queue_active frontmatter; preflight run_queue_maintenance pushes convergence through an active IPC listener after each halt/drain disk write (best-effort, non-fatal). JB plugin 0.2.137 parses queue_auto and applies via NativePatching.convergeQueueAuto in Document-API + VFS paths. Deterministic SimWorld repro queue_maintenance_converges_live_ipc_buffer_on_item_modified_halt + Kotlin ParsePatchJsonQueueAutoTest; make check green; both binaries built+installed (symbol verified in release cdylib); spec 07-orchestration-commands updated. GATED on live verification: install signed plugin 0.2.137, restart the route-owned agent-doc process to load the new binary, then confirm a real IDE-owned session no longer re-diverges (no recurring agent:queue diff / stuck_captured_cycle / inactive_queue_residue) after a consume->halt cycle without a raw git checkout. Plan: tasks/agent-doc/plan-queue-ipc-drift.md.
- [/] [#cancel-orphans-preflight-cycle] Canceling a run then invoking Run Agent Doc fails closed because the explicit cancel does not abandon the orphaned preflight_started cycle (route waits for staleness). Distinct from the stale-busy actor/lease items: this is per-document cycle_state left open by cancel. Plan: tasks/agent-doc/plan-cancel-orphans-preflight-cycle.md (recommend Option A — JB cancel emits a signal that abandons the no-capture preflight_started cycle). Reported 2026-05-29.
- [/] [#exchange-prompt-dropped-on-merge] User-authored exchange prompt (observed: "go") silently dropped during an IPC content_ours merge while the IDE held ipc.sock — never reaches snapshot/HEAD, assistant never responds, next preflight reports no_changes. Same drift family as #adoc-queue-ipc-buffer-divergence / #duplicate-response-after-commit but the victim is user input. Repro 2026-05-29 on src/boost-client/tasks/monsterrodholders.md (dropped the "go" authorizing #gfwconnect; assistant replied "nothing changed" for two cycles). Plan: tasks/agent-doc/plan-exchange-prompt-dropped-on-merge.md (recommend Option A — union user prompt lines / fail closed + add dropped_prompt_guard to session-check).
- [/] [#duplicate-response-after-commit] Late-IPC reposition signal re-inserts already-committed response body into working tree, wrapped in duplicate boundary markers. Symptom: next preflight interrupts with "direct response patchback without agent-doc cycle"; agent-doc write --commit returns "empty response — nothing to write"; only manual Edit-based dedup recovers. Repro: tasks/dreamable/separation.md cycle after commit 231eafc83 on 2026-05-28. Plan: tasks/agent-doc/plan-duplicate-response-after-commit.md (recommend Option A — both detect over-application in session-check and make IPC reposition idempotent).
- [/] [#prompt-duplicated-while-typing] [/] [#prompt-duplicated-while-typing] User prompt duplicated in the live editor while typing. PARTIAL FIX 1 (v0.34.0, submodule 2a6f60e5): prefix-agnostic append dedup (strip_user_prompt_prefix) for the ❯-prefix mismatch shape. PARTIAL FIX 2 (this cycle): L1 buffer-snapshot typing race — component::append_patch_already_present now recognizes when a synthesized patch carries an EARLIER keystroke-snapshot of the live region (per-line prefix alignment via patch_is_typing_snapshot_of) and no-ops instead of re-appending the divergent tail. Confirmed live in tasks/resume.md 2026-05-29: half-typed '- [#s93y]: Add to' snapshot vs live '...Add to resume' broke the old contains() substring check and re-appended the entire tail (close marker + Queue/Backlog/Done duplicated). Boundary 70bccf9b stayed intact and no new boundary was minted, ruling out the reposition path. Tests: append_patch_dedupes_midblock_typing_snapshot, append_patch_typing_snapshot_does_not_collapse_distinct_lines, append_with_boundary_does_not_duplicate_typing_snapshot. Both binaries built+installed; cdylib lib-installed for JB hot-reload. GATED on live re-verify (type a prompt into resume.md during an in-flight cycle). RESIDUAL: the exact editor-side full-tail concatenation downstream of the dedup miss was not statically traced — the dedup no-op short-circuits the append entirely so it should not surface, but confirm live. Plan: tasks/agent-doc/plan-prompt-duplicated-while-typing.md.
- [/] [#queue-auto-no-continue] Queue auto trigger does not continue after finalize session-check warning about pending shadow items. Steps to reproduce: (1) Session document with queue_active=true, queue_trigger=auto, and multiple queued items. (2) finalize --stream completes cycle but session-check issues pending shadow warning for items outside live agent:backlog. (3) Queue consumes head item but auto trigger does not fire for next item. (4) agent-doc write --commit with --done reaps the item, queue reports 9 remaining but auto continues stop. Expected: queue auto should continue to next item after successful closeout. Actual: queue stops after first item. Session: tasks/software/tsift.md. agent-doc version: 0.34.0. The pending shadow warnings about items appearing both inside and outside agent:backlog may be blocking the auto-continue path.
- [/] [#snrun] [recommended] Implement tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md: make JB Run Agent Doc distinguish rescued-stash accepted-only proof misses, active-turn busy collisions, and stale starting projections; add route/plugin regressions plus live replay evidence.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#misfire-recovery-snapshot] Pre-auto-run recovery snapshot/tag (mirroring `compact`'s pre-mutation tag) so a destructive auto-run misfire is recoverable without git/sidecar archaeology. Proposed fix #4 of `#backlog-auto-marker-misfire` (now IMPLEMENTED for fixes #1–#3). Parked: only worth doing if a new auto-run path can mutate the backlog in place — currently the misfire is inert (queue-scoped) and surfaced (misplaced_component_attr warning). Plan: tasks/agent-doc/plan-backlog-auto-marker-misfire.md.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
