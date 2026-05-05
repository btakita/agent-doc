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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260505-053952.md`*

Active backlog:
- [ ] [#jbdirsubmit] [recommended] Replay JetBrains/VS Code Run Agent Doc + Clear Session Context after direct tmux submit-path unification and confirm Codex no longer drafts literal \\n and Run Agent Doc no longer surfaces still booting.
- [ ] [#jbqbusy] [recommended] Replay JetBrains Run Agent Doc after the authoritative busy/starting queue fix and confirm reroutes now enqueue instead of fail-closing on supervisor busy/starting state.
- [ ] [#jbhalias] [recommended] Replay JetBrains Run Agent Doc against src/session-share/tasks/buildparty-investor-demo/dev.md after the canonical Claude harness alias fix and confirm authoritative reroutes no longer fail on claude-code/claude mismatch.
- 8 more active item(s)

Icebox:
- [ ] [#2dkn] Create a plan for a sqlite db store to manage sessions for better concurrency/parallelism support
- [ ] [#dyvw] Empty placeholder item — moved to icebox to unblock preflight.
- [ ] [#xthm] Historical malformed duplicate backlog id retained for guard continuity. Canonical replacement:
- 1 more active item(s)
❯ Fix all clippy warnings
```
• Ran cargo build --manifest-path src/agent-doc/Cargo.toml --release && cargo install --path src/agent-doc --force
  └ warning: function `send_command_once_checked` is never used
        --\> src/route.rs:3477:4
    … +106 lines (ctrl + t to view transcript)
        Replaced package `agent-doc v0.33.16 (/home/brian/work/btakita/agent-loop/src/agent-doc)` with `agent-doc
    v0.33.16 (/home/brian/work/btakita/agent-loop/src/agent-doc)` (executable `agent-doc`)
```
### Re: clippy cleanup — gpt-5

Fixed the `agent-doc` clippy warning surface across `route`, `sync`, `sessions`, support modules, and the debounce/finalize tests. The largest code changes were the dispatch-only ready-probe loop rewrite in `route.rs`, pruning or test-gating dead helpers, and tightening the test fixtures so the warning cleanup stays green.

Verification:
- `cargo clippy --manifest-path src/agent-doc/Cargo.toml --all-targets --all-features`
- `cargo test --manifest-path src/agent-doc/Cargo.toml --all-targets --all-features`
- `cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `cargo install --path src/agent-doc --force`

Clippy is clean now. Cargo still prints `ignoring invalid dependency module-harness which is missing a lib target`; that is a Cargo manifest/dependency warning rather than a Clippy lint. The local `agent-doc` binary was rebuilt and reinstalled successfully.
❯ In the JB Plugin context menu and in the equivalent VSCode plugin menu:
❯ - Remove `Run With Junie` from the numbered hotkeys but keep the menu item.
❯ - Remove `Force Claim for Tmux Pane` from the numbered hotkeys but keep the menu item.
❯ - Add `Compact Exchange` into a numbered hotkey
❯ - Add `Restart Supervisor Process` into a numbered hotkey. Implement agent-doc supervisor api to restart the supervisor process.

❯ spec-test-build-install-commit-push
### Re: editor popup controls — gpt-5

Implemented the editor-menu reshuffle across JetBrains, VS Code, and the CLI surface.

What changed:
- JetBrains now keeps `Compact Exchange` and `Restart Supervisor Process` in the primary numbered popup, while `Run with Junie` and `Force Claim for Tmux Pane` stay available from a `More Actions` overflow and remain present in the regular menus.
- VS Code now mirrors that shape with a numbered primary QuickPick, an overflow QuickPick for `Run with Junie` and `Force Claim for Tmux Pane`, and new command wiring for `Compact Exchange` plus the supervisor restart action.
- Added the explicit `agent-doc session restart-supervisor <file>` command surface, kept `session restart` as a compatible alias, and routed both editor plugins through that named supervisor-restart path.
- Updated the editor/plugin specs and added focused popup/command tests for both plugins.

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml --all-targets --all-features`
- `npm test` in `src/agent-doc/editors/vscode`
- `./gradlew test buildPlugin` in `src/agent-doc/editors/jetbrains`
- `cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `cargo install --path src/agent-doc --force`

❯ Commit / push:
❯ - `src/agent-doc`: `e1bbbf4` pushed to `origin/20260422/codex-agent-backend`
❯ - superproject gitlink: `7392b082f` pushed to `origin/main`
### Re: restart supervisor wording follow-up — gpt-5

