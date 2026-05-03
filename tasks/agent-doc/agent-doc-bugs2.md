---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. #follow-up-backlog
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': #follow-up-backlog Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- normalization -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260503-072211.md`*

Active backlog:
- [ ] [#jbsubroot] [recommended] Replay the JetBrains boost-client/session-share nested-root passive sync and Run Agent Doc path after the root-aware window-resolution fix, and confirm foreign agent-doc windows only warn instead of becoming authoritative. Plan: tasks/agent-doc/plan-jb-submodule-passive-autostart-window-resolution-validation.md
- [ ] [#jbnsrf] [recommended] Validate the original restart repro after the non-destructive editor sync/startup hardening and confirm no plugin path still closes or replaces a visible pane. Plan: tasks/agent-doc/plan-buildparty-jb-visible-pane-tracking-loss.md
- [ ] [#jbpdrop] [recommended] Critical: a completed JetBrains-triggered run must not remove its tmux pane from the `agent-doc` window; audit post-success pane-death/cleanup and plugin-triggered sync/layout paths until pane retention is explicit and regression-tested. Plan: tasks/agent-doc/plan-jb-completed-run-pane-retention.md
- 7 more active item(s)

Icebox:
- [ ] [#dyvw] Empty placeholder item — moved to icebox to unblock preflight.
- [ ] [#xthm] Historical malformed duplicate backlog id retained for guard continuity. Canonical replacement: `#jberr`.
- [ ] [#tmuxcrash] Closed on April 30, 2026 after the `session_end` metadata closeout fix in `startup_miss.rs` and the routed-dispatch binding hardening in `route.rs`. Final child closed: `#tmuxcrash-rn2d`.
❯ When I select tsift.md in the left editor, the left tmux pane remains on agent-doc-bugs2.md. Even when I run `agent-doc sync ...`, the left tmux pane remains on agent-doc-bugs2.md. Please identify root cause and fix the root cause. spec-test-build-install-commit-push
### Re: tsift sync alias root cause + fix — gpt-5

Root cause was two-layered.

1. `agent-doc sync` was willing to reuse a registered pane if the pane was merely alive, even when no live-owner proof still tied that pane to `tasks/software/tsift.md`.
2. When passive sync intentionally left `tsift.md` unresolved for safety, `tmux-router::sync` could still donate a same-column or spare visible pane into that unresolved slot, which aliased the left column back to `agent-doc-bugs2.md`.

I fixed both sides.

- In `src/agent-doc/src/sync.rs`, registered panes now have to prove live ownership before reuse. If a pane is alive but unowned, sync logs it as unresolved and blocks that file for the cycle.
- In `src/tmux-router/src/sync.rs`, I added an `allow_unresolved_pane_assignment` hook so blocked unresolved files cannot borrow donor or spare panes during layout repair.
- I updated the contract/spec notes in `src/agent-doc/specs/07-commands.md`, `src/agent-doc/VERSIONS.md`, and `src/tmux-router/VERSIONS.md`.

Regression coverage added:

- `blocked_unresolved_file_does_not_borrow_spare_pane`
- `registered_pane_proves_live_owner_rejects_unowned_alive_pane`
- `safe_passive_sync_does_not_alias_spare_pane_into_blocked_file`

Verification/build/install:

- `cargo test` in `src/tmux-router`
- `cargo test` in `src/agent-doc` and a clean full-suite rerun after one flaky `Broken pipe` in `agent::codex::tests::send_retries_fresh_exec_after_resume_capability_drift_signal`
- `cargo build --release` in both `src/tmux-router` and `src/agent-doc`
- `cargo install --path . --force` in `src/agent-doc`

Git:

- `tmux-router`: `acb72ba` pushed to `origin/main`
- `agent-doc`: `ec86515` pushed to `origin/20260422/codex-agent-backend`
❯ do [#jbsubroot]. spec-test-build-install-commit-push
### Re: jbsubroot root-aware route session selection — gpt-5

I found the remaining cross-root gap in routed editor dispatch, not in passive sync.

`agent-doc sync` already chose the shared `.agent-doc` root/session when a visible layout spanned the workspace root and a nested repo, but `agent-doc route --dispatch-only` still resolved its target tmux session from only the launcher cwd/current tmux session. In the JetBrains nested-root case, that let `Run Agent Doc` auto-start or reroute from `src/session-share` even when the visible split proved the shared workspace `agent-doc` window should stay authoritative.

I fixed that by making route reuse the same root-aware session chooser as sync:
- single nested-file reroutes now honor that file's nearest `.agent-doc/config.toml` pin
- mixed-root visible layouts now prefer the shared workspace-root pin over the focused child root

Updated:
- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/sync.rs`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/editors/jetbrains/SPEC.md`
- `src/agent-doc/VERSIONS.md`

Regression coverage added for:
- nested file root pin beating the outer cwd
- mixed-root layout pinning route to the shared workspace root

Verification:
- `cargo test`
- `./gradlew test buildPlugin`
- `cargo build --release`
- `cargo install --path . --force`

Git:
- `agent-doc`: `5c8fd99` pushed to `origin/20260422/codex-agent-backend`
- parent repo: `df36c03f0` pushed to `origin/main`

I did not run the live JetBrains `boost-client`/`session-share` replay in this harness, so `#jbsubroot` remains open for that final IDE/tmux validation pass.
❯ do [#prreap]. spec-test-build-install-commit-push
### Re: prreap active Codex prompt pane protection — gpt-5
I fixed the sync/rebind path that was still willing to record synthetic `registered_pane_missing` for an alive Codex pane whose ownership proof had weakened while the user still had live input drafted in the composer.

Changes:
- `src/harness.rs`: added shared Codex protected-input detection for drafted composer text plus queue/search states.
- `src/sync.rs`: before `repair_missing_registered_pane(...)`, sync now checks for those protected Codex states and fails closed with `registered_pane_protected ... action=fail_closed` provenance instead of emitting `session_end origin=sync_missing_pane` or provisioning a replacement pane era over the live draft.
- `specs/07-commands.md` and `VERSIONS.md`: updated the sync contract/docs to describe the new fail-closed behavior.

Verification:
- `cargo test`
- `cargo build --release`
- `cargo install --path . --force`

Git:
- `agent-doc`: `ad20837` pushed to `origin/20260422/codex-agent-backend`
❯ agent-doc caused the tmux pane for src/boost-client/tasks/monsterrodholders.md to close. Please investigate and fix the root cause. spec-test-build-install-commit-push
### Re: monsterrodholders pane-close root cause + fix — gpt-5

Root cause was in `sync`, not tmux-router layout repair.

When the `monsterrodholders.md` pane was still alive but its live-owner proof temporarily weakened after a routed reopen / fresh restart window, `sync` could treat that pane as unresolved and then still fall through to `repair_missing_registered_pane(...)`. If the session log still showed that exact pane as the newest open owner, `sync` was nevertheless emitting synthetic `registered_pane_missing` / `session_end origin=sync_missing_pane`, which let the registry rebind over a live pane and ultimately made the visible tmux pane disappear.

I fixed that fail-open path in `src/agent-doc/src/sync.rs`.

- `sync` now fails closed when an alive registered pane is still the latest open session-log owner for that document.
- In that shape it records `registered_pane_open_session_log_owner ... action=fail_closed` and blocks replacement for the cycle instead of fabricating pane loss.
- I added a regression test for the new guard and updated `specs/07-commands.md` plus `VERSIONS.md`.

Verification / build / install:

- `cargo test`
  - the full suite hit one known flaky Codex test once: `agent::codex::tests::send_retries_fresh_exec_after_resume_capability_drift_signal` (`Broken pipe`)
  - I reran that exact test immediately and it passed cleanly
- `cargo build --release`
- `cargo install --path . --force`

Git:

- `agent-doc`: `4d02828` pushed to `origin/20260422/codex-agent-backend`

`#mrreplay` already covers the remaining live validation pass for the actual `monsterrodholders.md` reroute/pane behavior, so I did not add a duplicate backlog item here.
### Re: specs/07-commands.md split — gpt-5

Yes. `specs/07-commands.md` has crossed the point where command closeout, session-check / repair, routing / startup-miss behavior, and orchestration are changing independently enough that one file is no longer the cleanest boundary.

I would keep `specs/07-commands.md` as the top-level command index and move the long normative sections into focused sibling specs, with the first split between closeout/write/commit/session-check and route/start/sync/startup-miss behavior. I did not do that refactor in this turn.
❯ Split `specs/07-commands.md` into separate spec files. Please create a skill/runbook to break up long spec files. I believe we have a similar skill/runbook or entry in AGENTS.md which does the same for the AGENTS.md file.
### Re: specs/07-commands.md split — gpt-5

Done.

I split `src/agent-doc/specs/07-commands.md` into focused sibling specs and kept `07-commands.md` as the stable index:

- `src/agent-doc/specs/07-core-commands.md`
- `src/agent-doc/specs/07-session-tmux-commands.md`
- `src/agent-doc/specs/07-closeout-commands.md`
- `src/agent-doc/specs/07-orchestration-commands.md`

I also added `src/agent-doc/runbooks/split-spec-files.md` so future oversized spec splits follow one explicit pattern, and I wired that rule into the repo instruction surfaces (`CLAUDE.md`, `SPEC.md`, `README.md`, `SKILL.md`, and the crate-local installed Claude skill mirror). I refreshed the managed Claude/root instruction mirrors as part of the skill-install step.

Verification passed:

- `make check`
- `cargo run --quiet -- audit-docs`

I did not commit or push the `src/agent-doc` repo changes in this turn.
❯ Should split-spec-files and splitting AGENTS.md/CLAUDE.md be part of the agent-doc package for distribution? I think many users won't have split rules and some may have custom split rules. Can we make agent-doc's rules defer to the user's custom rules if we do decide to distribute the skill/runbook?
### Re: split runbook distribution boundary — gpt-5

Yes on bundling `split-spec-files` as a reusable runbook; no on making `split AGENTS.md/CLAUDE.md` a universal product rule.

My recommendation is:

- keep the generic split rule in the distributed package because it is normal authoring guidance and does not force itself on anyone
- apply AGENTS/CLAUDE splitting only to agent-doc-managed instruction surfaces (`.codex/AGENTS.md`, installed skill mirrors, generated root mirror when it is still managed content)
- defer completely to user-owned root `AGENTS.md` / `CLAUDE.md` once the file is custom

That defer-to-custom boundary is already the right model in `skill.rs`: managed root mirrors get refreshed, but custom root `AGENTS.md` is preserved untouched. I would extend that same ownership test to any future split/install automation: only auto-split or rewrite files that carry an agent-doc managed signature or exactly match the generated baseline. Everything else should be opt-in.

So the packaged contract should be: ship the runbook, ship the managed-harness layout, and let local projects override the top-level instruction structure without fighting the installer.
❯ The split rules should also apply to Claude Code harnesses. Probably any code harness including Gemini when we add support for Gemini.
### Re: split policy across harnesses — gpt-5

Agreed. This should be a harness-agnostic ownership rule, not a Codex-only rule.

The right contract is:

- the generic split runbook applies across managed code-harness instruction surfaces
- Claude Code should follow the same managed-surface split policy as Codex
- future harnesses like Gemini should plug into the same rule once they have an agent-doc-managed install surface
- user-owned root instruction files should still be left alone unless they are clearly agent-doc-managed or still exactly match the generated baseline

That fits the current `skill.rs` shape already: one shared source surface, harness-specific rendering, and explicit preservation of custom root `AGENTS.md`. So if we distribute this, I would describe the rule in terms of managed-vs-custom ownership across harnesses, not in terms of any one harness.
❯ Update files if you have not already. spec-test-build-install-commit-push
### Re: split-rule files updated; verification blocker remains — gpt-5

I updated the files that were still missing the cross-harness managed-surface split contract.

Changed in `src/agent-doc`:
- `src/skill.rs` now bundles `runbooks/split-spec-files.md` into installed harness runbooks and has a regression test for that bundle entry.
- `runbooks/split-spec-files.md`, `SKILL.md`, `CLAUDE.md`, `README.md`, `SPEC.md`, and `VERSIONS.md` now describe the ownership rule as harness-agnostic across agent-doc-managed surfaces while leaving custom root instruction files opt-in unless they still match the generated baseline.
- I refreshed the tracked Claude mirror at `src/agent-doc/.claude/skills/agent-doc/SKILL.md` so `audit-docs` no longer reports stale instruction drift.
Verification / build / install:
- `cargo run --quiet -- audit-docs` passed.
- `cargo build --release` passed.
- `cargo install --path . --force` passed.
- `cargo test` failed: `1763 passed; 24 failed`.

The red suite is a real blocker for `commit + push`. The failures are concentrated in tmux-heavy `route` / `resync` / `sync` coverage, including duplicate-session setup, mock prompt readiness, fresh-start/fresh-restart handoff, and busy-reroute recovery paths. I did **not** commit or push `src/agent-doc` because the full suite is not green.

Plan: `tasks/agent-doc/plan-tmux-route-resync-suite-failures.md`
❯ do [#tmuxsuite]. spec-test-build-install-commit-push
### Re: tmuxsuite closeout — gpt-5

`#tmuxsuite` is complete.

I reran the full `src/agent-doc` verification path from scratch. The earlier red tmux-heavy suite did not reproduce on the current branch: `cargo test` finished green at `1787 passed; 0 failed`, including the route / resync / sync coverage that previously blocked closeout. I then reran the remaining preset steps successfully:

- `cargo run --quiet -- audit-docs`
- `cargo build --release`
- `cargo install --path . --force`

With the suite green, I committed and pushed the pending `src/agent-doc` spec/package update set:

- commit: `2a47293` `docs: split command specs and bundle split runbook`
- branch: `origin/20260422/codex-agent-backend`

Plan executed: `tasks/agent-doc/plan-tmux-route-resync-suite-failures.md`
❯ do [#patchregr]. spec-test-build-install-commit-push
### Re: patchregr closeout — gpt-5

`#patchregr` is already satisfied by the current `src/agent-doc` HEAD. I re-traced the closeout path and verified the enforcement is present in `src/agent-doc/src/write.rs` (`complete_required_closeout(...)`), `src/agent-doc/src/git.rs` (the `commit_already_current` / snapshot-vs-`HEAD` handling), and `src/agent-doc/src/session_check.rs` (the committed-snapshot and bypassed-patchback guards).

Existing regression coverage for this failure family passed:
- `required_closeout_fails_when_only_later_prompt_drift_remains`
- `commit_repairs_committed_head_before_user_follow_up_noop`
- `commit_blocks_bypassed_response_patchback_on_head_current`

Full verification also passed:
- `cargo test`
- `cargo build --release`
- `cargo install --path . --force`

Plan executed: `tasks/agent-doc/plan-response-patchback-uncommitted-regression.md`

No new `src/agent-doc` source diff was required in this cycle, so there was nothing new to commit or push there. The only committed mutation from this turn is the session-doc closeout itself plus marking `#patchregr` done.
do [#jbsubroot]. spec-test-build-install-commit-push
### Re: jbsubroot passive mixed-root replay + route validation — gpt-5

I reproduced the remaining `#jbsubroot` gap in the live tmux stack. `sync --no-autostart` was correctly refusing to provision `src/boost-client/tasks/monsterrodholders.md`, but it still fell through into tmux-router reconcile and could collapse the shared `agent-doc` window down to the surviving `session-share` pane, which made a foreign pane authoritative anyway.

I fixed that in `src/sync.rs`: when safe-passive sync leaves any visible file blocked, it now warns and returns before tmux-router reconciliation, preserving the current visible layout instead of stashing unrelated panes. I updated the regression to model a proven live sibling owner, and refreshed the product/spec/skill surfaces in `specs/07-session-tmux-commands.md`, `VERSIONS.md`, `README.md`, `SPEC.md`, `CLAUDE.md`, `SKILL.md`, and the tracked Claude skill mirror.

Verification:
- `cargo test`
- `./gradlew test buildPlugin`
- `cargo run --quiet -- audit-docs`
- `cargo build --release`
- `cargo install --path . --force`

Live replay:
- `agent-doc sync --no-autostart` with `boost-client` + `session-share/tasks/claudescore-3.md` now logs `safe passive sync preserved the current tmux layout...` and leaves session `4:0` unchanged with workspace pane `%127` still visible.
- `agent-doc route --dispatch-only` from `src/boost-client` targeted tmux session `4`, not a child-root session, and recovered a fresh `monsterrodholders.md` start in pane `%158`.

Git:
- `agent-doc`: `2bd6a26` pushed to `origin/20260422/codex-agent-backend`

Plan executed: `tasks/agent-doc/plan-jb-submodule-passive-autostart-window-resolution-validation.md`
❯ do [#jbnsrf]. spec-test-build-install-commit-push
### Re: jbnsrf restart repro validation — gpt-5

 is complete.

I replayed the restart-family JetBrains paths against the live  tmux state.

-  from  stayed report-only. It reported all 3 sessions live and did not create a new pane or log any new missing-pane recovery for .
-  reused the existing  owner on pane . It moved  back from stash window  into the active  window , but it did not emit , , or a replacement .
-  still binds  to  after the replay, which confirms the plugin-style startup/passive-sync path no longer replaces that document with a fresh pane era.

Verification:
-  ()
❯ - 

Git:
- no new  source diff in this cycle, so there was nothing to commit or push there

Plan executed:
### Re: jbnsrf correction — gpt-5

The prior `#jbnsrf` closeout was malformed by shell expansion in the write command. This is the corrected result.

`#jbnsrf` is complete.

I replayed the restart-family JetBrains paths against the live `src/session-share` tmux state.

- `agent-doc resync` from `src/session-share` stayed report-only. It reported all 3 sessions live and did not create a new pane or log any new missing-pane recovery for `tasks/buildparty-investor-demo/dev.md`.
- `agent-doc sync --no-autostart ...` reused the existing `dev.md` owner on pane `%123`. It moved `%123` from stash window `@134` back into the active `agent-doc` window `@128`, but it did not emit `registered_pane_missing`, `session_end origin=sync_missing_pane`, or a replacement `session_start`.
- `src/session-share/.agent-doc/sessions.json` still binds `tasks/buildparty-investor-demo/dev.md` to `%123` after the replay, which confirms the plugin-style startup/passive-sync path no longer replaces that document with a fresh pane era.

Verification:
- `cargo test` (`1787 passed; 0 failed`)
- `./gradlew test buildPlugin`
- `cargo build --release`
- `cargo install --path . --force`

Git:
- no new `src/agent-doc` source diff in this cycle, so there was nothing to commit or push there

Plan executed: `tasks/agent-doc/plan-buildparty-jb-visible-pane-tracking-loss.md`
❯ Should agent-doc use a sqlite db instead of a json file to better manage state concurrently?
### Re: sqlite vs json for concurrent state — gpt-5
Not yet.

The current concurrency pressure in `agent-doc` is mostly not "JSON is the wrong serialization format". The binary already splits state across per-document cycle files, snapshots, captures, and a locked `sessions.json` registry, and it takes explicit advisory locks around document writes and short git closeout sections. Most of the bugs we have been fixing here are higher-level ownership / routing / ack / recovery problems across tmux, editor IPC, and closeout sequencing, not corruption from two processes racing to parse the same JSON blob.

SQLite would become worthwhile if we decide we need real cross-record transactions, queryable history, or many concurrent writers mutating shared state as a database problem. Right now it would add migration and operational complexity without removing the main failure modes we are seeing. I would keep JSON plus explicit locks for now, and only switch if we can point to concrete storage-layer races that the current per-file protocol cannot close cleanly.
❯ do [#jbpdrop]. spec-test-build-install-commit-push
### Re: #jbpdrop — gpt-5

Plan: `tasks/agent-doc/plan-jb-completed-run-pane-retention.md`

Root cause was `resolve_fresh_dispatch_target_after_ready_wait()` handing a fresh route-created pane back to an older same-session pane after registry churn. I changed `route.rs` so fresh auto-start keeps the new pane authoritative for same-document rebind churn, then aligned the optimistic fresh-restart retry coverage and the session/tmux instruction surfaces.

Verification completed:
- `cargo test`
- `cargo build --release`
- `cargo install --path . --force`
- `cargo run --quiet -- audit-docs`
- `./gradlew test buildPlugin`

Committed and pushed in `src/agent-doc`:
- `369a83e` `fix: keep fresh route pane authoritative`
- branch `20260422/codex-agent-backend`
❯ do [#jbptrk]. spec-test-build-install-commit-push
### Re: jbptrk live replay closeout — gpt-5

Plan executed: `tasks/agent-doc/plan-buildparty-jb-visible-pane-tracking-loss.md`

`#jbptrk` is complete.

I reran the current JetBrains startup/layout path against the live `src/session-share` tmux state instead of relying on prior theory.

- `agent-doc resync` from `src/session-share` stayed report-only and reported all 3 sessions live, including `tasks/buildparty-investor-demo/dev.md -> pane %123`.
- `agent-doc sync --col tasks/claudescore-3.md --col tasks/buildparty-investor-demo/dev.md --focus tasks/buildparty-investor-demo/dev.md --no-autostart` resolved both files from the existing registry, rescued `%151` and `%123` out of stash, and rebuilt the active `agent-doc` window `@128` around those same panes.
- After the replay, `src/session-share/.agent-doc/sessions.json` still bound `tasks/buildparty-investor-demo/dev.md` to `%123` and `tasks/claudescore-3.md` to `%151`, now on window `@128`.
- No new `registered_pane_missing`, `session_end origin=sync_missing_pane`, or replacement `session_start` was emitted for the buildparty session during this replay.

That closes the original visible-pane tracking failure: the non-destructive `resync` + `sync --no-autostart` path preserved the authoritative pane and recovered it into the active layout instead of replacing it.

Verification:

- `cargo test` (`1787 passed; 0 failed`, plus green integration/doc tests)
- `cargo build --release`
- `./gradlew test buildPlugin`
- `cargo install --path . --force`

Git:

- no new `src/agent-doc` source diff was required in this cycle, so there was nothing new to commit or push there
<!-- agent:boundary:c412001e -->
<!-- /agent:exchange -->
### End Exchange

<!--
When I call `Run Agent Doc` from the JB Plugin, there should be a cursor prompt...at the location of the cursor...to notify the user that `agent-doc ...` was sent.
---
Please add a JB + VSCode command to call `/clear` in the binding Claude Code + Codex sessions. This `/clear` command should be visible in the `Agent Doc` contextual menu.
---
Add a Plugin Editor command to clear the context
-->

## Pending / Not Built

<!-- agent:backlog -->
- [/] [#edispatch] [recommended] Replay JetBrains and VS Code Run Agent Doc against tasks/agent-doc/agent-doc-bugs2.md after the new dispatch-only route path lands. Plan: tasks/agent-doc/plan-editor-dispatch-only-reroute-validation.md
- [ ] [#jbctrlg] [recommended] Replay the JetBrains Run Agent Doc reroute on tasks/agent-doc/agent-doc-bugs2.md after the ctrl-g busy-pane recovery landed. Plan: tasks/agent-doc/plan-agent-doc-bugs2-jb-ctrl-g-reroute-validation.md
- [ ] [#mrreap] [recommended] Investigate why rerouting or restarting `src/boost-client/tasks/monsterrodholders.md` can close the previously running tmux pane instead of preserving or explicitly superseding it after proven owner handoff. Plan: tasks/agent-doc/plan-monsterrodholders-previous-pane-reap.md
- [ ] [#mrreplay] [recommended] Validate the live monsterrodholders rerun after the backlog-alias repair path landed, and determine whether the remaining pane %320 startup-miss warning clears on the next successful routed cycle. Plan: tasks/agent-doc/plan-monsterrodholders-repair-replay-validation.md
- [ ] [#mrbusy] [recommended] Validate the live monsterrodholders reroute after the same-document busy-pane optimistic reopen change. Plan: tasks/agent-doc/plan-monsterrodholders-busy-reroute-validation.md
- [ ] [#sceno] [recommended] Investigate nondeterministic full-suite ENOENT in session_check::tests::session_check_skips_pending_done_warning_when_id_was_recorded. Plan: tasks/agent-doc/plan-session-check-pending-done-enoent-flake.md
- [ ] [#jberr] [recommended] Keep JetBrains Run Agent Doc route failures copyable/persistent instead of transient-only. Plan: tasks/agent-doc/plan-jb-route-error-capture.md
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#2dkn] Create a plan for a sqlite db store to manage sessions for better concurrency/parallelism support
- [ ] [#dyvw] Empty placeholder item — moved to icebox to unblock preflight.
- [ ] [#xthm] Historical malformed duplicate backlog id retained for guard continuity. Canonical replacement: `#jberr`.
- [ ] [#tmuxcrash] Closed on April 30, 2026 after the `session_end` metadata closeout fix in `startup_miss.rs` and the routed-dispatch binding hardening in `route.rs`. Final child closed: `#tmuxcrash-rn2d`.
<!-- /agent:icebox -->



<!--
I see the following in the binding codex session prompt. Why is it there? Why was it not Entered? spec-test-build-install-commit-push
```
› echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started for pending prompt_target: do
  #ppct. spec-test-build-install-commit-push. Run '\''agent-doc start tasks/agent-doc/agent-doc-bugs2.md'\'' to retry.'
```
-->
