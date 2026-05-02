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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260502-220619.md`*

Active backlog:
- [ ] [#dup-exchange] [recommended] Fail closed or auto-repair when a template/CRDT session document gains a second `<!-- agent:exchange ... -->` opener instead of appending inside the canonical exchange component. Repro: `src/session-share/tasks/buildparty-investor-demo/dev.md` ended up with two exchange blocks after a direct Codex patch/write cycle. Plan: verify whether this is a write-path structural-guard miss, a repair-path miss, or a direct-manual-edit escape hatch that should still be normalized.
- [/] [#jbxroot] [recommended] Replay the JetBrains cross-root split with tasks/agent-doc/agent-doc-bugs2.md and src/boost-client/tasks/monsterrodholders.md after the absolute cross-root layout reporting fix, and confirm passive sync no longer collapses to one pane or triggers stale-session cleanup. Plan: tasks/agent-doc/plan-jetbrains-cross-root-split-sync-validation.md
- [ ] [#jbmcol] [recommended] Replay JetBrains mixed-split Sync Layout on src/boost-client/tasks/monsterrodholders.md and confirm empty-column column-memory restoration keeps two tmux panes with monsterrodholders.md on the right and in focus. Plan: tasks/agent-doc/plan-monsterrodholders-empty-column-sync-replay.md
- 10 more active item(s)

Icebox:
- [ ] [#xthm] Historical malformed duplicate backlog id retained for guard continuity. Canonical replacement: `#jberr`.
- [ ] [#tmuxcrash] Closed on April 30, 2026 after the `session_end` metadata closeout fix in `startup_miss.rs` and the routed-dispatch binding hardening in `route.rs`. Final child closed: `#tmuxcrash-rn2d`.
❯ Why is agent-doc losing track of the currently running tmux panes that are running agent-doc on monsterrodholders.md? This has happened whenever I switched the editor to monsterrodholders.md. CRITICAL BUG
### Re: tmux pane tracking loss on monsterrodholders — gpt-5

Short answer: this does not look like tmux randomly "forgetting" a healthy pane. The persisted evidence shows `agent-doc` repeatedly rebinding the `monsterrodholders.md` session to fresh panes during editor-driven sync/reopen paths.

The clearest root cause is in the JetBrains tab-selection path. The plugin spec says a selection change should prefer `agent-doc focus <file>`, and any automatic layout sync should use `agent-doc sync --no-autostart ...`. But `EditorTabSyncListener.selectionChanged` currently does the opposite: it always runs `agent-doc sync` on every markdown tab switch, passes `noAutostart = false`, and anchors the command to the focused file's nearest root instead of the shared `chooseSyncProjectRoot(...)` workspace-root logic that manual sync already uses.

That is exactly the wrong combination for your repro. When you switch focus to `src/boost-client/tasks/monsterrodholders.md` while `tasks/agent-doc/agent-doc-bugs2.md` is still part of the visible split, the automatic sync path is still allowed to provision/rebind panes from the boost-client root instead of just refocusing the existing owner or doing a report-only layout reconcile.

❯ This is papering over the real issue. The real issue is why is agent-doc losing track over the tmux pane that is clearly running. I can briefly see it running. But it then gets replaced with a new pane. The option to autostart a pan is not the real problem. The real problem is the lack of detection on the already running pane.

The session log for `src/boost-client/tasks/monsterrodholders.md` matches that explanation. In `src/boost-client/.agent-doc/logs/a9421282-fd96-4943-9af5-3561ed5cb799.log`, the failure family is dominated by `session_superseded` plus `session_end origin=registry_rebind` transitions like `%529 -> %533 -> %536 -> %540`, not by child-crash provenance. Later, the same log records sync-driven `supervisor_exit code=missing_pane` on `%71`, followed immediately by fresh starts on `%72` and `%73`. So the system is churning ownership eras; it is not stably preserving and reusing the existing pane binding.

Plan: `tasks/agent-doc/plan-jb-selection-sync-non-destructive.md`.

I added backlog item `#jbselsync` for the concrete fix: make tab-selection handling non-destructive, split focus changes from layout changes, and reuse the workspace-root chooser on automatic sync so cross-root `monsterrodholders.md` focus changes cannot replace the live tmux pane.
### Re: visible pane detection is the real bug — gpt-5

Yes. The JetBrains tab-switch behavior is a trigger surface, but it is not the deepest defect. The deeper defect is that `agent-doc` can see a pane that is still alive in tmux, fail to prove it owns the document, and then legally replace it.

The proof gap is in the current ownership model in `src/agent-doc/src/sync.rs`. `find_live_owner_pane...` only accepts three proof paths: path provenance, process-tree file match, or supervisor-pid fallback. Then `find_associated_panes()` filters candidates down to panes with `ProcessTree` or `SupervisorPid` proof only. A merely visible/registered pane is not enough. On top of that, the process-tree path is narrow: `find_alive_pane_for_file_inner()` and `collect_process_tree_matches()` only look for `agent-doc` plus the file path in the pane pid or its direct children, and `pid_has_agent_doc_for_file()` depends on the live command line still advertising that file path.

So if the pane is still visibly alive, but the file-path-bearing argv has disappeared from that narrow process-tree shape, and the supervisor socket is unavailable or stale at the same moment, the pane stops proving ownership even though it has not actually disappeared. At that point sync falls through to `repair_missing_registered_pane(...)`, records synthetic `registered_pane_missing`, and replacement becomes allowed. That matches your repro much better than "autostart was enabled."

So the corrected framing is:

- JetBrains selection sync is one way to trigger the replacement path.
- The root bug is that visible-pane existence is weaker than the current owner-proof requirements.
- When those proofs fail transiently, `agent-doc` rewrites a still-real pane as missing instead of failing closed or preserving the last open owner.

I created a dedicated plan for that actual bug: `tasks/agent-doc/plan-visible-pane-owner-proof-gap.md`.

I also added backlog item `#ownergap` and left `#jbselsync` in place as the mitigation/trigger-surface follow-up. The new top-priority issue is the owner-proof gap itself.
❯ do [#ownergap]. We need a "fail-safe" owner detection. Erroring on making the user manually close the tmux pane is not ideal but acceptable. Erroring on automatically closing a running tmux pane is never acceptable. spec-test-build-install-commit-push
### Re: #ownergap fail-safe owner detection — gpt-5

Implemented the fail-safe live-owner path in `src/agent-doc/src/sync.rs`: sync now accepts the latest still-open session-log pane as an ownership proof source when that pane is still tmux-alive in the same project root, and associated-pane recovery can reuse/reregister it before `registered_pane_missing` replacement is allowed. I also added tmux-backed regressions for direct live-owner reuse and associated-pane recovery, and updated the sync spec plus `VERSIONS.md`.

Verification:
- `cargo test` in `src/agent-doc`: the new sync regressions pass, but the full suite still reports two unrelated existing failures in `resync::tests::detect_wrong_window_panes_in_different_windows` and `resync::tests::no_wrong_window_when_panes_in_same_window`
- `cargo build`
- `cargo install --path . --force`

Committed and pushed in `src/agent-doc`:
- `5798fe1` `fix: fail closed on live owner proof gaps`
<!-- agent:boundary:b624a742 -->
<!-- /agent:exchange -->

<!--
When I call `Run Agent Doc` from the JB Plugin, there should be a cursor prompt...at the location of the cursor...to notify the user that `agent-doc ...` was sent.
---
Please add a JB + VSCode command to call `/clear` in the binding Claude Code + Codex sessions. This `/clear` command should be visible in the `Agent Doc` contextual menu.
---
Add a Plugin Editor command to clear the context
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#jbselsync] [recommended] Critical: Make JetBrains tab-selection handling non-destructive for cross-root sessions. Use focus-only behavior for selection changes, reserve `sync --no-autostart` for visible-layout changes, and reuse the workspace-root chooser so switching to `src/boost-client/tasks/monsterrodholders.md` cannot rebind or replace a live tmux pane. Plan: tasks/agent-doc/plan-jb-selection-sync-non-destructive.md
- [ ] [#dup-exchange] [recommended] Fail closed or auto-repair when a template/CRDT session document gains a second `<!-- agent:exchange ... -->` opener instead of appending inside the canonical exchange component. Repro: `src/session-share/tasks/buildparty-investor-demo/dev.md` ended up with two exchange blocks after a direct Codex patch/write cycle. Plan: verify whether this is a write-path structural-guard miss, a repair-path miss, or a direct-manual-edit escape hatch that should still be normalized.
- [/] [#jbxroot] [recommended] Replay the JetBrains cross-root split with tasks/agent-doc/agent-doc-bugs2.md and src/boost-client/tasks/monsterrodholders.md after the absolute cross-root layout reporting fix, and confirm passive sync no longer collapses to one pane or triggers stale-session cleanup. Plan: tasks/agent-doc/plan-jetbrains-cross-root-split-sync-validation.md
- [ ] [#jbmcol] [recommended] Replay JetBrains mixed-split Sync Layout on src/boost-client/tasks/monsterrodholders.md and confirm empty-column column-memory restoration keeps two tmux panes with monsterrodholders.md on the right and in focus. Plan: tasks/agent-doc/plan-monsterrodholders-empty-column-sync-replay.md
- [ ] [#jbsubroot] [recommended] Replay the JetBrains boost-client/session-share nested-root passive sync and Run Agent Doc path after the root-aware window-resolution fix, and confirm foreign agent-doc windows only warn instead of becoming authoritative. Plan: tasks/agent-doc/plan-jb-submodule-passive-autostart-window-resolution-validation.md
- [ ] [#jbnsrf] [recommended] Validate the original restart repro after the non-destructive editor sync/startup hardening and confirm no plugin path still closes or replaces a visible pane. Plan: tasks/agent-doc/plan-buildparty-jb-visible-pane-tracking-loss.md
- [ ] [#jbpdrop] [recommended] Critical: a completed JetBrains-triggered run must not remove its tmux pane from the `agent-doc` window; audit post-success pane-death/cleanup and plugin-triggered sync/layout paths until pane retention is explicit and regression-tested. Plan: tasks/agent-doc/plan-jb-completed-run-pane-retention.md
- [ ] [#jbptrk] [recommended] Critical: Finish live validation/root-cause work for visible-pane tracking loss after editor sync/startup was reduced to report-only `resync` plus `sync --no-autostart`. Plan: tasks/agent-doc/plan-buildparty-jb-visible-pane-tracking-loss.md
- [/] [#edispatch] [recommended] Replay JetBrains and VS Code Run Agent Doc against tasks/agent-doc/agent-doc-bugs2.md after the new dispatch-only route path lands. Plan: tasks/agent-doc/plan-editor-dispatch-only-reroute-validation.md
- [ ] [#jbctrlg] [recommended] Replay the JetBrains Run Agent Doc reroute on tasks/agent-doc/agent-doc-bugs2.md after the ctrl-g busy-pane recovery landed. Plan: tasks/agent-doc/plan-agent-doc-bugs2-jb-ctrl-g-reroute-validation.md
- [ ] [#mrreplay] [recommended] Validate the live monsterrodholders rerun after the backlog-alias repair path landed, and determine whether the remaining pane %320 startup-miss warning clears on the next successful routed cycle. Plan: tasks/agent-doc/plan-monsterrodholders-repair-replay-validation.md
- [ ] [#mrbusy] [recommended] Validate the live monsterrodholders reroute after the same-document busy-pane optimistic reopen change. Plan: tasks/agent-doc/plan-monsterrodholders-busy-reroute-validation.md
- [ ] [#sceno] [recommended] Investigate nondeterministic full-suite ENOENT in session_check::tests::session_check_skips_pending_done_warning_when_id_was_recorded. Plan: tasks/agent-doc/plan-session-check-pending-done-enoent-flake.md
- [ ] [#jberr] [recommended] Keep JetBrains Run Agent Doc route failures copyable/persistent instead of transient-only. Plan: tasks/agent-doc/plan-jb-route-error-capture.md
<!-- /agent:backlog -->

<!-- agent:icebox -->
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
