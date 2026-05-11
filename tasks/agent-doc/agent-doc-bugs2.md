---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: claude-opus-4-6
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': ''
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Implemented Codex capability proof timing diagnostics.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260511-005003.md`*
❯ Evaluate sessions logs against already done work and git history. Please evaluate any standing bugs with agent-doc as #next-steps.
### Re: session-log triage against git history — gpt-5

I checked the current session captures, `ops.log`, the runtime projections, blocked-stop payloads, and recent `src/agent-doc` history.

Already covered by git history: prompt-only closeout guard `8252c60`, duplicate response closeouts `2bf50c5`, stale route diagnostics `c21ef89`, async Codex capability proof `275e82b`, cross-document pending adds `74b620f`, pending-add order `abae439`, stale diagnostics cleanup `6147f99`, and direct backlog reap archiving `3c9d4a0`.

Standing findings:

- Sidecar prefix-divergence fallback is still live. `ops.log` still has `sidecar_normalization_fallback ... reason=prefix_divergence` during the latest closed `agent-doc-bugs2` cycles, including `1778442645` and `1778443236`, after the prefix-body and duplicate-closeout fixes.
- Stale starting actor cleanup is not fully operational in the normal path. Current `.agent-doc/session-actors.json` still has `state: "starting"` rows for `agent-doc-bugs.md`, `agent-doc-bugs4.md`, and `interior-design.md`, even though `6147f99` added stale diagnostics cleanup.
- Blocked-stop repair still has a parser/recovery gap. The current `.agent-doc/codex-hooks/blocked-stop` and `.agent-doc/repair-blocked` payload for `1778389034` contains a valid patch block but was rejected because there was extra transcript content around it.
- Codex capability proof latency is not proven fixed from the live logs yet. Git has the async proof implementation, but the latest measured child network proof I found was still about `10108ms`; I did not find a post-`275e82b` fresh-restart sample proving pane render is decoupled from that cost.

I added backlog items for those next steps in priority order. No tests were run because this cycle was log and git-history triage only.
❯ do [#sidecarfallbackstill]. spec-test-build-install-commit-push
### Re: #sidecarfallbackstill — gpt-5

Implemented and pushed the sidecar prefix-divergence fix.

The root cause was editor-side ordering: JetBrains and VS Code applied `normalize_prefix_lines` before `reposition_boundary`, so prompts typed after the previous boundary marker were outside the user region seen by the ack-content sidecar. The binary's `content_ours` path already saw the post-reposition shape, so verification repeatedly rejected the sidecar with `reason=prefix_divergence` and fell back.

Changes made:
- JetBrains Document and VFS patch paths now reposition the exchange boundary before applying `normalizePrefixLines`.
- VS Code patch application now does the same for component-based IPC patches.
- Added JetBrains and VS Code regressions for prompts typed after a stale boundary.
- Updated `editors/PLUGIN-SPEC.md`, `VERSIONS.md`, and bumped the JetBrains plugin to `0.2.107`.

Verification/build/install:
- `cargo test -p agent-doc normalize_exchange_prefixes_for_targets -- --nocapture`
- `cargo test -p agent-doc verify_sidecar_normalization -- --nocapture`
- `npm test` in `editors/vscode`
- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test`
- `make check`
- `make install`
- `GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin`
- `agent-doc plugin install --local jetbrains`, installed to IntelliJ IDEA 2026.1; IDE restart is required to activate it.

