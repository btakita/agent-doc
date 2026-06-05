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
queue: go
---

## Status

 <!-- agent:status patch=replace -->
Analyzed JB Run Agent Doc stall — route.rs busy guard stale projection (not JB plugin guard, already removed). Recommended eager busy-cue check in DispatchOnlyBusyQueue. GPT delegation already addressed (Option A+C). Queue drained.
 <!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260604-214204.md`*

Compacted content:
- Archived 5 response topic(s): Frontmatter pipeline fields & reentrant phases; #finalize-owned-pane-response-patchback remaining work; #reentrant-phase5-integration & #finalize-owned-pane-response-patchback; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...

### Re: JB Run Agent Doc stall + #finalize-owned-pane-response-patchback — glm-5.1

Two fixes implemented, tested, and installed:

#### 1. JB `Run Agent Doc` stalls after 1 turn (#opencode-jb-stall)

**Root cause:** When `busy_dispatch_only_should_wait_for_ready()` skipped the 60s wait because a queue fallback existed (`prompt_context.is_some()` from JB), neither the ready-prompt recovery nor the timeout-idle recovery ran. The project controller projected the actor as `Busy` even though the opencode pane was idle (no busy cue in the bottom 12 lines). Every subsequent JB Run Agent Doc queued into `agent:queue auto` instead of dispatching, and the queue never drained.

**Fix:** Added an **eager busy-cue check** in `route_via_authoritative_actor()` (route.rs:4197) — when dispatch-only, actor is Busy, wait was skipped, and the live pane shows no busy cue, promote the stale Busy projection to Ready and dispatch immediately.

```
route_eager_busy_cue_recovery file=... pane=... harness=opencode busy_cue=false
```

#### 2. #finalize-owned-pane-response-patchback recovery adoption

**Plan step 5 implemented:** When the codex_final_gate detects an abandoned recursive-direct-invocation cycle with no captured response, but a response heading (`### Re:`) is already visible in `agent:exchange` (manually patched in), the gate adopts it idempotently instead of blocking. New `exchange_tail_has_response_heading()` helper. Original block (exit 2) preserved when no response heading exists.

**Files:** `route.rs` (eager busy-cue recovery), `session_check.rs` (patchback adoption + helper + regression test).

**Verification:** `make check` 2256 passed, 2 pre-existing failures unrelated. Binary rebuilt and installed.

### Re: snapshot resync — glm-5.1

