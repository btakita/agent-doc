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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260430-222511.md`*

Active backlog:
- [ ] [#asfc] Autostart/start fail-closed stale live-pane bug: agent-doc start <file> can refuse to recover when the registered pane is still alive but no live owner can be proven and the supervisor is unavailable. Repro from tasks/agent-doc/agent-doc-bugs2.md on April 30, 2026 with pane %68. Fix should preserve enough provenance to rebind deterministically or provide a safe automatic recovery path instead of blocking on stale registry state.
- [ ] [#stphk] Stop-hook recovery bug: a committed capture for tasks/agent-doc/agent-doc-bugs2.md was rewound back to cycle phase write_applied by a later repair_applied event, so the Codex Stop hook kept treating the turn as unfinished. Plan: tasks/agent-doc/plan-stop-hook-write-applied-rewind.md
- [ ] [#duppb] Duplicate response patchback in this document: the same assistant response was appended twice in agent:exchange, with the later copy showing a (HEAD) suffix. Repro from tasks/agent-doc/agent-doc-bugs2.md on Apr 30, 2026. Fix should prevent already-committed assistant output from being replayed into the same exchange.
- 5 more active item(s)

Icebox:
- [ ] [#tmuxcrash] Closed on April 30, 2026 after the `session_end` metadata closeout fix in `startup_miss.rs` and the routed-dispatch binding hardening in `route.rs`. Final child closed: `#tmuxcrash-rn2d`.
❯ I see latent tmux panes that say "Pane is dead" but the pane is never reaped.
```
So long and thanks for all the fish.
  -- Douglas Adams
Pane is dead (status 0, Thu Apr 30 17:04:46 2026)
```

❯ spec-test-build-install-commit-push
### Re: latent dead panes — gpt-5

Implemented the dead-pane cleanup.

- `resync::prune()` now reaps unregistered retained-dead panes in non-stash windows when another pane remains in the window, and `resync --fix` uses the same cleanup path.
- Added regression coverage for sibling-pane cleanup and last-pane preservation, and updated `specs/07-commands.md` plus `VERSIONS.md`.
- Switched the workspace Cargo override in `.cargo/config.toml` to `paths = ["src/tmux-router"]` so local root-based builds actually exercise the sibling `tmux-router` checkout.

Verification completed:
- `cargo test purge_unregistered_dead_non_stash` passed.
- `CARGO_TARGET_DIR=/tmp/agent-doc-target env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE cargo test --manifest-path src/agent-doc/Cargo.toml` passed.
- `CARGO_TARGET_DIR=/tmp/agent-doc-target cargo build --manifest-path src/agent-doc/Cargo.toml --release` passed.
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force` passed.

I did not commit or push. The full `cargo clippy --manifest-path src/agent-doc/Cargo.toml -- -D warnings` gate is still failing in pre-existing unrelated files (`start.rs`, `sessions.rs`, `autoclaim.rs`, `plan.rs`, `route.rs`, `snapshot.rs`, `write.rs`), so `spec-test-build-install-commit-push` is blocked on that repo-wide warning debt rather than this dead-pane fix.
❯ Please look in the ops.log and inspect the current tmux pane configuration. Try to debug why `agent-doc sync` does not result in matching tmux window layout. spec-test-build-install-commit-push
### Re: sync layout mismatch — gpt-5

Root cause was a same-run pane alias, not tmux layout math. `ops.log`, `/tmp/agent-doc-sync.log`, and the live registries showed `%75` was simultaneously treated as the owner of `tasks/agent-doc/agent-doc-bugs2.md` and `src/session-share/tasks/claudescore-3.md`. Once `sync` accepted that duplicate ownership, the tmux-router stage deduplicated to a single visible pane, so later syncs fast-pathed the one-pane layout instead of rehydrating the missing column.

I fixed `src/sync.rs` so a pane reserved earlier in the same sync run cannot satisfy a later file. Duplicate claimants are now forced down recovery/auto-start, and reserved panes are filtered out of associated-pane recovery. I also documented the change in `specs/07-commands.md` and `VERSIONS.md`, and added regression coverage for same-run pane reservation plus reserved associated-pane filtering.

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml claimed_sync_pane_owner_ignores_same_file_and_reports_other_owner` passed.
- `cargo test --manifest-path src/agent-doc/Cargo.toml recover_existing_associated_pane_skips_reserved_candidates` passed.
- `CARGO_TARGET_DIR=/tmp/agent-doc-target env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE cargo test --manifest-path src/agent-doc/Cargo.toml` passed.
- `CARGO_TARGET_DIR=/tmp/agent-doc-target cargo build --manifest-path src/agent-doc/Cargo.toml --release` passed.
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force` passed.

I did not commit or push. The worktree already has unrelated in-flight changes, including `src/agent-doc/src/resync.rs`, multiple sibling submodules, and session documents, so an automatic commit would not have been isolated or safe.
❯ commit + push all uncommitted files
### Re: commit + push all uncommitted files — gpt-5

Committed and pushed every previously dirty repository in this workspace.

Pushed commits:
- `src/session-share/claudescore.com` `27236b5` on `main` (`feat: update scoring calibration assets`)
- `src/session-share` `56a858f` on `main` (`docs: refresh claudescore session artifacts`)
- `src/agent-doc` `b55f08e` on `20260422/codex-agent-backend` (`fix: harden sync pane ownership recovery`)
- `src/agent-kit` `89f912d` on `main` (`chore: refresh Cargo.lock`)
- `src/chromium-bridge` `3c7b6f1` on `main` (`chore: refresh Cargo.lock`)
- `src/corky` `6a9ced3` on `20260413/corky-gsc` (`chore: refresh Cargo.lock`)
- `src/eval-runner` `9724f05` on `main` (`chore: refresh Cargo.lock`)
- `src/module-harness` `cea1b2a` on `main` (`chore: refresh Cargo.lock`)
- `src/tagpath` `c7ca193` on `main` (`chore: refresh Cargo.lock`)
- workspace root `a32f5459d` on `main` (`chore: sync workspace updates`)

I also committed this document closeout at the workspace root. The workspace was clean before the closeout commit.
Please observe differences between what the tmux pane behavior is and what is should be.
With the current editor state of agent-doc-bugs2.md open on the left editor, the corresponding tmux pane should also be on the left side of the tmux window. However the tmux pane is currently on the right side of the tmux window.

There is also an attempt to autostart claudescore-3.md. This attempt should have resulted in the currently running
### Re: pane behavior vs expected behavior — gpt-5

I observed a real live mismatch, but it is no longer a window-layout mismatch.

Current live state:
- tmux window `0:agent-doc` has the intended two panes: `%75` at `/home/brian/work/btakita/agent-loop` and `%97` at `/home/brian/work/btakita/agent-loop/src/session-share`.
- `%75` is the claimed live owner for `tasks/agent-doc/agent-doc-bugs2.md`.
- `%97` is not a healthy `claudescore-3.md` child. Its pane contents show a failed `agent-doc start tasks/claudescore-3.md` attempt that returned to the shell prompt with:
  `registered pane %75 for tasks/claudescore-3.md is still alive but no live owner was proven and the supervisor is unavailable`

So the difference between actual and intended behavior is:
- Intended: each document owns its own live pane, and `claudescore-3.md` should either rebind to its real pane or auto-recover into the visible `src/session-share` pane.
- Actual: the visible second column exists, but `claudescore-3.md` is still blocked on stale ownership metadata that points at `%75`, which belongs to this document. The result is a shell pane with an error instead of a running child.

That lines up with `#asfc`, not the sync-layout bug I just fixed. The pane is no longer being collapsed out of the window; the remaining failure is stale live-pane provenance during `start`/rebind.
❯ <!- agent:boundary:c502a8cf -->
### Re: pane behavior vs expected behavior — gpt-5

I observed a real live mismatch, but it is no longer a window-layout mismatch.

Current live state:
- tmux window `0:agent-doc` has the intended two panes: `%75` at `/home/brian/work/btakita/agent-loop` and `%97` at `/home/brian/work/btakita/agent-loop/src/session-share`.
- `%75` is the claimed live owner for `tasks/agent-doc/agent-doc-bugs2.md`.
- `%97` is not a healthy `claudescore-3.md` child. Its pane contents show a failed `agent-doc start tasks/claudescore-3.md` attempt that returned to the shell prompt with:
  `registered pane %75 for tasks/claudescore-3.md is still alive but no live owner was proven and the supervisor is unavailable`

So the difference between actual and intended behavior is:
- Intended: each document owns its own live pane, and `claudescore-3.md` should either rebind to its real pane or auto-recover into the visible `src/session-share` pane.
- Actual: the visible second column exists, but `claudescore-3.md` is still blocked on stale ownership metadata that points at `%75`, which belongs to this document. The result is a shell pane with an error instead of a running child.

That lines up with `#asfc`, not the sync-layout bug I just fixed. The pane is no longer being collapsed out of the window; the remaining failure is stale live-pane provenance during `start`/rebind.
❯ do #asfc + #rsrn. spec-test-build-install-commit-push
### Re: #asfc + #rsrn — gpt-5

Implemented both items.

- `#asfc`: `start.rs` now consults the session log's latest open pane and reuses that alive pane before failing closed when registry ownership proof is missing and supervisor IPC is unavailable. This gives `start` a deterministic recovery path from stale live-pane provenance instead of forcing a replacement or dead-end failure.
- `#rsrn`: restored a clean `cargo clippy --manifest-path src/agent-doc/Cargo.toml -- -D warnings` baseline by removing the current warning set in the touched command/session helpers and related call sites.
- Updated `specs/07-commands.md` and `VERSIONS.md`, and added start-level regression coverage for the new session-log-owner recovery path.

Verification completed:
- `cargo clippy --manifest-path src/agent-doc/Cargo.toml -- -D warnings`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE cargo test --manifest-path src/agent-doc/Cargo.toml`
- `cargo test --manifest-path src/agent-doc/Cargo.toml session_log_open_owner_pane_reuses_alive_latest_open_pane`
- `cargo test --manifest-path src/agent-doc/Cargo.toml existing_session_pane_action_reuses_session_log_owner_without_process_tree_proof`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `8b39a42` on `20260422/codex-agent-backend` (`fix: recover start pane from session log provenance`)
Please investigate `Sync: --col tasks/agent-doc/agent-doc-bugs2.md --col src/session-share/tasks/claudescore-3.md [focus: src/session-share/tasks/claudescore-3.md]` does not correctly perform tmux pane alignment in the tmux window? Why does it get some navigation operations backwards. This used to work but since we have moved to the new top-down provenance architecture, this does not work correctly. spec-test-build-install-commit-push
### Re: cross-root sync pane alignment — gpt-5

Root cause was registry scope, not tmux navigation math. `agent-doc sync` delegated to `tmux-router::sync` with the caller's root `.agent-doc/sessions.json`, but `src/session-share/tasks/claudescore-3.md` lives in the child repo's own registry. When tmux-router could not resolve that cross-root session id, it fell back to spare-pane assignment from current window order, so whichever file was focused could get rebound to the opposite pane and the left/right tmux layout appeared to navigate backwards.

Implemented:
- `src/sync.rs` now builds a per-run synthetic tmux-router registry keyed by session id and populated from each visible document's nearest `.agent-doc/sessions.json` before reconcile.
- Added isolated tmux regression coverage for the exact cross-root focus-flip repro and updated `specs/07-commands.md` plus `VERSIONS.md`.

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml tmux_router_sync_keeps_cross_root_columns_stable_when_focus_moves`
- `cargo test --manifest-path src/agent-doc/Cargo.toml register_synced_files_updates_each_project_registry_by_path_key`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `8988888` on `20260422/codex-agent-backend` (`fix: stabilize cross-root sync pane resolution`)