Commits pushed:
- `src/agent-doc`: `5fc0701 fix: normalize editor prefixes after boundary reposition`
- parent repo: `da75c3938 agent-doc: update prefix normalization fix`
❯ do [#autogcstart]. spec-test-build-install-commit-push
### Re: #autogcstart — gpt-5

Implemented and pushed the stale `starting` actor cleanup fix.

Root cause: stale-starting actor cleanup existed, but it only ran as part of full `agent-doc gc`, and preflight only runs that full GC once per day via `.agent-doc/gc.stamp`. A fresh stamp could therefore leave old `starting` projections visible to normal preflight/start/sync work.

Changes made:
- Split stale-starting actor closure into a caller-aware controller helper while preserving `agent-doc gc` behavior.
- Added lightweight stale `starting` actor cleanup to every normal `preflight`, `start`, and `sync` cycle.
- Kept full orphan-file/diagnostic GC on the existing daily stamp cadence.
- Added regressions proving preflight closes stale actors even when `gc.stamp` is fresh, and that normal-path cleanup records the calling surface in actor transitions.
- Updated `README.md`, `SPEC.md`, `VERSIONS.md`, and the core/session-actor specs.

Verification/build/install:
- `cargo test -p agent-doc preflight_closes_stale_starting_actors_even_when_daily_gc_stamp_is_fresh -- --nocapture`
- `cargo test -p agent-doc normal_path_actor_cleanup_records_calling_surface -- --nocapture`
- `make check`
- `make install`
- `agent-doc --version` -> `agent-doc 0.33.16`

Commits pushed:
- `src/agent-doc`: `83e2f72 fix: run stale actor cleanup on normal paths`
- parent repo: `94a2f347a agent-doc: update stale actor cleanup`
<!-- agent:boundary:2aeee76f -->
<!-- /agent:exchange -->
###

<!--
minor (v0.34.0) release + publish. merge into main
---
/home/brian/work/btakita/agent-loop/tasks/agent-doc/plan-project-controller-session-actor-mvp.md
---
"Press Enter to restart, or 'q' to exit."
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#blockedstopextract] Fix blocked-stop repair for transcript-wrapped patch responses: current codex-hooks/blocked-stop and repair-blocked payloads contain valid patch blocks but are rejected for extra transcript content, so extract the patch or fail with a precise recoverable instruction.
- [ ] [#capproofverify] [recommended] Verify async Codex capability proof on a fresh managed restart: last logged network_child proof cost was about 10.1s, and there is no post-275e82b live proof sample yet showing pane render is decoupled from proof latency.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-05-07 [#backlogreap] [recommended] Make completed/reaped backlog items visible in the session document or closeout response so items like #commenttail, #tmuxcoord, and #coordspike do not look silently removed after --pending-done.
- 2026-05-07 [#sqliterepros] [recommended] Before prototyping .agent-doc/state.db, capture 2-3 current agent-doc failures where cross-file authority drift or non-atomic state writes would have been fixed by one transactional store rather than by the single-owner actor/supervisor model alone.
- 2026-05-07 [#sqliteeval] [recommended] Review plan tasks/agent-doc/plan-sqlite-session-store-concurrency.md against the single-owner actor rollout and decide whether to prototype .agent-doc/state.db as the authoritative session store.
- 2026-05-07 [#pcperf] [recommended] Add sync/route latency instrumentation and budgets to the Project Controller rollout: measure prune/proof/router/dispatch-ack phases, keep no-op safe-passive sync on the fast path, and surface when a 10s dispatch-proof wait is the reason a pane appears non-responsive.
- 2026-05-07 [#pcctrl] Implement Phase A from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: add the project-local controller shell, singleton socket/launch lock, lazy connect-or-launch helper, controller status command, and bootstrap epoch/launch-mode persistence without changing route/start behavior yet.
- 2026-05-08 [#pcstore] Implement Phase B from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: move actor-record load/store behind controller commands, write SQLite first, emit session-actors.json and sessions.json as projections, and add projection drift diagnostics.
- 2026-05-08 [#pcsuper] Implement Phase C from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: make Harness Supervisor startup register with the controller and report ready/busy/waiting-input/blocked/closed lifecycle facts through controller-owned actor transitions.
- 2026-05-08 [#pcroutes] Implement Phase D from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: convert start, route, and sync to read/dispatch through controller authority and keep tmux-router as layout reconciliation only.
- 2026-05-08 [#pcops] Implement Phase E from tasks/agent-doc/plan-project-controller-session-actor-mvp.md: wire status/history/attach/restart/clear/doctor through controller clients, keep destructive recovery behind doctor --repair, and expose stage-specific editor diagnostics.
- 2026-05-08 [#ctrlreload] [recommended] Add controller binary-generation/version invalidation so agent-doc restarts a stale project controller after local install/update instead of surfacing unknown controller command errors for newly-added controller RPCs.
- 2026-05-08 [#ctrlsock] Implement tasks/agent-doc/plan-controller-stalled-client-start-hang.md: make project controller RPCs robust to stalled clients so a stuck sync/no-autostart or closed prior pane cannot make agent-doc start/session status hang; fail closed with stale-pane diagnostics instead.
- 2026-05-08 [#tigertest] [recommended] Create a TigerBeetle-inspired deterministic simulation/property-test plan for agent-doc: model session-cycle state, generate concurrent document edits and crash/fault schedules, assert closeout/pending/git invariants, and keep most cases in fast pure tests.
- 2026-05-08 [#closeguard] Implement tasks/agent-doc/plan-closeout-active-prompt-guard.md: fail closeout when a committed response is followed by a newer unresolved prompt-bearing exchange line or when the matching pending item has a malformed checklist prefix.
- 2026-05-08 [#tigersim] [recommended] Implement Phase 1-2 from tasks/agent-doc/plan-tigerbeetle-style-simulation-testing.md: inventory pure closeout seams, build the first SimWorld closeout simulator, and promote fixed seeds for recent closeout/pending/snapshot bugs.
- 2026-05-08 [#tigerfault] Implement Phase 3 from /home/brian/work/btakita/agent-loop/tasks/agent-doc/plan-tigerbeetle-style-simulation-testing.md: add deterministic crash and fault injection points around snapshot save, fallback patch write, IPC delivery, template merge, working-tree write, index update, git commit, post-commit boundary reposition, and session-check.
- 2026-05-08 [#harnessprompt] Implement tasks/agent-doc/plan-harness-prompt-no-changes-closeout.md: make harness-native agent-doc turns fail closed when preflight reports no markdown changes but the chat prompt contains actionable text that was never persisted to the exchange.
- 2026-05-08 [#tigerroute] Implement Phase 4 from /home/brian/work/btakita/agent-loop/tasks/agent-doc/plan-tigerbeetle-style-simulation-testing.md: extend SimWorld with route/controller schedules, session actor generations, supervisor lifecycle facts, dispatch acceptance/proof, stale/missing pane observations, and projection-drift invariants.
- 2026-05-08 [#tigerci] Implement Phase 5 from /home/brian/work/btakita/agent-loop/tasks/agent-doc/plan-tigerbeetle-style-simulation-testing.md: wire fast fixed-seed simulator coverage into cargo test, add a wider deterministic make check budget if runtime stays acceptable, document seed-range replay/promotion, and track generated-coverage runtime budgets.
- 2026-05-08 [#autosync] Implement Phase 1 from tasks/agent-doc/plan-editor-autosync-latency-test-budget.md: fix editor selection autosync so changing focused markdown files applies the same tmux pane/focus result as manual Sync Tmux Layout, with editor and CLI regressions.
- 2026-05-08 [#synclag] Implement Phase 2 from tasks/agent-doc/plan-editor-autosync-latency-test-budget.md: capture slow manual sync runs, split sync/controller latency buckets, keep warm-controller safe-passive sync under budget, and surface the exact over-budget phase.
- 2026-05-08 [#tmuxbudget] Implement Phase 3 from tasks/agent-doc/plan-editor-autosync-latency-test-budget.md: inventory slow tmux-backed edge tests, keep only real tmux smoke coverage, and migrate pure ownership/controller schedules into deterministic SimWorld traces with runtime-budget tracking.
- 2026-05-08 [#syncproof] [recommended] Reduce safe-passive proof overhead after lock acquisition: cache cheap controller/session facts for unchanged editor selection, avoid repeated ownership proof on no-op syncs, and keep controller_actor_lookup plus ownership_proof inside explicit latency budgets.
- 2026-05-08 [#syncprune] [recommended] Shorten no-op prune work: skip or incrementally rate-limit stale/stash pane pruning when the active file/session mapping is unchanged, and keep prune_stash_panes under its 250ms subphase budget.
- 2026-05-09 [#synclock] Coalesce editor autosync and manual Sync Tmux Layout requests so routine selection churn does not wait the full 3s sync_lock_wait budget; make redundant safe-passive syncs join, supersede, or fail fast with a visible over-budget phase.
- 2026-05-09 [#editorswitch] Implement a bounded editor-document switch fast path: resolve the focused markdown file through the controller actor binding, update tmux focus without waiting on stale safe-passive sync/prune work, and add regressions for stale starting sibling sessions like tasks/software/tsift.md.
- 2026-05-09 [#finalize-do-cascade] finalize --pending-done cascade failure: do #id closeout requires up to 4 finalize attempts due to conflicting pending_capture_guard and pending_done_guard. Plan: tasks/agent-doc/plan-finalize-pending-done-cascade.md
- 2026-05-09 [#codex-manifest-warnings] Fix repeated Codex plugin/skill manifest warnings: overlong interface.defaultPrompt, more than 3 default prompts, invalid skills path ./, and icon paths containing ..; decide whether to patch local plugin metadata, installer validation, or warning suppression.
- 2026-05-09 [#editorswitchctl] Fix the current editor-document switch split by completing/regressing the Phase D/E controller authority boundary: tasks/software/tsift.md remains actor_state=starting with a healthy supervisor, route refuses injection, and tmux window 4 still has four agent-doc panes. Plan: tasks/agent-doc/plan-project-controller-session-actor-mvp.md
- 2026-05-09 [#controller-timeout] Re-open the controller stalled-client/start hang work for the current route timeout: .agent-doc/state/editor-route-errors/tasks__agent-doc__agent-doc-bugs2.md.txt still reports timed out waiting for .agent-doc/controller.sock. Plan: tasks/agent-doc/plan-controller-stalled-client-start-hang.md
- 2026-05-09 [#sidecar-diverge] [immediate] Finalize sidecar normalization divergence loses patchback — editor does not show response despite git commit. Plan: tasks/agent-doc/plan-finalize-sidecar-divergence.md
- 2026-05-09 [#syncbudget-regress] [recommended] Turn the current sync latency evidence into a regression budget: repeated sync_lock_wait=100/3004ms over-budget and prune_stash_panes=1395-2073ms over-budget should become a focused fast-path test before more tmux test expansion. Plan: tasks/agent-doc/plan-editor-autosync-latency-test-budget.md
- 2026-05-09 [#root-empty-preflight] [recommended] Add a focused route/preflight recovery test for repeated empty preflight_started turns with no capture, using src/session-share/tasks/root.md cycle-1778336475478 as evidence: no response exists to replay, so the next invocation should fail with precise retry/restart guidance instead of repeated silent patchback misses.
- 2026-05-09 [#g4w2] Controller singleton/handoff must allow only one authoritative same-project controller while replacing stale binaries with zero downtime; retire or reap duplicate controller processes only after a promoted replacement preserves active session actors and supervisor leases. Plan: tasks/agent-doc/plan-controller-singleton-zero-downtime-handoff.md
- 2026-05-09 [#instrdrift] Root AGENTS.md / installed Codex instruction drift causes repeated version-mismatch notices — make .codex/AGENTS.md the canonical generated surface or thin/update root AGENTS.md, then add an audit/CI check so future agent-doc-version bumps cannot leave stale duplicate instructions behind.
- 2026-05-09 [#dodone] Skill does not auto-run --pending-done on do #id completion — when a user prompts 'do [#id]' and the task is completed, the skill's finalize step should include --pending-done <id> automatically. Currently the skill completes the work and finalizes the response but forgets to mark the pending item as done, requiring manual correction.
- 2026-05-09 [#routeerrstale] [recommended] Clear stale editor-route-errors after a later successful route/session status so the editor does not keep surfacing obsolete proof-timeout or starting-actor diagnostics.
- 2026-05-09 [#ctrlstatedrift] [recommended] Reconcile controller/supervisor runtime_state with session-actor projection; current tsift status shows actor ready while supervisor/controller lease still reports starting with a stale heartbeat.
- 2026-05-09 [#stashprunefast] [recommended] Make safe-passive editor sync skip or amortize expensive registered-live stash pane pruning; current logs spend 2.7-4.7s in prune_stash_panes on routine selection changes.
- 2026-05-09 [#panehang] Post-completion pane not reaped + startup routing miss — agent-doc start spawns Claude Code panes that either (1) complete their cycle but stay alive at idle prompt, or (2) never receive the /agent-doc routing command and sit idle indefinitely. Plan: tasks/agent-doc/plan-hung-pane-reap-and-routing.md
- 2026-05-09 [#codexcapstale] Managed Codex pane capability drift — ready actor/process args can show `-s danger-full-access` and expected `--add-dir` roots while the long-lived pane still lacks DNS/SSH/write access that a fresh runtime has. Plan: tasks/agent-doc/plan-codex-managed-session-capability-proof.md
- 2026-05-09 [#codexnonet] Codex sandbox denies all network calls in agent-doc-managed sessions even when the intended launch contract requires network access; prove live network capability before dispatch/reuse and fail closed with sandbox-specific diagnostics. Plan: tasks/agent-doc/plan-codex-network-sandbox-denial.md
- 2026-05-09 [#preflighthang] Direct `agent-doc <file>` invocation can hang after opening `preflight_started` when it self-dispatches into the same active Codex pane; detect recursive/self dispatch, bound the wait for capture progress, and persist a recoverable timeout state. Plan: tasks/agent-doc/plan-direct-invocation-preflight-hang.md
- 2026-05-09 [#commitfs] Codex commit filesystem sandbox denial — managed session can edit nested submodule files but cannot create the backing `.git/modules/.../index.lock`, so `git add`/commit fails late with `Read-only file system`. Plan: tasks/agent-doc/plan-codex-commit-filesystem-sandbox.md
- 2026-05-09 [#caplat] [recommended] Reduce Codex managed capability proof latency and surface phase timings for network/SSH/write-root probes so slow agent-doc start checks identify the expensive step.
- 2026-05-09 [#tmuxdevci] Split agent-doc test budgets: keep default development tests pure/fast by gating real tmux-backed tests behind an explicit CI target, add GitHub Actions coverage for that target, and require matching SimWorld edge-case traces before demoting any tmux regression.
- 2026-05-10 [#rspcmt2] Response patchback remains uncommitted after reported closeout — visible response in session doc can require a later explicit `agent-doc commit <file>` to finish the target/parent commit boundary. Plan: tasks/agent-doc/plan-response-patchback-uncommitted.md
- 2026-05-10 [#stateproj] [recommended] Audit remaining .agent-doc JSON session/layout state files and migrate authoritative state to the project controller SQLite store, leaving JSON files only as projections or short-lived caches.
- 2026-05-10 [#duppb] Duplicate response patchback after sidecar normalization fallback / full-content IPC re-delivery — the same response block can appear twice back-to-back after finalize reports success. Plan: tasks/agent-doc/plan-duplicate-sidecar-fallback-patchback.md
- 2026-05-10 [#runhb] [recommended] Add a parent-visible heartbeat/watchdog for long direct `agent-doc <file>` and child-agent waits: emit elapsed phase/cycle output every 30s, persist last-progress timestamps, and fail closed when capture/write/commit progress stalls past budget.
- 2026-05-10 [#codexasyncnet] [recommended] Split Codex managed capability proof from pane startup: launch/render the Codex pane immediately, run network/SSH/write-root proof asynchronously, gate prompt dispatch until required proof succeeds, and fail closed with diagnostics if background proof fails.
- 2026-05-10 [#syncroutelog] Fix stale/opaque sync-route diagnostics: controller RPC can surface 'ok response without data', saved editor-route-errors can remain stale after later success, and route can submit on a stale starting projection. Plan: tasks/agent-doc/plan-sync-command-route-error-follow-up.md
- 2026-05-10 [#spfxnorm] Sidecar ❯ prefix normalization loss — content_ours fallback recovers snapshot but not working tree. Plan: tasks/agent-doc/plan-sidecar-prefix-normalization-loss.md
- 2026-05-10 [#oobtaildel] Valid deletion of escaped prompt content outside `agent:exchange` is misclassified as post-commit local drift; distinguish cleanup deletion from real later user edits without swallowing fresh prompts. Plan: tasks/agent-doc/plan-valid-oob-tail-deletion.md
- 2026-05-10 [#pendrev] --pending-add inserts multiple items in reverse order — second item ends up above first (LIFO instead of FIFO). Fix: batch-insert all adds at once in specified order. Plan: tasks/agent-doc/plan-pending-add-reverse-order.md
- 2026-05-10 [#crossdocpend] Cross-document --pending-add targets wrong file — skill adds to current document backlog instead of preset-specified target file. Plan: tasks/agent-doc/plan-cross-doc-pending-add.md
- 2026-05-10 [#iceboxpatch] Icebox component mutation has no binary-owned path — patch:icebox / replace:icebox in response heredoc not parsed as component patches, --pending-* flags only target agent:backlog. Skill falls back to boundary synthesis, dumping icebox content into exchange body. Needs either --icebox-add flag or template patch parsing for non-backlog components.
- 2026-05-10 [#oobpmt] OOB prompt absorption — preflight commit step absorbs user-content changes (new prompts in status) before diff runs, causing `no_changes: true`. Fix: use pre-commit snapshot as diff baseline, not post-commit snapshot.
- 2026-05-10 [#staleflt] Stale preflight_started cycle recovery — auto-close empty preflight cycles (no capture, phase=PreflightStarted) on next preflight invocation instead of failing closed. Add time-based staleness check.
- 2026-05-10 [#busychk] Pane busy-check before sync eviction — check for open agent-doc cycles before killing panes during layout reconciliation. Skip eviction if busy, log warning.
- 2026-05-10 [#chkptcap] Periodic checkpoint capture during response streaming — capture partial responses every 30s so mid-generation pane death doesn't lose work entirely.
- 2026-05-10 [#257p] OAuth flow interrupts finalize — skill fails to persist response when an MCP OAuth flow interrupts the cycle. The authenticate tool call consumes the turn and the skill never reaches `agent-doc finalize`. Fix: treat MCP auth as a sub-step and still finalize after, or detect response-without-finalize and warn/recover.
- 2026-05-10 [#pfxleak2] content_ours prefix leak + commit churn — plan: tasks/agent-doc/plan-content-ours-prefix-and-commit-churn.md
- 2026-05-10 [#bppfxstrip2] content_ours strips ❯ from multi-line user prompts during finalize — plan: tasks/agent-doc/plan-prefix-strip-on-user-prompts.md
- 2026-05-10 [#rootpatchmiss] Direct Codex agent-doc turn completed commits/pushes but did not write the final response into `src/session-share/tasks/root.md`; extend `tasks/agent-doc/plan-response-missing-after-manual-closeout.md` with the May 10 `#vt-agent-deploy` reproduction and fail closed on prompt-only exchange closeout.
- 2026-05-10 [#next-steps] Triage the current activity/session-log findings: sidecar prefix-divergence fallback still fires, stale starting actors remain in session-actors, blocked-stop repair payloads exist for transcript-wrapped patch responses, and Codex child network proof still adds about 10s on restart.
- 2026-05-10 [#sidecarfallbackstill] Fix recurring sidecar prefix-divergence fallback after the latest prefix-body and duplicate-closeout fixes: ops.log still shows sidecar_normalization_fallback reason=prefix_divergence on clean agent-doc-bugs2 closeouts, so tighten snapshot/ack normalization until fallback is exceptional again.
- 2026-05-10 [#autogcstart] Ensure stale starting actor cleanup runs in the normal preflight/start/sync path: session-actors.json still has old starting projections for agent-doc-bugs.md, agent-doc-bugs4.md, and interior-design after 6147f99, so auto-gc or route-time reconciliation is still incomplete.
<!-- /agent:done -->
