---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: opencode
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
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Queue contamination cleaned. Session committed.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260603-154519.md`*

Compacted content:
- Archived 8 response topic(s): queue; queue; queue drain; 5 more
-  Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: stdout bleed behind OpenCode TUI — opus-4-6

Queue item reports that JB `Clear Session Context` causes agent-doc stdout to bleed behind the OpenCode TUI. This provides additional reproduction evidence for **#t9xf** (`#opencode-log-bleed`).

The `--review-edit` binary flag failed to locate item `#t9xf`, suggesting the review-edit path may not resolve custom-bracketed IDs (e.g., `[#t9xf]`). Filed as a secondary observation.

Updated #t9xf text: *agent-doc stderr/stdout logs bleed over the OpenCode UI. Confirmed via JB `Clear Session Context` trigger — agent-doc output renders behind the opencode TUI (see img_17.png). Needs reproduction to confirm source and fix.*

### Re: stdout bleed — secondary: review-edit ID resolution — opus-4-6

> **Queue prompt:**
>
> JB `Clear Session Context` results with opencode having stdout bleed behind the opencode TUI. ![img_17.png](img_17.png)


Confirmed: response patch from prior finalize attempt landed in the exchange. Re-running to clear the lint-gate block.

The lint findings are false positives caused by component-name substrings (`agent:backlog`, `agent:queue`) appearing inside review item description text, not actual structural mismatches.

### Re: JB Run Agent Doc / Compact Exchange malforms OpenCode TUI — opus-4-6

> **Queue prompt:**
>
> JB `Run Agent Doc` after JB `Clear Session Context` and JB `Compact Exchange` malforms opencode tui. ![img_18.png](img_18.png)


Cannot view img_18.png (model does not support image input).

This extends the #t9xf bleed/corruption pattern: after JB `Clear Session Context` and `Compact Exchange`, a subsequent JB `Run Agent Doc` malforms the OpenCode TUI. The sequence (Clear → Compact → Run Agent Doc) suggests the TUI state is corrupted by agent-doc's stdout/stderr during the JB operations, and the subsequent dispatch renders into a malformed terminal state.

This may be a compound trigger: the IPC/write activity from `Clear Session Context` and `Compact Exchange` both emit output that the OpenCode TUI cannot contain, leaving the terminal in a dirty state that `Run Agent Doc` then inherits.

Updated #t9xf scope: the bleed is not limited to `Clear Session Context` — JB `Compact Exchange` also triggers it, and the accumulated corruption affects subsequent `Run Agent Doc` dispatches.
Can GLM sessions create a GPT subagent to view images? What are #next-steps?

### Re: GLM subagent for images / next-steps — opus-4-6

> Can GLM sessions create a GPT subagent to view images?

No. OpenCode's Task tool launches subagents but they run the same model (GLM). There is no cross-model subagent bridge. To view img_17/img_18, use `agent-doc claim` from a Claude Code or Codex session with a vision-capable model.

> What are #next-steps?

**Backlog priority order** (top items first):

1. **#t9xf** `#opencode-log-bleed` — two new evidence items (img_17, img_18). Needs vision-capable session to view screenshots, then root-cause the stdout bleed source. Highest-value diagnostic item.

2. **#queue-not-clearing-opencode** — if the OpenCode auto-loop gap is the root cause of #t9xf accumulation, closing this closes a class of drain bugs.

3. **#4h81** `#opencode-codex-capacity-proof` — capacity proof ran in wrong harness.

4. **#7fmb** `#opencode-prompt-navigation` — OpenCode TUI navigation issue; may be upstream.

5. **#opencode-jb-stall** — depends on live-verify of installed binary + JB plugin.

6. **#codex-upgrade-restart-cross-doc-focus** — implemented + shipped, remaining restart-rebind facet gated on live repro.

7. **#route-busy-wrong-owner-diagnostic** — recommended; logs confirm cross-document execution but missing diagnostic naming.

8. **#lr-queue-patchback-miss** — recommended; queue-head provenance consolidation.

9. **#submodule-response-commit-proof** — recommended; nested-repo commit ownership.

10. **#finalize-retry-ipc-response-duplication** — recommended; multi-retry + IPC duplication.