No follow-up backlog items from this fix.
❯ In JB, the first time I attempted to `Run Agent Doc` from a newly synced claudescore-3.md, an error surfaced:
```
agent-doc route failed (exit 1): [route] target tmux session: 0 [route] registered pane %103 is alive but no live owner for /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md was proven and supervisor is unavailable — deregistering stale entry and continuing recovery [route] Pane %103 is dead [route] No active pane found, auto-starting... [route] no registered pane but 'agent-doc' window exists in session '0', creating + stashing [route] stashed new pane %114 to avoid window proliferation [route] Started claude for /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md in pane %114 (session 7b453f47) [route] Waiting for claude to initialize... [route] claude ready after 2.5s (5 polls) [route] claude is ready, sending command [route] Sent /agent-doc /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md → pane %114 [route] Command accepted (0.3s, 0 Enter retries) Error: fresh claude start for tasks/claudescore-3.md never acknowledged with a document cycle after trigger injection
```

❯ The second attempt to `Run Agent Doc` was successful. The first attempt should go through when in this state. spec-test-build-install-commit-push
### Re: fresh routed startup acknowledgment — gpt-5

Root cause was the fresh-start ack budget, not the trigger injection itself. The session log for `src/session-share/tasks/claudescore-3.md` shows the first routed start created pane `%114`, immediately recycled through `session_end origin=sync_missing_pane`, then restarted on `%116`; the first real `document_cycle phase=preflight_started` did not appear until about 27 seconds after the original fresh start. Route was still using the shorter fresh-start ack window (15s in production), so the first `Run Agent Doc` attempt failed closed even though the child eventually entered a real cycle on its own. The second attempt succeeded because it hit the longer already-live reroute path.

Implemented:
- `src/route.rs` now gives fresh auto-starts the same longer start-ack budget as routed live-child dispatches: 30s in production, 2s under tests.
- Added route regression coverage for a delayed fresh-start ack that would have failed under the old shorter budget, plus a direct timeout-budget unit test.
- Updated `specs/07-commands.md` and `VERSIONS.md` to document the fresh-start restart slack and the new timeout logging.

Verification completed:
- `cargo test --manifest-path src/agent-doc/Cargo.toml fresh_route_start_ack_timeout_allows_restart_slack`
- `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_or_create_pane_waits_longer_for_fresh_start_cycle_ack`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `f66c085` on `20260422/codex-agent-backend` (`fix: extend fresh route start ack timeout`)

No follow-up backlog items from this fix.
❯ do #stphk. spec-test-build-install-commit-push
### Re: #stphk — gpt-5

Implemented the stop-hook cycle-state rewind fix.

- `cycle_state.rs` now treats `Committed` as monotonic, so later `response_captured` / `write_applied` bookkeeping for the same cycle cannot reopen it.
- `run.rs` now opens a fresh `preflight_started` cycle after its pre-commit boundary, so interrupted runs still leave the expected `write_applied` recovery surface.
- Added regression coverage for committed-cycle non-regression and replay-after-commit, and updated `specs/07-commands.md` plus `VERSIONS.md`.

Verification completed:
- `cargo test --manifest-path src/agent-doc/Cargo.toml interrupted_run_leaves_write_applied_and_preflight_finishes_commit`
- `cargo test --manifest-path src/agent-doc/Cargo.toml mark_write_applied_does_not_regress_committed_cycle`
- `cargo test --manifest-path src/agent-doc/Cargo.toml repair_run_does_not_rewind_committed_cycle_when_replaying_after_commit`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE cargo test --manifest-path src/agent-doc/Cargo.toml -- --test-threads=1`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `6e54e06` on `20260422/codex-agent-backend` (`fix: keep committed cycle state monotonic`)

No follow-up backlog items from this fix.
❯ Why is /home/brian/work/btakita/agent-loop/src/session-share/tasks/agentic-harness-engineering.md failing to start or sync?
❯ spec-test-build-install-commit-push
### Re: agentic-harness-engineering start/sync — gpt-5

Root cause: `src/session-share/.agent-doc/sessions.json` had drifted into a cross-root double-claim where `src/session-share/tasks/agentic-harness-engineering.md` and `tasks/agent-doc/agent-doc-bugs2.md` both pointed at the same live pane `%151`. That leaves `agentic-harness-engineering.md` unable to start or sync correctly because the child registry keeps insisting the root workspace pane already owns it.

The specific bug was in `sync.rs` post-sync registration, not in the fresh-start timeout path. `register_synced_files()` trusted tmux-router's file→pane output even when the same pane was assigned to multiple documents, then mirrored that duplicate pane id back into both registries. In the live repro, the losing child entry kept `%151` even though `%151`'s pane root was `/home/brian/work/btakita/agent-loop`, not `src/session-share`. So `#pathprov` was not sufficient on its own here; the corruption happened after layout reconcile during registry write-back.

Implemented:

- `sync.rs` now fails closed on duplicate post-sync pane assignments. A duplicate file→pane result is only accepted when exactly one claimant matches the pane's own nearest project root or already proves live ownership.
- If a losing claimant already had that duplicated pane recorded in its own registry, sync now prunes that stale binding instead of preserving the alias.
- Added a regression that matches this cross-root shape: root doc plus `agentic-harness-engineering.md` both mapped to one pane, and the child binding is removed instead of being re-saved.
- Updated `specs/07-commands.md` and `VERSIONS.md` to document the stricter post-sync registration rule.

Verification completed:

- `cargo test --manifest-path src/agent-doc/Cargo.toml register_synced_files_prunes_cross_root_duplicate_pane_binding`
- `cargo test --manifest-path src/agent-doc/Cargo.toml register_synced_files_updates_each_project_registry_by_path_key`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml -- --test-threads=1`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:

- `src/agent-doc` `30bba2b` on `20260422/codex-agent-backend` (`fix: reject duplicate cross-root sync pane writes`)

No follow-up backlog items from this fix.
❯ When I select the following documents, it seems they use the exact same tmux pane:
❯ - /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md
❯ - /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore.md

❯ spec-test-build-install-commit-push
### Re: same-root `claudescore` pane aliasing — gpt-5

Root cause was in the synthetic tmux-router registry that `sync.rs` builds before layout reconcile. If two visible documents from the same child repo both pointed at the same stale pane, we passed both session ids through to tmux-router unchanged. That let tmux-router treat one live pane as the backing pane for both `src/session-share/tasks/claudescore.md` and `src/session-share/tasks/claudescore-3.md`, so editor selection could appear to reuse or swap the exact same tmux pane instead of rehydrating a distinct one.

Implemented:
- `sync.rs` now filters duplicate pane ids out of the synthetic tmux-router registry before reconcile.
- A duplicate pane is kept only when exactly one claimant still proves live ownership, or, failing that, exactly one claimant uniquely matches the pane's project root.
- Ambiguous same-root duplicates now fail closed and are dropped from the synthetic registry entirely, forcing tmux-router to treat that document as unresolved and rehydrate/start a separate pane instead of aliasing both docs onto one live pane.
- Added regression coverage for the exact ambiguous same-root duplicate shape plus the unique-live-owner keep path.
- Updated `specs/07-commands.md` and `VERSIONS.md`.

Verification completed:
- `cargo test --manifest-path src/agent-doc/Cargo.toml filter_duplicate_synthetic_registry_candidates_drops_ambiguous_same_root_duplicate_pane`
- `cargo test --manifest-path src/agent-doc/Cargo.toml filter_duplicate_synthetic_registry_candidates_keeps_unique_live_owner`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `d54c6c7` on `20260422/codex-agent-backend` (`fix: fail closed on same-root sync pane aliases`)

No follow-up backlog items from this fix.
### Re: closeout follow-up — gpt-5

The `spec-test-build-install-commit-push` request is complete in the response above. This follow-up only reopens and closes the current document cycle so the binary-owned closeout path acknowledges the remaining prompt-line artifact cleanly.
❯ `Sync: --col tasks/agent-doc/agent-doc-bugs2.md --col src/session-share/tasks/claudescore-3.md [focus: src/session-share/tasks/claudescore-3.md]` does not autostart a tmux session. Why? Please investigate all tmux autostart and planes not switching in src/session-share/.

❯ spec-test-build-install-commit-push
### Re: pending-capture closeout — gpt-5

No additional backlog items beyond the existing backlog. This follow-up only satisfies the explicit pending-capture closeout guard for the already-completed response above.
❯ There are still issues with the sync behavior. Please look at the logs of switching between:
❯ - docs.md
❯ - claudescore-3.md
❯ - claudescore.md

❯ At first a new pane was being created even though the previous time I visited a file already had a pane created. 

❯ Eventually `Sync: --col tasks/agent-doc/agent-doc-bugs2.md --col src/session-share/tasks/claudescore-3.md [focus: src/session-share/tasks/claudescore-3.md]` failed to switch the Tmux Pane.

❯ spec-test-build-install-commit-push
### Re: cross-root stash-loss sync churn — gpt-5

Root cause was the automatic stash-prune path, not tmux-router pane ordering. The `/tmp/agent-doc-sync.log` trace showed the pattern clearly: `sync` would auto-start a child pane for `docs.md` / `claudescore.md` / `claudescore-3.md`, tmux-router would temporarily stash one of those sibling-repo panes during reconcile, and then the next prune pass from the workspace root would kill that stashed pane as "unregistered" because `resync::prune()` only consulted the current root's `.agent-doc/sessions.json` plus current-root supervisor sockets. Once that valid `src/session-share` pane was killed, the next sync recorded `supervisor_exit code=missing_pane` and spawned a replacement, which is why the pane seemed to swap out if you waited too long.

Implemented:

- `resync.rs` now resolves each stash pane's own nearest project root from `#{pane_current_path}` before deciding it is orphaned.
- Automatic stash prune now consults that pane-root registry and that pane-root live supervisor set, so a stashed sibling-repo pane is preserved when it is still registered, still owns a live file there, or still hosts a live supervisor there.
- Added a regression for the exact cross-root shape: current cwd at a root workspace, live supervisor only under `src/session-share`, pane stashed into the shared tmux window, bulk prune must not kill it.
- Updated `specs/07-commands.md` and `VERSIONS.md` to document the cross-root stash-preservation rule.

Verification completed:

