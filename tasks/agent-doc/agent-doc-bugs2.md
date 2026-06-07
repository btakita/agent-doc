---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
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
queue: stop
---

## Status

 <!-- agent:status patch=replace -->

Completed #md-ast-mutations-node-keyed: node-keyed markdown AST mutations shipped in src/agent-doc 316681b0; current agent-doc main is 6a017bea and superproject pointer pushed as 1b933127b. Superproject Agent Doc workflow could not start because GitHub reported a billing/spending-limit runner allocation blocker.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260607-091343.md`*

Compacted content:
- Archived 3 response topic(s): do [#md-ast-document-model]; closeout repair - gpt-5; do [#recursion-guard-wedge-escape] -- gpt-5
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ do [#recursion-guard-wedge-escape]. #spec-test-build-install-commit-push
### Re: do [#recursion-guard-wedge-escape] -- gpt-5

Closed as shipped. Part 2 is present in `src/agent-doc` commit `91f06782` (`Add force interrupt-clear wedge escape`), which is included in current `origin/main` at `9df1244f`.

Proof checked in source:

- `agent-doc session interrupt-clear <FILE> --force` is exposed in `src/main.rs`.
- `force_interrupt_clear` closes actor state, removes matching session registry projections, signals child/supervisor PIDs, kills the selected owner pane, removes the supervisor socket, writes clear cooldown, and reclaims orphaned empty preflight cycles.
- `specs/07-core-commands.md` pins the recursive-start guard and the one-command destructive hatch.

Verification run now:

```
cargo test -p agent-doc force_interrupt_clear_summary_reports_destructive_cleanup -- --nocapture
cargo test -p agent-doc-orchestration recursive_start_diagnostic_refuses_and_names_out_of_pane_recovery -- --nocapture
cargo test -p agent-doc-orchestration owned_pane_queue_wedge_halt_diagnostic_names_halt_and_both_recoveries -- --nocapture
```

All three passed. Source CI for `src/agent-doc` `9df1244f` was already green in run `27088129850`, so this was a stale backlog/queue mirror and is now done.

### Re: do [#partial-staging-closeout-guard] — gpt-5

> **Queue prompt:**
>
> :round_pushpin: do [#partial-staging-closeout-guard]


Shipped and closed.

Implemented `session-check` partial-staging detection in `src/agent-doc` commit `c87f64b6` (`Warn on partial staging closeouts`). The guard inspects the active repo plus dirty submodule repos surfaced by the owning worktree, compares the latest commit against tracked dirty/staged companion changes, and warns only when related source/test paths share changed string literals. That targets the failure mode where local verification passed against a dirty worktree while CI saw only a partial commit.

Spec and regression coverage were updated in `SPEC.md`, `specs/09-git-integration.md`, `agent-doc-orchestration/src/session_check.rs`, and `tests/test_cli.rs`. The superproject pointer is committed in `47bc0ccb7`.

Verification:

```text
cargo test -p agent-doc-orchestration partial_staging_closeout_guard_warns_on_dirty_companion_test_literal -- --nocapture
cargo test flowcore_hot_path_guard_and_proof_tokens_are_budgeted -- --nocapture
make check
make install
```

The first `make check` rerun exposed one sync test failure plus one IPC timeout during nextest cancellation; both passed in isolation. A clean full `make check` rerun then passed: 3892 tests passed, 233 skipped, doctests passed, and the ignored medium SimWorld check passed. `make install` replaced `/home/brian/.cargo/bin/agent-doc` and installed `libagent_doc-0.34.0.so`.

### Re: #codex-mcp-server-root-fix split — gpt-5

> **Queue prompt:**
>
> :round_pushpin: do [#codex-mcp-server-root-fix]


I did not implement the MCP server in this cycle. I reviewed the Option A plan and the current binary surface; there is no existing MCP server scaffold, and this queue head was a draft structural plan whose exit criteria include a persistent server, Codex config/Stop-hook integration, and live multi-item queue-drain validation.

I converted the broad parent into three concrete backlog phases and marked only the decomposition parent done. The implementation work remains in #codex-mcp-server-mvp, #codex-mcp-codex-integration, and #codex-mcp-live-verify, so the queue can make progress without treating the whole root fix as shipped.

### Re: #live-verify-batch live gate — gpt-5

> **Queue prompt:**
>
> :round_pushpin: do [#live-verify-batch]


I captured the non-disruptive part of the live verification batch, but I did not restart IntelliJ or drive interactive IDE actions from this agent pane.

Proof captured:
- Binary is installed as `agent-doc 0.34.0`.
- IntelliJIdea2026.1 has Agent Doc plugin jar `/home/brian/.local/share/JetBrains/IntelliJIdea2026.1/agent-doc-jetbrains/lib/agent-doc-jetbrains-0.2.150.jar`; its plugin.xml reports version `0.2.150` and id `com.github.btakita.agent-doc`.
- The live IDE/plugin is active: `idea.log` shows Agent Doc `LayoutDetector`, `PromptPoller`, and `PatchWatcher` entries for `tasks/agent-doc/agent-doc-bugs2.md` and `src/boost-client/tasks/monsterrodholders.md` during this session.
- The folded apply-fence gate is verified: `idea.1.log:865-866` records the PatchWatcher generation fence detecting an already committed cycle and dropping the superseded patch file; the same proof repeats at `idea.1.log:44690-44691` and `idea.1.log:45376-45377`.

I marked #late-ipc-patch-plugin-apply-fence done from that evidence. I moved #live-verify-batch to a live-verification gate because the remaining busy-pane checks require deliberate interactive JetBrains actions. No additional backlog follow-up is needed because #live-verify-batch itself remains the external live-verification tracker.

### Re: #automated-completion-detection split -- gpt-5

> **Queue prompt:**
>
> :round_pushpin: do [#monsterrod-queue-items-deleted-live]


> **Queue prompt:**
>
> :round_pushpin: do [#automated-completion-detection]


I did not ship the whole automated completion feature in this cycle. The current spec/code already has a narrower `auto_done` path for explicit completion prompts, but the backlog item asks for broader evidence-driven completion from done archives, ops/cycle proof, and semantic retrieval. That scope needs separate implementation phases rather than one queue closeout.

I split the parent into three concrete child items: deterministic done-archive mirror cleanup, ops/cycle proof detection, and semantic retrieval integration. I marked only the decomposition parent done. I left the unrelated dirty `src/agent-doc` source changes untouched.

### Re: #monsterrod-queue-items-deleted-live -- gpt-5

Closed this as covered by the shipped queue-preservation fixes. I did not make source changes in this cycle.

Evidence checked:
- `tasks/agent-doc-bugs2.done.md` already contains the shipped/fixed queue-loss classes: `#queue-user-edit-overwrite`, `#queue-edit-lost-during-finalize-crdt`, `#queue-dedup-destroys-intentional-duplicates`, and `#md-ast-mutations-node-keyed`.
- `queue.rs` now dedups only explicit `do #id` / `do [#id]` identity prompts, while preserving free-text queue prompts.
- `session_check.rs` and `write.rs` contain the dropped-queue-prompt guard/provenance path for `content_ours` adoption.

Verification passed:
- `cargo test -p agent-doc-orchestration dedup_live_prompts_preserves_free_text_duplicates -- --nocapture`
- `cargo test -p agent-doc-orchestration preflight_preserves_intentional_duplicate_tracked_queue_prompt -- --nocapture`
- `cargo test -p agent-doc-orchestration dropped_queue_prompt_lines_after_content_ours_captures_adjacent_free_text_items -- --nocapture`

The item was archived as done because the live MonsterRod deletion report is covered by those shipped guards and regression tests.

### Re: #clear-blocked-by-own-drafted-clear-command -- gpt-5

> **Queue prompt:**
>
> :round_pushpin: do [#clear-blocked-by-own-drafted-clear-command]


Shipped and closed.

Changed:
- `agent-doc-orchestration/src/harness.rs` now treats Codex prompt candidates that are `agent-doc session clear`, `interrupt-clear`, `stop`, `restart`, or `restart-supervisor` as operator control commands, not protected drafted prompt input.
- Ordinary drafted Codex text remains protected.
- Added an Unreleased note in `VERSIONS.md`.

Commits:
- `src/agent-doc` `e6c7b7b2` (`fix: allow self-drafted session clear controls`)
- superproject pointer `910ec31fe` (`agent-doc: allow self-drafted clear controls`)

Verification:
- `cargo test -p agent-doc-orchestration protected_prompt_input_reason_ -- --nocapture` passed.
- `make check` passed locally.
- GitHub Actions `agent-doc` CI run `27089249574` passed, including `make check` and `make tmux-ci`.
- Superproject has no workflow named `CI`.

Unrelated dirty files in `src/agent-doc` (`queue.rs`, `queue_cmd.rs`, `start.rs`) and `src/boost-client` were left out of this commit.
### Re: #tmux-status-turn-in-progress -- gpt-5

Closed as shipped and verified; no source changes were made in this cycle.

Evidence checked:
- `turn_status.rs` implements `agent-doc turn-status active|idle`, sets the pane-border title to `⟳ agent-doc: turn in progress`, clears it back to empty on idle, and maintains `.agent-doc/turn-active.json` with a self-expiring TTL.
- `main.rs` wires `agent-doc turn-status active|idle|install`.
- `skill.rs` installs Claude, Codex, and OpenCode hooks for active/idle turn transitions and can enable tmux `pane-border-status` for visibility.
- `specs/08-session-routing.md` documents the queued busy-route status surface.

Verification:
- `cargo test -p agent-doc-orchestration turn_status -- --nocapture` passed: 4 tests.
- `cargo test -p agent-doc skill::tests::install_turn_status_hooks_all_harnesses_idempotent_and_preserves_existing -- --exact --nocapture` passed: 1 test.
- Live check in pane `%30`: `agent-doc turn-status active` set the pane title to `⟳ agent-doc: turn in progress` and wrote `.agent-doc/turn-active.json`; `agent-doc turn-status idle` cleared the pane title and removed the marker.
- Existing `agent-doc` CI run `27089249574` passed earlier this cycle, including `make check` and `make tmux-ci`.

The harder fail-closed busy lease remains tracked separately by the `#subagent-blocks-session` scope decision; this item was the visible turn-in-progress status surface.
### Re: #stale-busy-self-heal-live-verify -- gpt-5

Closed as live-verified; no source changes were made in this cycle.

Evidence checked:
- The shipped code path in `start.rs` reconciles a stale busy/starting actor to ready when the idle-queue watch observes no live pane busy cue for `STALE_BUSY_RECONCILE_TICKS`, clears the one-shot prompt latch, preserves drain dedup state, and logs `idle_queue_watch_stale_busy_reconciled` with the pane id and tick count.
- The focused regression suite passed: `cargo test -p agent-doc-orchestration stale_busy_reconcile -- --nocapture` reported 6 passed tests.
- Live Codex supervisor log `.agent-doc/logs/4602f467-ef2a-4094-a750-5eb533368988.log` for `tasks/software/tagpath.md` shows the idle-queue watch on pane `%36` emitting `idle_queue_watch_stale_busy_reconciled harness=codex pane=%36 after_ticks=4`, then continuing `idle_queue_watch_drain harness=codex cmd="agent-doc tasks/software/tagpath.md"`.
- The same live log then reaches `document_cycle phase=response_captured` and `document_cycle phase=committed ... event=commit_success` for `cycle-1780811070625`; the inspected window contains no `session_clear`, `interrupt_clear`, `kill-pane`, or pane-kill recovery event.

I did not manually force controller lifecycle state or kill/restart the pane. The live proof came from an actual managed Codex supervisor self-heal and queue drain, which is the behavior this tracker needed verified.

### Re: do [#codex-mcp-server-mvp] — gpt-5

> **Queue prompt:**
>
> :round_pushpin: do [#codex-mcp-server-mvp]


Shipped and closed.

`agent-doc mcp serve` now exposes the MCP stdio server path with these tools: `agent_doc_read`, `agent_doc_preflight`, `agent_doc_plan`, `agent_doc_session_check`, and `agent_doc_finalize`. The finalize tool feeds responses through the strict binary-owned write/commit/session-check path. The plan tool calls the structured planner directly, and the preflight tool captures the same binary's preflight stdout/stderr so JSON-RPC stdout stays protocol-clean.

Source commits:
- `e737af82 Add agent-doc MCP server`
- `09ce0ce6 Add MCP preflight and plan tools`

The source commits are pushed to `src/agent-doc` `origin/main`; this closeout stages the superproject pointer to `09ce0ce6`.

Verification:
- `cargo test mcp -- --nocapture`
- `cargo test mcp_serve_handles_initialize_list_and_read -- --nocapture`
- `make check` (3901 tests passed, 233 skipped, doctests passed, ignored medium SimWorld check passed)
- GitHub Actions CI `27089580418` passed on rerun with `make check` and `make tmux-ci`; the first attempt failed before tests on a crates.io HTTP/2 dependency download reset.
- Installed from a clean temporary worktree at `09ce0ce6`; `agent-doc --version` reports `agent-doc 0.34.0`.
- Live stdio smoke: installed `agent-doc mcp serve` advertises all five MVP tools.

### Re: do [#codex-mcp-codex-integration] — gpt-5

> **Queue prompt:**
>
> :round_pushpin: do [#codex-mcp-codex-integration]


Shipped and closed.

Codex install now registers the repo-local `agent-doc` MCP server in `.codex/config.toml` as `agent-doc mcp serve --project-root <root>`, while preserving existing hooks/config and leaving Claude, OpenCode, and direct CLI behavior unchanged.

The Codex Stop hook now prefers the configured MCP continuation for active queue cycles: `agent_doc_preflight`, `agent_doc_plan` / `agent_doc_read` as needed, `agent_doc_finalize` for strict write/commit closeout, and `agent_doc_session_check` for the final gate. If MCP tools are unavailable in that Codex run, the hook still gives the existing in-pane `agent-doc finalize` / `agent-doc write --commit` fallback and still forbids `agent-doc <FILE>` from the owner pane.

Source commit: `39dcab3c` (`Wire Codex MCP continuation`), pushed to `btakita/agent-doc main`.

Verification:
- `cargo test -p agent-doc-orchestration configured_mcp_tools -- --nocapture`
- `cargo test -p agent-doc install_for_codex -- --nocapture`
- `cargo test -p agent-doc test_codex_skill_install_writes_hook_artifacts -- --nocapture`
- `cargo test -p agent-doc mcp -- --nocapture`
- `cargo test -p agent-doc-orchestration stop_ -- --nocapture`
- `make check`
- GitHub Actions CI `27090148828` passed with `make check` and `make tmux-ci`.

### Re: do [#codex-mcp-live-verify] — gpt-5

> **Queue prompt:**
>
> :round_pushpin: do [#codex-mcp-live-verify]


Closed with live evidence.

I installed the `39dcab3c` agent-doc binary from a clean detached worktree under `src/`, reran `agent-doc skill install --harness codex --reload restart` in this root, and verified `.codex/config.toml` now contains `[mcp_servers.agent-doc]` with `command = "agent-doc"` and `args = ["mcp", "serve", "--project-root", "/home/brian/work/btakita/agent-loop"]`.

Live Stop-hook probe against this active document returned a durable continuation block for `#codex-mcp-live-verify` that prefers the configured MCP flow: `agent_doc_preflight`, `agent_doc_plan` / `agent_doc_read`, `agent_doc_finalize`, and `agent_doc_session_check`. The same reason preserved the CLI fallback and the recursion-guard warning forbidding `agent-doc tasks/agent-doc/agent-doc-bugs2.md` from the owner pane.

This closeout itself is being written through the installed stdio MCP server with `agent_doc_finalize` using the current preflight baseline and `done = ["codex-mcp-live-verify"]`, so the multi-item Codex auto-queue drain has now crossed the MCP finalize path after the prior two MCP implementation/configuration items.

Verification:
- Clean-worktree install: `cargo install --path /home/brian/work/btakita/agent-loop/src/agent-doc-clean-39dcab3c --force` completed and `agent-doc --version` reports `agent-doc 0.34.0`.
- `agent-doc skill install --harness codex --reload restart`
- `.codex/config.toml` includes the repo-local `mcp_servers.agent-doc` entry.
- `agent-doc hook codex-stop` live probe returned `decision = "block"` with `agent_doc_preflight`, `agent_doc_finalize`, `agent_doc_session_check`, CLI fallback, and the recursive direct-invocation guard.
- `agent_doc_finalize` over `agent-doc mcp serve --project-root /home/brian/work/btakita/agent-loop` performed this closeout.

### Re: do [#auto-complete-done-archive-mirrors] — gpt-5

> **Queue prompt:**
>
> do [#sync-fast-pane-hotpath]


Implemented #auto-complete-done-archive-mirrors.

> **Queue prompt:**
>
> :round_pushpin: do [#auto-complete-done-archive-mirrors]


Source commit ca3feab8 (`Reap done archive mirrors during preflight`) is pushed to btakita/agent-doc main.

What changed:
- Added a pending-core helper to remove active items by archived id without re-archiving them.
- Preflight now removes stale active backlog/review mirrors whose ids are already in inline `agent:done` or external `agent:done archive=...`.
- Queue coverage now verifies external done archives both exclude synced queue ids and strike live queue mirrors.
- Updated SPEC.md and VERSIONS.md for the new preflight mirror cleanup contract.

Verification:
- `cargo test -p agent-doc-core op_take_active_items_by_ids_removes_open_and_gated_matches_only -- --nocapture`
- `cargo test -p agent-doc-orchestration pending_maintenance_reaps_ -- --nocapture`
- `cargo test -p agent-doc-orchestration external_archive_done -- --nocapture`
- `make check`
- GitHub Actions CI `27090585631` passed (`make check` and `make tmux-ci`; only annotation was the actions/checkout Node.js 20 deprecation).
Implemented #auto-complete-ops-proof.

> **Queue prompt:**
>
> :round_pushpin: do [#auto-complete-ops-proof]


Source commit: e223f403 Auto-complete ops-proof pending items (pushed to btakita/agent-doc main).

What changed:
- Preflight pending maintenance now auto-completes active agent:backlog and agent:review items only when the item text has an explicit completion marker plus deterministic commit or successful-CI proof.
- It promotes matched items to done, removes them from the active surface, archives them through the existing agent:done path, records pending-done/reaped ids in cycle state, and logs auto_complete_ops_proof telemetry.
- It leaves blocker cases active, including partial, remaining work, reopened, deferred, false-closeout, follow-up, WIP, failing/failed, and proofless completion text.
- SPEC.md and VERSIONS.md document the deterministic ops-proof behavior and blocker rule.

Verification:
- cargo test -p agent-doc-orchestration pending_maintenance_auto_reaps_ops_proof_done_items -- --nocapture
- cargo test -p agent-doc-orchestration pending_maintenance_reaps_ -- --nocapture
- cargo test -p agent-doc-orchestration external_archive_done -- --nocapture
- make check
- GitHub Actions CI 27090982349 passed: make check and make tmux-ci. The only annotation was the existing actions/checkout@v4 Node.js 20 deprecation notice.
Implemented #auto-complete-semantic-retrieval.

> **Queue prompt:**
>
> :round_pushpin: do [#auto-complete-semantic-retrieval]


Source commit: bb356345 Propose semantic completion matches (pushed to btakita/agent-doc main).

What changed:
- Added an advisory semantic-completion retrieval helper on the shared tsift-memory path, comparing open backlog/review items and free-text queue prompts against done-state memory events without invoking the tsift CLI.
- Preflight now emits semantic_completion_match warnings for high-confidence already-resolved candidates, and plan includes the same warning text.
- The signal is proposal-only; it does not mark work done or mutate tracked work without a deterministic closeout path.
- SPEC.md and VERSIONS.md document the advisory contract.

Verification:
- cargo test -p agent-doc-orchestration semantic_completion_matches_done_archive_for_free_text_queue_prompt -- --nocapture
- cargo test -p agent-doc build_plan_warns_on_semantic_completion_match_for_free_text_queue -- --nocapture
- make check
- GitHub Actions CI 27091278238 passed: make check and make tmux-ci. The only annotation was the existing actions/checkout@v4 Node.js 20 deprecation notice.
- Root make check passed through tsift digest-runner (artifact tart-4c18e9caad).
Implemented #jb-file-cache-conflict-syntax-loss.

> **Queue prompt:**
>
> do [#jb-file-cache-conflict-syntax-loss]


Source commit: 5a735299 Refresh JetBrains highlights after cache conflicts (pushed to btakita/agent-doc main).

What changed:
- Added a JetBrains VisualHighlighterManager.refreshFile entrypoint so other plugin paths can explicitly reschedule agent-doc visual token highlighting for an open markdown file.
- PatchWatcher now refreshes agent-doc visual highlighters on the File Cache Conflict pending, Cancel, accepted/reloaded, and deferred-patch-applied paths.
- Added a JetBrains regression that pins the cache-conflict syntax-preservation path and checks the refresh ordering around conflict detection, cancel detection, resolution, and document writes.
- Updated editors/jetbrains/SPEC.md, runbooks/jb-cache-conflict.md, and VERSIONS.md to document the highlighting preservation contract.

Verification:
- ./gradlew test --tests com.github.btakita.agentdoc.NormalizeExchangePrefixesTest (from editors/jetbrains; tsift artifact tart-c6317c3ba1)
- make check (source repo; tsift artifact tart-fb9b3943e0)
- GitHub Actions CI 27091520101 passed: make check and make tmux-ci. The only annotation was the existing actions/checkout@v4 Node.js 20 deprecation notice.
- make check (root repo; tsift artifact tart-79b1d4d011)
Implemented #jb-busy-active-turn-stall-live-verify.

> **Queue prompt:**
>
> do [#jb-busy-active-turn-stall-live-verify]


Source commit: 440cf6cb Map busy active-turn routes to JetBrains still-running (pushed to btakita/agent-doc main).

What changed:
- JetBrains now recognizes the route core's skip-wait refusal wording (`pane is busy on an active ... turn`) as a BUSY_RUNNING Run Agent Doc outcome.
- That maps the shipped `#jb-run-agent-doc-busy-active-turn-stall` route behavior to the immediate still-running notification instead of the persistent route-failure path.
- Added a regression for the exact active Codex turn skip-wait output and kept the existing route-core skip-wait tests as proof.
- Updated VERSIONS.md.

Verification:
- ./gradlew test --tests com.github.btakita.agentdoc.TerminalUtilTest (from editors/jetbrains; tsift artifact tart-f11b56855c)
- cargo test -p agent-doc-orchestration busy_dispatch_only_skips_ready_wait_on_proven_active_turn -- --nocapture (tsift artifact tart-80fc803cff)
- cargo test -p agent-doc-orchestration dispatch_only_busy_refusal_message_distinguishes_active_turn_from_cold_wait -- --nocapture (tsift artifact tart-bc8d30a1fe)
- make check (source repo; tsift artifact tart-0ec0d65588)
- GitHub Actions CI 27091755420 passed: make check and make tmux-ci. The only annotation was the existing actions/checkout@v4 Node.js 20 deprecation notice.
- make check (root repo; tsift artifact tart-bd7d628e3d)
- gh run list --workflow CI --limit 1 (root repo) could not run because no root workflow named CI exists.
Completed #adoc-route-busy-fix-superproject-pointer-push.

> **Queue prompt:**
>
> do [#adoc-route-busy-fix-superproject-pointer-push]


What changed:
- Pushed the root superproject `main` branch after the earlier pointer-push blocker cleared.
- `origin/main` now points at `d13416095`, which includes the `src/agent-doc` pointer at `440cf6cb` on the source repo main branch. That source commit includes the busy active-turn route mapping work and is newer than the originally deferred `b93b3c3c` route-core fix.
- No source code changes were required for this queue item.

Proof:
- `git fetch origin main && git rev-list --left-right --count origin/main...HEAD` returned `0 0`.
- `git log --oneline --decorate -1 origin/main` returned `d13416095 (HEAD -> main, origin/main, origin/HEAD) agent-doc(agent-doc-bugs2): 2026-06-07 07:58:08`.
- `git push origin main` advanced root main from `b77844416` to `d13416095`.

Verification:
- make check (root repo; tsift artifact tart-f00417fe02)
- gh run list --workflow CI --limit 1 (root repo) could not run because no root workflow named CI exists.

Residual state:
- The root worktree still has unrelated dirty submodule work in `src/agent-doc`, `src/boost-client`, `src/corky`, and `src/instruction-files`; I left that untouched.
Implemented #multi-session-shared-build-isolation.

> **Queue prompt:**
>
> do [#multi-session-shared-build-isolation]


Source commit: 8f264c18 Add isolated self install command (pushed to btakita/agent-doc main).

Changed paths:
- VERSIONS.md
- src/commands.rs
- src/lib_install.rs
- src/main.rs
- src/self_install.rs

What changed:
- Added `agent-doc self-install`, an operator command that installs the current committed checkout from a temporary sibling git worktree.
- The worktree is created next to the source repo instead of under it, so relative Cargo path dependencies like `../agent-kit` and the `../tmux-router` patch continue to resolve while dirty files in the shared `src/agent-doc` checkout are excluded.
- The command runs `cargo install --path .`, builds the release cdylib, installs that library via the existing atomic `lib-install` path, and removes the worktree unless `--keep-worktree` is set.
- `lib_install` now has an internal path-based entry point so self-install can pass non-string paths cleanly.
- Added command metadata for `/agent-doc self-install` so editor autocomplete sees it.

Verification:
- cargo test -p agent-doc self_install::tests -- --nocapture (tsift artifact tart-b33e8fcafc)
- cargo test -p agent-doc commands::tests::serializes_to_json -- --nocapture (tsift artifact tart-69de92d88c)
- cargo run -p agent-doc --quiet -- self-install --help
- cargo run -p agent-doc --quiet -- commands | rg 'agent-doc self-install'
- git diff --cached --check
- make check (source repo; tsift artifact tart-b97d0f6799)
- GitHub Actions CI 27092119102 passed: make check and make tmux-ci. The only annotation was the existing actions/checkout@v4 Node.js 20 deprecation notice.
- make check (root repo; tsift artifact tart-d0883d9cd4)
- gh run list --workflow CI --limit 1 (root repo) could not run because no root workflow named CI exists.

Confidence: high. The regression proves the isolated worktree reads committed HEAD instead of dirty tracked/untracked checkout state, and the command uses the same source CI path after push.
Escalation: none.

Residual state:
- The source repo still has unrelated unstaged edits in agent-doc-orchestration/src/queue.rs, queue_cmd.rs, and start.rs; I left them untouched.
- The root repo still has unrelated dirty submodule state in src/boost-client, src/corky, and src/instruction-files; I left that untouched.
Closed #jb-run-preempt-followups as an evidence/proof closeout; no new source change was needed in this cycle.

> **Queue prompt:**
>
> do [#jb-run-preempt-followups]


The follow-up parts are already covered by shipped commits and current tests:

- Empty-queue / operator autoloop preemption: #autoloop-command-preemption shipped in 51d7e174, 203d51a4, and 841bbc4f, adding queue stop/resume planning, deferred clear delivery, and resume from the supervisor idle-queue watch.
- 60s busy ready-wait audit when a queue fallback exists: b93b3c3c updates the route path so a proven active busy turn skips the busy ready-wait, and current route coverage includes busy_dispatch_only_should_wait_for_ready cases where queue fallback avoids the wait.
- Recursion-guard escape hatch for a wedged loop: 8627ad1b adds the wedged recursive self-invocation halt, and f24889fc hard-refuses recursive self-owned-pane start with out-of-pane recovery guidance.
- Manual Run Agent Doc preemption remains covered by 79e0406d, which head-inserts priority reruns ahead of pending auto-queue items while preserving the pending auto item.

Verification already run in this overall drain: source make check, source make tmux-ci, source GitHub CI 27092596507, and root make check.
<!-- agent:boundary:2ec3743c -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority -->
<!-- /agent:queue -->
###
<!--
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added.
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
- This session's console stdout is bleeding in front of the opencode tui. ![img_16.png](img_16.png)
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue --> 
1. [ ] [#md-ast-document-model] #agent-doc-bug PARTIAL SHIPPED 9df1244f: runtime writes now persist structured .overlay.yrs sidecars beside legacy .yrs from the same markdown snapshot, and preflight queue dedup is markdown-AST node-keyed so intentional duplicate do/free-text prompts survive. REMAINING: legacy text CRDT is still the merge base; editor/IPC plugins still need the authoritative node_patches apply path and realtime watcher convergence. Plan: tasks/agent-doc/plan-md-ast-document-model.md
2. [ ] [#stale-busy-after-auto-inject-no-clear] #agent-doc-bug DONE 7b60fcdc (CI 27075841879 green): supervisor idle-queue watch self-heals a stale-busy actor over an idle pane (poll reconcile from has_busy_cue, ~2s debounce, persisted via mark_lifecycle). Re-listed to match the still-pending queue mirror; strike via the queue drain. Live-verify tracked as #stale-busy-self-heal-live-verify.
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
1. [/] [#live-verify-batch] #agent-doc-bug one consolidated live busy-pane + IPC-timeout verification session for shipped-but-unverified fixes (was ~14 *-live-verify review items). Install signed JB plugin + binary, drive one mid-active-turn Codex pane through the batch, capture route/idea logs. Blocked on live IntelliJ/Codex panes.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
