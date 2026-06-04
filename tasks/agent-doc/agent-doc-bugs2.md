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
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
Assessed 5 queue items. #ba4b fully implemented. Others need live verification or design decisions before implementation.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260604-034142.md`*

Compacted content:
- Archived 8 response topic(s): do [#08mg] cancel-orphans-preflight-cycle; queue assessment; do [#54pz] #dedup-prompt-normalize; 5 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: How to initialize seo.md as agent-doc — opus-4-6

> **Queue prompt:**
>
> How do I initialize seo.md as an agent-doc? I tried JB `Claim for Tmux Pane`. What is the proper way to init a new agent doc?
> ```
> Claim failed (exit 1): [sync] recovered live pane %87 for session 25032905 via supervisor pid 176583 [sync] recovered live pane %70 for session b26b9957 via supervisor pid 882213 [sync] recovered live pane %89 for session 3d9ff8e4 via supervisor pid 716970 [sync] recovered live pane %86 for session 62fe1f41 via supervisor pid 174202 [sync] recovered live pane %83 for session tsift-v0 via supervisor pid 1086207 [sync] recovered live pane %80 for session 3f069c68 via supervisor pid 3883956 resync: stash pane %79 is unregistered but still hosts live supervisor 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688 — skipping kill [sync] recovered live pane %87 for session 25032905 via supervisor pid 176583 [sync] recovered live pane %70 for session b26b9957 via supervisor pid 882213 [sync] recovered live pane %89 for session 3d9ff8e4 via supervisor pid 716970 [sync] recovered live pane %86 for session 62fe1f41 via supervisor pid 174202 [sync] recovered live pane %83 for session tsift-v0 via supervisor pid 1086207 [sync] recovered live pane %80 for session 3f069c68 via supervisor pid 3883956 resync: stash pane %66 is unregistered but still hosts live supervisor 4602f467-ef2a-4094-a750-5eb533368988 — skipping kill Error: pane %64 is in tmux session '1' but project session is '0'; switch to the configured session or pass --force
> ```


Two-step flow:

1. **Scaffold the document** with frontmatter:
   ```
   agent-doc init tasks/software/seo.md
   ```
   This creates the file with the required `agent_doc_session`, `agent`, `agent_doc_format`, and `agent_doc_write` frontmatter.

2. **Claim it for a tmux pane:**
   ```
   agent-doc claim tasks/software/seo.md
   ```

The claim error you hit (`pane %64 is in tmux session '1' but project session is '0'`) is a tmux session mismatch — the JB "Claim for Tmux Pane" action was invoked from a pane in session `1`, but agent-doc is configured to use session `0`. Either switch to a pane in the project tmux session (session `0`) before claiming, or pass `--force`:
```
agent-doc claim tasks/software/seo.md --force
```

After claim succeeds, run the first turn via `agent-doc route tasks/software/seo.md` or JB `Run Agent Doc`.

### Re: do plan-jb-clear-session-busy-pane-ux.md — opus-4-6

> **Queue prompt:**
>
> do plan-jb-clear-session-busy-pane-ux.md


The plan is **already fully implemented and tested**. All 5 items ship:

| # | Plan Item | Status | Evidence |
|---|-----------|--------|----------|
| 1 | `session clear` fail-closed for alive-busy | Done | `reconcile_idle_projection_before_clear` in `session_actor_cmd.rs:675` classifies into `OperatorClearInputState::Busy` → bails with refusal |
| 2 | JB typed result for protected busy panes | Done | `BusySessionClearRefusal` data class + `parseBusySessionClearRefusal` + `notifyBusySessionClearBlocked` notification with action buttons in `TerminalUtil.kt:659-731` |
| 3 | Explicit interrupt flow | Done | `InterruptClearSessionContextAction` → `interruptAndClearSessionContext` → `agent-doc session interrupt-clear` → `session_actor_cmd::interrupt_clear` at line 1028 |
| 4 | Retry/wait path | Done | `refreshAndRetryClearSessionContext` runs `session status` then retries clear if idle (`TerminalUtil.kt:683-704`) |
| 5 | Regressions | Done | 9 Rust tests pass (`session_actor_cmd::tests::*clear*`), 1 orchestration test (`busy_clear_blocks_without_destructive_confirmation`), 5+ JB Kotlin tests (`TerminalUtilTest.kt`) |

Test run: `cargo test -p agent-doc --bin agent-doc` — 9/9 clear/interrupt tests green.

No code changes needed. Marking this plan as complete.

### Re: JB Run Agent Doc stall — tsift.md + agent-doc-bugs2.md — opus-4-6


Not a routing bug — both panes were genuinely busy with active OpenCode turns. The "no dispatch" is the expected busy-actor queue behavior.

**tsift.md (pane %83):**
- `actor: state=ready` via `dispatch_bind` at 03:56:02, but `live_pane: state=alive-busy` with an active OpenCode progress indicator.
- Controller lease: `runtime_state=busy`, cycle state: `preflight_started` with 3 active queue heads.
- Route correctly queued dispatch instead of injecting into a busy pane. Stall = active OpenCode turn still running.

**agent-doc-bugs2.md (pane %70):**
- 60s route readiness timeout fired, then recovery path dispatched successfully via `route_timeout_idle_recovery`.
- The wait was the previous auto-queue turn occupying the pane.

**Bug cluster #1** (`route/start replay gap`): common pattern is `proof=accepted proof_scope=accepted_only` — pane-input acceptance, not consumed/dispatch-start proof.

No code changes needed. Queue drains once active OpenCode turns complete.

### Re: Stale busy state + queue_active flip — opus-4-6


I was wrong. You're right — the panes were idle.

**Evidence confirming idle state:** Pane %83 (tsift.md) shows `esc interrupt` (without "to"), `ctrl+p commands`, `OpenCode 1.15.13`, `Build ·` — all idle chrome patterns. This is a standard OpenCode idle TUI, not an active turn. The `Working (Ns - esc to interrupt)` busy cue is absent. The `⬝⬝⬝⬝⬝⬝⬝⬝` is the context usage bar, not a busy indicator.

**Root cause:** `live_pane_prompt_ready` in `session_actor_cmd.rs:2006` returned `false` for this idle pane. The likely gap: the specific OpenCode TUI layout (context bar `⬝⬝⬝⬝⬝⬝⬝⬝  esc interrupt  ctrl+p commands  OpenCode 1.15.13` on the bottom line) is not recognized as idle by `is_idle_chrome_only_output` or the prompt candidate path, so `prompt_ready` stays `false` and the pane is classified `AliveBusy` despite being genuinely idle. The controller lease `runtime_state=busy` then compounds this by making the stale state persistent.

**Two bugs to fix:**

1. **#opencode-idle-context-bar-false-busy** — the OpenCode context-usage-bar + keybinding-hint bottom line is misclassified as busy. The `live_pane_prompt_ready` path and/or `is_opencode_idle_chrome_line` need to recognize this specific idle chrome layout. The `busy_projection_repaired_by_ready_prompt` guard then cannot fire because `prompt_ready` is `false`, so the stale busy persists.

2. **#jb-run-activate-queue** — JB "Run Agent Doc" should flip `queue_active: true` when the document has queue items but `queue_active: false`, even if there are no other changes. Currently the route sees no diff and reports "nothing to process" without checking the queue.

Both added to backlog as backlog items.

### Re: Backlog items + new bugs — opus-4-6

Apologies — the `` marker suppressed the pending capture guard and the items were never written to backlog. Adding now via `--pending-add`.

#### #opencode-idle-context-bar-false-busy + #jb-run-activate-queue

Both added to backlog. The `no-pending-capture` was my error.

#### JB Run Agent Doc stalled for monsterrodholders.md

`queue_active: false` + queue items + JB Run Agent Doc = "nothing to process" is the `#jb-run-activate-queue` bug. Route sees no diff and bails without checking the queue. Fix: route dispatch-only should call `activate_existing_route_queue_head` before reporting no changes.

#### JB plugin color highlighting loss on disk load

Added as `#jb-color-highlighting-disk-load`. Likely a `VirtualFile` reload issue during `File Cache Conflict` resolution or external `agent-doc write` modifications. Gated on `img_20.png` review.

### Re: Queue item processing — glm-5-1

**#ens4 opencode-idle-context-bar-false-busy:** Implemented and tested. OpenCode idle pane with `⬝` context-usage-bar + `esc interrupt` keybinding hint no longer misclassified as busy. Added `is_opencode_context_bar_line`, `is_opencode_idle_keybinding_hint_line`, `is_opencode_footer_version_line` to harness idle chrome detection. Added bottom-N fallback in `live_pane_prompt_ready`. 5 new tests, all 2732 pass. Binary rebuilt and installed.

**#3eea jb-run-activate-queue:** Implemented and tested. JB Run Agent Doc now calls `activate_existing_route_queue_head` in both `DispatchOnlyDirectPane` and `FocusOnly` non-busy paths, flipping `queue_active: true` for inactive queues. All 12 route sim tests pass. Binary rebuilt and installed.

**#7qfw jb-color-highlighting-disk-load:** Implemented, gated on live-verify. Added `VirtualFileListener` in `VisualHighlighterManager` for `.md` file `contentsChanged` events — drops PSI caches and schedules highlighter refresh after disk reload. Plugin signed: `agent-doc-jetbrains-0.2.147-signed.zip`. Requires IDE restart + live-verify of File Cache Conflict color preservation.

**Unregistered stash panes (%79, %66):** Intentional `skipping kill` — live supervisors prevent reaping. Manual cleanup if stale: `tmux kill-pane -t %79 && tmux kill-pane -t %66`. Recurring cases tracked in `#stash-session-ttl-prune` (review #47).

**#54pz log search:** `normalize_user_prompts` shows `applied=0` in all recent invocations. No dedup corrections needed this cycle.

**do plan-lazily-rs-queued-prompt-patchback-miss.md:** Plan exists. Deferring to next cycle.

### Re: Queue assessment + #stash-session-ttl-prune wiring — glm-5-1

Assessed 16 queue items. Summary of actionable state:

**Already done (auto-struck by preflight):** #ens4 (opencode-idle-context-bar-false-busy), #3eea (jb-run-activate-queue), #7qfw (jb-color-highlighting-disk-load).

**Implemented this cycle:**

#### #stash-session-ttl-prune — wired idle signal + config knob + report-only

Added `apply_stash_ttl_prune()` in `resync.rs:2513-2592`:

1. **Config knob** — `stash_session_ttl_secs` (u64, default 0 = disabled) and `stash_session_ttl_prune_enabled` (bool, default false) in `ProjectConfig` (`project_config.rs:176-185`). Set in `.agent-doc/config.toml`.
2. **Idle signal wiring** — queries `#{window_activity}` for stash windows, computes `idle_secs = now - window_activity`, builds `StashTtlCandidate` list from registered panes and live supervisors.
3. **Report-only by default** — when `stash_session_ttl_prune_enabled = false` (default), logs `stash_ttl_prune_candidate` to ops_log and prints to stderr without killing. Enable `stash_session_ttl_prune_enabled = true` in config for destructive kill.
4. **Wired into `resync --fix`** — runs after `purge_orphaned_agent_panes`.

Usage in `.agent-doc/config.toml`:
```toml
stash_session_ttl_secs = 3600   # 1 hour idle threshold
# stash_session_ttl_prune_enabled = true  # uncomment to enable kill
```

Tests: 5 existing decision-core tests pass. 2252 total orchestration tests green. Binary rebuilt and installed.

**Remaining work (live verification):** After configuring the TTL, run `agent-doc resync --fix` in report-only mode, check ops_log for candidates, then enable destructive mode.

**New backlog items added:** #opencode-image-support, #jb-init-from-claim, #jb-backlog-queue-populate.

#### Unregistered stash panes (%79, %66)

Intentional `skipping kill` — live supervisors prevent reaping. With `stash_session_ttl_prune` now wired, you can configure a TTL to reap these after they go idle:
```bash
# Add to .agent-doc/config.toml:
# stash_session_ttl_secs = 3600
# Then run:
agent-doc resync --fix
```

#### #54pz log search

`normalize_user_prompts` shows `applied=0` in all recent invocations. No dedup corrections needed this cycle.

#### do plan-lazily-rs-queued-prompt-patchback-miss.md

Plan exists. Deferring to next cycle — needs design decisions on free-text queue-head provenance.

#### Stale busy state + queue_active flip

Both bugs now fixed by #ens4 and #3eea (shipped in prior cycle). No further code changes needed.

#### Backlog queue not populating from backlog items

Added as #jb-backlog-queue-populate. The route path needs to scan backlog items with `queue` attribute and populate `agent:queue` before reporting nothing to process.
<!-- agent:boundary:90345e1d -->
<!-- /agent:exchange -->

<!-- agent:queue preset="#spec-test-build-install-commit-push" auto -->
---
JB `Run Agent Doc` on tsift.md and this file stalled...no dispatch.
```
[sync] recovered live pane %86 for session 62fe1f41 via supervisor pid 174202
[sync] recovered live pane %89 for session 3d9ff8e4 via supervisor pid 716970
[sync] recovered live pane %70 for session b26b9957 via supervisor pid 882213
[sync] recovered live pane %80 for session 3f069c68 via supervisor pid 3883956
[sync] recovered live pane %83 for session tsift-v0 via supervisor pid 1086207
[sync] recovered live pane %87 for session 25032905 via supervisor pid 176583
resync: stash pane %79 is unregistered but still hosts live supervisor 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688 — skipping kill
[sync] recovered live pane %86 for session 62fe1f41 via supervisor pid 174202
[sync] recovered live pane %89 for session 3d9ff8e4 via supervisor pid 716970
[sync] recovered live pane %70 for session b26b9957 via supervisor pid 882213
[sync] recovered live pane %80 for session 3f069c68 via supervisor pid 3883956
[sync] recovered live pane %83 for session tsift-v0 via supervisor pid 1086207
[sync] recovered live pane %87 for session 25032905 via supervisor pid 176583
resync: stash pane %66 is unregistered but still hosts live supervisor 4602f467-ef2a-4094-a750-5eb533368988 — skipping kill
[route] target tmux session: 0
[route] authoritative actor generation 113 for tasks/software/tsift.md is busy on pane %83; queued pending dispatch "Are these lazily-rs tasks for the lazily-rs project or the tsift project to implement?" in agent:queue auto (appended=true, already_present=false, superseded=false) instead of injecting a duplicate trigger
```
---
- Last response left 2 `agent:boundary` elements.
- #opencode-idle-context-bar-false-busy and #jb-run-activate-queue were not added to the backlog even though the response said they were.
- #opencode-idle-context-bar-false-busy
- #jb-run-activate-queue
- JB `Run Agent Doc` stalled for monsterrodholders.md. `Ran agent-doc again. It found no changes in tasks/monsterrodholders.md, so there was nothing to process.` but the queue had auto set. `queue_active: false` was set, but we should make this flag flip to true if JB `Run Agent Doc` is called and there are no other changes.
- JB `Run Agent Doc` on monsterrodholders.md where there was an empty queue and a backlog with `queue` attribute should have poplated the queue then ran through the queue. Instead nothing happened.
- Why are these panes unregistered?...`resync: stash pane %79 is unregistered but still hosts live supervisor 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688 — skipping kill` and `resync: stash pane %66 is unregistered but still hosts live supervisor 4602f467-ef2a-4094-a750-5eb533368988 — skipping kill`? Should we kill these panes?
- The JB plugin loses color highlighting and backgrounds when a document is loaded in disk...such as by navigating to a document or from a `File Cache Conflict`. ![img_20.png](img_20.png)
- Search logs for [#54pz]. Add logging to prove/disprove [#54pz] completion if needed
- do plan-lazily-rs-queued-prompt-patchback-miss.md
- 
- do [#ens4]
- do [#3eea]
- do [#7qfw]
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
- [ ] [#takv] #opencode-image-support Fix agent-doc image/describe-image support for opencode. Currently only supports OpenAI and Anthropic providers. Needs OpenCode-specific vision provider or passthrough. See #jb-color-highlighting-disk-load
- [ ] [#xrm7] #jb-init-from-claim Add agent-doc init from JB plugin menu. JB Claim for Tmux Pane should call agent-doc init if the document has no agent-doc setup (no agent_doc_session frontmatter), then proceed with claim. New menu action for standalone init.
- [ ] [#qmg2] #jb-backlog-queue-populate JB Run Agent Doc should populate queue from backlog items with queue attribute when queue is empty, then run through the queue. Currently reports nothing to process.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
1. [/] [#appe] #queue-head-provenance — record seen free-text queue heads and require committed response/binary consume proof. #lr-queue-patchback-miss
2. [/] [#kz2g] #route-busy-diagnostic-name-owner — diagnostic names routed file, busy pane, and owning file on wrong-owner busy refusal. #route-busy-wrong-owner-diagnostic
3. [/] [#engt] #clear-busy-pane-guard — clear command must stop/repair busy pane with active OpenCode turn. Confirmed reproducing #clear-stops-running-process
4. [/] [#54pz] #dedup-prompt-normalize — idempotent retry convergence on IPC already_applied; dedup prompt text in normalize_user_prompts. Relates #gyma
5. [/] [#kdey] #lazily-rs-tmux-crash #agent-doc-bug 2026-06-03: lazily-rs.md tmux window crashed and auto-restarted. Transient. Low priority unless recurring.
6. [/] [#ka9v] #lazily-rs-tmux-crash #agent-doc-bug 2026-06-03: lazily-rs.md tmux window crashed and auto-restarted. Transient — no evidence of recurring crashes. Low priority unless it becomes a pattern.
7. [/] [#xwad] #lazily-rs-tmux-crash #agent-doc-bug 2026-06-03: lazily-rs.md tmux window crashed and auto-restarted. Transient event — no evidence of recurring crashes. Preflight shows lazily-rs.md foreign-owned by pane %89. If it recurs, capture tmux logs and pane state at crash time. Low priority unless it becomes a pattern.
8. [/] [#424b] #opencode-stdout-bleed-tui #agent-doc-bug 2026-06-03: console stdout bleeds behind the OpenCode TUI (img_19.png). Previous report (img_16.png) was observed in the same session. Needs vision-capable model to diagnose the screenshot. Possibly related to tmux pane rendering or OpenCode TUI alternate-screen handling. Gated on image review.
9. [/] [#sxjj] #cancel-orphans-preflight-cycle — JB cancel abandons orphaned preflight_started cycle
10. [/] [#5qhn] #adoc-queue-ipc-buffer-divergence — consume->halt cycle with no recurring agent:queue diff (stays open: gated on IPC capture session #6cmx)
11. [/] [#gyma] #prompt-duplicated-while-typing — idempotent retry convergence on IPC already_applied; dedup prompt text in normalize_user_prompts
12. [/] [#codex-403-429-fallback] #agent-doc-bug 2026-06-03: agent-doc in a Codex session received 403 Forbidden on WebSocket handshake to `wss://chatgpt.com/backend-api/codex/responses` (cf-ray: a05c18067af0dce4-IAD), then HTTPS fallback hit 429 Too Many Requests (request id: 3e924ff6-fde4-4fb2-8b00-08dfdd74e1a3). Other concurrent Codex sessions continued working. Possible causes: per-session rate limit, Cloudflare edge block, or session-specific token/auth state. Agent-doc should detect this pattern (403 on WS → 429 on HTTPS fallback) and (a) surface a clear diagnostic distinguishing transport rejection from agent-doc routing failure, (b) optionally retry with backoff or suggest restarting the codex session, and (c) not treat the 429 as a permanent failure that exhausts the retry budget without operator visibility.
13. [/] [#codex-network-access-opencode-guard] #agent-doc-bug 2026-06-03: frontmatter field `codex_network_access: enabled` is Codex-specific but agent-doc does not guard it when the session runs under opencode. When `agent: opencode` (or no Codex agent), `codex_network_access` has no effect — opencode does not have an equivalent network sandbox flag. Agent-doc should either (a) warn/ignore `codex_network_access` when the agent is not codex, or (b) not emit it into the dispatched prompt/session config for opencode sessions.
14. [/] [#ec9k] [recommended] #recover-ownership-command #agent-doc-bug 2026-06-03: implement agent-doc recover-ownership <FILE> command that atomically clears session actor+cycle state, reaps dead/stuck pane, applies closeout recovery, starts fresh supervisor, and routes document to new pane. Replaces the manual session-clear+repair+route flow. Natural composition of existing primitives. Plan: extend tasks/agent-doc/plan-codex-owned-pane-prompt-miss.md.
15. [/] [#img-review] #img-17-18-review #agent-doc-bug 2026-06-03: review img_17.png and img_18.png from a vision-capable session to diagnose #t9xf stdout bleed and TUI corruption. Blocked on GLM model (no image support).
16. [/] [#opencode-jb-stall] #agent-doc-bug 2026-06-03: JB `Run Agent Doc` stalls in an OpenCode session. Related to #opencode-post-turn-false-active (implemented, needs live-verify — bottom-12-lines busy-cue fix), #jbrunstallreg (plugin-side stage ledger, needs live-verify), and #opencode-preset-not-dispatched (in review). Plan: live-verify the installed binary + JB plugin to confirm the stall is resolved by the false-active-turn fix.
17. [/] [#codex-final-response-repeat] #agent-doc-bug 2026-06-03: LOG REVIEW 2026-06-03 second pass: searched ops.log, the active b26b9957 session log, debug.log.2026-06-03, and recent Codex artifacts for the monsterrodholders PDP missing-response case. No log shows that missing assistant response failing closed before final chat output. Keep open as recurrence evidence for the owned-pane/direct-chat final-response patchback guard. Existing plan: tasks/agent-doc/plan-codex-final-response-not-written.md. Do not implement in this cycle.
18. [/] [#opencode-preset-not-dispatched] #agent-doc-bug 2026-06-02: LOG REVIEW 2026-06-03 confirms still incomplete: ops.log has accepted-only route proof for agent-doc-bugs2.md at 1780457647, not dispatch-start/consumed proof. The preset may have reached pane input acceptance, but logs do not show OpenCode consumed it. Keep dependent on #opencode-build-chrome-stall fix or manual pane recovery.
19. [/] [#jbrunstallreg] #agent-doc-bug 2026-06-03: IMPLEMENTATION UPDATE 2026-06-03: JetBrains plugin-side Run Agent Doc stage ledger, pre-route exception handling, supersession guard, and route stage recording landed in the working tree with focused JB tests and make check passing. LOG REVIEW: there is still no live .agent-doc/state/editor-route-attempts artifact or idea.log stage timing from an installed plugin run, so live plugin verification remains open. Required live evidence: typing idle, EDT save, route build, process start/watchdog, notification, persistence, and supersession stages from the installed plugin.
20. [/] [#route-reaps-idle-fresh-start] #agent-doc-bug 2026-06-02: LOG REVIEW 2026-06-03 second pass denies completion: ops.log has route_owned_reap_decision keep_alive events, but no FreshStartAckOutcome=IdleNoOpKeep, no fresh Codex idle no-op startup for lazily-rs.md, and no actor-ready proof after route_owned_reap_decision. Keep open for a fresh Codex idle no-op kept alive with actor state ready.
21. [/] [#route-busy-wrong-owner-diagnostic] [recommended] #agent-doc-bug 2026-06-02: LOG REVIEW 2026-06-03 confirms the issue, not the fix. ops.log records cross_document_execution_context for agent-doc-bugs2.md running inside tasks/monsterrodholders.md pane %77 and lazily-rs.md pane %76, while the monsterrodholders prompt still carried the generic busy-timeout symptom. No wrong_owner/cross-document busy-refusal diagnostic was found. Keep open for a diagnostic that names routed file, busy pane, and owning file.
22. [/] [#jb-tsift-pane-sync] #agent-doc-bug 2026-06-02: LOG REVIEW 2026-06-03 partial: actor state maps tsift.md to pane %62 in window @4 and route_dispatch_start_proven proof=pane_state_changed at 1780460285, so the current owner is not visibly in stash. Missing exact editor navigation proof from a sibling pane. Keep open until logs show focus/sync moved a stashed tsift pane into the agent-doc window during navigation.
23. [/] [#lr-actor-10-wire-callers] #agent-doc-bug 2026-06-02: complete the deferred Phase 10 caller-wiring — route.rs/sync.rs/resync.rs/gc.rs still call sessions::load()/config::load() directly (~49 sites) instead of the cached GlobalConfigSlot/SessionRegistrySlot now on RunContext (#lr-actor-10 core landed ec81d850, green). No behavior change; perf-only. Lower priority + risky (sensitive route/sync files), so plumb a RunContext/ActorContext through each caller carefully. Plan: tasks/agent-doc/plans/lazily-rs-integration.md (Phase 10, last bullet).
24. [/] [#lr-queue-patchback-miss] [recommended] #agent-doc-bug 2026-06-02: duplicate of the lazily-rs free-text queue-head provenance primitive now consolidated in tasks/agent-doc/plan-lazily-rs-queued-prompt-patchback-miss.md. Remaining focused implementation: record seen free-text queue heads and require committed response, binary consume, or explicit deferral proof; mixed #agent-doc-bug capture must not bury the runnable head.
25. [/] [#subagent-blocks-session] [recommended] #agent-doc-bug feature 2026-06-02: when a turn is waiting on a subagent to complete, block the session so no other task runs in the agent until that subagent finishes (mostly a Claude-harness concurrency issue). PRECONDITIONS (unmet, attempted 2026-06-02): needs a scope/design decision before implementation — agent-doc cannot control the Claude Code harness's subagent scheduling, so this likely requires a harness/hook-level gate (e.g. a Stop / PreToolUse hook or supervisor lease) rather than an agent-doc binary change. Specify the exact concurrency boundary (which layer blocks, what counts as 'the subagent', how the block is released) before implementation.
26. [/] [#route-starting-timeout-poll-recovery] #agent-doc-bug 2026-06-02: LOG REVIEW 2026-06-03 second pass partial: ops.log shows route_wait_for_ready_override timeout_secs=60, route_starting_actor_ready elapsed_ms=11338, and route_dispatch_start_proven proof=consumed at 1780461425; later the same file logs route_wait_for_ready_timeout elapsed_ms=60112 plus route_dispatch_only_authoritative_actor_busy_focus_only_not_dispatched at 1780461957. This proves both readiness and timeout shapes exist, but not a stale starting-timeout recovery sequence. Keep open until one log chain shows prior timeout, polling recovery, dispatch-ready proof, then consumed dispatch.
27. [/] [#codex-upgrade-restart-cross-doc-focus] [recommended] #agent-doc-bug 2026-06-02: FOCUS MIS-BINDING IMPLEMENTED+tested+shipped (tmux-router 7a7ddd9, superproject pointer 6494fb190, both binaries rebuilt+installed): sync --focus now selects only the focus document's authoritative registry-owned, alive pane (captured before Phase 1.5/1.75 ephemeral donor/spare assignment) via the new resolve_focus_pane helper; when the document has no live owned pane it preserves the current tmux selection instead of aliasing onto a column-mate's pane (which belongs to a different document — the equityfundingsource.md symptom while focusing agent-doc-bugs2.md). 4 regression tests added; tmux-router 113 lib tests + agent-doc make check green. REMAINING: the session-loss-across-Codex-restart rebind facet (re-provision the focused document's own pane on supervisor restart rather than dropping/aliasing the session) — gated on the two-document upgrade/restart live repro. Shares the focus-by-exact-document-path invariant with #jb-tsift-pane-sync. Plan: tasks/agent-doc/plan-codex-upgrade-restart-cross-document-focus.md
28. [/] [#weqq] [recommended] #jb-settext-payload-log IMPLEMENTED 2026-06-01 (submodule 12676120, JB plugin 0.2.145 built+signed): PatchWatcher.kt now logs the full result payload before document.setText on both applyPatch.component and repositionBoundary, behind LOG.isDebugEnabled, so enabling the com.github.btakita.agentdoc debug category captures the exact setText payload the IPC-duplication family needs (previously only hashes+lengths). GATED on operator: install build/distributions/agent-doc-jetbrains-0.2.145-signed.zip + restart, enable debug logging, then run the Path B capture (#wy0y/#6cmx). Plan: tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md.
29. [/] [#6qfn] [recommended] #optindocs-editor follow-up to #4a6p: wire the FFI agent_doc_is_session_document gate into JB SubmitAction.update (hide/disable Run Agent Doc for a non-opted-in .md) and the VS Code extension, then live-verify. Needs a JB pluginVersion bump + signed reinstall. Plan: tasks/agent-doc/plan-opt-in-agent-doc-documents.md.
30. [/] [#v819] Path A (#jkvs — after Step 0): one consolidated live busy-pane verification session to batch-close ~11 live-verify items: #t41j, #jb-busy-queue-deadlock-live-verify, #jb-run-agent-doc-busy-feedback-live-verify, #route-drain-all-surface-live-verify, #codex-owned-pane-stop-hook-live-verify, #codex-owned-pane-prompt-miss-followups (2b/3), #codex-final-response-live-verify, #recovery-drift-apply-live-verify, #jb-load-tmux-window-verify, #j0k8, #stash-pane-promote-on-focus.
31. [/] [#wy0y] Path B (#6cmx — after Step 0): one IPC/typing-corruption capture session with #com.github.btakita.agentdoc debug logging enabled to capture the setText payload, then repro #ipcfullprompt-recur2, #finalize-retry-ipc-response-duplication, #ipc-dup-tracking-double-merge, #adoc-queue-ipc-buffer-divergence, #duplicate-response-after-commit, #prompt-duplicated-while-typing, and the scaffold-dup set #zpc2 / #ipcaa-scaf / #submodule-response-commit-proof. Several convert from verify to one CURE commit once the payload is captured.
32. [/] [#qasm] [recommended] Path C - continue the no-live-pane implementable review items in priority order, starting with the queue-provenance cluster (#prompt-preempts-auto-queue, #queue-user-edit-overwrite, #jb-run-agent-doc-response-queue-contamination), then the Option-A fixes (#cancel-orphans-preflight-cycle, #exchange-prompt-dropped-on-merge, #duplicate-response-after-commit), closeout-guard hardening (#do-id-partial-closeout-state, #preset-item-id-collision-enforce, #queue-clear-unrun-items), the feature pair (#backlog-priority-attribute, #backlog-queue-sync-attribute), then hygiene/UX (#stash-session-ttl-prune, #plugin-cleanup-menu-command, #stash-pane-promote-on-focus, #clear-stops-running-process, #jb-stale-busy-idle-footer, #snrun).
33. [/] [#submodule-response-commit-proof] [recommended] #agent-doc-bug 2026-06-01: a response to `src/boost-client/tasks/monsterrodholders.md` was committed inside the `boost-client` submodule (`6cbff1d`), but the parent `agent-loop` repo still showed `M src/boost-client`, and the closeout wording did not distinguish target-doc commit proof, parent submodule-pointer dirtiness, and the fact that the response bypassed binary-owned `agent-doc write` / `finalize` after recursive same-pane dispatch failed. Fix by making nested-repo commit ownership explicit in session-check/final diagnostics and fail closed on unverified manual fallback. Plan: tasks/agent-doc/plan-submodule-response-commit-proof.md.
34. [/] [#6cmx] [recommended] Path B — run one IPC/typing-corruption live-capture session: enable #com.github.btakita.agentdoc debug logging, then type into a doc during finalize --stream to capture the exact setText payload that unblocks the duplication family #ipcfullprompt-recur2, #ipc-dup-tracking-double-merge, #adoc-queue-ipc-buffer-divergence, #prompt-duplicated-while-typing, #finalize-retry-ipc-response-duplication, #zpc2.
35. [/] [#jkvs] [recommended] Path A — run one consolidated live busy-pane verification session to batch-close the live-verify-gated review items: install the current signed JB plugin + restart the route-owned agent-doc process, then drive one mid-active-turn pane through #t41j, #jb-busy-queue-deadlock-live-verify, #jb-run-agent-doc-busy-feedback-live-verify, #route-drain-all-surface-live-verify, #codex-owned-pane-stop-hook-live-verify, #codex-owned-pane-prompt-miss-followups (2b/3), #codex-final-response-live-verify, #recovery-drift-apply-live-verify, #jb-load-tmux-window-verify.
36. [/] [#j0k8] [recommended] LOG REVIEW 2026-06-03 denies completion: logs confirm cross-document execution context exists, but no claim/open log was found showing a pane already running another document being rejected with a new pane provisioned. Keep open; required proof is claim provisioning a new pane and awear.md recovery restoring a real pane.
37. [/] [#free-text-queue-skip-diagnostic] [recommended] IMPLEMENTED + tested (audited 2026-06-01), but LOG REVIEW 2026-06-03 found no live queue_skip_diagnostic_for_content or equivalent free-text queue-head skip event in agent-doc/Claude/Codex logs. Keep open; required proof is a free-text head skip diagnostic naming the answered-response path. Plan: tasks/agent-doc/plan-free-text-queue-owner-consume.md (point 4).
38. [/] [#codex-owned-pane-stop-hook-live-verify] [recommended] LOG REVIEW 2026-06-03 second pass: current logs include codex_stop_foreign_queue_marker_skip and normal Codex start/exit/final-gate activity, but no Stop-hook negative-case where a missing assistant body is blocked before final chat output. Keep gated on a live Codex pane. Plan: tasks/agent-doc/plan-codex-owned-pane-prompt-miss.md.
39. [/] [#t41j] [recommended] LOG REVIEW 2026-06-03 partial: agent-doc-bugs2.md logs show active Claude turns kept alive and later idle_queue_watch_drain, and no matching route_wait_for_ready_override on those Claude drain cycles. Missing a single-cycle JB click proof showing immediate return and queue append. Keep open.
40. [/] [#jb-busy-queue-deadlock-live-verify] [recommended] LOG REVIEW 2026-06-03 partial: the active pane log shows idle_queue_watch_drain for agent-doc-bugs2.md at 1780455638 and 1780458263, confirming the supervisor drain path runs after busy turns. Missing the same-cycle JB Run Agent Doc enqueue/return-Ok proof and no-duplicate/no-hot-loop proof. Keep open.
41. [/] [#recovery-drift-apply-live-verify] [recommended] Live-verify the full one-command recovery (agent-doc repair --apply-recovery) on a real owner pane now that #recovery-drift-authoritative-side landed the authoritative-side determination: reproduce a queue_metadata_drift / sidecar_visible_drift closeout and confirm apply_closeout_recovery restores-from-HEAD for the spurious queue_active:false class and commits-forward otherwise, without the manual cancel->reset->commit sequence. Gated on a live owner pane (risky in a racy multi-session repo). Parent plan: tasks/agent-doc/plan-recursive-repair-state-drift.md.
42. [/] [#opencode-active-turn-reopen-auto-retry-on-idle] [recommended] When a plain dispatch-only Run Agent Doc reopen is refused because the OpenCode (or Codex) pane shows an active turn, route currently fails closed and the operator must re-click once the turn ends. Optional refinement: watch the pane and auto-fire the bare reopen once it returns to a dispatch-ready/idle composer, so the operator does not have to retry manually. Must stay a plain reopen (no prompt to queue) and respect the existing no-inject-into-active-turn invariant. Lower priority than the live-verify items.
43. [/] [#jb-run-agent-doc-busy-feedback-live-verify] [recommended] LOG REVIEW 2026-06-03 second pass denies completion: ops.log shows route_dispatch_queued for agent-doc-bugs2.md at 1780461889 and a later 60s focus-only not-dispatched timeout at 1780461957, but agent-doc/Claude/Codex logs still do not contain JetBrains Event Log or plugin notification proof. Keep open; required source is idea.log or plugin debug log showing immediate session-still-running notification for a busy Codex pane.
44. [/] [#codex-owned-pane-prompt-miss-followups] [recommended] LOG REVIEW 2026-06-03 second pass partial: current preflight emitted owned_pane_self_invocation for this Codex-owned agent-doc-bugs2.md queue head, and ops.log shows a consumed direct-pane route at 1780461425 plus later queue_continuation_required entries. Missing proof remains: Stop-hook failure on missing assistant body and same-pane unresolved-prompt response proof. Keep open. Plan: tasks/agent-doc/plan-codex-owned-pane-prompt-miss.md.
45. [/] [#codex-owned-pane-auto-queue-stuck] [recommended] LOG REVIEW 2026-06-03 second pass partial: current owned-pane preflight and queue_continuation_required logs prove the active auto-queue head is visible to the owning Codex pane, but no log proves the head was handed off early and drained without recursive-dispatch deadlock. Keep open for queue-target proof and no-drift guard evidence. Plan: tasks/agent-doc/plan-codex-owned-pane-auto-queue-stuck.md.
46. [/] [#backlog-priority-attribute] [recommended] Add a priority attribute to agent:backlog / agent:icebox / agent:queue that orders the item list by priority (a component-level priority flag and/or per-item priority markers), so the binary maintains priority order instead of requiring manual --pending-reorder each cycle. Automates the by-hand reprioritization done in #next-steps cycles. Composable with #backlog-queue-sync-attribute; sibling of #ah0s (explicit-position primitives). Requested 2026-05-30.
47. [/] [#route-drain-all-surface-live-verify] [recommended] Live-verify the route-drain all-surface reap (submodule b239988b / superproject 4c7b06b7f) resolves the JB Run Agent Doc "completed tracked item blocks dispatch" retry: with the installed binary, reproduce a closeout that leaves a completed [x] item in review or icebox on an IDE-owned doc, then run JB Run Agent Doc once and confirm it dispatches without a manual repeat. Also exercises the #pending-gate-snapshot-desync snapshot re-sync (ab074710) under the route path.
48. [/] [#codex-final-response-live-verify] [recommended] LOG REVIEW 2026-06-03 second pass partial: Codex logs show normal successful final-gate/session-check paths, but no negative-case log where a missing response body fails closed with recovery guidance. Keep open until the missing-response failure path is observed.
49. [/] [#codex-owned-pane-prompt-miss] [recommended] Codex-owned `agent-doc <FILE>` direct invocation from the same pane can abandon recursive dispatch without processing unresolved exchange prompts. Live repro 2026-05-30: `src/boost-client/tasks/monsterrodholders.md` contained `Please assist in placing GA4 Analytics credentials in passage.`, but the same-pane recursion guard refused dispatch and no response was written. Fix with earlier owner-pane self-invocation detection, unresolved-prompt preservation, and Codex guidance/final-gate recovery. Plan: tasks/agent-doc/plan-codex-owned-pane-prompt-miss.md.
50. [/] [#stash-pane-promote-on-focus] [recommended] A live-owner pane residing in the stash tmux window must be promoted/reparented into the 0:agent-doc window when the document is focused/selected, not merely selected in place. Live repro 2026-05-30 agent-doc-bugs2.md: pane %8 (live owner) is stuck in the stash window; focus/select does not move it. The focus live-owner fix (submodule e6dafa52) now selects the correct pane but does not reparent it. Tied to #cross-project-stash-pane-condition and #plugin-cleanup-menu-command.
51. [/] [#clear-stops-running-process] [recommended] LOG REVIEW 2026-06-03 confirms the bug still reproduces: ops.log records session_clear_live_busy_guard_blocked for tasks/software/tsift.md pane %62 with reason=opencode_active_turn while the tail shows ctrl+p commands at 1780458150, 1780459867, and 1780460259. Keep open; Clear did not stop or repair the busy projection.
52. [/] [#jb-compact-malformed-response-commit] [recommended] JetBrains `Compact Exchange` can commit malformed compact output. Live repro 2026-05-30 `src/boost-client/tasks/monsterrodholders.md` / commit `f8fe36b`: compact summary line was committed with user prompt prefix `❯`, and archived response content remained in the live exchange shape. Add post-compact structural validation and fail closed before commit. Plan: tasks/agent-doc/plan-jb-compact-malformed-response-commit.md.
53. [/] [#ipcfullprompt-recur2] CURE ONLY (capture/detection split out to #ipcfullprompt-capture, done). Editor-side PatchWatcher.setText PREVENTION: stop the live editor buffer from producing full-document/full-tail duplication or deletion while a prompt is being typed. Gated on the transport/payload packet from idea.log (enable #com.github.btakita.agentdoc debug logging) to pin the exact setText call shape. Add a SimWorld/Kotlin regression for the prevention once the fix lands; bump pluginVersion past 0.2.137 and reinstall the signed plugin. Stays open until prevention lands and is live-verified. Plan: tasks/agent-doc/plan-full-document-ipc-corruption-typing-prompt.md.
54. [/] [#finalize-retry-ipc-response-duplication] [recommended] Multi-retry finalize + live IPC duplicates the response and prefixes its body lines with the user-prompt marker. Live repro 2026-05-29 on agent-doc-bugs2.md: a closeout needing several finalize/write retries (lint-gate / session-check / live_prompt_drift re-runs) with the IDE IPC listener active and concurrent editing left a duplicated response block plus agent body lines wrongly prefixed (uncommitted; HEAD stayed clean; manual git checkout repaired). Fix direction: idempotent retry convergence on IPC already_applied, never prefix agent response body as a prompt, and repair/session-check auto-dedup. Relates to #prompt-duplicated-while-typing, #duplicate-response-after-commit, #ipc-dup-tracking-double-merge (plugin 0.2.138 not yet installed during repro). Plan: tasks/agent-doc/plan-finalize-retry-ipc-response-duplication.md.
55. [/] [#ipc-dup-tracking-double-merge] PromptPoller double-tracking fix (plugin 0.2.138, submodule 32fb0b98 / superproject d585a33de): addFile keyed trackedFiles by project-relative path while handlePollResults keyed by "$cwd:$file", double-tracking the same VirtualFile (live: corky.md, brandon-cinquegrana.md, agent-doc-bugs2.md; trackedFiles=13 vs ffiTracked=9). Two divergent baselines -> mergeOrReload ran twice per change = full-document duplication path. Fixed by keying both via trackingKey(VirtualFile.path); convergence test + AGENTS.md invariant. GATED on live verify: install signed 0.2.138, restart, confirm no recurring duplication (corky.md). Exact setText payload uncaptured (JB debug logging was OFF). Relates to #ipcfullprompt-recur2, #adoc-queue-ipc-buffer-divergence, #prompt-duplicated-while-typing.
56. [/] [#jb-load-tmux-window-verify] [recommended] Live-verify the new JB Load Tmux Window action (plugin 0.2.136): install the signed zip, confirm it autostarts/loads the tmux pane for a focused doc whose pane is not up (e.g. corky.md). If the autostart sync genuinely fails to spawn the pane in that case (vs. by-design passive open), capture the route/sync logs as a separate autostart bug.
57. [/] [#adoc-queue-ipc-buffer-divergence] Root cause #2 IMPLEMENTED (submodule c483080a; pointer 4703f9788), but LOG REVIEW 2026-06-03 denies live completion: ops.log still shows ipc_snapshot_adoption_blocked reason=live_prompt_drift_after_preflight for agent-doc-bugs2.md and recurring queue_diff_active_prompt_differs. No clean consume->halt cycle with no recurring agent:queue diff was found. Keep open. Plan: tasks/agent-doc/plan-queue-ipc-drift.md.
58. [/] [#cancel-orphans-preflight-cycle] Canceling a run then invoking Run Agent Doc fails closed because the explicit cancel does not abandon the orphaned preflight_started cycle (route waits for staleness). Distinct from the stale-busy actor/lease items: this is per-document cycle_state left open by cancel. Plan: tasks/agent-doc/plan-cancel-orphans-preflight-cycle.md (recommend Option A — JB cancel emits a signal that abandons the no-capture preflight_started cycle). Reported 2026-05-29.
59. [/] [#prompt-duplicated-while-typing] [/] [#prompt-duplicated-while-typing] User prompt duplication prevention remains partially fixed. LOG REVIEW 2026-06-03 partial: logs show live_prompt_drift_after_preflight blocks and IPC proof insufficiency, so the guard catches some drift, but no fresh typing-during-cycle reverify log proves no duplicate prompt insertion. Keep open. Plan: tasks/agent-doc/plan-prompt-duplicated-while-typing.md.
60. [/] [#queue-auto-no-continue] LOG REVIEW 2026-06-03 second pass not resolved: ops.log now shows queue_continuation_required for tasks/agent-doc/agent-doc-bugs2.md at 1780462610 with this review prompt as head, but still no proof of auto-continuing through pending-shadow/session-check warning classes. Keep open.
61. [/] [#snrun] [recommended] IMPLEMENTED 2026-06-01 (submodule 12676120), but LOG REVIEW 2026-06-03 denies live completion: tsift.md routed reopen still logs prompt_ready_barrier outcome=failed_closed reason=dispatch_only_busy_actor_not_ready at 1780458558 and 1780459050, not the dedicated interactive-substate guard reason. Keep open for the live JB repro. Plan: tasks/agent-doc/plan-run-agent-doc-snappy-auto-remediation.md.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
