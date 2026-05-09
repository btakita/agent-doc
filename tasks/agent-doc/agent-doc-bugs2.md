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

Added Codex manifest warning cleanup to the backlog.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260509-161931.md`*

Active backlog:
- [ ] [#crossdocpend] Cross-document --pending-add targets wrong file — skill adds to current document backlog instead of preset-specified target file. Plan: tasks/agent-doc/plan-cross-doc-pending-add.md
- [ ] [#iceboxpatch] Icebox component mutation has no binary-owned path — patch:icebox / replace:icebox in response heredoc not parsed as component patches, --pending-* flags only target agent:backlog. Skill falls back to boundary synthesis, dumping icebox content into exchange body. Needs either --icebox-add flag or template patch parsing for non-backlog components.
- [ ] [#spfxnorm] Sidecar ❯ prefix normalization loss — content_ours fallback recovers snapshot but not working tree. Plan: tasks/agent-doc/plan-sidecar-prefix-normalization-loss.md
- 7 more active item(s)
❯ When I switch the editor to a different file on the left or right editor pane, a 1 tmux pane view on the agent-doc tmux window shows...then after a few seconds the correct 2 tmux pane view shows. The 2 tmux pane view should be shown with only the tmux pane on the same side of the editor document switch.
❯ spec-test-build-install-commit-push
### Re: focus-only editor sync split layout — gpt-5

Fixed the transient one-pane tmux collapse on editor tab switches.

The safe-passive sync path now treats a focus-only editor update as a same-side tab switch when the last recorded layout had multiple columns. It captures the active tmux column before any actor focus handoff, expands the one-file projection back through the remembered sibling layout, and replaces only that active side before tmux-router reconciliation runs.

Updated specs in `src/agent-doc/SPEC.md` and `src/agent-doc/editors/SPEC.md`, and added regressions in `src/agent-doc/src/sync.rs` for both the pure column expansion and a real tmux-backed two-pane handoff.

Verification completed:
- `cargo test safe_passive_focus_only`
- `make check`
- `make build-release install`
- `agent-doc --version` -> `agent-doc 0.33.16`

Commits pushed:
- `src/agent-doc`: `69bc440 fix: preserve split layout on focus-only sync`
- outer `agent-loop`: `0c3e837ca chore: update agent-doc submodule`
❯ The same 1 tmux pane transition is still occurring. Does the agent-doc controller need to restart?
### Re: controller restart for one-pane transition — gpt-5

The active controller did not look stale: before restart it reported `/home/brian/.cargo/bin/agent-doc`, version `0.33.16`, and a binary timestamp from the local install.

I restarted it anyway through the binary-owned path:

- `agent-doc controller shutdown` returned `{"ok":true}`
- `agent-doc controller status --ensure` relaunched PID `1992202`
- the relaunched controller still reports `agent-doc 0.33.16`

One suspicious thing did show up: there was also an older same-project controller process still alive from 01:33 (`PID 4085114`) even after the active controller shutdown. The active socket/status now points at the new controller, so that old process is probably not the reason the current sync path is still collapsing, but it is a real cleanup/singleton bug signal. I added a recommended backlog item for that.

Given the active controller already matched the installed binary, a restart alone was unlikely to be the full fix. If the one-pane transition still happens after this restart, the remaining likely cause is that the editor is still sending a full one-column `sync --no-autostart` event, or the plugin/native library path is not using the newly installed library, rather than the controller holding old sync logic.
❯ The agent-doc binary logic should ensure there is only a single controller processes to manage requests is present. The old process must either gracefully shutdown or be externally killed by the new controller process. The transition from old to new controller process must have 0 downtime...meaning in a live environment, the session actors and supervisors should continue to work seamlessly.
❯ Please plan #next-steps.
### Re: controller singleton next steps — gpt-5

Planned this as a focused controller-handoff fix rather than folding it into the broader session-actor MVP.

Created and committed `tasks/agent-doc/plan-controller-singleton-zero-downtime-handoff.md`. The plan tightens the invariant from "restart stale controller" to "one promoted authoritative controller generation per project root," with a zero-downtime handoff path:

- persist controller generation and handoff state separately from the socket path
- prepare the old controller, launch the replacement on a temporary socket, import durable actor/supervisor state, then promote
- keep the old controller authoritative if replacement fails before promotion
- retire the old PID after promotion, and force-reap only verified same-project duplicate controllers after a grace period
- make supervisors reconnect by controller generation while preserving leases and actor records through SQLite

I updated existing backlog item `#g4w2` to point at that plan instead of adding a duplicate item. `#g4w2` stays open because this cycle only planned the work; it did not implement the controller handoff.

