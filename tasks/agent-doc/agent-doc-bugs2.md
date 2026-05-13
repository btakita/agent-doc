---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_model: claude-opus-4-6
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
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

Recorded Codex capability proof verification.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260513-164109.md`*

Compacted content:
- Archived 1 response topic(s): OpenCode permission arrows
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ There seem to be orphaned tmux panes that are prompting JB with the permissions selection dialog.
❯ How can we prevent this edge case within agent-doc? #next-steps
❯ Please clean up the orphaned tmux panes.
### Re: orphaned tmux pane cleanup — gpt-5.5

Cleaned up the orphaned panes.

Evidence:
- `agent-doc resync --fix` deregistered stale stash records and purged 10 orphaned agent panes from non-stash windows.
- Follow-up `agent-doc resync` reports `No session/process issues detected`.
- Follow-up tmux pane listing now shows only one `agent-doc` pane in the managed session: `%1`, the current `agent-doc-bugs2.md` authoritative pane.

The prevention change I would make in agent-doc is to separate "live pane exists" from "pane is allowed to surface editor prompts." A pane should only be eligible for JB/VSCode permission prompt UI when it is owned by the current document's controller lease, matches the current generation, and belongs to an open cycle. Completed idle panes in stash, or panes whose registry was deregistered but whose supervisor is still alive, should be shut down/reaped or hidden from prompt discovery.

I added a backlog item for that prevention work: `#orphanpromptprune`.
### Re: note — gpt-5.5

Noted. I kept the cleanup result and the `#orphanpromptprune` prevention item in the document.
❯ You seem to have reaped too many tmux panes. The root.md tmux pane is lost. JB `Sync Tmux Layout` does not show the panes anymore. Please recover.
❯ spec-test-build-install-commit-push
### Re: recover root tmux pane — gpt-5.5

Recovered the lost `src/session-share/tasks/root.md` pane and fixed the cleanup path that caused this class of loss.

Operational recovery:
- Removed the stale root binding for missing pane `%2` with targeted `agent-doc resync --fix`.
- Ran a full `agent-doc sync` for `tasks/agent-doc/agent-doc-bugs2.md` plus `src/session-share/tasks/root.md`.
- `root.md` is now registered on pane `%52` in the `0:agent-doc` window next to the current pane `%1`. `agent-doc session status` shows the root actor ready with healthy supervisor/controller state; the pane is currently alive and running the root session.

Code fix:
- `src/agent-doc/src/resync.rs` now makes non-stash orphan-agent cleanup check the pane-local project root before killing an apparently unregistered agent pane. It preserves panes that are registered, live-owner-proven, or supervisor-backed in a nested project such as `src/session-share`.
- Updated `src/agent-doc/SPEC.md` and `src/agent-doc/specs/07-session-tmux-commands.md` with the cross-project orphan cleanup invariant.
- Added a live tmux regression: `purge_orphan_agent_in_non_stash_preserves_nested_project_owner`.

Verification passed:
- `cargo test purge_orphan_agent_in_non_stash_preserves_nested_project_owner -- --ignored`
- `cargo test purge_orphan_agent_in_non_stash_window -- --ignored`
- `make check`
- `make tmux-ci` (`225 passed; 0 failed` in the live tmux suite)
- `make install`
- `agent-doc --version` -> `agent-doc 0.33.16`
- Latest GitHub CI inspected: run `25773430221`, conclusion `success`.

Commits created:
- `src/agent-doc`: `9d4b4933 fix: preserve nested agent panes during resync`
- parent `agent-loop`: `d8cbcd8c0 agent-doc: preserve nested panes during resync`