Top of backlog: **#t9xf**.
I ran `npm i -g @ai-sdk/openai-compatible`. How can we ensure the GLM model supports vision?

### Re: GLM vision support — opus-4-6

> I ran `npm i -g @ai-sdk/openai-compatible`. How can we ensure the GLM model supports vision?

The `@ai-sdk/openai-compatible` SDK package does not control vision capability — that depends on the model. The current model (`glm-5.1`) does not support images.

The vision-capable GLM model is **`zai-coding-plan/glm-5v-turbo`** (the `v` = vision). Available in your OpenCode provider. To switch:

1. Open OpenCode TUI → settings/model selector, or
2. Update `opencode.jsonc` to set the model to `zai-coding-plan/glm-5v-turbo`, or
3. In a new OpenCode session, select `glm-5v-turbo` as the model.

Once on `glm-5v-turbo`, invoke `agent-doc claim tasks/agent-doc/agent-doc-bugs2.md` and view img_17/img_18 to diagnose #t9xf.

> do [#img-review]

Blocked — this model (`glm-5.1`) cannot view images. Gated #img-review pending vision-capable model switch.

### Re: GLM-5v-turbo subagent for images — opus-4-6

> Can we have opencode use a `glm-5v-turbo` subagent from the `glm-5.1` main session to read the images?

No. OpenCode subagents (explore, general) inherit the session's model. There is no per-agent model override in `.opencode/config.json` or the agent definitions. A `glm-5.1` session cannot spawn a `glm-5v-turbo` subagent.

**Options to view img_17/img_18:**

1. **Switch the OpenCode session model** — select `zai-coding-plan/glm-5v-turbo` from the TUI model picker, then claim this document.
2. **Use a different harness** — Claude Code or Codex with a vision model can `agent-doc claim` this session.
3. **External tool** — use a CLI image descriptor (e.g., `file`, `identify`, or an external API) to extract metadata, though this won't give visual analysis.

The `@ai-sdk/openai-compatible` package you installed is an SDK for building AI apps — it doesn't affect OpenCode's model routing or enable cross-model subagents.

#img-review remains gated on a vision-capable model.
<!-- agent:boundary:b55f6ecd -->
<!-- /agent:exchange -->

<!-- agent:queue -->
<!-- /agent:queue -->
###
<!--
- This session's console stdout is bleeding in front of the opencode tui. ![img_16.png](img_16.png)
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue -->
- [/] [#4h81] #opencode-codex-capacity-proof #agent-doc-bug 2026-06-03: Codex Capacity Proof incorrectly runs in OpenCode harness. See monsterrodholders.md session + agent-doc logs.
- [/] [#t9xf] #opencode-log-bleed #agent-doc-bug 2026-06-03: agent-doc stderr logs bleed over the OpenCode UI. Needs reproduction to confirm source.
- [/] [#7fmb] #opencode-prompt-navigation #agent-doc-bug 2026-06-03: OpenCode TUI prompt list cannot be navigated with arrow keys or Tab key. Enter works. Likely OpenCode TUI issue.
- [/] [#0ada] #queue-not-clearing-opencode #agent-doc-bug 2026-06-03: Queue did not clear when all items complete in an opencode session. OpenCode has no auto-loop; binary drain path may not fire.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#img-review] #img-17-18-review #agent-doc-bug 2026-06-03: review img_17.png and img_18.png from a vision-capable session to diagnose #t9xf stdout bleed and TUI corruption. Blocked on GLM model (no image support).
- [/] [#opencode-jb-stall] #agent-doc-bug 2026-06-03: JB `Run Agent Doc` stalls in an OpenCode session. Related to #opencode-post-turn-false-active (implemented, needs live-verify — bottom-12-lines busy-cue fix), #jbrunstallreg (plugin-side stage ledger, needs live-verify), and #opencode-preset-not-dispatched (in review). Plan: live-verify the installed binary + JB plugin to confirm the stall is resolved by the false-active-turn fix.
- [/] [#codex-final-response-repeat] #agent-doc-bug 2026-06-03: LOG REVIEW 2026-06-03 second pass: searched ops.log, the active b26b9957 session log, debug.log.2026-06-03, and recent Codex artifacts for the monsterrodholders PDP missing-response case. No log shows that missing assistant response failing closed before final chat output. Keep open as recurrence evidence for the owned-pane/direct-chat final-response patchback guard. Existing plan: tasks/agent-doc/plan-codex-final-response-not-written.md. Do not implement in this cycle.
- [/] [#opencode-preset-not-dispatched] #agent-doc-bug 2026-06-02: LOG REVIEW 2026-06-03 confirms still incomplete: ops.log has accepted-only route proof for agent-doc-bugs2.md at 1780457647, not dispatch-start/consumed proof. The preset may have reached pane input acceptance, but logs do not show OpenCode consumed it. Keep dependent on #opencode-build-chrome-stall fix or manual pane recovery.
- [/] [#jbrunstallreg] #agent-doc-bug 2026-06-03: IMPLEMENTATION UPDATE 2026-06-03: JetBrains plugin-side Run Agent Doc stage ledger, pre-route exception handling, supersession guard, and route stage recording landed in the working tree with focused JB tests and make check passing. LOG REVIEW: there is still no live .agent-doc/state/editor-route-attempts artifact or idea.log stage timing from an installed plugin run, so live plugin verification remains open. Required live evidence: typing idle, EDT save, route build, process start/watchdog, notification, persistence, and supersession stages from the installed plugin.
- [/] [#route-reaps-idle-fresh-start] #agent-doc-bug 2026-06-02: LOG REVIEW 2026-06-03 second pass denies completion: ops.log has route_owned_reap_decision keep_alive events, but no FreshStartAckOutcome=IdleNoOpKeep, no fresh Codex idle no-op startup for lazily-rs.md, and no actor-ready proof after route_owned_reap_decision. Keep open for a fresh Codex idle no-op kept alive with actor state ready.
- [/] [#route-busy-wrong-owner-diagnostic] [recommended] #agent-doc-bug 2026-06-02: LOG REVIEW 2026-06-03 confirms the issue, not the fix. ops.log records cross_document_execution_context for agent-doc-bugs2.md running inside tasks/monsterrodholders.md pane %77 and lazily-rs.md pane %76, while the monsterrodholders prompt still carried the generic busy-timeout symptom. No wrong_owner/cross-document busy-refusal diagnostic was found. Keep open for a diagnostic that names routed file, busy pane, and owning file.
- [/] [#jb-tsift-pane-sync] #agent-doc-bug 2026-06-02: LOG REVIEW 2026-06-03 partial: actor state maps tsift.md to pane %62 in window @4 and route_dispatch_start_proven proof=pane_state_changed at 1780460285, so the current owner is not visibly in stash. Missing exact editor navigation proof from a sibling pane. Keep open until logs show focus/sync moved a stashed tsift pane into the agent-doc window during navigation.
- [/] [#lr-actor-10-wire-callers] #agent-doc-bug 2026-06-02: complete the deferred Phase 10 caller-wiring — route.rs/sync.rs/resync.rs/gc.rs still call sessions::load()/config::load() directly (~49 sites) instead of the cached GlobalConfigSlot/SessionRegistrySlot now on RunContext (#lr-actor-10 core landed ec81d850, green). No behavior change; perf-only. Lower priority + risky (sensitive route/sync files), so plumb a RunContext/ActorContext through each caller carefully. Plan: tasks/agent-doc/plans/lazily-rs-integration.md (Phase 10, last bullet).
- [/] [#lr-queue-patchback-miss] [recommended] #agent-doc-bug 2026-06-02: duplicate of the lazily-rs free-text queue-head provenance primitive now consolidated in tasks/agent-doc/plan-lazily-rs-queued-prompt-patchback-miss.md. Remaining focused implementation: record seen free-text queue heads and require committed response, binary consume, or explicit deferral proof; mixed #agent-doc-bug capture must not bury the runnable head.
- [/] [#subagent-blocks-session] [recommended] #agent-doc-bug feature 2026-06-02: when a turn is waiting on a subagent to complete, block the session so no other task runs in the agent until that subagent finishes (mostly a Claude-harness concurrency issue). PRECONDITIONS (unmet, attempted 2026-06-02): needs a scope/design decision before implementation — agent-doc cannot control the Claude Code harness's subagent scheduling, so this likely requires a harness/hook-level gate (e.g. a Stop / PreToolUse hook or supervisor lease) rather than an agent-doc binary change. Specify the exact concurrency boundary (which layer blocks, what counts as 'the subagent', how the block is released) before implementation.
- [/] [#route-starting-timeout-poll-recovery] #agent-doc-bug 2026-06-02: LOG REVIEW 2026-06-03 second pass partial: ops.log shows route_wait_for_ready_override timeout_secs=60, route_starting_actor_ready elapsed_ms=11338, and route_dispatch_start_proven proof=consumed at 1780461425; later the same file logs route_wait_for_ready_timeout elapsed_ms=60112 plus route_dispatch_only_authoritative_actor_busy_focus_only_not_dispatched at 1780461957. This proves both readiness and timeout shapes exist, but not a stale starting-timeout recovery sequence. Keep open until one log chain shows prior timeout, polling recovery, dispatch-ready proof, then consumed dispatch.
- [/] [#codex-upgrade-restart-cross-doc-focus] [recommended] #agent-doc-bug 2026-06-02: FOCUS MIS-BINDING IMPLEMENTED+tested+shipped (tmux-router 7a7ddd9, superproject pointer 6494fb190, both binaries rebuilt+installed): sync --focus now selects only the focus document's authoritative registry-owned, alive pane (captured before Phase 1.5/1.75 ephemeral donor/spare assignment) via the new resolve_focus_pane helper; when the document has no live owned pane it preserves the current tmux selection instead of aliasing onto a column-mate's pane (which belongs to a different document — the equityfundingsource.md symptom while focusing agent-doc-bugs2.md). 4 regression tests added; tmux-router 113 lib tests + agent-doc make check green. REMAINING: the session-loss-across-Codex-restart rebind facet (re-provision the focused document's own pane on supervisor restart rather than dropping/aliasing the session) — gated on the two-document upgrade/restart live repro. Shares the focus-by-exact-document-path invariant with #jb-tsift-pane-sync. Plan: tasks/agent-doc/plan-codex-upgrade-restart-cross-document-focus.md
- [/] [#weqq] [recommended] #jb-settext-payload-log IMPLEMENTED 2026-06-01 (submodule 12676120, JB plugin 0.2.145 built+signed): PatchWatcher.kt now logs the full result payload before document.setText on both applyPatch.component and repositionBoundary, behind LOG.isDebugEnabled, so enabling the com.github.btakita.agentdoc debug category captures the exact setText payload the IPC-duplication family needs (previously only hashes+lengths). GATED on operator: install build/distributions/agent-doc-jetbrains-0.2.145-signed.zip + restart, enable debug logging, then run the Path B capture (#wy0y/#6cmx). Plan: tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md.
- [/] [#6qfn] [recommended] #optindocs-editor follow-up to #4a6p: wire the FFI agent_doc_is_session_document gate into JB SubmitAction.update (hide/disable Run Agent Doc for a non-opted-in .md) and the VS Code extension, then live-verify. Needs a JB pluginVersion bump + signed reinstall. Plan: tasks/agent-doc/plan-opt-in-agent-doc-documents.md.
- [/] [#v819] Path A (#jkvs — after Step 0): one consolidated live busy-pane verification session to batch-close ~11 live-verify items: #t41j, #jb-busy-queue-deadlock-live-verify, #jb-run-agent-doc-busy-feedback-live-verify, #route-drain-all-surface-live-verify, #codex-owned-pane-stop-hook-live-verify, #codex-owned-pane-prompt-miss-followups (2b/3), #codex-final-response-live-verify, #recovery-drift-apply-live-verify, #jb-load-tmux-window-verify, #j0k8, #stash-pane-promote-on-focus.
- [/] [#wy0y] Path B (#6cmx — after Step 0): one IPC/typing-corruption capture session with #com.github.btakita.agentdoc debug logging enabled to capture the setText payload, then repro #ipcfullprompt-recur2, #finalize-retry-ipc-response-duplication, #ipc-dup-tracking-double-merge, #adoc-queue-ipc-buffer-divergence, #duplicate-response-after-commit, #prompt-duplicated-while-typing, and the scaffold-dup set #zpc2 / #ipcaa-scaf / #submodule-response-commit-proof. Several convert from verify to one CURE commit once the payload is captured.
- [/] [#qasm] [recommended] Path C - continue the no-live-pane implementable review items in priority order, starting with the queue-provenance cluster (#prompt-preempts-auto-queue, #queue-user-edit-overwrite, #jb-run-agent-doc-response-queue-contamination), then the Option-A fixes (#cancel-orphans-preflight-cycle, #exchange-prompt-dropped-on-merge, #duplicate-response-after-commit), closeout-guard hardening (#do-id-partial-closeout-state, #preset-item-id-collision-enforce, #queue-clear-unrun-items), the feature pair (#backlog-priority-attribute, #backlog-queue-sync-attribute), then hygiene/UX (#stash-session-ttl-prune, #plugin-cleanup-menu-command, #stash-pane-promote-on-focus, #clear-stops-running-process, #jb-stale-busy-idle-footer, #snrun).
- [/] [#submodule-response-commit-proof] [recommended] #agent-doc-bug 2026-06-01: a response to `src/boost-client/tasks/monsterrodholders.md` was committed inside the `boost-client` submodule (`6cbff1d`), but the parent `agent-loop` repo still showed `M src/boost-client`, and the closeout wording did not distinguish target-doc commit proof, parent submodule-pointer dirtiness, and the fact that the response bypassed binary-owned `agent-doc write` / `finalize` after recursive same-pane dispatch failed. Fix by making nested-repo commit ownership explicit in session-check/final diagnostics and fail closed on unverified manual fallback. Plan: tasks/agent-doc/plan-submodule-response-commit-proof.md.
- [/] [#6cmx] [recommended] Path B — run one IPC/typing-corruption live-capture session: enable #com.github.btakita.agentdoc debug logging, then type into a doc during finalize --stream to capture the exact setText payload that unblocks the duplication family #ipcfullprompt-recur2, #ipc-dup-tracking-double-merge, #adoc-queue-ipc-buffer-divergence, #prompt-duplicated-while-typing, #finalize-retry-ipc-response-duplication, #zpc2.
- [/] [#jkvs] [recommended] Path A — run one consolidated live busy-pane verification session to batch-close the live-verify-gated review items: install the current signed JB plugin + restart the route-owned agent-doc process, then drive one mid-active-turn pane through #t41j, #jb-busy-queue-deadlock-live-verify, #jb-run-agent-doc-busy-feedback-live-verify, #route-drain-all-surface-live-verify, #codex-owned-pane-stop-hook-live-verify, #codex-owned-pane-prompt-miss-followups (2b/3), #codex-final-response-live-verify, #recovery-drift-apply-live-verify, #jb-load-tmux-window-verify.
- [/] [#j0k8] [recommended] LOG REVIEW 2026-06-03 denies completion: logs confirm cross-document execution context exists, but no claim/open log was found showing a pane already running another document being rejected with a new pane provisioned. Keep open; required proof is claim provisioning a new pane and awear.md recovery restoring a real pane.
- [/] [#free-text-queue-skip-diagnostic] [recommended] IMPLEMENTED + tested (audited 2026-06-01), but LOG REVIEW 2026-06-03 found no live queue_skip_diagnostic_for_content or equivalent free-text queue-head skip event in agent-doc/Claude/Codex logs. Keep open; required proof is a free-text head skip diagnostic naming the answered-response path. Plan: tasks/agent-doc/plan-free-text-queue-owner-consume.md (point 4).
- [/] [#codex-owned-pane-stop-hook-live-verify] [recommended] LOG REVIEW 2026-06-03 second pass: current logs include codex_stop_foreign_queue_marker_skip and normal Codex start/exit/final-gate activity, but no Stop-hook negative-case where a missing assistant body is blocked before final chat output. Keep gated on a live Codex pane. Plan: tasks/agent-doc/plan-codex-owned-pane-prompt-miss.md.
- [/] [#t41j] [recommended] LOG REVIEW 2026-06-03 partial: agent-doc-bugs2.md logs show active Claude turns kept alive and later idle_queue_watch_drain, and no matching route_wait_for_ready_override on those Claude drain cycles. Missing a single-cycle JB click proof showing immediate return and queue append. Keep open.
- [/] [#jb-busy-queue-deadlock-live-verify] [recommended] LOG REVIEW 2026-06-03 partial: the active pane log shows idle_queue_watch_drain for agent-doc-bugs2.md at 1780455638 and 1780458263, confirming the supervisor drain path runs after busy turns. Missing the same-cycle JB Run Agent Doc enqueue/return-Ok proof and no-duplicate/no-hot-loop proof. Keep open.
- [/] [#recovery-drift-apply-live-verify] [recommended] Live-verify the full one-command recovery (agent-doc repair --apply-recovery) on a real owner pane now that #recovery-drift-authoritative-side landed the authoritative-side determination: reproduce a queue_metadata_drift / sidecar_visible_drift closeout and confirm apply_closeout_recovery restores-from-HEAD for the spurious queue_active:false class and commits-forward otherwise, without the manual cancel->reset->commit sequence. Gated on a live owner pane (risky in a racy multi-session repo). Parent plan: tasks/agent-doc/plan-recursive-repair-state-drift.md.
- [/] [#opencode-active-turn-reopen-auto-retry-on-idle] [recommended] When a plain dispatch-only Run Agent Doc reopen is refused because the OpenCode (or Codex) pane shows an active turn, route currently fails closed and the operator must re-click once the turn ends. Optional refinement: watch the pane and auto-fire the bare reopen once it returns to a dispatch-ready/idle composer, so the operator does not have to retry manually. Must stay a plain reopen (no prompt to queue) and respect the existing no-inject-into-active-turn invariant. Lower priority than the live-verify items.
- [/] [#jb-run-agent-doc-busy-feedback-live-verify] [recommended] LOG REVIEW 2026-06-03 second pass denies completion: ops.log shows route_dispatch_queued for agent-doc-bugs2.md at 1780461889 and a later 60s focus-only not-dispatched timeout at 1780461957, but agent-doc/Claude/Codex logs still do not contain JetBrains Event Log or plugin notification proof. Keep open; required source is idea.log or plugin debug log showing immediate session-still-running notification for a busy Codex pane.
- [/] [#codex-owned-pane-prompt-miss-followups] [recommended] LOG REVIEW 2026-06-03 second pass partial: current preflight emitted owned_pane_self_invocation for this Codex-owned agent-doc-bugs2.md queue head, and ops.log shows a consumed direct-pane route at 1780461425 plus later queue_continuation_required entries. Missing proof remains: Stop-hook failure on missing assistant body and same-pane unresolved-prompt response proof. Keep open. Plan: tasks/agent-doc/plan-codex-owned-pane-prompt-miss.md.
- [/] [#codex-owned-pane-auto-queue-stuck] [recommended] LOG REVIEW 2026-06-03 second pass partial: current owned-pane preflight and queue_continuation_required logs prove the active auto-queue head is visible to the owning Codex pane, but no log proves the head was handed off early and drained without recursive-dispatch deadlock. Keep open for queue-target proof and no-drift guard evidence. Plan: tasks/agent-doc/plan-codex-owned-pane-auto-queue-stuck.md.
- [/] [#backlog-priority-attribute] [recommended] Add a priority attribute to agent:backlog / agent:icebox / agent:queue that orders the item list by priority (a component-level priority flag and/or per-item priority markers), so the binary maintains priority order instead of requiring manual --pending-reorder each cycle. Automates the by-hand reprioritization done in #next-steps cycles. Composable with #backlog-queue-sync-attribute; sibling of #ah0s (explicit-position primitives). Requested 2026-05-30.
- [/] [#stash-session-ttl-prune] [recommended] Optional opt-in hygiene to cap stash-session accumulation: reap agent-doc stash panes whose session has been idle in the stash beyond a configurable TTL. Distinct from the existing orphan reap (resync.rs step 8), which only kills unregistered + no-supervisor + no-owned-file panes; live registered/supervisor-backed sessions are correctly never auto-killed, which is why ~9 agent-loop agent-doc panes accumulate in the shared session-0 stash. Must be opt-in (frontmatter/.agent-doc config) + live-verified before enabling (destructive). Relates to #cross-project-stash-pane-condition and #jb-busy-queue-ux-feedback.
- [/] [#route-drain-all-surface-live-verify] [recommended] Live-verify the route-drain all-surface reap (submodule b239988b / superproject 4c7b06b7f) resolves the JB Run Agent Doc "completed tracked item blocks dispatch" retry: with the installed binary, reproduce a closeout that leaves a completed [x] item in review or icebox on an IDE-owned doc, then run JB Run Agent Doc once and confirm it dispatches without a manual repeat. Also exercises the #pending-gate-snapshot-desync snapshot re-sync (ab074710) under the route path.
- [/] [#codex-final-response-live-verify] [recommended] LOG REVIEW 2026-06-03 second pass partial: Codex logs show normal successful final-gate/session-check paths, but no negative-case log where a missing response body fails closed with recovery guidance. Keep open until the missing-response failure path is observed.
- [/] [#codex-owned-pane-prompt-miss] [recommended] Codex-owned `agent-doc <FILE>` direct invocation from the same pane can abandon recursive dispatch without processing unresolved exchange prompts. Live repro 2026-05-30: `src/boost-client/tasks/monsterrodholders.md` contained `Please assist in placing GA4 Analytics credentials in passage.`, but the same-pane recursion guard refused dispatch and no response was written. Fix with earlier owner-pane self-invocation detection, unresolved-prompt preservation, and Codex guidance/final-gate recovery. Plan: tasks/agent-doc/plan-codex-owned-pane-prompt-miss.md.
- [/] [#stash-pane-promote-on-focus] [recommended] A live-owner pane residing in the stash tmux window must be promoted/reparented into the 0:agent-doc window when the document is focused/selected, not merely selected in place. Live repro 2026-05-30 agent-doc-bugs2.md: pane %8 (live owner) is stuck in the stash window; focus/select does not move it. The focus live-owner fix (submodule e6dafa52) now selects the correct pane but does not reparent it. Tied to #cross-project-stash-pane-condition and #plugin-cleanup-menu-command.
- [/] [#clear-stops-running-process] [recommended] LOG REVIEW 2026-06-03 confirms the bug still reproduces: ops.log records session_clear_live_busy_guard_blocked for tasks/software/tsift.md pane %62 with reason=opencode_active_turn while the tail shows ctrl+p commands at 1780458150, 1780459867, and 1780460259. Keep open; Clear did not stop or repair the busy projection.
- [/] [#jb-compact-malformed-response-commit] [recommended] JetBrains `Compact Exchange` can commit malformed compact output. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md` / commit `f8fe36b`: compact summary line was committed with user prompt prefix `❯`, and archived response content remained in the live exchange shape. Add post-compact structural validation and fail closed before commit. Plan: tasks/agent-doc/plan-jb-compact-malformed-response-commit.md.
- [/] [#ipcfullprompt-recur2] CURE ONLY (capture/detection split out to #ipcfullprompt-capture, done). Editor-side PatchWatcher.setText PREVENTION: stop the live editor buffer from producing full-document/full-tail duplication or deletion while a prompt is being typed. Gated on the transport/payload packet from idea.log (enable #com.github.btakita.agentdoc debug logging) to pin the exact setText call shape. Add a SimWorld/Kotlin regression for the prevention once the fix lands; bump pluginVersion past 0.2.137 and reinstall the signed plugin. Stays open until prevention lands and is live-verified. Plan: tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md.
- [/] [#finalize-retry-ipc-response-duplication] [recommended] Multi-retry finalize + live IPC duplicates the response and prefixes its body lines with the user-prompt marker. Live repro 2026-05-29 on agent-doc-bugs2.md: a closeout needing several finalize/write retries (lint-gate / session-check / live_prompt_drift re-runs) with the IDE IPC listener active and concurrent editing left a duplicated response block plus agent body lines wrongly prefixed (uncommitted; HEAD stayed clean; manual git checkout repaired). Fix direction: idempotent retry convergence on IPC already_applied, never prefix agent response body as a prompt, and repair/session-check auto-dedup. Relates to #prompt-duplicated-while-typing, #duplicate-response-after-commit, #ipc-dup-tracking-double-merge (plugin 0.2.138 not yet installed during repro). Plan: tasks/agent-doc/plan-finalize-retry-ipc-response-duplication.md.
- [/] [#ipc-dup-tracking-double-merge] PromptPoller double-tracking fix (plugin 0.2.138, submodule 32fb0b98 / superproject d585a33de): addFile keyed trackedFiles by project-relative path while handlePollResults keyed by "$cwd:$file", double-tracking the same VirtualFile (live: corky.md, brandon-cinquegrana.md, agent-doc-bugs2.md; trackedFiles=13 vs ffiTracked=9). Two divergent baselines -> mergeOrReload ran twice per change = full-document duplication path. Fixed by keying both via trackingKey(VirtualFile.path); convergence test + AGENTS.md invariant. GATED on live verify: install signed 0.2.138, restart, confirm no recurring duplication (corky.md). Exact setText payload uncaptured (JB debug logging was OFF). Relates to #ipcfullprompt-recur2, #adoc-queue-ipc-buffer-divergence, #prompt-duplicated-while-typing.
- [/] [#jb-load-tmux-window-verify] [recommended] Live-verify the new JB Load Tmux Window action (plugin 0.2.136): install the signed zip, confirm it autostarts/loads the tmux pane for a focused doc whose pane is not up (e.g. corky.md). If the autostart sync genuinely fails to spawn the pane in that case (vs. by-design passive open), capture the route/sync logs as a separate autostart bug.
- [/] [#jb-stale-busy-idle-footer] LOG REVIEW 2026-06-03 confirms issue: the tsift.md clear logs classify pane %62 as opencode_active_turn while the captured footer tail is idle-composer style ctrl+p commands with context percentages. This is log-backed evidence for stale-busy/idle-footer handling; keep open until route.rs/session_actor_cmd.rs treats that footer as idle repair evidence.
- [/] [#adoc-queue-ipc-buffer-divergence] Root cause #2 IMPLEMENTED (submodule c483080a; pointer 4703f9788), but LOG REVIEW 2026-06-03 denies live completion: ops.log still shows ipc_snapshot_adoption_blocked reason=live_prompt_drift_after_preflight for agent-doc-bugs2.md and recurring queue_diff_active_prompt_differs. No clean consume->halt cycle with no recurring agent:queue diff was found. Keep open. Plan: tasks/agent-doc/plan-queue-ipc-drift.md.
- [/] [#cancel-orphans-preflight-cycle] Canceling a run then invoking Run Agent Doc fails closed because the explicit cancel does not abandon the orphaned preflight_started cycle (route waits for staleness). Distinct from the stale-busy actor/lease items: this is per-document cycle_state left open by cancel. Plan: tasks/agent-doc/plan-cancel-orphans-preflight-cycle.md (recommend Option A — JB cancel emits a signal that abandons the no-capture preflight_started cycle). Reported 2026-05-29.
- [/] [#prompt-duplicated-while-typing] [/] [#prompt-duplicated-while-typing] User prompt duplication prevention remains partially fixed. LOG REVIEW 2026-06-03 partial: logs show live_prompt_drift_after_preflight blocks and IPC proof insufficiency, so the guard catches some drift, but no fresh typing-during-cycle reverify log proves no duplicate prompt insertion. Keep open. Plan: tasks/agent-doc/plan-prompt-duplicated-while-typing.md.
- [/] [#queue-auto-no-continue] LOG REVIEW 2026-06-03 second pass not resolved: ops.log now shows queue_continuation_required for tasks/agent-doc/agent-doc-bugs2.md at 1780462610 with this review prompt as head, but still no proof of auto-continuing through pending-shadow/session-check warning classes. Keep open.
- [/] [#snrun] [recommended] IMPLEMENTED 2026-06-01 (submodule 12676120), but LOG REVIEW 2026-06-03 denies live completion: tsift.md routed reopen still logs prompt_ready_barrier outcome=failed_closed reason=dispatch_only_busy_actor_not_ready at 1780458558 and 1780459050, not the dedicated interactive-substate guard reason. Keep open for the live JB repro. Plan: tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