No code tests were run because this cycle only created a plan document and updated the session backlog.
<!-- agent:boundary:ce001260 -->
<!-- /agent:exchange -->
###

<!--
/home/brian/work/btakita/agent-loop/tasks/agent-doc/plan-project-controller-session-actor-mvp.md
---
`❯ The current tree has already started making this accountable...` had the `❯ ` prefix stripped during the patchback. This issue keeps on reoccurring. Is the logic too brittle? Is the selection criteria opt out? If yes, can we make the criteria opt in instead?
spec-test-build-install-commit-push
---
Comments after a run starts are deleted by agent-doc.
---
"❯ Commit / push:" should not have the `❯ ` prefix since this was the agent response. #agent-doc-bug
---
Please optimize the AGENTS.md + SKILL.md to reduce context usage yet maintain effectiveness for Claude Code + Codex
---
Does the supervisor actor ever block on `Run Agent Doc`? I noticed that /home/brian/work/btakita/agent-loop/tasks/software/tsift did not respond to `Run Agent Doc` in a previous turn. It seems to run ok now but I'm concerned that `Run Agent Doc` was non-responsive.
---
"Press Enter to restart, or 'q' to exit."
-->

<!-- agent:queue -->
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#g4w2] Controller singleton/handoff must allow only one authoritative same-project controller while replacing stale binaries with zero downtime; retire or reap duplicate controller processes only after a promoted replacement preserves active session actors and supervisor leases. Plan: tasks/agent-doc/plan-controller-singleton-zero-downtime-handoff.md
- [ ] [#crossdocpend] Cross-document --pending-add targets wrong file — skill adds to current document backlog instead of preset-specified target file. Plan: tasks/agent-doc/plan-cross-doc-pending-add.md
- [ ] [#iceboxpatch] Icebox component mutation has no binary-owned path — patch:icebox / replace:icebox in response heredoc not parsed as component patches, --pending-* flags only target agent:backlog. Skill falls back to boundary synthesis, dumping icebox content into exchange body. Needs either --icebox-add flag or template patch parsing for non-backlog components.
- [ ] [#spfxnorm] Sidecar ❯ prefix normalization loss — content_ours fallback recovers snapshot but not working tree. Plan: tasks/agent-doc/plan-sidecar-prefix-normalization-loss.md
- [ ] [#oobpmt] OOB prompt absorption — preflight commit step absorbs user-content changes (new prompts in status) before diff runs, causing `no_changes: true`. Fix: use pre-commit snapshot as diff baseline, not post-commit snapshot.
- [ ] [#staleflt] Stale preflight_started cycle recovery — auto-close empty preflight cycles (no capture, phase=PreflightStarted) on next preflight invocation instead of failing closed. Add time-based staleness check.
- [ ] [#busychk] Pane busy-check before sync eviction — check for open agent-doc cycles before killing panes during layout reconciliation. Skip eviction if busy, log warning.
- [ ] [#chkptcap] Periodic checkpoint capture during response streaming — capture partial responses every 30s so mid-generation pane death doesn't lose work entirely.
- [ ] [#257p] OAuth flow interrupts finalize — skill fails to persist response when an MCP OAuth flow interrupts the cycle. The authenticate tool call consumes the turn and the skill never reaches `agent-doc finalize`. Fix: treat MCP auth as a sub-step and still finalize after, or detect response-without-finalize and warn/recover.
- [ ] [#pfxleak2] content_ours prefix leak + commit churn — plan: tasks/agent-doc/plan-content-ours-prefix-and-commit-churn.md
- [ ] [#bppfxstrip2] content_ours strips ❯ from multi-line user prompts during finalize — plan: tasks/agent-doc/plan-prefix-strip-on-user-prompts.md
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:pending-done -->
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
<!-- /agent:pending-done -->



<!--
I see the following in the binding codex session prompt. Why is it there? Why was it not Entered? spec-test-build-install-commit-push
```
› echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started for pending prompt_target: do
  #ppct. spec-test-build-install-commit-push. Run '\''agent-doc start tasks/agent-doc/agent-doc-bugs2.md'\'' to retry.'
```
-->