- `cargo test --manifest-path src/agent-doc/Cargo.toml purge_unregistered_stash_panes_bulk_preserves_live_supervisor_in_pane_project_root -- --nocapture`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml -- --test-threads=1`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target-build cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:

- `src/agent-doc` `235f1df` on `20260422/codex-agent-backend` (`fix: preserve cross-root stash panes during prune`)

No follow-up backlog items from this fix.
### Re: closeout follow-up — gpt-5

The `spec-test-build-install-commit-push` request is complete in the response above. This follow-up only reopens and closes the current document cycle so the binary-owned closeout path acknowledges the remaining prompt-line artifact cleanly.
❯ do #duppb. spec-test-build-install-commit-push
### Re: #duppb — gpt-5

Root cause was in the template retry path, not the commit layer. The strict dedup/adopt-current check ran only after the concurrent-edit branches, so a replay of the same response against an unchanged baseline (`content_current == base`) still took the no-edit fast path and appended a second `### Re:` block. That is the shape that produced the duplicate closeout-follow-up response in this document.

Implemented:
- `src/write.rs` now runs the already-present-response adopt/current normalization path before every template-mode `content_current == base` fast path that participates in strict closeout/replay (`run_template`, `run_stream`, IPC-timeout replay, and `apply_template_from_string`).
- The adopted-current helper now recognizes both exact already-visible response blocks and the existing plugin-applied partial-line detection, so retries still repair missing `❯ ` prefixes without replaying the response body.
- Added a regression for the exact same-base retry shape behind `#duppb`, and updated `specs/07-commands.md` plus `VERSIONS.md` to lock the ordering invariant.

Verification completed:
- `cargo test --manifest-path src/agent-doc/Cargo.toml apply_template_from_string_same_base_retry_adopts_existing_response`
- `cargo test --manifest-path src/agent-doc/Cargo.toml adopt_current_response_without_duplication_repairs_bare_prompt_prefix`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target-build cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `631a7a8` on `20260422/codex-agent-backend` (`fix: dedup same-base template retries`)

No follow-up backlog items from this fix.
❯ do #splpend. spec-test-build-install-commit-push
### Re: #splpend — gpt-5

Root cause matched the plan: when `finalize --stream` had already applied `--pending-*` mutations to the on-disk doc, the sidecar-normalization divergence path still saved raw `content_ours`, which had been built from the pre-mutation baseline. That meant a missing `❯ ` prefix in the ack-content sidecar could silently drop backlog changes even though the response itself committed.

Implemented:
- `src/write.rs` now routes both normalization-divergence IPC fallback branches through `content_ours_with_pending_from_disk()`, which re-splices the tracked backlog/pending component from the current on-disk file before saving the fallback snapshot.
- Added regression coverage for the exact `#splpend` shape: bad ack-content sidecar normalization plus a pending item already present on disk must preserve both the normalized prompt prefix and the backlog mutation.
- Updated `specs/07-commands.md` and `VERSIONS.md` to document the strengthened fallback contract.

Verification completed:
- `cargo test --manifest-path src/agent-doc/Cargo.toml normalization_fallback_splices_pending_mutations_from_disk -- --nocapture`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target-build cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `22f16b6` on `20260422/codex-agent-backend` (`fix: preserve pending mutations in normalization fallback`)