> **Queue prompt:**
>
> do [#finalize-owned-pane-response-patchback]


Snapshot resync after repair commit.

### Re: JB stall + queue items — glm-5.1

JB stall fixed (eager busy-cue check, route.rs). Frontmatter step/run_id not yet implemented. Recommend queue: start/stop (Option B). Queue driving exchange prompts is follow-up. Review management: split actionable items + annotations.

### Re: queue semantic framework + harness parity — opus-4-8

Took ownership (`agent: claude`). Evaluated the semantic framework you proposed. Recommendation: **collapse all queue activation onto a single tri-state frontmatter key `queue:`**, and retire both `auto` (marker attribute) and `queue_active` (boolean) into deprecated input aliases.

**Recommended model**

| frontmatter | meaning | replaces |
|-------------|---------|----------|
| `queue: start` (alias `queue: go`) | active — drive the queue now | `queue_active: true` + marker `auto` |
| `queue: stop` | inactive — halt | `queue_active: false` |
| (absent) | unmanaged / inactive | absence of both |

**Why a verb tri-state, not `queue_auto: true` or `queue: auto`/`stop`:** `queue_active` is a boolean — it records *state* but loses *intent*, and lives in a different place than `auto`, so the two drift. A verb (`start`/`stop`/`go`) is an imperative that reuses agent-doc's existing directive grammar — `do #id`, `run queue`, and the `--- start` / `--- stop` control fences already in the queue body. One vocabulary, one concept. And unlike `auto`, a single key with start/stop is symmetric, so the bidirectionality you want is structural rather than a sync rule: there is exactly one field to read and write.

**Bidirectional / migration (normalize-on-read):**
- `&lt;!-- agent:queue … auto --&gt;` → normalizes to `queue: start`, strips `auto` from the marker.
- `queue_active: true|false` → normalizes to `queue: start|stop` (kept as deprecated input so existing docs — including this one — keep working).
- `go` ≡ `start`; drain sets `queue: stop` (today's "auto removed, queue_active cleared", one field); manual `queue: stop` never re-adds start.

**Harness parity (your point 5):** the three continuation paths already gate on `queue_active`/`auto` — Claude Code `/loop`, the Codex Stop-hook (`active_auto_queue_prompt`), and the OpenCode auto-loop. This is mostly a read-path consolidation: they all read the unified `queue:` state, and preflight keeps emitting `queue_active`/`queue_trigger` derived from it. So `queue: start`/`go` starts and drains identically under Codex and Claude Code, matching OpenCode.

Captured as plan `tasks/agent-doc/plan-queue-state-frontmatter-unification.md` (4 phases) and backlog `[#queue-state-unify]`. The separate "drive `agent:exchange` prompts into backlog then prepend their ids to the queue" prompt composes on top of this state model — kept as its own item, not folded in.

### Re: implemented #queue-state-unify (phase 1+2) — opus-4-8

Implemented the unified queue control you selected. Committed, tested, isolation-verified.

#### What landed (`src/agent-doc` `3f9b32d4`, superproject `568ff2fd8`)

- **`QueueControl` enum** — `start`/`go` → active, `stop` → inactive. Lenient parse (case/whitespace-insensitive); an unknown value (typo) is ignored, never flips activation.
- **`Frontmatter.queue` field + `normalize_queue_control()`** — folds a recognized `queue:` value onto the existing `queue_active` boolean in **both** parse paths. So every existing reader — preflight activation, Claude `/loop` continuation, Codex Stop-hook, OpenCode auto-loop — honors `queue: start|go|stop` through one read path, no per-harness wiring. The canonical key wins over a stale `queue_active:`.
- `queue_active:` and the `auto` marker stay accepted as **deprecated input** (back-compat; this doc keeps working).
- Spec updated (`specs/07-orchestration-commands.md`); 7 core + 2 orchestration tests.

This satisfies your core ask — **`queue: start` / `queue: go` now activates the queue identically under Claude/Codex/OpenCode**, and `queue: stop` halts it.

#### Verification

Full orchestration lib suite in a clean-HEAD sibling worktree (with my files applied): **2257 passed / 2 failed**. The 2 failures (`route::tests::fresh_start_ack_outcome…`, `…rejects_codex_footer_without_prompt`) are **pre-existing route prompt-detection failures at HEAD** on this branch — they fail on clean HEAD without my change too, and sit in the concurrent session's `route.rs` area. Not introduced here.

#### Boundaries (honest)

- **Not built/installed live yet.** The working tree carries a concurrent session's uncommitted `route.rs`/`session_check.rs`; a `cargo install` would bundle that incomplete work. Live install is deferred until it lands — the code is committed + green for its own scope.
- **Phases 3–4 remain** (`#queue-state-unify`): writer emitting `queue:` / drain → `queue: stop`, the `auto` → `queue: start` normalize-on-write migration, and harness-parity doc rewrites. The read path is the high-value slice and it's done.

#### Your new queue item

Captured *"Implement `run_id` + `step` frontmatter…for re-entrancy / pipeline state"* as backlog `[#fm-run-id-step]` (relates the reentrant-finalize-pipeline plan). It's a separate substantial feature — flagged, not folded into this turn.

### Re: test suite green + marker control (phase 3) — opus-4-8

Did the test-suite fix you asked for, and shipped the marker-side queue control. All committed.

#### Test suite — fixed, `make check` green (exit 0, 0 failures)

Root cause of the 2 pre-existing route failures: `ready_prompt_candidate` treated a Codex **status/footer-only** capture (`gpt-5.5 high · … Context 70% used`, no `›` prompt) as dispatch-ready. Codex always renders a real `›` prompt when ready, so a footer alone is not ready — only OpenCode treats a bare footer/idle splash as ready. Split the branch by harness. `src/agent-doc` `40859a62`. Both `ready_prompt_candidate_rejects_codex_footer_without_prompt` and `fresh_start_ack_outcome_keeps_idle_no_op_and_reaps_genuine_miss` now pass; verified green in a clean-HEAD worktree (independent of the concurrent uncommitted `route.rs` work, which I left untouched).

#### Marker control `start`/`go`/`stop` — phase 3 (`60a466af`)

`&lt;!-- agent:queue go --&gt;` / `start` now fresh-activates the queue (Auto trigger, like legacy `auto`); `stop` halts + clears; the token is stripped on drain/halt; `start`/`go`/`stop` are recognized (no typo warning). With phase 1+2, `queue: go` in frontmatter also activates. 5 new tests.

#### Why your `go` warned and the queue items didn't strike

Both are the **same root cause**: the *live* `agent-doc` binary predates my commits (install is deferred — the working tree still carries another session's uncommitted `route.rs`/`session_check.rs`, so a `cargo install` would bundle their incomplete work). So the running binary:
- doesn't recognize marker `go` → emits `misplaced_component_attr` and ignores it;
- doesn't recognize `queue: go` → the queue never activated → **nothing consumed the heads, so no `~strikethrough~`**.

Your strikethrough expectation is correct existing behavior — it just never fired because the queue stayed inactive under the old binary. Once the concurrent route work lands and I can `cargo install`, marker `go` / `queue: go` will activate and the consume pass will strike completed `do [#id]` heads. (Note: the queue heads `- [#fm-run-id-step]` / `- [#queue-state-unify]` are bare `[#id]` refs, not `do [#id]` directives — the strike/consume pass keys off `do [#id]`.) I did not hand-strike them: striking is the binary's job and a manual edit would be lossy.

#### State

Committed: phase 3 `60a466af`, route fix `40859a62`, superproject `66a841dfc`. `#queue-state-unify` remains for phase 4 (writer emits canonical `queue:`) + the live install once the concurrent work lands.

### Re: installed — `queue: go` / marker `go` now live — opus-4-8

Done. Built + installed both binaries; the feature works live.

- **tagpath lint fix** (`src/tagpath` `9289827`): `start`/`go`/`stop` are now valid bare `agent:queue` markers, so `&lt;!-- agent:queue go --&gt;` no longer trips `agent-doc/malformed-attr`. 114 lib tests pass. agent-doc builds against local `../tagpath` via the `.cargo/config.toml` patch.
- **`make check` green**; installed both binaries (`cargo install --path .` + `cargo build --release`).
- **Live verify:** preflight now reports `queue_active: true`, `queue_trigger: auto`, zero warnings — `go` is recognized and activates the queue; the lint-gate no longer blocks it.

Your `[#fm-run-id-step]` / `[#queue-state-unify]` heads are bare `[#id]` refs, not `do [#id]` — prefix with `do ` to have the queue drive them. `#queue-state-unify` gated (phases 1-3 live; phase 4 = writer emits canonical `queue:`). Not auto-driving the big features without an explicit `do`.

### Re: queue reorder — opus-4-8

Reorder registered: `[#queue-state-unify]` is now the head, then `[#tagpath-lint-queue-control]`, then `[#fm-run-id-step]`.

These are still **bare `[#id]` refs, not `do [#id]` directives**, so the live queue treats them as references and won't execute them. To have the active queue actually drive a head, prefix it with `do ` — e.g. change `- [#queue-state-unify]` to `- do [#queue-state-unify]`. (That's the same `do` grammar as the rest of the queue; bare `[#id]` stays a pointer so reordering/prioritizing doesn't accidentally kick off work.)

Status of the reordered heads:
- `[#queue-state-unify]` — phases 1-3 shipped + live; **phase 4** remains and is the riskiest slice (switching the writer to emit canonical `queue:`/drain→`queue: stop` instead of `queue_active:`, which touches the spurious-drift + strip-queue-active invariants in `closeout.rs`/`git.rs`). I'll implement it on an explicit `do [#queue-state-unify]`, ideally on its own focused turn.
- `[#tagpath-lint-queue-control]` — **done** (`src/tagpath 9289827`, installed); it can be struck/reaped.
- `[#fm-run-id-step]` — not started; `do [#fm-run-id-step]` to drive.

Tell me `do [#queue-state-unify]` (or any head) and I'll run it.
❯ do [#queue-state-unify]. #spec-test-build-install-commit-push
### Re: do [#queue-state-unify] — phase 4 shipped — opus-4-8

Done — `#queue-state-unify` phase 4 implemented, tested, built, installed, committed, and pushed (your `#spec-test-build-install-commit-push` preset).

#### What changed (`src/agent-doc` `f256ff71`)
The queue-maintenance writer now persists the **canonical `queue:` control** instead of `queue_active:`. `frontmatter::merge_queue_state` writes `queue: start` on activation and `queue: stop` on drain/halt, clearing any legacy `queue_active:` line in the same write. All 14 write sites (preflight / route / write) migrated.

Drift-safety preserved: the replay-hash and boundary-compare normalizers now strip `queue:` and `queue_active:` together, so a legacy `queue_active:` and a migrated `queue: stop` compare equal — no snapshot/HEAD churn loop. The metadata-drift authority logic reads via `live_continuation_head` (normalized field), so it needed no change. Reads still accept `queue_active:` as deprecated input.

#### Spec + tests + build/install + push
- Spec updated (`specs/07-orchestration-commands.md`).
- Core `merge_queue_state` round-trip test + migrated active-write assertions to `queue: start|stop` across the preflight/route/finalize/run suites (preserved-legacy assertions kept on `queue_active:`).
- `make check` green (exit 0, clippy clean). Both binaries reinstalled (`cargo install` + `cargo build --release`).
- Pushed: tagpath `9289827` (main), agent-doc `f256ff71` (`20260422/codex-agent-backend`), superproject `d80595d2c` (main). CI is running on the agent-doc push.

#### Status
`#queue-state-unify` is now **fully implemented and live** across all phases (canonical `queue:` frontmatter + marker `start`/`go`/`stop` + writer migration). The only thing left is harness-parity instruction-surface doc rewrites (Claude `/loop` / Codex Stop-hook / OpenCode auto-loop wording) — minor polish, not behavior.
<!-- agent:boundary:8931bdaf -->
<!-- /agent:exchange -->

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- do [#tagpath-lint-queue-control]
- do [#fm-run-id-step]
- `do [task-id]` should not be necessary to kick off tasks. However, don't lose referencing the task such as `re [task-id]` which should not execute the task.
- Fix test suite. The turn shouldn't end with a failing test suite...even with pre-existing failures.
- When a queue item is complete, it should have a strikethrough `~[task-id]~`. This queue run did not update the state of the queue items.
- We need to have agent-doc actively try to reduce review gated items. By splitting out actionable items in the backlog. The review items should have annotations on next steps that are token-efficient to parse so a quick pass can determine next actions. Also, there should be a token-efficient way to query gated review items to address them. The review items lists are getting too long on agent-doc-bugs2.md and mosterrodholders.md and have become unmanagable due to long running tasks. It would be great to simplify managing these review items and feed them back into the backlog pipeline.
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
1. [ ] [#tagpath-lint-queue-control] DONE (src/tagpath 9289827, installed): the tagpath agent-doc lint dialect now accepts bare `start`/`go`/`stop` markers on agent:queue, so a bare `go` marker no longer trips agent-doc/malformed-attr. Part of #queue-state-unify.
2. [ ] [#fm-run-id-step] Implement `run_id` + `step` (and `turn_id`, `queue_task_id`) frontmatter fields surfacing live pipeline state for re-entrancy + observability. Relates tasks/agent-doc/plans/reentrant-finalize-pipeline.md.
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
