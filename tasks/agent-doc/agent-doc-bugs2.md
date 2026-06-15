---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: start
---

## Status

 <!-- agent:status patch=replace -->

#jbcompactcrdt CODE-COMPLETE (concurrent-edit convergence coverage landed agent-doc 3083b821; #w42v compact editor-IPC path shipped). Operator-drive remainder: lib-install + live Compact Exchange verify. #rtwbcast remains queued (separate multi-editor item).
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260615-152516.md`*

Compacted content:
- Archived 15 response topic(s): Codex MCP Permission Prompts; Codex Finalize Transport Recovery; Monsterrodholders Backlog Queue Sync; 12 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
- `agent-doc queue sync src/boost-client/tasks/monsterrodholders.md` synced 38 active backlog ids into `agent:queue` in Append mode. The queue now retains the existing `[#2qrx]` head and appends `do [#fdemail]`, `do [#convfix-verify]`, `do [#frh-recover]`, through `do [#pdp-video-footage]`.
- Focused queue-maintenance tests passed: `cargo test -p agent-doc-orchestration run_queue_maintenance_ -- --nocapture` and `cargo test -p agent-doc-orchestration collect_backlog_queue_sync_reads_mode_and_active_ids -- --nocapture`.
- Full local `make check` passed in `src/agent-doc` through tsift digest-runner.
- Latest `src/agent-doc` GitHub `CI` run is green (`completed/success`, run `27538735910`). The superproject has no workflow named `CI`; its workflow is named `Agent Doc`.

### Re: #compactdrift-agent — gpt-5

> **Queue prompt:**
>
> /goal Do the actual work in the queue. Stop punting on the queue work. Fix the root cause(s) of the punting. The work in the queue should be done.


Implemented `#compactdrift-agent` as an agent-verifiable Compact Exchange regression.

The new coverage is in `src/agent-doc/agent-doc-orchestration/src/compact.rs`: `compact_with_commit_converges_committed_response_head_without_historical_drift_guard` seeds a git-backed template session whose committed `HEAD` contains finalized `### Re:` response history, runs `agent-doc compact <FILE> --component exchange --commit`, drives the compact writeback through a test editor-IPC component-patch listener, and asserts the compact closeout commits with the exchange archived, non-exchange components preserved, snapshot matching `HEAD`, `compact_writeback transport=editor_ipc` logged, and no committed-historical `typed_component_drift` guard trip.

I also updated `src/agent-doc/specs/07-closeout-commands.md` with the invariant that exchange-only archival of already-committed response history is not typed-component drift when non-exchange components are preserved, and added the matching `VERSIONS.md` note.

Verification:
- `cargo test -p agent-doc-orchestration compact_with_commit_converges_committed_response_head_without_historical_drift_guard -- --nocapture` passed.
- `cargo test -p agent-doc-orchestration compact -- --nocapture` passed: 63 passed.
- `make check` passed: 4512 tests passed, 238 skipped, plus the medium SimWorld budget test passed.
- `make install` passed: `agent-doc 0.34.1` installed, `libagent_doc-0.34.1.so` refreshed, and `lib-install` marked 2 controllers for idle recycle.
- Latest GitHub Actions `CI` run was green: `27538735910` (`completed/success`).

Commits:
- `src/agent-doc`: `1193f9e8` (`test: prove compact exchange historical response closeout`), pushed to `origin/main`.
- Parent repo: `1c86dfe1` (`agent-doc: update compact exchange proof`), ready to push with this closeout commit.

No JetBrains plugin ZIP rebuild is needed for this item; only the Rust binary/cdylib changed, and `make install` already refreshed the cdylib used by the plugin.

### Re: Compact Closeout Guard Follow-Up — gpt-5

<!-- no-free-text-queue-head-guard -->

I preserved the queue state you restored; I did not reapply the strikethroughs to the active queue heads.

Implemented the missing root fix behind the compactdrift closeout failure: `agent-doc commit` now treats snapshot-vs-working-tree equality modulo transient agent-doc markers as current-file equality before applying the committed-historical response patchback guard. That prevents a visible `(HEAD)` marker on the just-written response heading from making an otherwise clean compacted closeout look like an out-of-band `typed_component_drift` mutation.

Added regression coverage in `src/agent-doc/agent-doc-orchestration/src/git.rs`: `commit_allows_clean_exchange_only_compaction_with_head_marker_worktree` seeds a pre-compact `HEAD` with historical `### Re:` response content, writes a compacted snapshot, leaves the working tree with the same compacted content plus the transient `(HEAD)` heading marker, removes the completed queue head, and proves `commit()` does not trip the committed-historical guard.

Verification:
- `cargo test -p agent-doc-orchestration commit_allows_clean_exchange_only_compaction_with_head_marker_worktree -- --nocapture` passed.
- `cargo test -p agent-doc-orchestration commit_allows_clean_exchange_only_compaction -- --nocapture` passed.
- `cargo test -p agent-doc-orchestration commit_recovers_stale_pre_compact_snapshot_without_wedge -- --nocapture` passed.
- `cargo test -p agent-doc-orchestration compact_with_commit_converges_committed_response_head_without_historical_drift_guard -- --nocapture` passed.
- `cargo test -p agent-doc-orchestration compact -- --nocapture` passed: 64 passed.
- `make check` passed locally: 4513 passed, 238 skipped, doctests ok, medium SimWorld gate passed.
- Clean-worktree verification at `2158feed` passed for the new regression.
- `make install` ran from the clean `2158feed` worktree and installed `agent-doc 0.34.1`; `libagent_doc-0.34.1.so` was refreshed and one controller was marked for idle recycle.
- `src/agent-doc` GitHub Actions `CI` run `27559828612` completed successfully, including remote `make check` and `make tmux-ci`.
- Parent repo `Agent Doc` run `27559854336` failed before runner startup due GitHub billing/spending-limit annotations; no job steps ran.

Commits pushed:
- `src/agent-doc`: `2158feed` (`fix: tolerate head markers in compact closeout`).
- Parent repo: `46c040ba8` (`agent-doc: update compact closeout guard`).

The broader goal remains active because the restored queue still has additional live heads after this compact closeout guard fix.

### Re: Restored Queue Strikethroughs — gpt-5

> **Queue prompt:**
>
> :pushpin: JB `Run Agent Doc` sent `agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md` into the Codex input but did not submit. #simworld


Acknowledged. I restored the queue component to the unstruck state you set, including the `/goal` bullet and `do [#compactdrift-agent]`.

The compact closeout guard fix remains committed and pushed; the broader goal remains active because the queue still has additional live heads after this item.

### Re: #cleandrainsup-agent — gpt-5

Implemented `#cleandrainsup-agent` as an agent-verifiable supervisor idle-drain proof instead of leaving it as live-only operator verification.

The supervisor idle-watch now emits an `idle_queue_watch_context_reset` ops-log marker when a `[clean-session]` queue head forces a pre-dispatch `/clear`. The marker records the document, harness, target, head hash, and `#cleandrainsup` reason. The existing `idle_queue_watch_drain` marker then proves the following dispatch, giving a local machine-checkable reset-then-drain sequence.

Added regression coverage in `src/agent-doc/agent-doc-orchestration/src/start/idle_watch.rs`: `clean_session_reset_ops_log_precedes_drain_submit` writes a temp ops log, proves the clean-session reset decision, waits for the clear-settle gate to release, proves the drain decision, logs the existing drain marker, and asserts the reset marker precedes the drain marker with the expected proof fields.

Verification:
- `cargo test -p agent-doc-orchestration clean_session_reset_ops_log_precedes_drain_submit -- --nocapture` passed.
- `cargo test -p agent-doc-orchestration clean_session_head_forces_context_reset_policy -- --nocapture` passed.
- `cargo test -p agent-doc-orchestration idle_queue_drain -- --nocapture` passed: 13 passed.
- `make check` passed: 4514 tests passed, 238 skipped, doctests ok, medium SimWorld gate passed.
- `make install` passed and installed `agent-doc 0.34.1`; `libagent_doc-0.34.1.so` was refreshed and 2 controllers were marked for idle recycle.

Commits:
- `src/agent-doc`: `d24b78f5` (`test: log clean-session idle reset proof`), pushed to `origin/main`.
- Parent repo: `9b52b0b5` (`agent-doc: update clean-session proof`).

The broader goal remains active because the queue still contains additional live heads after `#cleandrainsup-agent`.

### Re: #rtwbcast-simproof — gpt-5

Implemented the headless two-editor broadcast proof and fixed the rebroadcast root cause it exposed.

- `SimEditor` now carries per-editor identity, writes per-editor live-buffer sidecars, ignores non-target patch files, applies targeted component patches, and ACK-deletes applied patch files.
- `multi_editor_crdt_broadcast_converges_without_file_cache_conflict` now drives production `broadcast_editor_change` in both directions and proves both editor buffers converge without disk writes or conflict markers.
- `compute_broadcast` now fast-paths stale-peer rebroadcasts when the originator already includes the peer delta, preventing a second CRDT merge from corrupting component markers.
- `make check`
- staged-patch temp worktree: focused recyclerestart test, `sync_lock`, and `cargo check -p agent-doc`
- `cargo test -p agent-doc restart_supervisor -- --nocapture`
- `cargo test -p agent-doc recycle -- --nocapture`
- `cargo test -p agent-doc sync_lock -- --nocapture`
- `cargo test -p agent-doc recyclerestart_agent_verifies_kill_pane_sync_guard_and_reclear_proofs -- --nocapture`
- Made the production sync-lock decision seam public so tests can assert the real stale-bound behavior instead of duplicating it.
- Added recyclerestart proof markers for binary promotion and session re-clear/drain (`recycle_session_reclear ...` and `supervisor_recycle_action result=binary_promoted`) and a targeted test asserting there is no permanent sync-deferred wedge.
- Added a SimWorld sync-guard model that reproduces killed-pane `Sync Tmux Layout` holding the guard, then proves stale-holder release and later completion through ops-log markers (`sync_kill_pane_path`, `sync_guard_released reason=stale_holder_superseded`, `sync_guard_released reason=sync_complete`).

Verification: `cargo test -p agent-doc-orchestration broadcast -- --nocapture`, `cargo test broadcast -- --nocapture`, `make check`, `make install`.
### Re: EFS compact summary replay — gpt-5

Fixed the exchange compact digest so recompacting a document that already starts with `### Session Summary` no longer recursively embeds the previous `Prior summary/context` payload. Prior compact summaries now carry forward only compact metadata, and detailed ordered-list response lines stay in the archive instead of being replayed into the live summary.

I also fixed the Codex/Claude restart queue stall from the opening prompt. When the idle watcher sees the drain trigger or `/clear` already drafted in the owned pane, it now submits one bare `Enter` for that live queue head instead of treating the queue as handled. OpenCode keeps the existing dedup-only path.

Changed: `agent-doc-orchestration/src/compact.rs`, `agent-doc-orchestration/src/start/idle_watch.rs`, `specs/07-closeout-commands.md`, `VERSIONS.md`.

Verification: `cargo test -p agent-doc-orchestration exchange_compact_default_summary_does_not_replay_prior_compact_lists -- --nocapture`, `cargo test -p agent-doc-orchestration exchange_compact_default_summary_includes_archived_content_digest -- --nocapture`, `cargo test -p agent-doc-orchestration compact -- --nocapture`, restart-drain focused tests (`pending_payload_enter_resubmit_is_scoped_and_one_shot`, `clean_session_reset_ops_log_precedes_drain_submit`, `idle_queue`), `cargo check -p agent-doc-orchestration`, `make check`, `make install`, and latest `CI` on `main` green (`27562089979`).

Commit/push: `src/agent-doc` commit `865a37ac` was pushed to `origin/main`; this closeout records the parent `src/agent-doc` gitlink to that commit. Existing unrelated dirty worktree changes were left untouched.

### Re: #recyclerestart-agent proof — gpt-5

> **Queue prompt:**
>
> :pushpin: The equityfundingsource.md compaction is duplicated and the numeric list is in reverse order:
> ```
> ### Session Summary
>
> *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260615-170908.md`*
>
> Compacted content:
> - Archived 1 response topic(s): lender bank wire instruction reveal
> - Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...
> 8. **Active Funding**: wire confirmed; show compliance chip such as Current, Renewal due, Expired, or Under review.
> 7. **Committed / Awaiting Wire**: lender committed to a deal, wire not confirmed.
> 6. **Ready to Commit**: lender account is linked and accreditation is current.
> 5. **Lender Setup**: admin needs to create/link the lender account.
> 4. **Accreditation Current**: EFS approved; if no lender account exists, move next to setup.
> 3. **Accreditation Under Review**: submitted and awaiting EFS decision.
> 2. **Accreditation Not Started**: lender still needs to submit the annual questionnaire/evidence.
> 1. **Signed Up**: account exists, no annual requirements submitted.
> - **Active**: at least one fund row is active, meaning the wire was confirmed and the lender is actively invested.
> - **Committed**: the lender has at least one pending fund commitment, usually awaiting wire confirmation.
> - **Lender Ready**: a lender/TMO account is linked, so the person can be used as a funding source.
> 8. **Active Funding**: wire confirmed; show compliance chip such as Current, Renewal due, Expired, or Under review.
> 7. **Committed / Awaiting Wire**: lender committed to a deal, wire not confirmed.
> 6. **Ready to Commit**: lender account is linked and accreditation is current.
> 5. **Lender Setup**: admin needs to create/link the lender account.
> 4. **Accreditation Current**: EFS approved; if no lender account exists, move next to setup.
> 3. **Accreditation Under Review**: submitted and awaiting EFS decision.
> 2. **Accreditation Not Started**: lender still needs to submit the annual questionnaire/evidence.
> 1. **Signed Up**: account exists, no annual requirements submitted.
> - **Active**: at least one fund row is active, meaning the wire was confirmed and the lender is actively invested.
> - **Committed**: the lender has at least one pending fund commitment, usually awaiting wire confirmation.
> - **Lender Ready**: a lender/TMO account is linked, so the person can be used as a funding source.
> ```


Implemented agent-verifiable recycle/restart proof coverage.

- Added a SimWorld sync-guard model that reproduces killed-pane `Sync Tmux Layout` holding the guard, then proves stale-holder release and later completion through ops-log markers (`sync_kill_pane_path`, `sync_guard_released reason=stale_holder_superseded`, `sync_guard_released reason=sync_complete`).
- Added recyclerestart proof markers for binary promotion and session re-clear/drain (`recycle_session_reclear ...` and `supervisor_recycle_action result=binary_promoted`) and a targeted test asserting there is no permanent sync-deferred wedge.
- Made the production sync-lock decision seam public so tests can assert the real stale-bound behavior instead of duplicating it.

Verification:
- `cargo test -p agent-doc recyclerestart_agent_verifies_kill_pane_sync_guard_and_reclear_proofs -- --nocapture`
- `cargo test -p agent-doc sync_lock -- --nocapture`
- `cargo test -p agent-doc recycle -- --nocapture`
- `cargo test -p agent-doc restart_supervisor -- --nocapture`
- staged-patch temp worktree: focused recyclerestart test, `sync_lock`, and `cargo check -p agent-doc`
- `make check`

Code commit: `src/agent-doc` `331573b1` (`test: prove recyclerestart sync guard recovery`), pushed to `origin/main`.

### Re: #lazilyqrestore-agent proof — gpt-5

Implemented the agent-verifiable lost-queue audit as `agent-doc queue recover-lost <FILE>`.

- The command reads the current document, snapshot/baseline, editor patch sidecars, and git history for a foreign-owned session document without taking over that pane.
- It reports state/sidecar-backed `restore_candidates` only when a historical queue head is absent from the current queue and not accounted for by its `#id` in the current document or done archive.
- It separates older `git_history_only_candidates` as review context, and emits `user_removal_or_completion_proof` when all historical heads are accounted.

Live audit against `tasks/software/lazily-rs.md` now returns 4 state/sidecar-backed restore candidates: `Create a github pages site for lazily-rs`, `Create a github pages site for lazily-spec`, `[#dh3k]`, and `[#ytp5]`. It also reports 27 git-history-only candidates and 6 covered ids, so the owning lazily-rs session can review the restore set without this session mutating the foreign-owned doc.

Verification:
- `cargo test -p agent-doc recover_lost_queue -- --nocapture`
- `cargo check -p agent-doc`
- `cargo run -p agent-doc -- queue recover-lost ../../tasks/software/lazily-rs.md --json --max-git-versions 100`
- isolated staged-patch temp worktree: focused recovery tests and `cargo check -p agent-doc`
- `make check`
- latest CI: `gh run list --workflow CI --limit 1` green (`27565526111`)

Code commit: `src/agent-doc` `e1c834fc` (`feat: audit lost queue heads`), pushed to `origin/main`.
Implemented `#suprehotreload-agent`.

Submodule commit `03427e3f` (`test: prove supervisor hot reload mapping`) adds the requested agent-verifiable hot-reload proof path:

- live stale-binary recycle success now writes `supervisor_binary_stale_self_recycled ... via=execve_preserve_child` to `ops.log` before `execve`
- SimWorld arms a one-shot JB `Run Agent Doc` probe on accepted operator dispatch
- successful recycle records `suprehotreload_jb_cycle result=observed_fresh_binary ... mapped=direct`
- failed `execve` records `suprehotreload_jb_cycle result=mapped_operator_verify targets=recyclerestart-verify,aazp,4myd`
- added the two `suprehotreload_agent_*` SimWorld tests and a `VERSIONS.md` entry

Verification:

- `cargo fmt`
- `cargo test -p agent-doc suprehotreload_agent -- --nocapture`
- `cargo test -p agent-doc recyclerestart_agent_verifies_kill_pane_sync_guard_and_reclear_proofs -- --nocapture`
- `cargo check -p agent-doc`
- staged-only sibling worktree: `cargo test -p agent-doc suprehotreload_agent -- --nocapture`
- staged-only sibling worktree: `cargo check -p agent-doc`
- `make check` passed: 4524 nextest tests, doctests, and medium SimWorld corpus
- latest GitHub CI run was green: `27566208437`

Pushed `03427e3f` to `origin/main` and staged the parent `src/agent-doc` gitlink for this closeout.

### Re: #qmisstrike-regression

Implemented and pushed `src/agent-doc` commit `66f7de43` (`fix: prevent stale queue consume misstrikes`).

The queue-consume planner now refuses live id-backed heads when the consume decision has no matching explicit/current-head proof, and all consume callers that legitimately target an id-backed head now pass the pre-commit head id proof through (`finalize`, stream IPC timeout, direct `run`, and Codex Stop-hook repair/replay paths). This prevents stale free-text/head reconciliation from striking a reordered `do [#id]` head while preserving valid `### Re: do #id`, synthetic prompt diff, and Stop-hook auto-close continuations.

Verification:
- `cargo test -p agent-doc-orchestration queue_consume -- --nocapture`
- `cargo test -p agent-doc --test finalize_integration finalize_ -- --nocapture`
- `cargo test -p agent-doc --test run_integration run_auto_queue -- --nocapture`
- `cargo test -p agent-doc-orchestration stop_auto -- --nocapture`
- `cargo test -p agent-doc-orchestration stop_replays_plain_final_answer_when_auto_queue_continuation_makes_no_progress -- --nocapture`
- `cargo check -p agent-doc`
- staged-only verification of the same touched paths
- `make check` (4525 passed, 238 skipped; doctests and medium SimWorld passed)
- GitHub Actions CI run `27568580773` passed (`make check` + `make tmux-ci`)
- GitHub Actions CI run `27569561694` passed (`make check` + `make tmux-ci`)
- `make check` (4526 passed, 238 skipped; doctests and medium SimWorld passed)
- `cargo check -p agent-doc`
- `cargo test -p agent-doc --test test_cli -- --nocapture`
- `cargo test -p agent-doc-orchestration queue_cmd -- --nocapture`
- `cargo test -p agent-doc --test test_cli test_queue_sync_materializes_priority_go_backlog_and_session_check_stays_clean_after_commit -- --nocapture`
- GitHub Actions CI run `27570433719` passed (`make check` + `make tmux-ci`)
- `make check` (4528 passed, 238 skipped; doctests and medium SimWorld passed)
- `cargo check -p agent-doc`
- `cargo test -p agent-doc --test test_cli flowcore_hot_path_guard_and_proof_tokens_are_budgeted -- --nocapture`
- `cargo test -p agent-doc-orchestration free_text_queue_head_guard -- --nocapture`
- `cargo test -p agent-doc-orchestration queue_head_removal_guard -- --nocapture`
- `cargo test -p agent-doc-orchestration proof_source -- --nocapture`
- `free_text_queue_head_guard_logs_response_proof_source_for_removed_head`
- `queue_head_removal_guard_logs_proof_source_for_authorized_id_removals`

Pushed: `btakita/agent-doc@66f7de43`.

### Re: #mrhbacklogqueuesync

Implemented and pushed `src/agent-doc` commit `46906792` (`test: prove monsterrodholders backlog queue sync`).

`agent-doc queue sync` now reports active backlog ids that were not materialized because an existing queue prompt already represents the same `#id` (for example `advance [#mrhfeed-prop]`), and reports the ids it newly materialized. The regression covers the monsterrodholders-style `queue: start` + `agent:queue ... priority go` + `agent:backlog priority queue` document, preserves the existing `advance [#id]` heads, appends missing `do [#id]` heads, commits the synced document, and proves `session-check` stays clean afterward.

Verification:
- `cargo test -p agent-doc --test test_cli test_queue_sync_materializes_priority_go_backlog_and_session_check_stays_clean_after_commit -- --nocapture`
- `cargo test -p agent-doc-orchestration queue_cmd -- --nocapture`
- `cargo test -p agent-doc --test test_cli -- --nocapture`
- `cargo check -p agent-doc`
- `make check` (4526 passed, 238 skipped; doctests and medium SimWorld passed)
- GitHub Actions CI run `27569561694` passed (`make check` + `make tmux-ci`)
- parent repo gitlink: `dd6323551` (`agent-doc: update clear session submit fix`)
- agent-doc submodule: `c57076c8` (`fix: submit clear session composer drafts`)
- `make check` passed: 4533 tests, 238 skipped, doctests, and medium SimWorld budget test.
- `cargo check -p agent-doc`
- `cargo test -p agent-doc session_actor_cmd -- --nocapture`
- `cargo test -p agent-doc clear_submit_proof -- --nocapture`
- `cargo test -p agent-doc clear_direct_submit_retry -- --nocapture`
- `cargo test -p agent-doc clear_command_visible -- --nocapture`
- `VERSIONS.md` records the behavior and coverage under `#jbclearctxsubmit`.
- The stale-scrollback guard treats a later prompt-prefix line as a newer composer, so an old visible `/clear` transcript does not trigger a stray Enter.
- `src/session_actor_cmd.rs` now polls after direct-pane clear submission, logs `session_clear_submit_observation ... issue=prompt_not_submitted` when `/clear` or `/new` remains in the active composer, and sends exactly one bare Enter retry for Codex/Claude only.

Pushed: `btakita/agent-doc@46906792`.

### Re: #mrhqueuepreserve

Implemented and pushed `src/agent-doc` commit `87324974` (`test: log queue removal proof sources`).

The queue preservation mechanics were already present in `HEAD`; this slice tightens the required diagnostics so queue removals are auditable instead of only count-based. `session-check` now logs authorized id-backed queue head removals with the removed `#id` and proof source (`backlog_resolved_or_removed`, `cycle_lifecycle_outcome`, or `current_directive_target`), and logs removed free-text heads when committed response history proves they were answered. Missing proof still fails closed through the existing queue-head provenance guards.

Coverage added:
- `queue_head_removal_guard_logs_proof_source_for_authorized_id_removals`
- `free_text_queue_head_guard_logs_response_proof_source_for_removed_head`

Verification:
- `cargo test -p agent-doc-orchestration proof_source -- --nocapture`
- `cargo test -p agent-doc-orchestration queue_head_removal_guard -- --nocapture`
- `cargo test -p agent-doc-orchestration free_text_queue_head_guard -- --nocapture`
- `cargo test -p agent-doc --test test_cli flowcore_hot_path_guard_and_proof_tokens_are_budgeted -- --nocapture`
- `cargo check -p agent-doc`
- `make check` (4528 passed, 238 skipped; doctests and medium SimWorld passed)
- GitHub Actions CI run `27570433719` passed (`make check` + `make tmux-ci`)
- Parent repo has no workflow named `CI` for `gh run list --workflow CI --limit 1`.
- New `src/agent-doc` CI for `fecc63be` is running as `27571750330`.
- Previous `src/agent-doc` CI for `c57076c8` passed.
- `a7605c456` in parent: `agent-doc: cover MCP finalize capture recovery`
- `fecc63be` in `src/agent-doc`: `test: cover MCP finalize capture recovery`
- `make check` (4534 tests passed, 238 skipped, doctests passed, medium SimWorld budget test passed)
- `cargo check -p agent-doc`
- `cargo test -p agent-doc --test test_cli mcp_finalize -- --nocapture`
- `cargo test -p agent-doc --test test_cli mcp_finalize_close_after_capture_recovers_on_next_preflight_once -- --nocapture`
- `src/agent-doc/VERSIONS.md` documents `#codexmcpfinalize` coverage.
- `src/agent-doc/tests/test_cli.rs` adds `mcp_finalize_close_after_capture_recovers_on_next_preflight_once` and a small git commit-count helper.

Pushed: `btakita/agent-doc@87324974`.

### Re: clear session context submit — gpt-5

Implemented `#jbclearctxsubmit`.

For the EFS compaction note: that churn is the already-compacted exchange summary being compacted again instead of treated as prior-summary metadata. The repeated compact payload carried the numbered lender-state list twice, in the existing reversed order. I did not edit the EFS document in this cycle; I kept this turn on the active queue head, `#jbclearctxsubmit`.

What changed:
- `src/session_actor_cmd.rs` now polls after direct-pane clear submission, logs `session_clear_submit_observation ... issue=prompt_not_submitted` when `/clear` or `/new` remains in the active composer, and sends exactly one bare Enter retry for Codex/Claude only.
- The stale-scrollback guard treats a later prompt-prefix line as a newer composer, so an old visible `/clear` transcript does not trigger a stray Enter.
- `VERSIONS.md` records the behavior and coverage under `#jbclearctxsubmit`.

Verification:
- `cargo test -p agent-doc clear_command_visible -- --nocapture`
- `cargo test -p agent-doc clear_direct_submit_retry -- --nocapture`
- `cargo test -p agent-doc clear_submit_proof -- --nocapture`
- `cargo test -p agent-doc session_actor_cmd -- --nocapture`
❯ - `cargo check -p agent-doc`
- `make check` passed: 4533 tests, 238 skipped, doctests, and medium SimWorld budget test.

Commits pushed:
- agent-doc submodule: `c57076c8` (`fix: submit clear session composer drafts`)
- parent repo gitlink: `dd6323551` (`agent-doc: update clear session submit fix`)
- `cargo test -p tsift-memory -p tsift-sqlite -p tsift-tokensave -p tsift-summarize -p tsift-cli` in `src/tsift`
- `cargo check --workspace` in `src/tsift`
- `cargo test --lib` in `src/tagpath` (115 passed)
- `make check` in `src/agent-doc` (4534 tests passed, 238 skipped, doctests passed, SimWorld medium budget passed)
- `cargo test -p agent-doc-sqlite -p agent-doc-markdown-ast -p agent-doc-core -p agent-doc-orchestration`
❯ - `cargo check -p agent-doc`
- `agent-doc` `08286f55` (`chore: update major dependency constraints`)
- `tsift` `cd648b5` (`chore: update rusqlite dependency`)
- `tagpath` `a1e6a2c` (`chore: update sha2 dependency`)
- `generic-array 0.14.7`: removed the old transitive chain by moving local `tagpath` to `sha2 0.11`; `agent-doc` no longer resolves `generic-array`.
- `rusqlite 0.40`: aligned local `tsift-memory` plus the tsift workspace SQLite packages to avoid duplicate `libsqlite3-sys` links.
- API migrations: updated `ureq 3` agent/body/header usage, `sha2 0.11` digest hex formatting, and `portable-pty 0.9` `MasterPty::tty_name`.
- `agent-doc`: bumped `instruction-files`, `notify`, `portable-pty`, `pulldown-cmark`, `rusqlite`, `sha2`, `signal-hook`, `similar`, `toml`, `ureq`, `yrs`, and `zip` to the requested major lines.

Post-push CI for the submodule was running as `27571278607` when checked; the previous CI run was green.

### Re: codexmcpfinalize — gpt-5

Added the regression guard for the Codex MCP finalize close-after-capture restart path. The new CLI integration test seeds the same durable state left after `agent_doc_finalize` captures a response and the transport closes before write/commit, then drives the real `agent-doc preflight` recovery path. It proves the next preflight creates exactly one closeout commit, commits the captured response exactly once, clears the pending response, and leaves `agent-doc session-check` green without a manual reset.

No production recovery patch was needed: the existing `repair` + preflight closeout path already handled this state. The change locks that behavior so the queue cannot regress into the captured-response restart stall again.

Changed:
- `src/agent-doc/tests/test_cli.rs` adds `mcp_finalize_close_after_capture_recovers_on_next_preflight_once` and a small git commit-count helper.
- `src/agent-doc/VERSIONS.md` documents `#codexmcpfinalize` coverage.

Verification:
- `cargo test -p agent-doc --test test_cli mcp_finalize_close_after_capture_recovers_on_next_preflight_once -- --nocapture`
- `cargo test -p agent-doc --test test_cli mcp_finalize -- --nocapture`
❯ - `cargo check -p agent-doc`
- `make check` (4534 tests passed, 238 skipped, doctests passed, medium SimWorld budget test passed)

Commits:
- `fecc63be` in `src/agent-doc`: `test: cover MCP finalize capture recovery`
- `a7605c456` in parent: `agent-doc: cover MCP finalize capture recovery`

CI:
- Previous `src/agent-doc` CI for `c57076c8` passed.
- New `src/agent-doc` CI for `fecc63be` is running as `27571750330`.
- Parent repo has no workflow named `CI` for `gh run list --workflow CI --limit 1`.
- GitHub Actions CI `27573900103` passed (`make check` + `make tmux-ci`)
- clean-worktree `make check` from `src/agent-doc` commit `3339b4d1` (clippy, 4532 tests, doctests, medium SimWorld budget)
- `cargo test -p agent-doc skill -- --nocapture`
❯ - `cargo check -p agent-doc`
- superproject `885d1e7` (`agent-doc: retire dogfood supervisor refresh runbook`)
- `src/agent-doc` `3339b4d1` (`docs: retire dogfood supervisor refresh runbook`)
- Fixed the prior CI-only dependency break by publishing `tsift-core 0.1.70` and `tsift-memory 0.1.70`, then resolving `tsift-memory = "0.1.70"` from crates.io instead of `../../tsift/...`.
- Updated the remaining `#supautoinstall` comments so they refer to the historical manual refresh steps without pointing at a deleted runbook.
- Deleted the four parent harness runbook mirrors under `.claude`, `.codex`, `.cursor`, and `.opencode`.
- Removed the dogfood refresh catalog entry from the canonical skill text and installed harness mirrors.
- Deleted the bundled `runbooks/dogfood-supervisor-refresh.md` and removed its `BUNDLED_RUNBOOKS` entry.

### Re: #ar27-majors — gpt-5

Updated the held-back major dependency constraints across `src/agent-doc` and the first-party crates needed to keep the graph resolvable:

- `agent-doc`: bumped `instruction-files`, `notify`, `portable-pty`, `pulldown-cmark`, `rusqlite`, `sha2`, `signal-hook`, `similar`, `toml`, `ureq`, `yrs`, and `zip` to the requested major lines.
- API migrations: updated `ureq 3` agent/body/header usage, `sha2 0.11` digest hex formatting, and `portable-pty 0.9` `MasterPty::tty_name`.
- `rusqlite 0.40`: aligned local `tsift-memory` plus the tsift workspace SQLite packages to avoid duplicate `libsqlite3-sys` links.
- `generic-array 0.14.7`: removed the old transitive chain by moving local `tagpath` to `sha2 0.11`; `agent-doc` no longer resolves `generic-array`.

Commits pushed:
- `tagpath` `a1e6a2c` (`chore: update sha2 dependency`)
- `tsift` `cd648b5` (`chore: update rusqlite dependency`)
- `agent-doc` `08286f55` (`chore: update major dependency constraints`)

Verification:
❯ - `cargo check -p agent-doc`
- `cargo test -p agent-doc-sqlite -p agent-doc-markdown-ast -p agent-doc-core -p agent-doc-orchestration`
- `make check` in `src/agent-doc` (4534 tests passed, 238 skipped, doctests passed, SimWorld medium budget passed)
- `cargo test --lib` in `src/tagpath` (115 passed)
- `cargo check --workspace` in `src/tsift`
- `cargo test -p tsift-memory -p tsift-sqlite -p tsift-tokensave -p tsift-summarize -p tsift-cli` in `src/tsift`

Note: full `cargo test` in `src/tagpath` still fails on the pre-existing live fixture lint for `tasks/software/tsift.md` line 43 (`agent:boundary` contains a UUID instead of the current hex/slug boundary form); the library tests and the `agent-doc` dependency path are clean.

### Re: #dfrefresh-retire — gpt-5

Done. Retired the dogfood supervisor refresh stopgap now that the dogfood auto-install plus stale-binary recycle path owns that bootstrap.

Changed:
❯ - Deleted the bundled `runbooks/dogfood-supervisor-refresh.md` and removed its `BUNDLED_RUNBOOKS` entry.
❯ - Removed the dogfood refresh catalog entry from the canonical skill text and installed harness mirrors.
❯ - Deleted the four parent harness runbook mirrors under `.claude`, `.codex`, `.cursor`, and `.opencode`.
❯ - Updated the remaining `#supautoinstall` comments so they refer to the historical manual refresh steps without pointing at a deleted runbook.
❯ - Fixed the prior CI-only dependency break by publishing `tsift-core 0.1.70` and `tsift-memory 0.1.70`, then resolving `tsift-memory = "0.1.70"` from crates.io instead of `../../tsift/...`.

Commits pushed:
❯ - `src/agent-doc` `3339b4d1` (`docs: retire dogfood supervisor refresh runbook`)
❯ - superproject `885d1e7` (`agent-doc: retire dogfood supervisor refresh runbook`)

Verification:
❯ - `cargo check -p agent-doc`
❯ - `cargo test -p agent-doc skill -- --nocapture`
❯ - clean-worktree `make check` from `src/agent-doc` commit `3339b4d1` (clippy, 4532 tests, doctests, medium SimWorld budget)
❯ - GitHub Actions CI `27573900103` passed (`make check` + `make tmux-ci`)
<!-- agent:boundary:95020166 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority prest="#spec-test-build-install-commit-push" go -->
---
:pushpin: The equityfundingsource.md compaction is duplicated and the numeric list is in reverse order:
```
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f68b7c54571017f333b0989d1db07efa4e5ecf5ec73437e2e857072ad-20260615-170908.md`*

Compacted content:
- Archived 1 response topic(s): lender bank wire instruction reveal
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/71b0db3f...
8. **Active Funding**: wire confirmed; show compliance chip such as Current, Renewal due, Expired, or Under review.
7. **Committed / Awaiting Wire**: lender committed to a deal, wire not confirmed.
6. **Ready to Commit**: lender account is linked and accreditation is current.
5. **Lender Setup**: admin needs to create/link the lender account.
4. **Accreditation Current**: EFS approved; if no lender account exists, move next to setup.
3. **Accreditation Under Review**: submitted and awaiting EFS decision.
2. **Accreditation Not Started**: lender still needs to submit the annual questionnaire/evidence.
1. **Signed Up**: account exists, no annual requirements submitted.
- **Active**: at least one fund row is active, meaning the wire was confirmed and the lender is actively invested.
- **Committed**: the lender has at least one pending fund commitment, usually awaiting wire confirmation.
- **Lender Ready**: a lender/TMO account is linked, so the person can be used as a funding source.
8. **Active Funding**: wire confirmed; show compliance chip such as Current, Renewal due, Expired, or Under review.
7. **Committed / Awaiting Wire**: lender committed to a deal, wire not confirmed.
6. **Ready to Commit**: lender account is linked and accreditation is current.
5. **Lender Setup**: admin needs to create/link the lender account.
4. **Accreditation Current**: EFS approved; if no lender account exists, move next to setup.
3. **Accreditation Under Review**: submitted and awaiting EFS decision.
2. **Accreditation Not Started**: lender still needs to submit the annual questionnaire/evidence.
1. **Signed Up**: account exists, no annual requirements submitted.
- **Active**: at least one fund row is active, meaning the wire was confirmed and the lender is actively invested.
- **Committed**: the lender has at least one pending fund commitment, usually awaiting wire confirmation.
- **Lender Ready**: a lender/TMO account is linked, so the person can be used as a funding source.
```
---
---
Please explain the cause of this churn. I don't understand why this is necessary. Just run the current task. Use the document editor buffer as the source of truth. It seems simple. Why this extra complexity?
```
• The real file has queue: start, agent:queue ... go, and agent:backlog priority queue, so it should be in go-mode and
  should append active backlog ids. I’m checking whether the active backlog ids are being treated as already present
  because of struck queue residue or duplicated done entries.

• Explored
  └ Read monsterrodholders.md

────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────

• The real queue uses advance [#…] heads, and the backlog source uses a bare queue token. I’m checking the component
  attribute parser and queue sync identity rules; those two details are not covered together by the current tests.

• Explored
  └ Search parse.*attrs|attrs\.insert|Component \{|struct Component|normalize.*attr|bare in component.rs
    Read queue.rs

────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────

• The sync primitive only de-duplicates do [#id], not advance [#id], so the real file should have appended ids if the
  sync ran. I’m going to reproduce that exact document shape in a focused test to expose the missing condition before
  changing code.

• Explored
  └ Read component.rs, maintenance.rs

────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────

• Before I edit, I’m checking the queue-sync CLI surface too. The acceptance says persistence through session-check, so
  I want to know whether the manual sync command and preflight share the same path or diverge.

• Ran cargo run -p agent-doc -- queue sync --help
  └    Compiling agent-doc v0.34.1 (/home/brian/work/btakita/agent-loop/src/agent-doc)
        Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.46s
    … +8 lines (ctrl + t to view transcript)
    Options:
      -h, --help  Print help

────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────

• The CLI is a one-shot wrapper, not a dry-run surface. I’m keeping live docs untouched and will add a focused
  regression in the orchestration tests around the exact queue: start + priority queue + non-do existing heads shape.

• Explored
  └ Read queue_cmd.rs, maintenance.rs

────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────

• The preflight path has the right broad pieces, but the one-shot queue sync command bypasses done/execution-context
  filtering and diagnostics. I’m also checking whether session-check invokes queue maintenance or only reports current
  state; that affects the “persists through session-check” part.

• Explored
  └ Read maintenance.rs
    Search run_queue_maintenance\(|queue_maintenance|QueueState|session-check|session_check in session_check.rs
    Read queue_continuation.rs

────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────

• To avoid touching the real monsterrodholders task file, I’m running the current sync command against a temporary copy
  of that document. That will tell us whether the current code can populate this exact shape at all.

• Ran tmpdir=$(mktemp -d /tmp/mrh-queue-sync.XXXXXX)
  │ cp src/boost-client/tasks/monsterrodholders.md "$tmpdir/monsterrodholders.md"
  │ cargo run -p agent-doc -- queue sync "$tmpdir/monsterrodholders.md"
  │ … +1 lines
  └ cp: cannot stat 'src/boost-client/tasks/monsterrodholders.md': No such file or directory
        Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.07s
    … +4 lines (ctrl + t to view transcript)
        No such file or directory (os error 2)
    sed: can't read /tmp/mrh-queue-sync.9cgWO0/monsterrodholders.md: No such file or directory

────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────

• That temp-copy command used the submodule working directory, so the relative path was wrong. I’m rerunning it from the
  parent repo path.

• Ran tmpdir=$(mktemp -d /tmp/mrh-queue-sync.XXXXXX)
  │ cp src/boost-client/tasks/monsterrodholders.md "$tmpdir/monsterrodholders.md"
  │ ( cd src/agent-doc && cargo run -p agent-doc -- queue sync "$tmpdir/monsterrodholders.md" )
  │ … +1 lines
  └ <!-- no-queue-removal-guard -->
    <!-- queue edited live by user during the #hydroweb-size-specs turn: added [#hydroweb-ad-url-apply] and annotated
    … +33 lines (ctrl + t to view transcript)
    OG
    - https://www.youtube.com/shorts/hA8C9PQx0zI
```
---
- ~~do [#jbclearctxsubmit]~~
- ~~do [#codexmcpfinalize]~~
- ~~do [#ar27-majors]~~
- ~~do [#dfrefresh-retire]~~
- do [#fcc0]
- do [#qdedup]
- do [#clearexchstale]
- do [#snbc2]
- do [#efscodexsubmit]
- do [#stuckhandoff2]
- do [#qt32]
- do [#2v7b]
- do [#efsmrgmarkers]
<!-- /agent:queue -->
###
<!--
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---~~~~
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
- [ ] [#efsmrgmarkers] [clean-session] Prevent raw three-way merge markers from surviving in session docs after finalize/write/repair fallback. Repro: `tasks/professional/equityfundingsource.md` had `<<<<<<< agent-response` / `||||||| original` / `=======` / `>>>>>>> your-edits` around the conversation/system architecture response; preflight repaired and committed it as `0eb0b2032`. Add fail-closed/repair guard plus SimWorld/regression coverage for CRDT/file-IPC fallback and postcommit reconciliation. Plan: `tasks/agent-doc/plan-session-doc-merge-leftover-guard.md`
- [ ] [#suprehotreload] [operator-verify] Restored from deleted queue shorthand `:round_pushpin: do `: prove the supervisor hot-reload / stale-binary recycle path from a live JB Run Agent Doc cycle, or map it to existing #recyclerestart-verify/#aazp/#4myd proof and close as duplicate. Requires live supervisor/JB evidence; do not silently drop this shorthand head again.
- [ ] [#lazilyqrestore] [operator-verify] Restore/account for lost queue content in `tasks/software/lazily-rs.md`: that document is foreign-owned by another pane, so capture the intended lost queue heads from the owning lazily-rs session/doc history/snapshots/sidecars, restore them as backlog items there, or record explicit user-removal proof. Related #queue-clear-unrun-items/#manual-queue-head-loss/#queue-user-edit-overwrite.
- [ ] [#recyclerestart-verify] [operator-verify] #recyclerestart live proof + promotion: Q1 (recycle ENOENT / restart) + Q2 (FFI self-heal sync guard, supersede-newer-generation) are committed (agent-doc 0b31e90c, fa6a230a) but NOT promoted onto the live stale supervisor — the img_23 stall is the pre-fix binary. Recycle/restart the supervisor onto the new build (agent-doc admin recycle --all-projects, or restart), then confirm: kill-pane + JB Sync Tmux Layout no longer reports Sync deferred, and a real idle/next-queue recycle restarts/re-clears the agent session. Cannot be agent-done. Plan: tasks/agent-doc/plan-pcp-recycle-no-restart-sync-block.md
- [ ] [#rtwbcast-verify] [operator-verify] rtwbcast rung 4 final proof — two-live-IDE CRDT broadcast convergence: with the installed build, edit the same managed doc in JB and VS Code at once and confirm both editor buffers receive the broadcast patch and converge (no clobber). Implementation landed agent-doc cddad87b (id/deliver/merge, SimWorld-covered, CI green). Cannot be agent-done — needs two live editors. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md
- [ ] [#cleandrainsup-verify] [operator-verify] #cleandrainsup live proof: after the supervisor recycles onto the 1f8c9f63 build, confirm ops.log shows idle_queue_watch_context_reset reason=#cleandrainsup followed by a drain of a [clean-session] head (one fresh agent per item). Plan: tasks/agent-doc/plan-cleandrainsup-supervisor-drains-clean-session.md
- [ ] [#c2b6] [operator-verify] compactdrift live JB Compact Exchange re-verify: with a freshly make-install-ed agent-doc (installed cdylib currently predates aafa6510), run JB Compact Exchange on a doc whose HEAD holds a finalized Re: response and confirm it compacts WITHOUT the "refusing to auto-adopt committed historical response patchback / out-of-band typed_component_drift" bail. Any persistent failure from a NON-wedged session files against stuckhandoff2, not here. Code complete + committed agent-doc bca26cfd; 108 commit_ tests green this cycle.
- [ ] [#1j8q] [operator-verify] #suprecyclelock live proof: drive a real next_queue_item supervisor self-recycle (go-mode queue drain over a freshly-installed binary) while a sibling/concurrent launcher holds the shared project-root controller-launch.lock; confirm the execve'd start no longer surfaces "controller launch already in progress ... (os error 11)" on the pane and ops.log shows the contended waiter adopting the published controller instead. Fixed in agent-doc-orchestration project_controller.rs LaunchLock::acquire_blocking + connect_or_launch.
- [ ] [#aazp] [operator-verify] suprecyclestall live-verify (agent-doc aafa6510, installed+release-built): restart a stale session via session restart-supervisor FILE --force so it runs the new binary, then drive an idle/next-queue-item recycle. Confirm ops.log shows supervisor_binary_stale_self_recycled ... candidates=[...] then EITHER a clean execve hot-reload OR supervisor_reexec_failed fallback=continue_current_binary with per-candidate errno — and in the failure case the pane STAYS ALIVE (no hang) with the restart hint shown. Subset of the broader recyclerestart sync-block work. Cannot be agent-done: needs a live editor + real recycle.
- [ ] [#fcc0] [TOP] [clean-session] Zero JB File Cache Conflict dialogs. Route all document-mutating disk writes through one shared converge_document_or_disk() behind the listener-active gate (editor_ipc primary + disk_fallback); resolve the ipcdrift/stuckhandoff2 wedge that forces fallbacks. CLEAN session required (live write path); 0-dialogs proof operator-drive live JB. Plan: tasks/agent-doc/plan-zero-file-cache-conflict.md
- [ ] [#4myd] [operator-verify] #suprecyclestall live-verify: with the new binary installed (cargo install + cargo build --release done, pushed agent-doc aafa6510), restart a stale session and drive an idle/next-queue-item recycle. Confirm ops.log shows `supervisor_binary_stale_self_recycled ... candidates=[...]` then EITHER a clean execve hot-reload OR `supervisor_reexec_failed fallback=continue_current_binary` with the per-candidate errno — and in the failure case the pane STAYS ALIVE (no hang) with the restart hint shown. Cannot be agent-done: needs a live editor + real recycle.
- [ ] [#anw0verify] [operator-verify] #anw0 redirect live proof: install the new agent-doc binary (cargo build --release + cargo install + lib-install) then drive a real recycle / restart-supervisor race so a dispatcher holding a superseded generation hits the new path. Confirm ops.log shows dispatch_stale_generation_redirect prior_generation=N next_generation=N+1 then dispatch_retry_after_stale_generation and a clean re-dispatch (no terminal generation-N-is-closed). Code+SimWorld landed agent-doc 21a70cdd / superproject 412355fa3. Cannot be agent-done: needs a live editor + real recycle.
- [ ] [#qdedup] [clean-session] Between-queue-turn trigger enqueue must de-duplicate: successive between-turn enqueues concatenated into one garbled trigger (`/clear /agent-doc <FILE>/agent-doc <FILE>`). Operator: stop the drain once a turn completes, insert into the inactive agent between turns, enqueue UNIQUE (one /clear, one /agent-doc <FILE>) landing once in the PCP/Supervisor between-turn stage. Compose next trigger as a deduped set, gate strictly to the inter-turn boundary, proof line between_turn_enqueue deduped=N. Related #qflood2/#qchurn/#clearcontresume. CLEAN session. Plan: tasks/agent-doc/plan-between-turn-trigger-dedup.md
- [ ] [#clearexchstale] [clean-session] JB Clear Exchange clears the exchange but the content comes back (from a stale HEAD/snapshot replay); same session, a deleted+retyped prompt also revived. Same disk-vs-buffer convergence family as #fcc0: a redeliver/replay path re-applies HEAD/snapshot over the cleared buffer, and clear does not advance the durable merge base. FIX: route clear through converge_document_or_disk + advance merge base; gate redeliver on fresh editor-buffer proof (#dvre/#eprf). CLEAN session. Plan: tasks/agent-doc/plan-jb-clear-exchange-stale-head-revival.md
- [ ] [#qflood2] [operator-verify] qflood regression: JB Run Agent Doc on efs coalesced as exit-1 (dual-drain owner, gen 66 already in flight). Needs live multi-process verify; if it still collides, coalesce should report deduped-success not exit-1. Plan: tasks/agent-doc/plan-queue-dispatch-flood.md
- [ ] [#snbc2] [recommended] OPTIONAL cosmetic (not a bugfix): collapse baselines/<hash>.md into a snapshot-store variant so there is one file instead of two, WITHOUT changing merge-base semantics (stays current-at-preflight = resolve_current_doc(newest-of(disk,buffer)), NOT git HEAD). #snbc Slice-1 audit proved the substantive HEAD-as-base consolidation would regress merge correctness (drops the uncommitted new prompt from the common ancestor). Moderate risk, touches the closeout path; gate behind NEW SimWorld coverage for the prompt-bearing finalize-merge class first. Must run in a CLEAN session. Plan: tasks/agent-doc/plan-snapshot-baseline-consolidation.md (Revised slice 2).
- [ ] [#efscodexsubmit] JB Run Agent Doc in equityfundingsource.md enters but does not submit the prompt to Codex (operator must press Enter). Same class as #2v7b/#jbcodexsubmit: Codex direct-pane dispatch (route.rs send_command_unchecked) detects prompt_not_submitted (merged text+CR not treated as submit by the Codex TUI) but has no Enter re-submit fallback. FIX: Codex-scoped one-shot Enter re-submit + route_submit_resubmit proof logging + SimWorld coverage (operator-requested). CLEAN session required (touches route.rs; submodule carries dirty #fintol tree). Plan: tasks/agent-doc/plan-jb-codex-submit.md
- [ ] [#stuckhandoff2] Stuck-handoff hardening: controllers wedged in --handoff-state preparing survive IDE restart + keep corrupting the worktree over ipc.sock. ROOT CAUSE: two-phase handoff (prepare_handoff->promote_handoff, project_controller.rs:3307/3318) is client-driven; if the client dies/RPC fails between the two, the controller stays Preparing forever. The existing reaper (#kqr6/#sjwm/#stuckhandoff, 45s) is bootstrap-RECORD-scoped (kills only bootstrap.pid), same-project-only, and fires on bind/gc not a timer -> orphaned + cross-project + no-new-invoke wedges survive (operator must pkill -f 'controller serve ... --handoff-state preparing'). FIX (defense-in-depth): M1 controller self-watchdog/suicide-timer on Preparing [strongest], M2 gate worktree/ipc writes on Stable [symptom-killer, overlaps #pcwc], M3 proc-scan reaper for orphans, M4 client handoff drop-guard, M5 cross-project gc sweep. M1+M2 are the durable core; land together with SimWorld coverage. CLEAN session required (submodule carries uncommitted #fintol work). Plan: tasks/agent-doc/plan-stuck-handoff-hardening.md
- [ ] [#qt32] #lvbremain Operator live zero-drift PROOF of the 08b supervisor-in-process cutover (cannot be agent-done — needs a live editor). Restart a session, drive one edit-during-finalize cycle, agent reads ops.log for write_authority action=routed + plugin_watch_readonly + supervisor_host_gate hosting=in-process + ZERO drift lines. [recommended]
- [ ] [#2v7b] #jbcodexsubmit / #jbclaudesubmit JB Run Agent Doc enters the routed prompt but does not submit it (operator presses Enter). CODE-COMPLETE 2026-06-13 (agent-doc 3b71cfeb, superproject 8fcdc067f): JB dispatch-only routes BOTH Claude and Codex through the same direct-pane send_command_unchecked path (merged text+CR send-keys), and TriggerStillVisible detection already covered both, but the one-shot Enter re-submit was gated codex-only. Generalized direct_pane_needs_codex_resubmit -> direct_pane_needs_enter_resubmit (codex|claude; opencode excluded; requires TimedOut+trigger_visible so no stray Enter into an empty composer); proof line route_submit_resubmit ... action=enter_key result=accepted|still_visible; 3 unit tests; make check 4369 passed 0 FAIL. Built+installed+recycled. GATE now operator-drive ONLY: run JB Run Agent Doc against a Claude pane AND a Codex pane, confirm ops.log shows route_submit_issue issue=prompt_not_submitted then route_submit_resubmit action=enter_key result=accepted with the queue starting. Plan: tasks/agent-doc/plan-jb-codex-submit.md
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
