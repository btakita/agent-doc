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
queue: start
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
<!-- agent:boundary:058238f1 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto preset="#spec-test-build-install-commit-push" priority go -->
- ~~:pushpin: do [#md-ast-document-model]. #spec-test-build-install-commit-push~~
- ~~:round_pushpin: do [#partial-staging-closeout-guard]~~
- ~~:round_pushpin: do [#codex-mcp-server-root-fix]~~
- ~~:round_pushpin: do [#live-verify-batch]~~
- ~~:round_pushpin: do [#automated-completion-detection]~~
- ~~:round_pushpin: do [#monsterrod-queue-items-deleted-live]~~
- ~~:round_pushpin: do [#clear-blocked-by-own-drafted-clear-command]~~
- ~~:round_pushpin: do [#tmux-status-turn-in-progress]~~
- ~~:round_pushpin: do [#stale-busy-self-heal-live-verify]~~
- ~~:round_pushpin: do [#codex-mcp-server-mvp]~~
- ~~:round_pushpin: do [#codex-mcp-codex-integration]~~
- ~~:round_pushpin: do [#codex-mcp-live-verify]~~
- ~~:round_pushpin: do [#auto-complete-done-archive-mirrors]~~
- :round_pushpin: do [#auto-complete-ops-proof]
- ~~do [#recursion-guard-wedge-escape]~~
- ~~do [#queue-dedup-destroys-intentional-duplicates]~~
- :round_pushpin: do [#auto-complete-semantic-retrieval]
- do [#jb-file-cache-conflict-syntax-loss]
- do [#jb-busy-active-turn-stall-live-verify]
- do [#adoc-route-busy-fix-superproject-pointer-push]
- do [#multi-session-shared-build-isolation]
- do [#sync-fast-pane-hotpath]
- do [#jb-run-preempt-followups]
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
2. [ ] [#jb-file-cache-conflict-syntax-loss] #agent-doc-bug REOPENED false closeout: previous closeout verified queue-consume behavior, not JetBrains File Cache Conflict syntax-highlighting loss; define and verify the actual cache-conflict syntax preservation path before closing.
3. [ ] [#auto-complete-ops-proof] #agent-doc-bug Automated completion detection phase 2: derive completion candidates from ops or cycle state, capture records, commit or CI proof, and review-gated shipped markers; auto-gate or mark done only when evidence is deterministic and logged.
4. [ ] [#auto-complete-semantic-retrieval] #agent-doc-bug Automated completion detection phase 3: integrate the tsift-memory or MemGraphRag retrieval path so preflight and plan can propose already-resolved semantic matches such as completed free-text queue mirrors without embedding the heavy tsift CLI. Relates #agent-doc-memgraphrag-retrieval.
5. [ ] [#stale-busy-after-auto-inject-no-clear] #agent-doc-bug DONE 7b60fcdc (CI 27075841879 green): supervisor idle-queue watch self-heals a stale-busy actor over an idle pane (poll reconcile from has_busy_cue, ~2s debounce, persisted via mark_lifecycle). Re-listed to match the still-pending queue mirror; strike via the queue drain. Live-verify tracked as #stale-busy-self-heal-live-verify.
6. [/] [#jb-run-preempt-followups] #agent-doc-bug follow-ups from plan-jb-run-preempt-autoloop-priority.md (sub-parts 2-4, head-insert shipped 79e0406d): (2) empty-queue autoloop suppression — overlaps #post-clear-auto-queue-suppress; (3) 60s busy ready-wait stall audit when a queue fallback exists; (4) recursion-guard escape hatch for a wedged loop ('recursive direct invocation would deadlock' blocking the one Run Agent Doc that could break the stall) — overlaps #autoloop-command-preemption. Plan: tasks/agent-doc/plan-jb-run-preempt-autoloop-priority.md
7. [ ] [#jb-busy-active-turn-stall-live-verify] [recommended] live-verify #jb-run-agent-doc-busy-active-turn-stall end-to-end: with prompt-bearing drift present in a doc whose Codex pane is mid-turn, click JB Run Agent Doc and confirm the session-still-running notification fires IMMEDIATELY (logs route_dispatch_only_busy_active_turn_skip_wait) instead of after a 60s stall. Deterministic core shipped b93b3c3c (skip busy ready-wait on proven active-turn busy_proof_line); could not repro the exact prompt-drift-while-busy path non-disruptively on the live monsterrodholders pane this session.
8. [ ] [#adoc-route-busy-fix-superproject-pointer-push] superproject main pointer push for the #jb-run-agent-doc-busy-active-turn-stall fix is DEFERRED (local commit 13600139e): pull --rebase conflicts on src/boost-client submodule pointer (concurrent-session shared-checkout divergence). Submodule fix b93b3c3c is pushed to origin/20260422/codex-agent-backend; land the superproject pointer on a clean cycle once boost-client divergence resolves.
9. [ ] [#multi-session-shared-build-isolation] #agent-doc-bug: the multi-session model lets a concurrent session's untracked/uncommitted work break the shared src/agent-doc crate build, forcing an isolated git-worktree-inside-src verification dance to ship a fix (committed+CI-green but not live until the other session compiles). Consider session-isolated build/worktree isolation so concurrent agent-doc sessions don't block each other's build/install. Replaces the deleted feedback_verify_via_worktree_inside_src memory workaround.
10. [ ] [#sync-fast-pane-hotpath] #agent-doc-bug: editor file-switch sync blocks because the fast pane-focus (safe_passive_focus_actor_after_sync_lock, sync.rs:467) runs AFTER the sync lock (SYNC_LOCK_WAIT_BUDGET 3s) + stacked reconcile budgets (router 1s, ownership proof 750ms, prune 1s). The claiming pane is usually already known (cheap cached read) but showing it is serialized behind lock contention + heavy reconcile. Fix: pre-lock fast pane resolution from the lock-free local projection + select_pane immediately (≤300ms target), create-on-miss without blocking, defer prune/router/ownership-proof/repair to a background reconcile, and make the fast select idempotent with the reconciler's final placement (race-safe; the lock guards concurrent stash/swap). Plan: tasks/agent-doc/plan-sync-pipeline-fast-pane-hotpath.md
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