❯ Handled the wording/API follow-up on the same implementation pass: the editor menus now expose `Restart Supervisor Process`, and the explicit CLI surface is `agent-doc session restart-supervisor <file>` with `session restart` kept as a compatibility alias. No additional repo changes were needed beyond the code/spec/test/build/install/commit/push work already completed above.
❯ Do we need to bump, build, and install the JB + VSCode plugins?
### Re: plugin bump/install question — gpt-5

For this change set:

- **Local testing / local use:** you do **not** need a version bump, but you **do** need to build/package and install the plugins if you want the JetBrains and VS Code menus on your machine to pick up the new actions.
- **Published release / `plugin update` flow:** you **do** need a version bump before publishing new plugin artifacts, otherwise the released version identity does not advance.

Current repo state:
- JetBrains plugin version is still `0.2.92` in `editors/jetbrains/gradle.properties`.
- VS Code extension version is still `0.2.12` in `editors/vscode/package.json`.
- I already ran `./gradlew test buildPlugin`, so the JetBrains zip is built, but I did **not** install it.
- I ran `npm test` for VS Code, but I did **not** package a `.vsix` or install it.

Why the split is different:
- JetBrains local install removes the old plugin dir and installs from the local built zip, so same-version local testing is fine.
- VS Code local install uses the local `.vsix`, so same-version local testing is also fine.
- The release/update path is version-sensitive: JetBrains `plugin update` explicitly skips reinstall when the installed version already matches the released version, so a release without a version bump is the wrong shape.

❯ So the practical answer is:
❯ - **Need install for live editor testing:** yes.
❯ - **Need version bump right now:** only if you want me to package/publish the plugins as a new release rather than just install local builds.
❯ A visible unselected editor's (/home/brian/work/btakita/agent-loop/src/session-share/tasks/buildparty-investor-demo/dev.md) tmux pane is removed from the agent-doc tmux window during an active turn. Syncing brings back the tmux pane. The pane should always remain visible if the document is visible in the editor or was the last visible agent doc. This regression has been around for a few days at least. Please identify root cause(s) and fix. spec-test-build-install-commit-push
### Re: tmux pane retention regression — gpt-5


Root cause identified and fixed.

- The pane loss was not coming from the normal editor `sync --no-autostart` path.
- `route::sync_after_claim()` was calling `sync::run(...)`, which jumps to the default tmux server instead of staying on the injected tmux handle.
- During local verification, that let an isolated reconcile using dummy files mutate the real live `agent-doc` window and stash a visible sibling pane like `src/session-share/tasks/buildparty-investor-demo/dev.md` out of view.
- A later normal sync would rescue the pane back, which is why the issue looked intermittent and recoverable.

What I changed:
- switched `sync_after_claim()` to `sync::run_with_tmux(...)` so post-claim reconcile stays on the caller's tmux server
- added a focused regression that proves the injected server's overflow pane is stashed locally instead of the default server being touched
- updated `specs/06-config.md` and `VERSIONS.md` to record the tmux-context invariant

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml sync_after_claim_stays_on_injected_tmux_server -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml --all-targets --all-features`
- `cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `cargo install --path src/agent-doc --force`

Submodule commit / push:
- `src/agent-doc`: `b9a7f1a` pushed to `origin/20260422/codex-agent-backend`

