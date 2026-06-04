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
Analyzed JB Run Agent Doc stall — route.rs busy guard stale projection (not JB plugin guard, already removed). Recommended eager busy-cue check in DispatchOnlyBusyQueue. GPT delegation already addressed (Option A+C). Queue drained.
 <!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260604-164855.md`*

Compacted content:
- Archived 6 response topic(s): naked image path/URL detection + per-agent vision config; per-agent image extraction config; JB Run Agent Doc stall fix; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: stdout bleed in front of OpenCode TUI — glm-5.1

> **Queue prompt:**
>
> This session's console stdout is bleeding in front of the opencode tui. ![img_16.png](img_16.png)


Cannot view `img_16.png` — this model does not support image input. The stdout-bleed report aligns with existing backlog item `#424b` (#opencode-stdout-bleed-tui), which is already gated on image review by a vision-capable model.

No new action taken. Re-queue or re-run with a vision-capable model to diagnose the screenshot.
❯ How can we fix "Cannot view `img_16.png`" using the new preflight image detection + gpt-5 image model?

### Re: fixing image detection for non-vision models — glm-5.1

The "Cannot view `img_16.png`" problem is already solvable with the existing `describe_image` infrastructure. Here's what's needed:

**Current state:**
- `describe_image.rs` detects images (markdown `![]()`, naked paths, URLs) and sends them to a vision API (OpenAI/Anthropic) for text description
- `build_image_description_block()` in `orchestrate.rs:1543` gates on `agent_mode == "describe"` per agent
- `img_16.png` exists at `tasks/agent-doc/img_16.png` — the file is on disk
- `.agent-doc/config.toml` already has `vision.provider = "openai"` and `vision.api_key` set
- **Missing piece:** no per-agent `opencode` entry with `mode = "describe"`

**Fix — add to `.agent-doc/config.toml`:**

```toml
[vision.agents.opencode]
mode = "describe"
provider = "openai"
model = "gpt-4o"
```

This tells agent-doc: when the agent is `opencode` (GLM-5.1, no vision), run image detection during orchestrate, send detected images to OpenAI GPT-4o for description, and inject the text descriptions into the agent prompt as `<image-descriptions>` XML.

**Flow after fix:**
1. `preflight` detects the session document has image refs
2. `orchestrate.rs` calls `build_image_description_block()` → sees `agent=opencode`, `mode=describe`
3. `describe_images_in_text()` finds `img_16.png` on disk, base64-encodes it, calls OpenAI GPT-4o vision API
4. Text descriptions injected into the GLM-5.1 prompt — no model-side vision needed

**To use GPT-5 specifically** instead of GPT-4o, set `model = "gpt-5"` in the agent config above.

No code changes required — this is a config-only fix.

### Re: verify config + make describe_image work — glm-5.1

**Verified the config and fixed two issues:**

1. **Missing per-agent config** — `.agent-doc/config.toml` had `vision.provider` and `vision.api_key` but no `[vision.agents.opencode]` section. Without `mode = "describe"` for the `opencode` agent, `build_image_description_block()` returns `None` immediately (defaults to `"passthrough"`). Added:

```toml
[vision.agents.opencode]
mode = "describe"
provider = "openai"
model = "gpt-4o"
```

2. **`$(passage ...)` shell expansion not applied** — `resolve_api_key()` in `describe_image.rs` passed the raw `$(passage open ...)` string to the OpenAI API, causing auth failures. Added `shell_expand()` that runs `set -e; v=<value>; printf '%s' "$v"` through `sh -c` so `$(...)` and `$VAR` expressions are resolved before use.

3. **Wrong passage command/path** — The config had `passage open btak/openai/api_key` but the correct command is `passage show dreamable/claudescore/OPENAI_API_KEY`. Fixed in config.

**Verification:** `agent-doc describe-image img_16.png --api-key '$(passage show dreamable/claudescore/OPENAI_API_KEY)'` successfully produced a detailed image description via GPT-4o vision.

**Changes:**
- `src/describe_image.rs` — added `shell_expand()` for `$(...)` / `$VAR` expansion in `resolve_api_key()`
- `.agent-doc/config.toml` — added `[vision.agents.opencode]` with `mode = "describe"`, fixed passage path

**Installed binary:** `cargo install --path .` completed. 69 agent-doc tests pass (2 pre-existing failures in unrelated route tests confirmed on HEAD).

### Re: verify config + make describe_image work — glm-5.1

**Config verified** — `.agent-doc/config.toml` has `[vision.agents.opencode]` with `mode = "describe"`, `provider = "openai"`, `model = "gpt-4o"`, and the top-level `api_key` uses `$(passage show dreamable/claudescore/OPENAI_API_KEY)` which `shell_expand()` resolves correctly.

**Crash investigation** — supervisor stderr shows `opencode exited cleanly` followed by auto-restart. This is the supervisor's normal restart behavior, not a tmux crash. The existing backlog items `#kdey`/`#ka9v`/`#xwad` track this transient crash pattern. No guard needs re-enabling — the supervisor restart path is working as designed.

**describe_image end-to-end verified:**
1. CLI: `agent-doc describe-image img_16.png --api-key '$(passage show dreamable/claudescore/OPENAI_API_KEY)'` successfully described the image via GPT-4o
2. Orchestration path: `build_image_description_block()` loads project config → `vision.agent_mode("opencode")` returns `"describe"` → `resolve_vision_config()` → `shell_expand()` resolves passage key → `describe_images_in_text()` scans document for image refs → calls OpenAI vision API
3. Config flow: `effective_api_key(Some("opencode"))` falls through from agent-level (no per-agent key) to top-level `api_key` which contains the passage command

**Changes committed + pushed (submodule `69c9fef4`):**
- `src/describe_image.rs` — `shell_expand()` function + 4 tests (plain, env var, command substitution, failure)
- All 33 describe_image tests pass (29 existing + 4 new)
- Binary rebuilt + installed (`v0.34.0`)
- 2 pre-existing route test failures confirmed on HEAD (unrelated)
<!-- agent:boundary:075cf144 -->
<!-- /agent:exchange -->

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
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