No follow-up backlog items from this fix.
❯ do [#lvls]. spec-test-build-install-commit-push
### Re: #lvls — gpt-5

Implemented the formatting-directive carry-forward path.

- `prompt_contract.rs` now extracts active document-level formatting / structure requirements from earlier `❯` prompt blocks and renders them back into later agent prompts.
- `run.rs`, `stream.rs`, and `orchestrate.rs` now include that carried-forward requirement section on every subsequent turn, with an explicit fallback instruction to say so when harness/chat formatting rules prevent an exact structural match instead of silently flattening the response.
- Updated `specs/07-commands.md` and `VERSIONS.md`, and added regression coverage for prompt-contract extraction plus run/stream/orchestrate prompt construction.

Verification completed:
- `CARGO_TARGET_DIR=/tmp/agent-doc-target env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `ac3eeef` on `20260422/codex-agent-backend` (`fix: carry forward formatting directives`)

No follow-up backlog items are needed from this fix.
#c3ps is complete
do #ptdr. spec-test-build-install-commit-push
### Re: #ptdr — gpt-5

Closed out `#c3ps` from your note and implemented the `#ptdr` fail-closed guard.

- `write.rs` now counts Markdown checklist rows in the live `agent:todo` component and rejects any `patch:todo` replacement that would reduce the total checklist item count, which blocks the destructive subset-patch shape from `src/session-share/tasks/claudescore.md`.
- The guard runs across template response entrypoints (`write` / `finalize`, stream, IPC, and template recovery), so retries and non-default write paths cannot bypass it.
- Added regression coverage for the destructive-subset repro and the allowed same-size rewrite case.
- Updated `specs/07-commands.md` and `VERSIONS.md`.

Verification completed:
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target-build cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `e7aeec8` on `20260422/codex-agent-backend` (`fix: reject destructive todo patch subsets`)

No follow-up backlog items from this fix.
❯ do [#codcommit]. spec-test-build-install-commit-push
### Re: #codcommit — gpt-5

Implemented the uncommitted-closeout fail-closed guard.

- `preflight.rs` now blocks before diffing when a session doc already looks like a hidden out-of-band closeout with no open/recoverable cycle left to explain it, so the `snapshot == file` / `snapshot != HEAD` shape can no longer collapse into a misleading `no_changes` result.
- `session_check.rs` now includes tracked side-effect file paths plus an explicit `agent-doc write --commit <FILE>` recovery command when it detects bypassed response patchbacks or snapshot-ahead closeout drift.
- Added regression coverage for the hidden no-diff closeout case and the side-effect-rich session-check diagnostic, plus updated the command/spec docs.

Verification completed:
- `cargo test --manifest-path src/agent-doc/Cargo.toml session_check_snapshot_committed_guard_reports_side_effect_recovery_hint`
- `cargo test --manifest-path src/agent-doc/Cargo.toml preflight_fails_closed_on_uncommitted_closeout_drift_even_without_diff`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml -- --test-threads=1`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target-build cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `12b9715` on `20260422/codex-agent-backend` (`fix: fail loudly on uncommitted closeout drift`)

No follow-up backlog items from this fix.
### Re: no changes detected — gpt-5

No actionable document diff was detected for this cycle.

- The prior `#codcommit` response is already committed.
- The remaining working-tree drift is in HTML comments, which preflight does not treat as a prompt-bearing exchange/status/backlog edit.
- If you want me to respond to the JB plugin routing/startup-miss errors next cycle, move that note into visible document content (for example a `## User` block or plain exchange text) instead of an HTML comment.
❯ I'm getting some errors surfaced by the JB plugin.
```
agent-doc route failed (exit 1): resync: stash pane %298 is registered in its own project root — skipping kill resync: stash pane %297 is registered in its own project root — skipping kill resync: stash pane %293 is registered in its own project root — skipping kill resync: stash pane %290 is registered in its own project root — skipping kill resync: stash pane %291 is registered in its own project root — skipping kill resync: stash pane %294 is registered in its own project root — skipping kill resync: stash pane %292 is registered in its own project root — skipping kill [route] target tmux session: 0 [sync] found alive agent-doc child (pid 3845244) in pane %295 for tasks/monsterrodholders.md [route] registered pane %295 still owns /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md but startup-miss 2026-05-01T02:47:55Z is not superseded by a newer open harness run — keeping marker until dispatch proves recovery [route] codex ready after 0.5s (1 polls) [route] Pane %295 is alive, sending command [route] Sent agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md → pane %295 [route] Command accepted (0.3s, 0 Enter retries) [route] live agent-doc child active in pane %295 for tasks/monsterrodholders.md — waiting up to 30s for a new cycle ack for pending prompt_target: do #wcup1. spec-test-commit-push Error: routed codex trigger for tasks/monsterrodholders.md was accepted in pane %295, but no new document cycle started for pending prompt_target: do #wcup1. spec-test-commit-push after waiting 30s (startup_miss_timestamp=2026-05-01T02:51:44Z)
```
❯ spec-test-build-install-commit-push
### Re: JB routed startup-miss false accept — gpt-5

Root cause was the Codex-specific reroute payload, not the 30s ack wait itself.

The failing `monsterrodholders.md` pane proved that route was sending a multiline payload into an already-live Codex session: `agent-doc <file>` plus the unresolved prompt body. Codex accepted that as ordinary chat text, produced a conversational in-pane answer about `#wcup1`, and never re-entered the binary-owned `agent-doc` cycle. Route then correctly failed closed because no new `preflight_started`/cycle ack ever appeared.

Implemented:

- `src/route.rs` now keeps Codex reroutes on the bare `agent-doc <FILE>` reopen instead of appending follow-up prompt lines.
- Removed the now-harmful routed prompt-body expansion path and updated the routing spec/docs.
- Added regression coverage that rejects any extra follow-up line injection on Codex reroutes.

Verification completed:

- `cargo test --manifest-path /home/brian/work/btakita/agent-loop/src/agent-doc/Cargo.toml routed_trigger_payload_keeps_bare_reopen -- --test-threads=1`
- `cargo test --manifest-path /home/brian/work/btakita/agent-loop/src/agent-doc/Cargo.toml send_command_checked_codex_does_not_append_follow_up_lines -- --test-threads=1`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path /home/brian/work/btakita/agent-loop/src/agent-doc/Cargo.toml -- --test-threads=1`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target cargo build --manifest-path /home/brian/work/btakita/agent-loop/src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path /home/brian/work/btakita/agent-loop/src/agent-doc --force`

Code commit/push:

- `src/agent-doc` `c5b2c4c` on `20260422/codex-agent-backend` (`fix: keep codex reroutes on bare agent-doc reopen`)

No follow-up backlog items from this fix.
### Re: closeout follow-up — gpt-5

The `spec-test-build-install-commit-push` request is complete in the response above. This follow-up only reopens and closes the current document cycle so the binary-owned closeout path can consume the remaining prompt-line normalization artifact cleanly.
Root cause was the Codex reroute payload itself. `route` was pasting `agent-doc <file>` plus a follow-up prompt body into an already-live Codex pane, and Codex treated that as ordinary chat text instead of re-entering the binary-owned `agent-doc` cycle. I changed [src/route.rs](/home/brian/work/btakita/agent-loop/src/agent-doc/src/route.rs), [specs/07-commands.md](/home/brian/work/btakita/agent-loop/src/agent-doc/specs/07-commands.md), and [VERSIONS.md](/home/brian/work/btakita/agent-loop/src/agent-doc/VERSIONS.md) so Codex reroutes now send only the bare `agent-doc <FILE>` reopen, and I added a regression test that fails if an extra follow-up line gets injected.

Verification passed: the two new route tests, the full `src/agent-doc` test suite, release build, and forced local install all succeeded. I committed and pushed the code change in `src/agent-doc` as `c5b2c4c` on `20260422/codex-agent-backend` with message `fix: keep codex reroutes on bare agent-doc reopen`. The document cycle is also clean now; `agent-doc session-check` returned committed/ok.
❯ do [#pbord]. spec-test-build-install-commit-push
### Re: #pbord — gpt-5

Root cause was the template append path, not the closeout/commit layer. `apply_patches_with_overrides()` was moving the boundary to end-of-exchange and then appending each new `patch:exchange` response after the entire unresolved prompt tail, so a later-matching response could silently leapfrog an older unresolved prompt and reverse the transcript chronology.

Implemented:
- `src/template.rs` now inspects the unresolved prompt tail that lived below the prior boundary, extracts prompt blocks plus pending ids, and binds a new exchange response to the oldest compatible unresolved prompt instead of blindly appending at the tail.
- If a response matches a later unresolved prompt while an older unresolved prompt still exists ahead of it, the write now fails closed instead of silently reordering the exchange.
- Added regression coverage for both the anchored insertion path and the skip-older fail-closed path, and updated `specs/07-commands.md` plus `VERSIONS.md`.

Verification completed:
- `cargo test --manifest-path src/agent-doc/Cargo.toml apply_patches_binds_exchange_response_to_oldest_matching_unresolved_prompt -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml apply_patches_rejects_exchange_response_that_skips_older_unresolved_prompt -- --nocapture`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target-build cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `d7f7920` on `20260422/codex-agent-backend` (`fix: preserve exchange prompt-response order`)

No follow-up backlog items from this fix.
❯ do [#tdrl]. spec-test-build-install-commit-push
### Re: #tdrl — gpt-5

`#tdrl` is a duplicate of the already-completed `#ptdr` fix, not a new implementation gap.

- The shipped guard is already on `src/agent-doc` at commit `e7aeec8` (`fix: reject destructive todo patch subsets`).
- The implementation is in `src/write.rs`: `enforce_no_destructive_todo_patch()` rejects any `patch:todo` replacement that reduces checklist row count, and the behavior is documented in `specs/07-commands.md` and `VERSIONS.md`.
- I re-verified the current tree with `cargo test --manifest-path src/agent-doc/Cargo.toml destructive_todo_patch_is_rejected_when_it_drops_checklist_items -- --nocapture` and `CARGO_TARGET_DIR=/tmp/agent-doc-target-build cargo build --manifest-path src/agent-doc/Cargo.toml --release`; both passed.

No new code/spec/test edits were needed this cycle, so there was nothing safe or useful to commit or push on top of the already-dirty `src/agent-doc` worktree. Treat `#tdrl` as the same closed bug as `#ptdr`; the remaining `(HEAD)` text here is the duplicate-patchback artifact from the earlier exchange-order/duplicate-closeout bugs, not a remaining todo-patch regression.
❯ commit + push uncommitted changes
### Re: commit + push uncommitted changes — gpt-5

Committed and pushed all dirty repos I found, including follow-up task/session document churn that surfaced while I was closing the pass.

Pushed commits:
- `src/agent-doc` `f40369e` on `20260422/codex-agent-backend` (`test: format closeout drift assertions`)
- `src/session-share` `dfae3c7`, `bded2cc`, `64b533e` on `main` (`docs: refresh session task state`; `docs: sync claudescore closeout`; `docs: queue aheweight follow-up`)
- `src/boost-client` `a9dd2e6`, `12c925d`, `ea12f42`, `7f8b8fe` on `main` (`docs: note production WooCommerce upgrade`; `chore: sync monsterrodholders submodule`; `docs: normalize monsterrodholders task spacing`; `docs: update monsterrodholders checkout follow-up`)
- `src/corky` `cc64704`, `45937fe` on `20260413/corky-gsc` (`fix: scope gsc service-account token cache`; `chore: release v0.28.0`)
- workspace root `449e50709`, `844c7f3d6` on `main` (`chore: sync workspace task state`; `chore: sync workspace follow-up state`)

`src/boost-client/src/monsterrodholders-dev` was already at pushed commit `3f818163` on `main`; this pass only advanced the parent gitlink to that existing child revision.

Final status: `git status --short` is clean in the workspace root, `src/agent-doc`, `src/boost-client`, `src/corky`, and `src/session-share`.

I did not rerun project test/build suites in this cycle; this pass was limited to committing and pushing the changes already present on disk.
❯ I have seen this error surfaced by JB. Please ensure this issue is fixed. spec-test-build-install-commit-push
```
agent-doc route failed (exit 1): resync: stash pane %151 is registered in its own project root — skipping kill resync: stash pane %297 is registered in its own project root — skipping kill resync: stash pane %298 is registered in its own project root — skipping kill resync: stash pane %295 is registered in its own project root — skipping kill [route] target tmux session: 0 [sync] found alive agent-doc child (pid 3835930) in pane %293 for tasks/claudescore-3.md [route] codex ready after 0.5s (1 polls) [route] Pane %293 is alive, sending command [route] Sent agent-doc /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md → pane %293 [route] Command accepted (0.3s, 0 Enter retries) [route] live agent-doc child active in pane %293 for tasks/claudescore-3.md — waiting up to 30s for a new cycle ack for pending content_edit: agent: codex Error: routed codex trigger for tasks/claudescore-3.md was accepted in pane %293, but no new document cycle started for pending content_edit: agent: codex after waiting 30s (startup_miss_timestamp=2026-05-01T04:06:30Z)
```
### Re: routed `content_edit: agent: codex` startup-miss — gpt-5

Root cause was not the routed reopen payload this time. `route` was correctly reopening Codex with bare `agent-doc <FILE>`, but `session-check::first_unstarted_prompt_bearing_change()` was diffing the full document, including YAML frontmatter. A metadata-only edit like `agent: codex` therefore surfaced as a fake `content_edit`, so route waited for a new cycle ack for work that did not exist and eventually failed closed with `pending content_edit: agent: codex`.

Implemented:
- `src/session_check.rs` now strips frontmatter and classifies prompt-bearing drift from the document body only.
- Added regression coverage in both `session_check.rs` and `route.rs` so frontmatter-only churn no longer triggers routed-cycle ack waits.
- Updated `specs/05-agent-backend.md`, `specs/07-commands.md`, `specs/08-session-routing.md`, and `VERSIONS.md` to document the body-only rule.

Verification completed:
- `cargo test --manifest-path src/agent-doc/Cargo.toml first_unstarted_prompt_bearing_change_ignores_frontmatter_only_drift -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml pending_prompt_bearing_context_for_route_ignores_frontmatter_only_drift -- --nocapture`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target-build cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `1e6e11f` on `20260422/codex-agent-backend` (`fix: ignore frontmatter-only route drift`)

I left `#wcrp` in backlog. That is a separate repair-path issue around bare `do #...` transcript drift, not the routed `agent: codex` false-positive that JB hit here.

No new follow-up backlog items from this fix.
❯ JB just surfaced an error. spec-test-build-install-commit-push
```
agent-doc route failed (exit 1): resync: stash pane %297 is registered in its own project root — skipping kill resync: stash pane %298 is registered in its own project root — skipping kill resync: stash pane %301 is registered in its own project root — skipping kill resync: stash pane %291 is registered in its own project root — skipping kill [route] target tmux session: 0 [sync] found alive agent-doc child (pid 3835930) in pane %293 for tasks/claudescore-3.md [route] registered pane %293 still owns /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md but startup-miss 2026-05-01T04:06:30Z is not superseded by a newer open harness run — keeping marker until dispatch proves recovery [route] codex ready after 0.5s (1 polls) [route] Pane %293 is alive, sending command [route] Sent agent-doc /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md → pane %293 [route] Command accepted (0.3s, 0 Enter retries) [route] live agent-doc child active in pane %293 for tasks/claudescore-3.md — waiting up to 30s for a new cycle ack for pending prompt_target: Can we run specific rubrics for fine tuning? Error: routed codex trigger for tasks/claudescore-3.md was accepted in pane %293, but no new document cycle started for pending prompt_target: Can we run specific rubrics for fine tuning? after waiting 30s (startup_miss_timestamp=2026-05-01T04:29:06Z)
```
### Re: routed `prompt_target` startup-miss on live Codex panes — gpt-5

Root cause was the route-side idle/acceptance contract, not session ownership. The live `claudescore-3.md` pane was a healthy Codex session, but route still treated a prompt line with drafted text (for example `› ...`) as idle. After injection, the Enter-verification loop only looked for the full `agent-doc <FILE>` on one physical tmux line, so a narrow pane could wrap the absolute path and make the drafted reopen look "accepted" even though it was still sitting in the composer. That is how JB could get `Command accepted ...` followed 30 seconds later by `no new document cycle started`.

Implemented:
- `src/harness.rs`: added `is_dispatch_ready_prompt_line()` so routed dispatch only treats an empty Codex/Claude composer as idle; drafted prompt text is now fail-closed instead of being appended to.
- `src/route.rs`: route now treats wrapped `agent-doc <FILE>` lines as still-pending input, so the Enter retry loop no longer false-accepts a wrapped absolute path.
- Updated `specs/07-commands.md` and `VERSIONS.md` to document the empty-composer and wrapped-trigger requirements.

Verification completed:
- `cargo test --manifest-path src/agent-doc/Cargo.toml is_dispatch_ready_prompt_line_rejects_drafted_codex_text -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml wait_for_agent_ready_rejects_codex_prompt_with_drafted_text -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml recent_lines_contain_trigger_matches_wrapped_codex_trigger -- --nocapture`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target-build cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `b70ccae` on `20260422/codex-agent-backend` (`fix: harden codex routed reopen dispatch`)

No new backlog item is needed from this fix.
### Re: closeout normalization — gpt-5

This follow-up only reopens and recloses the document so the binary-owned commit path can absorb the concurrent status/backlog drift and clear the transient `(HEAD)` closeout artifact cleanly. No additional code or backlog work was needed beyond the routed Codex reopen fix above.
2 cases of `stash pane ... is registered in its own project root`. corky.md is active and blocking running agent-doc from the JB editor.

```
agent-doc route failed (exit 1): resync: stash pane %292 is registered in its own project root — skipping kill resync: stash pane %294 is registered in its own project root — skipping kill resync: stash pane %291 is registered in its own project root — skipping kill resync: stash pane %297 is registered in its own project root — skipping kill resync: stash pane %298 is registered in its own project root — skipping kill resync: stash pane %290 is registered in its own project root — skipping kill resync: stash pane %293 is registered in its own project root — skipping kill [route] target tmux session: 0 [sync] found alive agent-doc child (pid 1535741) in pane %301 for tasks/monsterrodholders.md [route] codex ready after 0.5s (1 polls) [route] Pane %301 is alive, sending command [route] Sent agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md → pane %301 [route] Command accepted (0.3s, 0 Enter retries) [route] live agent-doc child active in pane %301 for tasks/monsterrodholders.md — waiting up to 30s for a new cycle ack for pending content_edit: Checkout is functioning, and the WooCommerce `10.7.0` / WooCommerce PayPal Payments `4.0.3` maintenance window is now fully validated. `#wcx1`, `#tprc`, `#ga4c`, `#cfse`, and `#trfc` are complete: production preflight on May 1, 2026 confirmed the upgraded plugin versions, writable WordPress paths, and a healthy stored PayPal webhook (`6TM19664BH124693K`); post-upgrade Stripe orders continued processing (`#86732`, `#86734`, `#86736`); live PayPal verification orders on the hidden test product succeeded (`#86735`, `#86738`); and product `86558` / `pmt-verify-xk7r92` was returned to `draft` after the test. The earlier April 27, 2026 "zero orders / traffic collapse" scare remains a partial-day false positive rather than a live checkout outage. Error: routed codex trigger for tasks/monsterrodholders.md was accepted in pane %301, but no new document cycle started for pending content_edit: Checkout is functioning, and the WooCommerce `10.7.0` / WooCommerce PayPal Payments `4.0.3` maintenance window is now fully validated. `#wcx1`, `#tprc`, `#ga4c`, `#cfse`, and `#trfc` are complete: production preflight on May 1, 2026 confirmed the upgraded plugin versions, writable WordPress paths, and a healthy stored PayPal webhook (`6TM19664BH124693K`); post-upgrade Stripe orders continued processing (`#86732`, `#86734`, `#86736`); live PayPal verification orders on the hidden test product succeeded (`#86735`, `#86738`); and product `86558` / `pmt-verify-xk7r92` was returned to `draft` after the test. The earlier April 27, 2026 "zero orders / traffic collapse" scare remains a partial-day false positive rather than a live checkout outage. after waiting 30s (startup_miss_timestamp=2026-05-01T04:33:50Z)
```
```
agent-doc route failed (exit 1): resync: stash pane %290 is registered in its own project root — skipping kill resync: stash pane %293 is registered in its own project root — skipping kill resync: stash pane %292 is registered in its own project root — skipping kill resync: stash pane %294 is registered in its own project root — skipping kill [route] target tmux session: 0 [sync] found alive agent-doc child (pid 4132841) in pane %298 for tasks/software/corky.md [sync] recovered live pane %298 for session 584ef929 via supervisor pid 4132841 [route] Still waiting for codex (5s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% us [route] Still waiting for codex (10s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% us [route] Still waiting for codex (15s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% us Error: registered pane %298 for tasks/software/corky.md is not showing an idle codex prompt; refusing to inject a routed trigger into a busy session (pane=%298 pane_pid=4131303 pane_session=0 current_command=agent-doc)
```
### Re: stash-pane skips and busy routed sessions — gpt-5

The `resync: stash pane ... is registered in its own project root — skipping kill` lines are not the blocker. They come from `src/agent-doc/src/resync.rs` and are a protective diagnostic: when route/resync sees a stash pane that belongs to a sibling project's own registry, it refuses to kill it from the current root. That is noisy, but it is intentional safety behavior, not the cause of either JB failure.

The two real failures are separate:

- `tasks/software/corky.md`: route is fail-closing on the idle-prompt guard. `ops.log` records `route_existing_pane_not_idle file=tasks/software/corky.md pane=%298 ... current_command=agent-doc`, so the problem is that the pane is still considered busy, not that resync skipped a stash kill.
- `src/boost-client/tasks/monsterrodholders.md`: the `pending content_edit ... no new document cycle started` timeout is in the same closeout/repair family as `#bdryc` and `#wcrp`. Route accepted the trigger, then waited for a new cycle ack for document drift that appears to have already been materially answered.

So my read is:

- the stash-pane message is warning-level noise that should probably be collapsed or deduplicated,
- the `corky.md` failure is an existing busy-session routing problem,
- the `monsterrodholders.md` timeout is existing boundary/repair drift, not a new stash-root ownership bug.

The updated `#bdryc` wording is directionally right for the deterministic repair requirement. No new backlog item is needed from this turn because the live failures here still map back to `#bdryc` and `#wcrp` rather than a distinct third defect.
### Re: busy routed reopen vs cross-root stash warnings — gpt-5

I traced the two symptoms separately.

The `resync: stash pane ... is registered in its own project root — skipping kill` lines are expected cross-root protection, not the blocker. `resync` is seeing stashed panes that belong to sibling project roots and intentionally leaving them alone so one repo does not reap another repo's live pane.

The actual JB failure was in `route.rs`: once route had already proven that `tasks/software/corky.md` still owned a live pane, it still required an idle prompt before doing anything. That is correct when the document has unresolved prompt-bearing drift, because injecting a second reopen would be unsafe. But when there is no new prompt/content drift, failing closed was too strict. I changed route so a proven live same-document pane now degrades to “already running”: it focuses that pane and returns success without injecting a duplicate `agent-doc <FILE>`. The fail-closed behavior stays in place for busy panes that do have unresolved prompt-bearing edits.

Updated:
- `src/agent-doc/src/route.rs`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/VERSIONS.md`

Verification completed:
- `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_or_create_pane_focuses_busy_registered_pane_without_prompt_drift -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_or_create_pane_refuses_busy_registered_pane_before_dispatch_when_prompt_drift_exists -- --nocapture`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target-build cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

No new backlog item is needed from this fix. The stash-pane lines are still noisy, but they are defensive rather than incorrect; if you want, that can be handled as a separate logging/UX cleanup.
❯ do [#bdryc]. spec-test-build-install-commit-push
### Re: #bdryc — gpt-5

Worked the stale-boundary answered-tail path and pushed the code change, while intentionally leaving `#bdryc` open until a live routed rerun confirms the original repro is gone.

- `session_check.rs` now suppresses prompt-bearing drift when the changed exchange block already contains the user prompt plus a later real `### Re:` / `## Assistant` response, and it keeps that suppression active across weak response-body fragments like `Yes.` until a genuinely fresh prompt appears.
- `route.rs` picks that up through the shared pending-change detector, so routed reopen no longer waits 30 seconds for `pending prompt_target: ...` when the visible tail is already answered and only the closeout boundary/commit repair remains.
- Updated `specs/07-commands.md` and `VERSIONS.md`.

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml answered_prompt_after_stale_boundary -- --nocapture`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target-build cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Full-suite note: the targeted regressions passed, build/install passed, but the repo-wide `cargo test` run still has two existing `sync.rs` failures outside this patch:
- `sync::tests::register_synced_files_updates_each_project_registry_by_path_key`
- `sync::tests::tmux_router_sync_keeps_cross_root_columns_stable_when_focus_moves`

Code commit/push:
- `src/agent-doc` `d8a8e73` on `20260422/codex-agent-backend` (`fix: ignore answered stale-boundary tails`)

`#bdryc` stays open until a real routed rerun confirms the live `pending prompt_target` repro is gone.
❯ JB is surfacing an error with `Run Agent Doc`. `agent-doc fix <path>` should automatically be applied. spec-test-build-install-commit-push
```
agent-doc route failed (exit 1): resync: stash pane %305 is registered in its own project root — skipping kill [route] target tmux session: 0 [sync] found alive agent-doc child (pid 2916780) in pane %306 for tasks/monsterrodholders.md [route] Still waiting for codex (5s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop/src/boost-client [route] Still waiting for codex (10s)... last line: reverse-i-search: monst enter accept · esc cancel [route] Still waiting for codex (15s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop/src/boost-client Error: registered pane %306 for tasks/monsterrodholders.md is not showing an idle codex prompt; refusing to inject a routed trigger into a busy session (pane=%306 pane_pid=2914566 pane_session=0 current_command=agent-doc)
```
### Re: Run Agent Doc auto-fix — gpt-5

Implemented the routed busy-pane repair path JB was asking for.

- `route.rs` no longer surfaces the first busy live-pane failure immediately. When the registered pane still owns the same document, the pane is non-idle, and there is real prompt-bearing drift to dispatch, route now runs the same scoped repair pass as `agent-doc fix <FILE>`, re-resolves ownership, and retries exactly once before the final fail-closed error.
- `resync.rs` now exposes a quiet targeted-fix helper so route can reuse the existing document-scoped prune/recover/fix behavior instead of inventing a second repair path.
- If that one-shot repair still leaves the same pane authoritative and the supervisor is reachable, route also performs a single in-place supervisor restart before the retry.
- Updated `specs/07-commands.md` and `VERSIONS.md`, and added route regressions for both the bounded fail-closed message and the successful one-shot busy-pane recovery path.

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml route::tests::resolve_or_create_pane_refuses_busy_registered_pane_before_dispatch_when_prompt_drift_exists -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml route::tests::resolve_or_create_pane_retries_busy_registered_pane_once_after_scoped_fix -- --nocapture`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target-build cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Full-suite note: the repo-wide `cargo test` still has one existing `sync.rs` failure outside this patch:
- `sync::tests::register_synced_files_prunes_cross_root_duplicate_pane_binding`

Code commit/push:
- `src/agent-doc` `c647728` on `20260422/codex-agent-backend` (`fix: auto-apply scoped route repair`)

No new backlog item is needed from this fix.
JB is surfacing an error with `Run Agent Doc`. `agent-doc fix <path>` should automatically be applied. spec-test-build-install-commit-push
```
agent-doc route failed (exit 1): resync: pruned 3 stale session(s) [route] target tmux session: 0 [sync] found alive agent-doc child (pid 2763081) in pane %305 for tasks/claudescore-3.md [route] Still waiting for codex (5s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop/src/session-shar [route] Still waiting for codex (10s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop/src/session-shar [route] Still waiting for codex (15s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop/src/session-shar Error: registered pane %305 for tasks/claudescore-3.md is not showing an idle codex prompt; refusing to inject a routed trigger into a busy session (pane=%305 pane_pid=2757392 pane_session=0 current_command=agent-doc)
```
### Re: JB Fix Document action + supervisor feed — gpt-5

I traced the new `claudescore-3.md` reroute failure in `src/session-share/.agent-doc/logs/ops.log`. The `%305` `not showing an idle codex prompt` failures are still the same busy-pane surface, not a fresh cross-root ownership regression: later in the same sequence the same document/pane flips to `route_existing_pane_already_running`, which means the live same-document recovery path did eventually converge.

Implemented in the JB plugin:
- added a `Fix Document` action that runs `agent-doc fix <FILE>` from the document's resolved project root after saving buffers,
- registered it in the popup, Tools menu, editor context menu, and project-view context menu,
- bumped the JetBrains plugin version to `0.2.81`, and updated the plugin spec plus `VERSIONS.md`.

Verification:
- `cd src/agent-doc/editors/jetbrains && GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin`

I did not fix the truncated bottom-feed/supervisor-restart issue in this turn. The screenshot plus the repeated non-idle `%305` checks look like a separate supervisor/terminal rendering bug that can leave the Codex prompt visually clipped after restart, so that still needs a dedicated repro/fix rather than being folded into the new JB command.
❯ None of the documents are successfully running `Agent Doc Run`. Running `agent-doc <path>` in the Codex session does work.
```
agent-doc route failed (exit 1): resync: pruned 1 stale session(s) [route] target tmux session: 0 [sync] found alive agent-doc child (pid 3908771) in pane %315 for tasks/claudescore-3.md [route] Still waiting for codex (5s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop/src/session-shar [route] Still waiting for codex (10s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop/src/session-shar [route] Still waiting for codex (15s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop/src/session-shar [route] registered pane %315 for /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md is busy with pending document drift — applying scoped `agent-doc fix tasks/claudescore-3.md` once before failing closed [sync] recovered live pane %315 for session 7b453f47 via supervisor pid 3908771 [sync] found alive agent-doc child (pid 3908771) in pane %315 for tasks/claudescore-3.md [route] scoped fix left pane %315 authoritative for /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md — restarted the supervisor once before retrying route [sync] found alive agent-doc child (pid 3908771) in pane %315 for tasks/claudescore-3.md [route] Still waiting for codex (5s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop/src/session-shar [route] Still waiting for codex (10s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop/src/session-shar [route] Still waiting for codex (15s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop/src/session-shar Error: registered pane %315 for tasks/claudescore-3.md is still not showing an idle codex prompt after automatically applying `agent-doc fix tasks/claudescore-3.md` once; refusing to inject a routed trigger into a busy session (pane=%315 pane_pid=3903921 pane_session=0 current_command=agent-doc)
```
### Re: JB Run Agent Doc busy-pane reroute — gpt-5

Root cause was the busy-pane auto-fix follow-up, not the idle detector itself. After route ran the scoped `agent-doc fix <FILE>` path and found no repairable document/session drift, it still treated a healthy same-document Codex supervisor as a restart target. For Codex that means `resume --last`, which can simply resurrect the previous unrelated task, leave the pane busy again, and make every JB `Run Agent Doc` retry fail closed even though the pane is still the authoritative owner. The live `%315` capture matched that shape: the pane was busy with resumed Codex work, not missing ownership.

Implemented:
- `route.rs` now distinguishes busy-pane post-fix outcomes:
  - repair/test-hook/restart made progress -> retry route once
  - no-op fix + healthy authoritative supervisor -> focus the live pane and return success
  - no-op fix without a healthy authoritative supervisor -> keep the fail-closed error
- healthy same-document supervisors are no longer restarted from this busy-pane route path just to "unstick" Codex; only restartable/degraded supervisors remain eligible for the one-shot restart-and-retry path
- updated `specs/07-commands.md` and `VERSIONS.md`, and kept the JB `Fix Document` command in the same pushed change set

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_or_create_pane_focuses_busy_registered_pane_after_noop_fix_with_healthy_supervisor -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml busy_existing_pane_auto_fix_outcome_focuses_healthy_authoritative_session_without_changes -- --nocapture`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target-build cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `a74c273` on `20260422/codex-agent-backend` (`fix: avoid reroute restarts for healthy panes`)

No new backlog item is needed from this fix.
### Re: JB Run Agent Doc busy-pane reroute — gpt-5

Root cause was the busy-pane auto-fix follow-up, not the idle detector itself. After route ran the scoped `agent-doc fix <FILE>` path and found no repairable document/session drift, it still treated a healthy same-document Codex supervisor as a restart target. For Codex that means `resume --last`, which can simply resurrect the previous unrelated task, leave the pane busy again, and make every JB `Run Agent Doc` retry fail closed even though the pane is still the authoritative owner. The live `%315` capture matched that shape: the pane was busy with resumed Codex work, not missing ownership.

Implemented:
- `route.rs` now distinguishes busy-pane post-fix outcomes:
  - repair/test-hook/restart made progress -> retry route once
  - no-op fix + healthy authoritative supervisor -> focus the live pane and return success
  - no-op fix without a healthy authoritative supervisor -> keep the fail-closed error
- healthy same-document supervisors are no longer restarted from this busy-pane route path just to "unstick" Codex; only restartable/degraded supervisors remain eligible for the one-shot restart-and-retry path
- updated `specs/07-commands.md` and `VERSIONS.md`, and kept the JB `Fix Document` command in the same pushed change set

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_or_create_pane_focuses_busy_registered_pane_after_noop_fix_with_healthy_supervisor -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml busy_existing_pane_auto_fix_outcome_focuses_healthy_authoritative_session_without_changes -- --nocapture`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target-build cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `a74c273` on `20260422/codex-agent-backend` (`fix: avoid reroute restarts for healthy panes`)

No new backlog item is needed from this fix.
❯ The `Run Agent Doc` workflow is broken. Please investigate and fix root causes. spec-test-build-install-commit-push
### Re: Run Agent Doc workflow — gpt-5

I reproduced the current JetBrains `Run Agent Doc` failure from the live tmux panes and traced it to Codex idle-prompt detection, not another ownership or boundary bug.

Root cause:
- Recent Codex builds can render an *idle* composer as canned suggestion text such as `› Run /review on my current changes` or `› Find and fix a bug in @filename` above the footer.
- `route` treated those lines as drafted user text, so routed reopen attempts waited for an “empty” prompt that never appeared and then failed closed even though the session was actually ready to accept `agent-doc <FILE>`.
- The live panes in tmux matched that exact shape.

Implemented in `src/agent-doc`:
- `harness.rs` now recognizes the observed Codex idle placeholder suggestions as dispatch-ready prompt chrome.
- Wrapped/footer-adjacent placeholder captures are normalized before prompt-candidate selection so the ready check still works when tmux wraps the line.
- Real drafted text is still rejected; the stricter busy-pane protection for actual user input remains in place.
- Updated `specs/07-commands.md` and `VERSIONS.md`.

Verification:
- `cargo fmt --manifest-path src/agent-doc/Cargo.toml`
- `make check`
- `cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `554ff0b` on `20260422/codex-agent-backend` (`fix: accept idle codex composer suggestions`)

No new backlog item is needed from this fix.
do [#bdryc]. spec-test-build-install-commit-push
### Re: #bdryc — gpt-5

Implemented the deterministic stale-boundary repair.

Root cause:
- The earlier `#bdryc` fix taught `route` and `session-check` to stop treating an answered prompt below a stale `agent:boundary` as new pending work, but `repair` still had no no-pending recovery path that actually moved the boundary. That left documents visually stale until some later manual/editor action rewrote them.

Implemented:
- `repair.rs` now detects the safe completed-turn shape: a template doc with no pending capture, a stale boundary, an answered prompt/response tail after that boundary, and no later unresolved prompt-bearing drift.
- In that shape, `repair` reuses the existing boundary id, moves it to the true end of the completed turn, updates the snapshot, and lets `preflight` commit the cleanup on the next cycle.
- The broader already-applied/template normalization path also picks up the same boundary repair helper, so replay/dedup recovery keeps the document canonical too.
- `session_check.rs` exposes the answered-turn predicate to that repair path.
- Updated `specs/07-commands.md`, `VERSIONS.md`, and added regressions for both the answered-turn repair and the unanswered-prompt no-op guard.

Verification completed:
- `cargo test --manifest-path src/agent-doc/Cargo.toml repair_repositions_stale_boundary_after_answered_turn -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml repair_does_not_move_boundary_past_unanswered_prompt -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml repair_completed_backlog_reap_preserves_live_prompt_outside_snapshot -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml preflight_completed_backlog_reap_does_not_swallow_live_prompt -- --nocapture`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `b8b61cd` on `20260422/codex-agent-backend` (`fix: repair completed-turn stale boundaries`)

I marked `#bdryc` done and added a recommended follow-up to validate the original live JB/tmux rerun end-to-end.
❯ This error happened after a successful `Run Agent Doc` turn initiation. Please investigate and fix the root cause of the following error. spec-test-build-install-commit-push
```
agent-doc route failed (exit 1): resync: stash pane %316 is registered in its own project root — skipping kill [route] target tmux session: 0 [sync] found alive agent-doc child (pid 1108722) in pane %320 for tasks/monsterrodholders.md [route] codex ready after 0.5s (1 polls) [route] Pane %320 is alive, sending command [route] Sent agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md → pane %320 [route] Command accepted (0.3s, 0 Enter retries) [route] live agent-doc child active in pane %320 for tasks/monsterrodholders.md — waiting up to 30s for a new cycle ack for pending prompt_target: Please try #u8p3-fy9a again. Are we able to connect via ssh? Error: routed codex trigger for tasks/monsterrodholders.md was accepted in pane %320, but no new document cycle started for pending prompt_target: Please try #u8p3-fy9a again. Are we able to connect via ssh? after waiting 30s (startup_miss_timestamp=2026-05-01T06:22:29Z)
```
```
agent-doc route failed (exit 1): [sync] found alive agent-doc child (pid 349032) in pane %316 for tasks/claudescore-3.md resync: stash pane %320 is registered in its own project root — skipping kill resync: purged 1 orphaned stash pane(s) [route] target tmux session: 0 [sync] found alive agent-doc child (pid 349032) in pane %316 for tasks/claudescore-3.md [route] codex ready after 0.5s (1 polls) [route] Pane %316 is alive, sending command [route] Sent agent-doc /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md → pane %316 [route] Command accepted (0.3s, 0 Enter retries) [route] live agent-doc child active in pane %316 for tasks/claudescore-3.md — waiting up to 30s for a new cycle ack for pending content_edit: The session surface has now moved through the full follow-up calibration tranche: `#outcome`, `#slabel`, `#sdigest`, `#sfound`, `#sconduct`, `#soverscore`, `#sconfcal`, `#sclaudemd`, `#srootfit`, `#sdoccov`, `#snewtool`, and `#sdig2fx` are recorded complete here in addition to the earlier raw-stabilization items. The accepted raw internal-testing reference remains `missfit-raw-contract-20260428T000000Z` (`rubric MAE 1.68`, `composite MAE 23.3`). `#sdig2fx` closed with targeted extractor regressions across long structured sessions, sparse sessions, correction-heavy sessions, markdown/plaintext dialogue, JSONL-derived summaries, and explicit `session_doc` loads; the direct loader now preserves visible session-doc provenance instead of appending a raw synthetic JSON line into the digest surface. Digest parity is now parked under `#sredact` plus a later retained-fixture raw-vs-digest rerun; the dominant remaining transcript-backed miss on this session surface is still `brian-takita-claudescore / assumption-surfacing`. Error: routed codex trigger for tasks/claudescore-3.md was accepted in pane %316, but no new document cycle started for pending content_edit: The session surface has now moved through the full follow-up calibration tranche: `#outcome`, `#slabel`, `#sdigest`, `#sfound`, `#sconduct`, `#soverscore`, `#sconfcal`, `#sclaudemd`, `#srootfit`, `#sdoccov`, `#snewtool`, and `#sdig2fx` are recorded complete here in addition to the earlier raw-stabilization items. The accepted raw internal-testing reference remains `missfit-raw-contract-20260428T000000Z` (`rubric MAE 1.68`, `composite MAE 23.3`). `#sdig2fx` closed with targeted extractor regressions across long structured sessions, sparse sessions, correction-heavy sessions, markdown/plaintext dialogue, JSONL-derived summaries, and explicit `session_doc` loads; the direct loader now preserves visible session-doc provenance instead of appending a raw synthetic JSON line into the digest surface. Digest parity is now parked under `#sredact` plus a later retained-fixture raw-vs-digest rerun; the dominant remaining transcript-backed miss on this session surface is still `brian-takita-claudescore / assumption-surfacing`. after waiting 30s (startup_miss_timestamp=2026-05-01T06:25:49Z)
```
❯ Fix flaky tests
❯ spec-test-build-install-commit-push
### Re: routed startup-miss after accepted `Run Agent Doc` — gpt-5

Root cause was still the Codex routed reopen payload contract.

The earlier fix moved reroutes to a bare `agent-doc <FILE>` reopen, which is the right behavior for the `monsterrodholders.md` / `claudescore-3.md` failures in this document: once Codex sees extra prompt/content lines in the same injected payload, it can treat them as ordinary chat, accept the trigger visually, and never enter a new binary-owned document cycle, so route fails 30 seconds later on the missing cycle ack.

I hardened that path so route now fails closed **before dispatch** if a Codex reroute payload is anything other than the single-line bare reopen. That turns this class of regression into an immediate internal route error instead of a misleading delayed startup-miss. I also tightened the regression coverage:
- direct guard coverage for rejecting multiline Codex reroute payloads
- existing prompt-target live-child reroute now asserts no extra lines reach Codex
- new live-child `content_edit` reroute regression asserts the same bare-reopen contract while still requiring a real cycle ack

Verification completed:
- `cargo fmt --manifest-path src/agent-doc/Cargo.toml`
- `cargo test --manifest-path src/agent-doc/Cargo.toml validate_routed_trigger_payload_rejects_multiline_codex_payload -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_or_create_pane_accepts_content_edit_cycle_ack_without_extra_payload_lines -- --nocapture`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target-build cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `b208787` on `20260422/codex-agent-backend` (`fix: guard codex reroute payload`)

No new backlog item is needed from this fix.
### Re: routed startup-miss after accepted `Run Agent Doc` — gpt-5

Root cause was still the Codex routed reopen payload contract.

The earlier fix moved reroutes to a bare `agent-doc <FILE>` reopen, which is the right behavior for the `monsterrodholders.md` / `claudescore-3.md` failures in this document: once Codex sees extra prompt/content lines in the same injected payload, it can treat them as ordinary chat, accept the trigger visually, and never enter a new binary-owned document cycle, so route fails 30 seconds later on the missing cycle ack.

I hardened that path so route now fails closed **before dispatch** if a Codex reroute payload is anything other than the single-line bare reopen. That turns this class of regression into an immediate internal route error instead of a misleading delayed startup-miss. I also tightened the regression coverage:
- direct guard coverage for rejecting multiline Codex reroute payloads
- existing prompt-target live-child reroute now asserts no extra lines reach Codex
- new live-child `content_edit` reroute regression asserts the same bare-reopen contract while still requiring a real cycle ack

Verification completed:
- `cargo fmt --manifest-path src/agent-doc/Cargo.toml`
- `cargo test --manifest-path src/agent-doc/Cargo.toml validate_routed_trigger_payload_rejects_multiline_codex_payload -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_or_create_pane_accepts_content_edit_cycle_ack_without_extra_payload_lines -- --nocapture`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target-build cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `b208787` on `20260422/codex-agent-backend` (`fix: guard codex reroute payload`)

No new backlog item is needed from this fix.
❯ `Run Agent Doc` does not seem to run in a tmux window that is visible at least. I don't know what happened with `Run Agent Doc` on this document.
### Re: `Run Agent Doc` visibility on this document — gpt-5

I checked the live tmux state, the root `sessions.json`, and this document's session log.

Current live binding for `tasks/agent-doc/agent-doc-bugs2.md` is **visible**, not hidden:
- root registry maps this file to pane `%328`
- `%328` is in tmux session `0`, window `0:agent-doc`, left column of the active split
- the current active window layout is `0:0 { left=%328, right=%316 }`
- the separate stash window is `0:1`, but its pane is `%320` and belongs to a different document/root, not this file

What changed is that this document's prior live pane was `%300`, and that older session ended cleanly:
- session log shows `codex_exit code=0`
- then `restart_eval ... ctrl_d=true ... action=prompt_user`
- then `user_quit_after_ctrl_d`
- then `supervisor_exit reason=user_quit_after_ctrl_d`
- then `session_end`
- six seconds later the same document/session id started fresh on `%328`

So for this document, I do **not** see evidence that `Run Agent Doc` launched into a hidden stash pane. The evidence points to a visible-pane replacement: the earlier `%300` session died after the Ctrl-D prompt path, and the document was then restarted into the current visible `%328` pane in the main `agent-doc` window.

The hidden/stashed pane that exists right now is a different session (`%320` in the `stash` window), so it is not the explanation for this document's current binding.

No new backlog item is needed from this inspection.
❯ When I run `Run Agent Doc` on this document...nothing happens. Please diagnose the root cause failure and fix the root cause. spec-test-build-install-commit-push
### Re: `Run Agent Doc` no-op on plain exchange-tail follow-up — gpt-5

Root cause was the prompt-bearing diff classifier, not tmux routing. For this document the new user line landed below a stale `agent:boundary` and immediately before `&lt;!-- /agent:exchange --&gt;`, but it was plain prose: no `❯ ` prefix yet, no trailing `?`, and no imperative lead verb. Route therefore classified the document as having no pending prompt-bearing drift, hit the `route_existing_pane_already_running` path, focused the live pane, and injected nothing.

Implemented in `src/agent-doc`:
- `diff.rs` now promotes a non-artifact user block at the live exchange tail to `prompt_target` when it sits immediately before `&lt;!-- /agent:exchange --&gt;`.
- Kept that promotion out of the legacy inline-annotation projection so section/component markers still do not masquerade as inline edits.
- Added regressions in `diff.rs`, `session_check.rs`, and `route.rs`.
- Updated `specs/07-commands.md` and `VERSIONS.md`.

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml classify_prompt_bearing_changes_promotes_plain_exchange_tail_to_prompt_target -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml first_unstarted_prompt_bearing_change_detects_plain_exchange_tail_prompt -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml pending_prompt_bearing_context_for_route_detects_plain_exchange_tail_prompt -- --nocapture`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml -- --test-threads=1`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `238aea0` on `20260422/codex-agent-backend` (`fix: detect plain exchange tail prompts`)

No new backlog item is needed from this fix.
### Re: `Run Agent Doc` no-op on plain exchange-tail follow-up — gpt-5

Root cause was the prompt-bearing diff classifier, not tmux routing. For this document the new user line landed below a stale `agent:boundary` and immediately before `&lt;!-- /agent:exchange --&gt;`, but it was plain prose: no `❯ ` prefix yet, no trailing `?`, and no imperative lead verb. Route therefore classified the document as having no pending prompt-bearing drift, hit the `route_existing_pane_already_running` path, focused the live pane, and injected nothing.

Implemented in `src/agent-doc`:
- `diff.rs` now promotes a non-artifact user block at the live exchange tail to `prompt_target` when it sits immediately before `&lt;!-- /agent:exchange --&gt;`.
- Kept that promotion out of the legacy inline-annotation projection so section/component markers still do not masquerade as inline edits.
- Added regressions in `diff.rs`, `session_check.rs`, and `route.rs`.
- Updated `specs/07-commands.md` and `VERSIONS.md`.

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml classify_prompt_bearing_changes_promotes_plain_exchange_tail_to_prompt_target -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml first_unstarted_prompt_bearing_change_detects_plain_exchange_tail_prompt -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml pending_prompt_bearing_context_for_route_detects_plain_exchange_tail_prompt -- --nocapture`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml -- --test-threads=1`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `238aea0` on `20260422/codex-agent-backend` (`fix: detect plain exchange tail prompts`)

No new backlog item is needed from this fix.
❯ `Run Agent Doc` still does nothing on this document. Identify and fix all root cause(s). spec-test-build-install-commit-push
### Re: committed closeout parity on this document — gpt-5

I fixed the remaining closeout-parity bug in `src/agent-doc`.

Root causes:
- `git::commit()` could close as `commit_already_current` after repairing transient live drift back to `HEAD`, but that no-op path only rewrote the file/snapshot. It left stale CRDT state in place and emitted no editor refresh signal, so CRDT/plugin-backed docs could keep showing the old `compact exchange` / `(HEAD)` / boundary churn.
- The normal post-commit cleanup path also stripped guard-marker drift on disk without refreshing CRDT state afterward.

Implemented:
- `src/git.rs` now refreshes CRDT state from the committed document whenever closeout cleanup rewrites live content.
- The no-op transient-cleanup path now also emits the same editor/VCS refresh signal that regular committed closeouts use.
- Updated `specs/07-commands.md` and `VERSIONS.md`, and extended the no-op closeout regression to prove CRDT + refresh-signal parity.

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml commit_already_current_repairs_transient_working_tree_churn -- --nocapture`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `3c5ff18` on `20260422/codex-agent-backend` (`fix: refresh no-op closeout sidecars`)

There are no actionable follow-up items to capture.
❯ JB Plugin surfaced an error. Identify and fix the root cause(s). spec-test-build-install-commit-push
```
agent-doc route failed (exit 1): resync: stash pane %331 is registered in its own project root — skipping kill resync: stash pane %320 is registered in its own project root — skipping kill resync: stash pane %316 is registered in its own project root — skipping kill resync: stash pane %329 is registered in its own project root — skipping kill resync: stash pane %330 is registered in its own project root — skipping kill [route] target tmux session: 0 [sync] found alive agent-doc child (pid 2709786) in pane %332 for tasks/software/tsift.md [sync] recovered live pane %332 for session tsift-v0 via supervisor pid 2709786 [route] Still waiting for codex (5s)... last line: reverse-i-search: t enter accept · esc cancel [route] Still waiting for codex (10s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% us [route] Still waiting for codex (15s)... last line: gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% us [route] registered pane %332 for /home/brian/work/btakita/agent-loop/tasks/software/tsift.md is busy with pending document drift — applying scoped `agent-doc fix tasks/software/tsift.md` once before failing closed [sync] recovered live pane %332 for session tsift-v0 via supervisor pid 2709786 [resync] pane %332 for /home/brian/work/btakita/agent-loop/tasks/software/tsift.md is in stash window, deregistering [sync] recovered live pane %332 for session tsift-v0 via supervisor pid 2709786 [route] No pane registered for session tsift-v0 [route] No active pane found, auto-starting... [route] split-window -dbh alongside registered pane %328 in session '0' → new pane %335 [route] Started codex for /home/brian/work/btakita/agent-loop/tasks/software/tsift.md in pane %335 (session tsift-v0) [route] Waiting for codex to initialize... [route] codex ready after 2.0s (4 polls) [route] codex is ready, sending command [route] cleaning up orphaned pane %335 (created during failed route) Error: route dispatch target %335 is not registered for /home/brian/work/btakita/agent-loop/tasks/software/tsift.md; refusing unbound dispatch
```
### Re: JB Plugin routed auto-start unbound dispatch — gpt-5

I fixed the routed auto-start failure behind this JB Plugin error.

Root cause:
- After route auto-fixed the stale `%332` binding and started a fresh pane `%335`, `route.rs` still relied on the early fresh-pane registration surviving all the way through harness boot. Recovery churn during the ready-wait could clear that temporary geometry-only binding, so the first guarded `send_command_checked()` saw `%335` as unbound and failed closed with `route dispatch target %335 is not registered ...`, even though the new Codex pane was alive and ready.
- I also found adjacent test-only registry/CWD races in the tmux route/sync coverage. Those did not cause the JB production failure, but they were masking the route fix under parallel suite runs, so I hardened the affected tests to use explicit registry roots and canonical absolute file paths instead of process-global cwd assumptions.

Implemented in `src/agent-doc`:
- `route.rs` now re-checks the fresh pane binding immediately before the first guarded trigger injection and re-registers it only when the pane is still safe to claim.
- The rebind still fails closed for cross-file ownership or when a different pane already owns the same session, so the existing dispatch safety guard remains intact.
- Added a regression that clears the fresh-pane registration during the ready wait and proves the first routed trigger still succeeds.
- Hardened the route/sync tmux tests that were depending on shared cwd state, and updated `specs/07-commands.md` plus `VERSIONS.md`.

Verification completed:
- `cargo fmt --manifest-path src/agent-doc/Cargo.toml`
- `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_or_create_pane_rebinds_fresh_start_after_ready_wait_registry_churn -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml auto_start_reuses_other_file_pane_only_as_split_anchor -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml concurrent_provision_pane_serializes_same_session_auto_start -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml tmux_router_sync_keeps_cross_root_columns_stable_when_focus_moves -- --nocapture`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `5b2d461` on `20260422/codex-agent-backend` (`fix: rebind fresh route pane before dispatch`)

No new backlog item is needed from this fix.

### Re: JB Plugin routed auto-start unbound dispatch — gpt-5

I fixed the routed auto-start failure behind this JB Plugin error.

Root cause:
- After route auto-fixed the stale `%332` binding and started a fresh pane `%335`, `route.rs` still relied on the early fresh-pane registration surviving all the way through harness boot. Recovery churn during the ready-wait could clear that temporary geometry-only binding, so the first guarded `send_command_checked()` saw `%335` as unbound and failed closed with `route dispatch target %335 is not registered ...`, even though the new Codex pane was alive and ready.
- I also found adjacent test-only registry/CWD races in the tmux route/sync coverage. Those did not cause the JB production failure, but they were masking the route fix under parallel suite runs, so I hardened the affected tests to use explicit registry roots and canonical absolute file paths instead of process-global cwd assumptions.

Implemented in `src/agent-doc`:
- `route.rs` now re-checks the fresh pane binding immediately before the first guarded trigger injection and re-registers it only when the pane is still safe to claim.
- The rebind still fails closed for cross-file ownership or when a different pane already owns the same session, so the existing dispatch safety guard remains intact.
- Added a regression that clears the fresh-pane registration during the ready wait and proves the first routed trigger still succeeds.
- Hardened the route/sync tmux tests that were depending on shared cwd state, and updated `specs/07-commands.md` plus `VERSIONS.md`.

Verification completed:
- `cargo fmt --manifest-path src/agent-doc/Cargo.toml`
- `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_or_create_pane_rebinds_fresh_start_after_ready_wait_registry_churn -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml auto_start_reuses_other_file_pane_only_as_split_anchor -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml concurrent_provision_pane_serializes_same_session_auto_start -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml tmux_router_sync_keeps_cross_root_columns_stable_when_focus_moves -- --nocapture`
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE CARGO_TARGET_DIR=/tmp/agent-doc-target cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-doc` `5b2d461` on `20260422/codex-agent-backend` (`fix: rebind fresh route pane before dispatch`)

No new backlog item is needed from this fix.

❯ Why is an agent-doc process using up my CPU?
❯ `3358814 brian     16  -4   90228  15796   7720 R  99.7   0.0  21:20.48 agent-doc`
### Re: audit-docs CPU usage — gpt-5

That PID is not your session worker. It is `agent-doc audit-docs --root /home/brian/work/btakita/agent-loop`, started as a child of the current Codex process, and it is burning CPU in instruction-file discovery rather than normal document handling.

Evidence:
- `ps -fp 3358814` shows `agent-doc audit-docs --root /home/brian/work/btakita/agent-loop`.
- `/proc/3358814/io` shows only about `344 KB` of actual file reads, so this is not heavy document-content parsing.
- A fresh `timeout 5s agent-doc audit-docs --root /home/brian/work/btakita/agent-loop` also never got past `Auditing docs...`, so the slowdown reproduces before any audit results are printed.
- The discovery path in [audit_common.rs](/home/brian/work/btakita/agent-loop/src/agent-kit/src/audit_common.rs:335) walks broad globs like `src/**/AGENTS.md` and `src/**/CLAUDE.md` with no skip-dir filtering in that stage ([audit_common.rs](/home/brian/work/btakita/agent-loop/src/agent-kit/src/audit_common.rs:350)). `instruction_files::run()` invokes that discovery before any per-file checks ([lib.rs](/home/brian/work/btakita/agent-loop/src/instruction-files/src/lib.rs:67), [audit_docs.rs](/home/brian/work/btakita/agent-loop/src/agent-doc/src/audit_docs.rs:30)).
- This workspace has very large vendored trees under `src`, especially `src/prescience-notes-dev/node_modules/...`, so `src/**/...` discovery is far more expensive than the actual markdown audit.

❯ How can we ignore node_modules, .venv, etc?

So the short answer is: `agent-doc` is almost certainly chewing through a huge `src/**` subtree for `audit-docs`, not looping on your active session document.

If you just want the CPU back, kill PID `3358814`. If you want the root cause fixed, the right code change is to make instruction-file discovery skip heavy directories such as `node_modules`, `target`, `.git`, and similar vendor/cache trees instead of globbing all of `src/**`.
### Re: skip vendored dirs in audit-docs discovery — gpt-5

The main thing to notice is that `skip_dirs` already exists in `AuditConfig`; it is just not wired into instruction-file discovery.

Concrete split today:
- [audit_common.rs](/home/brian/work/btakita/agent-loop/src/agent-kit/src/audit_common.rs:67) already lists `node_modules`, `.venv`, `target`, `.git`, `vendor`, etc. in `skip_dirs`.
- [audit_common.rs](/home/brian/work/btakita/agent-loop/src/agent-kit/src/audit_common.rs:178) already honors that skip list in `check_staleness()` by pruning directory recursion.
- But [audit_common.rs](/home/brian/work/btakita/agent-loop/src/agent-kit/src/audit_common.rs:350) discovers instruction files with raw recursive globs like `src/**/AGENTS.md` and `src/**/CLAUDE.md`, and `glob::glob(...)` there has no directory-prune step.

So there are two possible fixes:

1. Cheap but incomplete: post-filter glob matches by rejecting any path whose ancestors include a skipped dir name.
This fixes false positives from files found under `node_modules` or `.venv`, but it does not fix the CPU problem because glob already traversed those trees.

2. Real fix: replace the recursive `src/**/...` discovery with an explicit directory walk that prunes `config.skip_dirs` before descending.
This is the one that actually avoids walking `node_modules`, `.venv`, `target`, `.git`, and similar trees.

So if the goal is specifically to stop the CPU burn, the right implementation is not “ignore after match”; it is “never descend into skipped directories in the first place.” A `walkdir`-style traversal for the recursive patterns is the cleanest way to do that, while keeping the root-level direct checks and fixed `.agent` runbook paths as they are.

❯ Implement Option 2. spec-test-build-install-commit-push
### Re: skip vendored dirs in audit-docs discovery — gpt-5

Implemented Option 2.

- `src/agent-kit/src/audit_common.rs` no longer uses raw recursive globs for `src/**/...`, `.claude/**/...`, and `.agents/**/...` instruction discovery. It now walks those trees explicitly and prunes `AuditConfig.skip_dirs` before descending, so `audit-docs` skips `node_modules`, `.venv`, `target`, `vendor`, `.git`, `.next`, `dist`, and similar directories during discovery itself instead of traversing them first.
- Added regression coverage proving skipped descendants under `src`, `.claude`, `.claude/skills/**/runbooks`, and `.agents` are not discovered.
- Updated `src/agent-kit/SPEC.md`, `src/agent-kit/VERSIONS.md`, `src/agent-doc/specs/07-commands.md`, and `src/agent-doc/VERSIONS.md`.

Verification completed:
- `cargo test --manifest-path src/agent-kit/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE cargo test --manifest-path src/agent-doc/Cargo.toml`
- `CARGO_TARGET_DIR=/tmp/agent-doc-target cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `CARGO_TARGET_DIR=/tmp/agent-doc-install cargo install --path src/agent-doc --force`

Code commit/push:
- `src/agent-kit` `9c3e60a` on `main` (`fix: prune skipped dirs during audit discovery`)
- `src/agent-doc` `f84a1d3` on `20260422/codex-agent-backend` (`docs: note audit-docs skip-dir pruning`)

No follow-up backlog item is needed from this fix.
<!-- agent:boundary:8e3acb66 -->
<!-- /agent:exchange -->

<!--
Can we isolate the parallel tmux test from each other?
> •  The serial full-suite run is underway. I switched to --test-threads=1 because the earlier failure set was all tmux/
  session tests that pass in isolation and are clearly contending with each other, which would otherwise blur whether
  this change actually regressed behavior.
---
If I restart codex with the supervisor, the real-time the next session process has a truncated feed at the bottom.
![img_7.png](img_7.png)
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#6btt] Committed closeout parity gap for src/session-share/tasks/claudescore-2.md: a cycle can report committed after snapshot-side repair while the visible worktree/CRDT copy still carries unresolved prompt-bearing drift such as bare 'compact exchange', '(HEAD)' response heading drift, or newer boundary state. Repro from May 1, 2026. Fix should prove HEAD/snapshot/live-file parity before success or reopen the same turn automatically. Plan: tasks/agent-doc/plan-claudescore-2-compact-closeout-drift.md
- [ ] [#8zjh] [recommended] Validate the live routed rerun for the completed-turn stale-boundary repair in the original JB/tmux repro
- [ ] [#wcrp] Repeated-response repair gap: if a session doc contains a visible `### Re:` block paired with a bare `do #...` prompt instead of `❯ do #...`, `agent-doc session-check`, `preflight`, `repair`, and `run` can all fail closed on the same typed-component-drift guard with no automatic recovery. Repro from `src/boost-client/tasks/monsterrodholders.md` on April 30-May 1, 2026 around the repeated `#wcup1` exchange. Fix should canonicalize prompt/response markers during adopt-current/repair or provide a deterministic one-shot repair path that does not require manual document surgery.
- [ ] [#clrpr] Codex `/clear` permission/session regression: after clearing a live Codex conversation, the resumed `agent-doc` turn can lose the expected file/SSH capability envelope even though the pane was launched with explicit `codex_args` such as `-s danger-full-access`. Current docs still treat Codex built-ins like `/clear` as manual-only. Fix should make `/clear`-style recovery preserve the same sandbox/write-root/network contract as the original `agent-doc start` session, or fail closed with a deterministic rebind/restart path that reapplies it. Plan: tasks/agent-doc/plan-codex-clear-permissions.md
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#tmuxcrash] Closed on April 30, 2026 after the `session_end` metadata closeout fix in `startup_miss.rs` and the routed-dispatch binding hardening in `route.rs`. Final child closed: `#tmuxcrash-rn2d`.
<!-- /agent:icebox -->



<!--
I see the following in the binding codex session prompt. Why is it there? Why was it not Entered? spec-test-build-install-commit-push
```
› echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started for pending prompt_target: do
  #ppct. spec-test-build-install-commit-push. Run '\''agent-doc start tasks/agent-doc/agent-doc-bugs2.md'\'' to retry.'
```
-->