❯ Recommended follow-up kept open:
❯ - `#jbpdrop` — replay the live mixed-root editor scenario after this fix. Plan: `tasks/agent-doc/plan-jb-completed-run-pane-retention.md`
❯ do [#jbpdrop]. spec-test-build-install-commit-push
### Re: `#jbpdrop` replay — gpt-5

Verified the `#jbpdrop` pane-retention fixes by replaying the route/tmux regressions that model the mixed-root pane-theft path, rather than by introducing a new source change in this cycle.

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml sync_after_claim_stays_on_injected_tmux_server -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml register_dispatch_target_rejects_cross_file_rebind_and_preserves_registry -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_or_create_pane_dispatch_only_keeps_starting_authoritative_actor_on_direct_pane_submit -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_or_create_pane_keeps_fresh_start_authoritative_despite_existing_owner_rebind -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml --all-targets --all-features`
- `cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `cargo install --path src/agent-doc --force`

What this proved:
- post-claim sync stays on the injected tmux server instead of mutating the live default server
- dispatch-only reroutes fail closed on cross-file pane rebind attempts instead of transiently stealing another document's pane
- starting authoritative panes keep the live direct-submit path during boot
- fresh-start reroutes keep the newly-created pane authoritative even when another same-session registration arrives during boot

Plan: `tasks/agent-doc/plan-jb-completed-run-pane-retention.md`

I did not run a literal JetBrains/VS Code GUI click-through in this cycle, so I am leaving `#jbpdrop` gated as a human/editor validation gate instead of marking it fully done.

❯ I also did not create a repo commit/push: the worktree already contains unrelated dirty changes in both the superproject and `src/agent-doc`, and this cycle's delta was verification plus the plan update only.
❯ do [#jbdirsubmit] [#jbqbusy] [#jbhalias]. spec-test-build-install-commit-push
### Re: `#jbdirsubmit` `#jbqbusy` `#jbhalias` verification — gpt-5

I treated this as a verification turn rather than a new implementation pass. The current tree already contains the backend/editor changes for all three follow-ups, so I proved the behaviors directly instead of layering on another speculative source edit.

What I verified:
- `#jbdirsubmit`: dispatch-only reroutes stay on the same direct pane submit path as `session clear`, do not restart after tracked Codex `/clear`, and do not send extra Enter/newline retries.
  - `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_or_create_pane_dispatch_only_does_not_restart_after_tracked_codex_clear -- --nocapture`
  - `cargo test --manifest-path src/agent-doc/Cargo.toml dispatch_only_send_reopen_direct_pane_submit_avoids_extra_enter_retries -- --nocapture`
  - `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_or_create_pane_dispatch_only_submits_while_latest_run_is_still_starting -- --nocapture`
- `#jbqbusy`: prompt-bearing reroutes still dispatch when the authoritative actor reports `busy` or `starting`, and dispatch-only starting-pane reroutes stay on the live pane submit path instead of failing closed on the short boot window.
  - `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_or_create_pane_dispatches_busy_authoritative_actor_when_prompt_target_pending -- --nocapture`
  - `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_or_create_pane_dispatches_starting_authoritative_actor_when_prompt_target_pending -- --nocapture`
  - `cargo test --manifest-path src/agent-doc/Cargo.toml resolve_or_create_pane_dispatch_only_keeps_starting_authoritative_actor_on_direct_pane_submit -- --nocapture`
- `#jbhalias`: authoritative Claude actor lookup accepts the canonical `claude-code` actor record even when the live harness surface is `claude`.
  - `cargo test --manifest-path src/agent-doc/Cargo.toml load_authoritative_actor_dispatch_target_accepts_normalized_claude_harness_identity -- --nocapture`

Editor/build/install verification:
- `npm test` in `src/agent-doc/editors/vscode`
- `/usr/bin/zsh -lc "GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test buildPlugin"` in `src/agent-doc/editors/jetbrains`
- `cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `cargo install --path src/agent-doc --force`

No new `src/agent-doc` source edit was required in this cycle. I did not create a repo commit/push there because the submodule already had unrelated dirty changes before this replay and this turn's delta was proof/build/install plus the document response, not a new isolated backend patch.

❯ I am leaving `#jbdirsubmit`, `#jbqbusy`, and `#jbhalias` open as gated follow-ups rather than marking them fully done because I did not perform a literal live JetBrains/VS Code click-through in this cycle. The backend and plugin proof surface is green; the remaining gap is human GUI confirmation only.
❯ do [#tailpatch]
### Re: `#tailpatch` plan refresh — gpt-5

Refreshed the fix plan only for `#tailpatch`.

Plan: `tasks/agent-doc/plan-tail-patch-outside-exchange.md`

What changed in the plan:
- added the newer 2026-05-05 `tasks/agent-doc/agent-doc-bugs2.md` evidence alongside the older `claudescore-3.md` tail-placement example
- narrowed the investigation to the actual current surfaces: `write.rs`, `template.rs`, `repair.rs`, `preflight.rs`, and the closeout integration tests
- called out the existing `guard_no_conversation_tail_outside_exchange(...)` and `repair_duplicate_exchange_close_tail(...)` protections as part of the root-cause trace, since at least one live closeout/recovery path still reached a bad tail shape before those protections prevented commit
- tightened the exit criteria so this bug is not considered addressed until the malformed-tail shape fails closed or is deterministically repaired before commit

❯ I am keeping `#tailpatch` open because this cycle was plan refresh only, per the backlog contract. No `src/agent-doc` code, spec, or test files were changed in this turn.
❯ I manually transitioned backlog items with [/] to [x]. The agent-doc pipeline removed the done items but then brought some of the items back. Please investigate root cause(s) and fix.
❯ spec-test-build-install-commit-push
### Re: backlog reap resurrection fix — gpt-5

Root cause fixed.

What was happening:
- `repair` / `preflight` could reap user-marked `[x]` backlog items correctly.
- A later stale local/editor rewrite could put the same `[#id]` lines back into the live `agent:backlog` before closeout.
- `git::commit()` treated that as generic post-commit local drift, so HEAD stayed clean while the working tree resurrected the supposedly removed items, and the next cycle had to reap them again.

What changed:
- added a cycle-scoped `reaped_pending_ids` ledger in `cycle_state`
- recorded reaped tracked-work ids from both `preflight` and `repair`
- made `git::commit()` fail closed if any id reaped earlier in the same cycle reappears in the live `agent:backlog` or `agent:icebox` before commit
- updated the pending spec + version notes to make that resurrection case an explicit closeout invariant

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml --all-targets --all-features`
- `cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `cargo install --path src/agent-doc --force`

Commit / push:
- `src/agent-doc`: `28fb2fb` pushed to `origin/20260422/codex-agent-backend`
- superproject gitlink: `610ab0bb2` pushed to `origin/main`

❯ There were no actionable follow-up items to capture.
❯ In the previous response, the following had an uncommitted change that stripped out the `❯ ` prefix. Please identify the root cause of the regression. spec-test-build-install-commit-push
```
I am keeping `#tailpatch` open because this cycle was plan refresh only, per the backlog contract. No `src/agent-doc` code, spec, or test files were changed in this turn.
I manually transitioned backlog items with [/] to [x]. The agent-doc pipeline removed the done items but then brought some of the items back. Please investigate root cause(s) and fix.
spec-test-build-install-commit-push
```
### Re: prompt prefix repair regression — gpt-5

Root cause fixed.

What was happening:
- JetBrains was applying `normalize_prefix_lines` before the exchange patch/unmatched content rewrite, so the later patch could overwrite the repaired `❯ ` lines and persist a broken ack sidecar.
- VS Code's reposition-only debounce shortcut treated any `reposition_boundary: true` + `patches: []` payload as a pure boundary move, even when the payload still carried `normalize_prefix_lines` for post-commit prompt-prefix repair.

What changed:
- moved JetBrains exchange-prefix normalization to run after component/unmatched patch application and before boundary/head cleanup in both the open-document and VFS paths
- added a VS Code `isPureRepositionSignal(...)` gate so normalize-only repair payloads stay on the full patch path instead of being swallowed by the reposition fast path
- updated `editors/PLUGIN-SPEC.md` and `VERSIONS.md` to record the pure-reposition contract and the editor-side repair ordering invariant
- added a focused VS Code regression for the patch-shape gate

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml --all-targets --all-features`
- `npm test` in `src/agent-doc/editors/vscode`
- `./gradlew test buildPlugin` in `src/agent-doc/editors/jetbrains`
- `cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `cargo install --path src/agent-doc --force`

Commit / push:
- `src/agent-doc`: `362bcd1` pushed to `origin/20260422/codex-agent-backend`
- superproject gitlink: `e05975450` pushed to `origin/main`

❯ There were no actionable follow-up items to capture.
❯ do [#tailpatch]. spec-test-build-install-commit-push
### Re: `#tailpatch` exchange-gap closeout fix — gpt-5

Fixed the `#tailpatch` closeout blind spot in `src/agent-doc`.

Root cause:
- the template guard/repair only scanned for escaped conversation after the last parsed agent component
- that missed prompt/response blocks that landed in the gap between `&lt;!-- /agent:exchange --&gt;` and later components such as the stray `###` marker or `agent:backlog`, so malformed closeouts could survive while the document still parsed cleanly

What changed:
- `src/template.rs` now shares one range detector for escaped conversation outside exchange gaps
- the fail-closed guard, explicit repair path, and manual tail-strip path all use that same detector
- added regressions for guard failure, repair, and strip on the exchange-to-backlog gap shape
- updated `specs/07-closeout-commands.md`, `README.md`, and `VERSIONS.md` to document the inter-component gap invariant

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml gap_before_backlog`
- `cargo test --manifest-path src/agent-doc/Cargo.toml --all-targets --all-features`
- `cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `cargo install --path src/agent-doc --force`

Commit / push:
- `src/agent-doc`: `7f93d01` pushed to `origin/20260422/codex-agent-backend`

❯ I also restored the gated backlog items `#jbpdrop`, `#jbipcr`, and `#edispatch` after preflight failed closed on their absence; they remain open follow-ups in this session. I did not create a superproject gitlink commit because the workspace root already has unrelated dirty changes outside this fix.
❯ Before the previous run, I completed all `[/]` backlog tasks with `[x]`. The `[x]` items were removed in the preflight but then the repair readded the items with `[/]`. Since I marked the backlog items as complete, the repair should honor that. How can we ensure that user intent is completed while keeping the repair processes focused on repairing issues with the document? spec-test-build-install-commit-push
### Re: manual backlog completion replay regression — gpt-5

Root cause fixed.

What was happening:
- preflight / repair already knew how to reap a user-edited `[x]` backlog item
- the backlog replay guard still only exempted ids recorded through `--pending-done`
- when the completion came from a manual document edit instead, that just-reaped id looked like a dropped historical backlog item and the older `[ ]` / `[/]` state could be restored on the next repair path

What changed:
- added `cycle_state::resolved_pending_ids(...)` to unify explicit `pending_done_ids` with same-cycle `reaped_pending_ids`
- updated both the preflight history guard and the post-commit session-check history guard to treat same-cycle reaps as intentional completion
- added regressions for the exact manual `[x]` path in preflight and session-check
- updated the pending-system spec and `VERSIONS.md` to document the manual-completion invariant

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml preflight_allows_user_marked_done_item_reaped_in_same_cycle -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml session_check_backlog_replay_guard_accepts_reaped_ids_from_cycle_state -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml --all-targets --all-features`
- `cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `cargo install --path src/agent-doc --force`

Commit / push:
- `src/agent-doc`: `c3def78` pushed to `origin/20260422/codex-agent-backend`

I did not create a superproject gitlink commit because the workspace root already had unrelated dirty changes outside this fix.

❯ There were no actionable follow-up items to capture.
❯ do [#devmdcmt]. spec-test-build-install-commit-push
### Re: `#devmdcmt` closeout-gap plan — gpt-5

Created the fix plan for the BuildParty `write_stream` closeout gap.

Plan: `tasks/agent-doc/plan-buildparty-write-stream-closeout-gap.md`

Why this stays separate from `#rspcmt`:
- `#rspcmt` is the broader "visible response but no commit proof" family.
- `#devmdcmt` is narrower: the `dev.md` repro already recorded a synthetic binary-owned `response_captured -> write_applied (event=write_stream)` chain before the terminal commit lagged behind.

Evidence captured in the plan:
- `src/session-share/.agent-doc/logs/586ffeea-ca3a-490b-80bd-883dd0e95673.log` shows `synthetic-1777883041000` reaching `response_captured` and `write_stream` at `1777883041`, with `commit_success` only later at `1777883128`.
- `src/session-share/.agent-doc/captures/fbdde1a9f60dab3cf06c00e1e747fb607d46afdad89003acd3745214fff0dd30/synthetic-1777883041000.json` preserves the same gap with a visible response body already captured.
- Later `dev.md` cycles on May 5 close immediately, so the plan also treats this as a historical but distinct regression against the expected CRDT/template closeout path.

I am keeping `#devmdcmt` open because this cycle was plan-only, per the backlog contract. I did not change `src/agent-doc` code, specs, tests, build artifacts, or plugin outputs in this turn.
❯ Look at the current uncommitted git state of this document.

❯ "There were no actionable follow-up items to capture." has the `❯ ` prefix removed in the uncommitted state. The `❯ ` prefix is committed. Please identify room cause(s). spec-test-build-install-commit-push
### Re: answered prompt prefix drift root cause — gpt-5

Root causes identified and fixed.

- `src/git.rs` was over-greedy during commit-time answered-prompt canonicalization. When a real answered prompt such as `do [#tailpatch]...` shared one contiguous block with prior assistant tail prose, the canonicalizer prefixed the whole block and committed assistant text like `There were no actionable follow-up items to capture.` as if it were user input.
- The current uncommitted state of this document is the fallout from that bug: the working tree still held the bare assistant sentence, while `HEAD` contained the wrongly prompt-prefixed version. In other words, the committed `❯ ` on that line was the regression, not the bare working-tree text.

What changed:
- `src/git.rs` now starts answered-prompt prefix repair at the first prompt-like line in the block instead of the first nonblank line.
- Added a regression covering the exact mixed assistant-tail + `do [#...]` shape.
- Updated `specs/09-git-integration.md` and `VERSIONS.md` with the invariant.

Verification:
- `cargo test --manifest-path src/agent-doc/Cargo.toml commit_does_not_prefix_prior_response_tail_before_answered_prompt -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml --all-targets --all-features`
- `cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `cargo install --path src/agent-doc --force`

Commit / push:
- `src/agent-doc`: `71daf48` pushed to `origin/20260422/codex-agent-backend`

I did not create a superproject gitlink commit because the workspace root still has unrelated dirty changes outside this fix stream.

❯ There were no actionable follow-up items to capture.
<!-- agent:boundary:ffd814b5 -->
<!-- /agent:exchange -->
###

<!--
"Press Enter to restart, or 'q' to exit."
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#devmdcmt] [recommended] Investigate the historical BuildParty synthetic `write_stream` closeout gap on `src/session-share/tasks/buildparty-investor-demo/dev.md`: the response landed, the cycle stayed at `write_applied`, and a later explicit `agent-doc commit` provided the terminal closeout. Reproduce the exact template/CRDT path, compare it with `#rspcmt`, trace any boundary/backlog/session-check interaction that lets synthetic `write_applied` survive as a nonterminal state, and add fail-closed regression coverage. Plan: tasks/agent-doc/plan-buildparty-write-stream-closeout-gap.md
- [ ] [#resyncflake] [recommended] Stabilize resync::tests::purge_unregistered_stash_panes_kills_retained_dead_stash_pane; cargo test sync:: hit it once even though the same test passed in isolation.
- [ ] [#vssplitreplay] [recommended] Replay the live VS Code mixed-root split (tasks/agent-doc/agent-doc-bugs2.md + src/session-share/tasks/claudescore-3.md) to confirm passive sync no longer stashes a healthy running pane.
- [ ] [#rspcmt] [recommended] Previous response was written to
  `src/session-share/tasks/claudescore-3.md` but not committed. Create a fix plan only: reproduce the closeout path, trace where
  `finalize`/
  `write --commit` can report success without a persisted commit, identify any stash/sync/session-check interaction that can leave the response uncommitted, then add regression coverage and a fail-closed acceptance criterion so agent-doc never reports a completed closeout until the response commit is verifiably present.
- [ ] [#rprdrift] [recommended] `agent-doc repair` reported "No pending response found" for `src/boost-client/tasks/monsterrodholders.md` even though `session-check` still failed on a committed cycle with unresolved prompt-bearing user drift after a direct patchback. Fix plan only: reproduce the exact state transition where a committed cycle still has prompt-bearing follow-up edits, compare `repair`, `session-check`, `write --commit`, and `finalize --pending-done` behavior on that state, decide whether `repair` should automatically convert the drift into a recoverable finalize path or fail with the same actionable guidance as `session-check`, and add regressions plus a fail-closed invariant so repo/session repair never stops at a false "nothing to repair" result while the document remains blocked.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#2dkn] Create a plan for a sqlite db store to manage sessions for better concurrency/parallelism support
- [ ] [#dyvw] Empty placeholder item — moved to icebox to unblock preflight.
- [ ] [#xthm] Historical malformed duplicate backlog id retained for guard continuity. Canonical replacement:
  `#jberr`.
- [ ] [#tmuxcrash] Closed on April 30, 2026 after the
  `session_end` metadata closeout fix in
  `startup_miss.rs` and the routed-dispatch binding hardening in
  `route.rs`. Final child closed:
  `#tmuxcrash-rn2d`.

<!-- /agent:icebox -->



<!--
I see the following in the binding codex session prompt. Why is it there? Why was it not Entered? spec-test-build-install-commit-push
```
› echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started for pending prompt_target: do
  #ppct. spec-test-build-install-commit-push. Run '\''agent-doc start tasks/agent-doc/agent-doc-bugs2.md'\'' to retry.'
```
-->