Per the session-document commit boundary, I will push those commits after this finalize step succeeds.
<!-- agent:boundary:5ec300d0 -->
<!-- /agent:exchange -->
###
<!--
When I navigate the editor to tsift.md and JB `Sync Tmux Layout`, JB shows an error. It seems the manual reap killed the active tmux panes and now agent-doc cannot recover to sync the document. This issues happens with all documents where the tmux pane was killed. agent-doc sync should be more durable and recover tmux panes that crashed or were externally killed.
```
Sync deferred: another tmux layout sync is already running; the latest editor selection will retry shortly.
```
spec-test-build-install-commit-push
---
The agent:done tag should support an attribute to link to a sepate "done" file (*.done.md) so the document does not become bloated. Plan so we agree on the attribute name.
---
The sandbox check tmux prompt should be terse and more "human readable" on a single line. Explain the status without the extra information.
spec-test-build-install-commit-push
---
`--pending-only` should be renamed to `--backlog-only`.
spec-test-build-install-commit-push
---
Please ensure the JB opencode fixes are propagated to the VSCode plugin. Ensure the specs are updated for all editor plugins. Should we implement in the ffi plugin?
#next-steps
---
JB `Run Agent Doc` on root.md with `agent: claude` sends an incorrect command
```
❯ agent-doc /home/brian/work/btakita/agent-loop/src/session-share/tasks/root.md
  ⎿  API Error: 400 Could not process image
```
spec-test-build-install-commit-push
---
Should tsift fail fast in this case?
```
• Ran tsift search "transcript not found livekit-101"
  └ Error: tsift search timed out after 30s (strategy: lexical). The search root looks fresh, so reindexing is unlikely
    to help. Re-run with `--timeout 0` to disable the timeout, narrow `--path` / `--scope`, or try a different
    strategy.
```
spec-test-build-install-commit-push
---
With the latest supply chain attacks on npm and pypi, should agent-doc should run in a docker container? Should all software written run in a docker container?
---
JB `Restart Supervisor Process` error:
```
agent-doc command failed (exit 1): Error: session_restart refused for /home/brian/work/btakita/agent-loop/src/session-share/tasks/root.md because pane %2 is alive-busy (source=authoritative_actor, current_command=agent-doc, tail="⏵⏵ bypass permissions on (shift+tab to cycle)"). Run `agent-doc session status /home/brian/work/btakita/agent-loop/src/session-share/tasks/root.md` and wait for an idle prompt, or inspect/stop the pane explicitly before clearing or restarting it.
```
spec-test-build-install-commit-push
---
Move JB `Copy Session Diagnostics` outside the numeric shortcuts.
---
I don't have a good track on the lifecycle logic that starts panes, kills panes, edge cases, etc.
Can we make a nested list and diagram of the lifecycle logic?
---
Why is agent-doc starting a child Codex run?
```
• agent-doc started a child Codex run. Its preflight noticed the previous patchback was already committed, then
  submitted the new follow-up prompt for execution.
```
---
```
3. Workaround: delegate SSH via the host — if OpenCode can't SSH directly, have agent-doc run the SSH deploy           step outside the managed pane (in the harness supervisor process), not inside the OpenCode child.
```
---
Confirm that z.ai subscriptions are message based and not token based. If message based, is agent-doc inefficient on message based plans? Please review code + logs.
#next-steps
---
`[#2fha] 2026-05-11 [#nopatchbackopencode] ...` backlog item should have had the id `[#nopatchbackopencode]`.
---
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
- [ ] [#sshcapfalsecapture] #agent-doc-bug: Required-SSH guard falsely failed a Monster Rod Holders `agent-doc` run by matching historical `.agent-doc/captures/...json` text containing `socket: Operation not permitted`; `command_execution_required_ssh_failure()` scans arbitrary command output before proving the command was an SSH probe. Plan: tasks/agent-doc/plan-required-ssh-false-positive-historical-capture.md
- [ ] [#codexverbosepb] #agent-doc-bug: Codex child-run response patchback is too verbose because `src/agent-doc/src/agent/codex.rs` concatenates every `item.completed` `agent_message`, so progress/status chatter from a Monster Rod Holders run was committed into `tasks/monsterrodholders.md` along with the final closeout. Plan: tasks/agent-doc/plan-codex-response-patchback-verbose-progress.md
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
- 2026-05-10 [#blockedstopextract] Fix blocked-stop repair for transcript-wrapped patch responses: current codex-hooks/blocked-stop and repair-blocked payloads contain valid patch blocks but are rejected for extra transcript content, so extract the patch or fail with a precise recoverable instruction.
- 2026-05-10 [#capproofverify] [recommended] Verify async Codex capability proof on a fresh managed restart: last logged network_child proof cost was about 10.1s, and there is no post-275e82b live proof sample yet showing pane render is decoupled from proof latency.
- 2026-05-10 [#projecthostscope] Prevent cross-project host/SSH target leakage in agent-doc managed Codex sessions: route/tool context should not let globally approved commands or unrelated SSH profiles make an agent infer that an unrelated client host, such as monsterrodholders-server, is the production host for a different project; require project-local docs/frontmatter/runbook evidence or an explicit user directive before using a named remote host in answers or checks.
- 2026-05-10 [#partialpatchbackadopt] [recommended] Fix session-check/write recovery for agent-doc-owned partial patchbacks: when a prior agent-doc/Codex write path leaves an assistant heading/body visible outside the committed snapshot, classify it as an interrupted/adoptable agent-doc write instead of only direct response patchback, and make empty-stdin recovery point to the correct repair/adoption path.
- 2026-05-10 [#partialpatchbackadopt2] [recommended] Re-open session-check/write recovery for current src/session-share/tasks/root.md evidence: commit blocked on visible '### Re: livekit agent readiness - gpt-5' after zero-patch/IPC write state, then empty-stdin write --commit could not adopt the agent-doc-owned visible response and only reported direct response patchback.
- 2026-05-10 [#emptyrsprepair] Fix empty-response missed-patchback repair: already-committed/no-body recovery must stop before child-agent dispatch and point to write --commit. Plan: tasks/agent-doc/plan-empty-response-repair-write.md
- 2026-05-10 [#pbdupchurn] Stop duplicate response patchback two-commit cleanup churn: duplicate blocks must be rejected or deduped before the closeout commit, and successful closeout must not require follow-up dedupe/snapshot/parent-pointer repair. Plan: tasks/agent-doc/plan-duplicate-patchback-dedupe-churn.md
- 2026-05-10 [#bugtargetroot] `#agent-doc-bug` target resolves to nested session-share backlog instead of parent agent-loop backlog when session docs live under `src/session-share`; resolve preset backlog targets against the intended project root and fail closed on ambiguous mirrored task trees. Plan: tasks/agent-doc/plan-agent-doc-bug-target-resolution.md
- 2026-05-11 [#startgcleak] Fix the stale-starting cleanup gap after #autogcstart: current .agent-doc/session-actors.json still leaves agent-doc-bugs.md and agent-doc-bugs4.md in state=starting after normal preflight/sync; close stale records or explain why they are still live in the normal path.
- 2026-05-11 [#startingdispatch] Fail closed or wait before dispatching into actor_state=starting: ops.log shows route_dispatch_only_starting_actor_direct_pane_submit for agent-doc-bugs2.md generation=118 followed by interrupted_cycle_detected phase=PreflightStarted, so route can still inject during startup and create interrupted cycles.
- 2026-05-11 [#assistanttailprompt] #agent-doc-bug: session-check can misclassify assistant-owned closeout/status prose at the end of a template exchange response as an unresolved prompt-only tail, forcing a second 'Closeout repair:' patchback after a successful response commit. Plan: tasks/agent-doc/plan-session-check-assistant-tail-misclassified.md
- 2026-05-11 [#wvpf] [recommended] Fix Codex hook tests so parallel make check cannot race on shared /tmp/.agent-doc/codex-hooks session cleanup; the default make check run failed once when stop_auto_closes_patch_payload_with_safe_leading_commentary tried to remove an already-missing session file, while the isolated test and serial make check passed.
- 2026-05-11 [#syncbudgetstill] [recommended] Re-open sync latency budget work for current evidence: safe_passive_total still hits 1047-1282ms and postlock_actor_focus/window_resolution/controller_actor_lookup exceed sub-budgets after earlier sync-budget items were reaped.
- 2026-05-11 [#bugchainorder] `#agent-doc-bug` declaration-chain capture can add backlog items in reverse order from the declaration chain; preserve declaration order for batched bug captures and require explicit closeout text for intentional priority overrides. Plan: tasks/agent-doc/plan-agent-doc-bug-declaration-chain-order.md
- 2026-05-11 [#rspcmt3] Re-open response patchback commit recovery for the current agent-doc-bugs2 turn: user-visible prompt/patchback changes remained dirty after `agent-doc commit` reported the prior response already committed, and `session-check` required a second closeout for the active Codex prompt. Plan: tasks/agent-doc/plan-response-patchback-uncommitted.md
- 2026-05-11 [#2fha] 2026-05-11 [#nopatchbackopencode] OpenCode agent-doc turn completed but response was not patched back into the session document — the user had to manually request patchback for the monsterrodholders.md production-sync response. Patchback should fire automatically after the agent writes a response, regardless of agent backend (Claude Code / Codex / OpenCode). Plan: investigate whether OpenCode integration skips the finalize/patchback step that Claude Code and Codex run.
- 2026-05-11 [#opencodesshproof] OpenCode harness has no SSH capability proof — session log shows `codex_capability_proof` entries for Codex generations but zero proof entries for OpenCode generations, so `required_ssh_targets` frontmatter is not enforced and the agent discovers `socket: Operation not permitted` mid-response instead of failing closed. Plan: tasks/agent-doc/plan-opencode-harness-ssh-capability-proof.md
- 2026-05-11 [#opencodesshconfig] OpenCode session cannot SSH to `required_ssh_targets` — `socket: Operation not permitted` occurs because opencode's sandbox blocks network syscalls. Workaround options: (1) copy a per-project `.opencode/config.json` that grants network/SSH permissions, (2) file an agent-doc bug (this item), (3) both. Agent-doc should either detect the sandbox restriction and fail closed before dispatch, or inject the required `.opencode/config.json` permissions automatically when `required_ssh_targets` is set in the session frontmatter.
- 2026-05-11 [#marklifecyclefix] 2026-05-11 mark_lifecycle relative path resolution — fixed in c4f9d17: route promotion now uses bootstrap.project_root + canonical_document_id_in instead of transition_state_direct with controller-CWD-dependent canonicalize. Regression: controller_mark_lifecycle_resolves_relative_path_via_project_root.
- 2026-05-11 [#dupeexchangeblock] #agent-doc-bug: opencode agent writes responses into a second `<!-- agent:exchange patch=append -->...<!-- /agent:exchange -->` block instead of appending to the existing exchange block. This produces an invalid document with two adjacent exchange sections. Reproduction: monsterrodholders.md session a9421282 — the "Production deploy" response was written into a new exchange element (lines 126-147) instead of being appended to the existing exchange (lines 39-125). Root cause: likely the patchback/closeout logic does not detect the existing open exchange block and creates a new one. Plan: tasks/agent-doc/plan-duplicate-exchange-block-patchback.md
- 2026-05-11 [#jbcompactempty] #agent-doc-bug: JB Plugin Compact Exchange produced a defective compaction in boost-client/tasks/monsterrodholders.md (session a9421282). Two problems: (1) The compacted exchange block (lines 30-38) contains only a recursive archive pointer and a truncated prior summary (`Prior summary/context: ### Session Summary *Compacted. Content archived to ...`) with no substantive decisions, key facts, or open items preserved. The archive file itself (7b9f2a63...-20260511-101729.md) is a compaction of a prior compaction — 15 lines, no real content. (2) Compact Exchange did not merge the duplicate exchange blocks created by #dupeexchangeblock into a single block — the document still has two adjacent `agent:exchange` blocks (lines 30-38 and 39-146). Compact Exchange should detect and consolidate multiple exchange blocks into one, and the default summary must carry forward substantive context from the archive chain, not just a truncated pointer. Plan: tasks/agent-doc/plan-jb-compact-exchange-empty-summary.md
- 2026-05-11 [#stalegenfix] 2026-05-11 Fix stale actor transition generation mismatch: when session_id and pane_id both match, allow same-owner transitions despite generation bump from compact/restart/route. Committed in session_actor.rs transition_state_in. Regression: controller_lifecycle_allows_same_pane_stale_generation.
- 2026-05-11 [#escrecov] 2026-05-11 Dispatch-only OpenCode interrupt recovery sends Escape twice and waits for idle prompt before failing closed. Verify in a live OpenCode session that Run Agent Doc works after Clear Session Context when the pane is busy. Plan: verify fix in route.rs attempt_opencode_busy_interrupt_recovery + test resolve_or_create_pane_retries_busy_opencode_pane_after_escape_interrupt
- 2026-05-11 [#w3rb] 2026-05-11 [#capproofdegraded] Dispatch-only degraded authoritative actor path now skips capability proof gate — verify in a live OpenCode session that Run Agent Doc works after Clear Session Context. Plan: tasks/agent-doc/plan-degraded-authoritative-skip-capability-proof.md
- 2026-05-11 [#rspcmt4] 2026-05-11 OpenCode agent-doc turn response patchback not committed — boost-client/tasks/monsterrodholders.md v1.29 deploy response was patched into the exchange section but agent-doc finalize/closeout did not commit the document. User had to manually request commit. Same class as #rspcmt2, #rspcmt3. Reproduction: session a9421282, GLM-5.1 agent. Plan: extend tasks/agent-doc/plan-response-patchback-uncommitted.md with OpenCode-specific closeout evidence.
- 2026-05-11 [#rspcmt5] 2026-05-11 Direct Codex chat `commit-push` closeout completed commits/pushes but wrote no response patchback into `src/session-share/tasks/root.md`; user had to request "write response to document" afterward. Same class as #rspcmt2-#rspcmt4, but this evidence is the normal chat/preset path rather than an agent-doc child-run path. Plan: extend tasks/agent-doc/plan-response-patchback-uncommitted.md with a direct-chat closeout invariant and require session-check/write-back before final success reporting.
- 2026-05-11 [#noexchopencode2] #agent-doc-bug: OpenCode agent response output to user but not written into session document `agent:exchange` block. boost-client/tasks/monsterrodholders.md session a9421282, GLM-5.1 agent — sidebar revert assessment response appeared in CLI output but was never patched back into the exchange section. User had to manually request "Write the response into the document." Same class as #nopatchbackopencode, #rspcmt4, #rspcmt5. OpenCode integration may skip the finalize/patchback step that other harness backends run.
- 2026-05-11 [#rspcmt6] #agent-doc-bug: OpenCode agent-doc response patched into exchange but finalize/closeout did NOT commit the session document. boost-client/tasks/monsterrodholders.md session a9421282, GLM-5.1 agent — the diff-summary response after "Run the post docker commands." was written into the exchange section but `tasks/monsterrodholders.md` remains dirty (uncommitted patchback lines). Additionally, the v1.32 surgical-revert code changes in `src/monsterrodholders-dev/` (mrh-performance.php + test script) were also left uncommitted despite the status saying "awaiting spec-test-commit-push." Same class as #rspcmt2-#rspcmt5. Plan: extend tasks/agent-doc/plan-response-patchback-uncommitted.md with OpenCode finalize-not-committing evidence.
- 2026-05-11 [#rootlatepatch] Late fallback patch can re-dirty an already committed session document: `src/session-share/tasks/root.md` had a valid committed closeout, then a delayed fallback patch reintroduced a stale duplicate response and corrupted the `#vt-semantic-hero-screenshot` heading into `#vt-semantic-hero-scree nshot`, causing `session-check` to report an uncommitted/direct patchback. Plan: tasks/agent-doc/plan-late-fallback-patch-redirties-committed-doc.md
- 2026-05-11 [#garbledpane] OpenCode TUI crashes to help screen after >20s commit — pane shows garbled ANSI art from opencode --help output and auto-trigger fails. Related: #routeStartingRace, #startingdispatch. Plan: tasks/agent-doc/plan-opencode-garbled-console-after-long-commit.md
- 2026-05-11 [#routestartingrace] Route fails on freshly started actor — 5s wait timeout too short for OpenCode harness boot. Plan: tasks/agent-doc/plan-route-starting-actor-race-after-restart.md
- 2026-05-11 [#idprefix] Backlog item [#2fha] uses internal ID instead of declared tag [#nopatchbackopencode]. Plan: tasks/agent-doc/plan-id-prefix-mismatch-backlog.md
- 2026-05-11 [#routereap] #agent-doc-bug: route-owned pane reaped after successful finalize commit — completion thread in start.rs unconditionally kills the pane after detecting a committed cycle, even when the session document has an active backlog and ongoing exchange (multi-turn session). Should check session liveness signals before reaping. Plan: tasks/agent-doc/plan-route-owned-post-commit-pane-reap.md
- 2026-05-11 [#incorrect-close-running-turn] #agent-doc-bug: incorrect close/supersede handling across adjacent managed code sessions makes it unclear whether a prior pane is alive-idle or still running; add live pane evidence to status/closeout/reap decisions and fail closed before clearing busy panes. Plan: tasks/agent-doc/plan-incorrect-close-running-turn-status.md
- 2026-05-11 [#rspcmt7] Current SessionShare root response patchback was written but left uncommitted: `src/session-share/tasks/root.md` contains the BuildParty pause-regression closeout for `2336083`, yet the submodule remains dirty with `M tasks/root.md`. Plan: tasks/agent-doc/plan-session-share-root-response-uncommitted-current.md
- 2026-05-11 [#preflight-started-recovery] #agent-doc-bug: pane reaped during preflight_started cycle — recovery writes dead-end placeholder instead of abandoning the stuck cycle and letting the next preflight handle the unresolved prompt. Plan: tasks/agent-doc/plan-preflight-started-recovery-placeholder.md
- 2026-05-11 [#jbclearbusyux] #agent-doc-bug: JetBrains Clear Session Context surfaces the raw session_clear refused error for protected alive-busy panes; keep the fail-closed guard but add typed IDE UX plus explicit interrupt/wait flows. Plan: tasks/agent-doc/plan-jb-clear-session-busy-pane-ux.md
- 2026-05-12 [#jbclearbusyactions] #agent-doc-bug: JetBrains Clear Session Context protected busy warning still leaves no refresh-and-retry or intentional interrupt-and-clear action, and current tasks/root.md evidence shows live pane idle while supervisor/controller projection remains busy. Plan: tasks/agent-doc/plan-jb-clear-session-busy-actions.md
- 2026-05-12 [#nextstepscapture] #agent-doc-bug: previous turn acknowledged #next-steps but did not add backlog items — skill finalize did not run --pending-add for identified follow-ups. Plan: tasks/agent-doc/plan-next-steps-backlog-capture.md
- 2026-05-12 [#clauderouteproof] [recommended] #next-steps: Add Claude dispatch proof parity or explicit accepted-only semantics; current robert-ross Claude routes record direct_pane_submit proof=accepted without dispatch_start_proof, so operator success is weaker than Codex consumed-proof routing. Plan: add a focused route proof check or document accepted-only as a deliberate harness limit.
- 2026-05-12 [#baseindexrepair] [recommended] #next-steps: Make base-index layout repair deterministic for the active preflight or surface an immediate action; current preflight detected window index 0 missing in session 0, wrote state/base-index-repair.count=1, and deferred repair until the next detection.
- 2026-05-12 [#jbrestartretest] 2026-05-12 [recommended] Restart IntelliJIdea2026.1 to activate locally installed agent-doc JetBrains plugin 0.2.112, then re-test Clear Session Context against a protected alive-busy pane and confirm the typed Refresh/Interrupt/Status/Copy warning appears.
- 2026-05-12 [#4w5x] [recommended] Evaluate extending dispatch-only prompt gating beyond startup-window reroutes after collecting ready-actor accepted-but-unproven Codex route logs; current fix gates startup/latest-run and starting-authoritative actor paths only.
- 2026-05-12 [#tmuxroutesync] #next-steps: Implement the current tmux route/sync plan: typed actor_binding protocol, current-generation ready-prompt barrier before dispatch, safe-passive sync burst coalescing/cache, and deterministic plus real-tmux route/sync coverage. Plan: tasks/agent-doc/plan-current-tmux-route-sync-next-steps.md
- 2026-05-12 [#codexresumegitdir] #agent-doc-bug: Managed Codex writable-root proof is still insufficient for reused/resumed actors. Monster Rod Holders shipping-rule turn could edit `src/boost-client/src/monsterrodholders-dev` but could not update local `.git/modules/...` metadata, so it committed from a temporary clone and left the local checkout dirty. Agent-doc must prove the live actor generation has the exact gitdir/superproject writable roots before dispatch, and fresh-start instead of `codex exec resume` when roots are missing because resume strips `--add-dir`. Plan: tasks/agent-doc/plan-codex-resume-writable-root-contract.md
- 2026-05-12 [#va0j] [recommended] Restart IntelliJIdea2026.1 to activate locally installed agent-doc JetBrains plugin 0.2.113, then re-test Clear Session Context against the stale-busy Codex status-only pane.
- 2026-05-12 [#ipc-timeout-dup] #agent-doc-bug: IPC timeout during finalize causes duplicate response patchback — binary commits to git, plugin independently applies fallback patch, document ends up with two copies of response. Dedupe in write --commit does not cleanly remove duplicate from HEAD. Plan: tasks/agent-doc/plan-ipc-timeout-duplicate-patchback.md
- 2026-05-12 [#codexpatchbodyloop] #next-steps: Re-open Codex response-body recovery for current agent-doc-bugs2 evidence: ops.log still shows prior_patchback_without_response_body, out_of_band_write, and post_commit_local_drift after committed cycles, causing child dispatch/recovery churn. Plan: tasks/agent-doc/plan-empty-response-repair-write.md and tasks/agent-doc/plan-response-patchback-uncommitted.md
- 2026-05-12 [#opencodecapfail] #next-steps: Re-open OpenCode startup/capability gating with current logs: opencode_capability_proof fails by printing opencode help, auto-trigger times out or pty_write fails, and route still direct-submits into busy/starting OpenCode panes with over-budget submits. Plan: tasks/agent-doc/plan-opencode-harness-ssh-capability-proof.md and tasks/agent-doc/plan-route-starting-actor-race-after-restart.md
- 2026-05-12 [#directrunpfx] #agent-doc-bug: Direct `agent-doc <file>` committed the Monster Rod Holders `Read the image.` prompt and Markdown image line without the canonical `❯ ` prefix before the response block, making user prompt text look like assistant prose. Plan: tasks/agent-doc/plan-direct-run-unprefixed-exchange-prompt.md
- 2026-05-13 [#rspcmtdelay] #agent-doc-bug: Session response patchback had a long delay before being committed; current `src/session-share/tasks/root.md` evidence required failed `write --commit`/`finalize`, manual patching, stale preflight repair, and parent pointer closeout before `session-check` passed. Plan: tasks/agent-doc/plan-response-commit-delay.md
- 2026-05-13 [#rspcmt9] #agent-doc-bug: Direct Codex chat answered a Monster Rod Holders session prompt but did not write the response to `src/boost-client/tasks/monsterrodholders.md`; manual `agent-doc write --commit` then committed the submodule response but initially failed the parent pointer closeout on root `.git/index.lock`, requiring `agent-doc commit <file>` recovery. Plan: tasks/agent-doc/plan-direct-chat-response-patchback-required.md
- 2026-05-13 [#rspcmt8] #agent-doc-bug: OpenCode agent-doc response written into exchange section but finalize/closeout did NOT commit the session document. boost-client/tasks/monsterrodholders.md session a9421282, GLM-5.1 agent — the shopcozi mobile CSS fix v0.4.9 response was patched into the exchange section but `tasks/monsterrodholders.md` remained dirty (uncommitted). User had to manually request "commit the document." Same class as #rspcmt2-#rspcmt7. Plan: extend tasks/agent-doc/plan-response-patchback-uncommitted.md with evidence that OpenCode finalize consistently skips the git commit step after patchback.
- 2026-05-13 [#opencodepromptplugins] [recommended] Add editor-plugin prompt-answer coverage for JetBrains and VSCode: verify both prompt UIs pass the selected option index to agent-doc prompt --answer, handle flat agent-doc prompt --all JSON including selected, and include a live OpenCode permission prompt smoke after installing the plugin.
<!-- /agent:done -->
