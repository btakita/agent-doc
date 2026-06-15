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
agent_doc_pipeline:
  run_id: cycle-1781537509161
  step: committed
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

Verification: `cargo test -p agent-doc-orchestration broadcast -- --nocapture`, `cargo test broadcast -- --nocapture`, `make check`, `make install`.
<!-- agent:boundary:7d3d50d1 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" go -->
- ~~:pushpin: JB `Run Agent Doc` sent `agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md` into the Codex input but did not submit. #simworld~~
/goal Do the actual work in the queue. Stop punting on the queue work. Fix the root cause(s) of the punting. The work in the queue should be done.
- ~~do [#compactdrift-agent]~~
- ~~do [#cleandrainsup-agent]~~
- ~~do [#rtwbcast-simproof]~~
- do [#recyclerestart-agent]
- do [#lazilyqrestore-agent]
- do [#suprehotreload-agent]
- do [#qmisstrike-regression]
- do [#staleheaddupcontent]
- do [#mrhbacklogqueuesync]
- do [#mrhqueuepreserve]
- do [#jbclearctxsubmit]
- do [#codexmcpfinalize]
- do [#ar27-majors]
- do [#dfrefresh-retire]
- do [#fcc0]
- do [#qdedup]
- do [#clearexchstale]
- do [#snbc2]
- do [#efscodexsubmit]
- do [#stuckhandoff2]
- do [#qt32]
- do [#2v7b]
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
- [ ] [#suprehotreload] [operator-verify] Restored from deleted queue shorthand `:round_pushpin: do `: prove the supervisor hot-reload / stale-binary recycle path from a live JB Run Agent Doc cycle, or map it to existing #recyclerestart-verify/#aazp/#4myd proof and close as duplicate. Requires live supervisor/JB evidence; do not silently drop this shorthand head again.
- [ ] [#lazilyqrestore] [operator-verify] Restore/account for lost queue content in `tasks/software/lazily-rs.md`: that document is foreign-owned by another pane, so capture the intended lost queue heads from the owning lazily-rs session/doc history/snapshots/sidecars, restore them as backlog items there, or record explicit user-removal proof. Related #queue-clear-unrun-items/#manual-queue-head-loss/#queue-user-edit-overwrite.
- [ ] [#recyclerestart-verify] [operator-verify] #recyclerestart live proof + promotion: Q1 (recycle ENOENT / restart) + Q2 (FFI self-heal sync guard, supersede-newer-generation) are committed (agent-doc 0b31e90c, fa6a230a) but NOT promoted onto the live stale supervisor — the img_23 stall is the pre-fix binary. Recycle/restart the supervisor onto the new build (agent-doc admin recycle --all-projects, or restart), then confirm: kill-pane + JB Sync Tmux Layout no longer reports Sync deferred, and a real idle/next-queue recycle restarts/re-clears the agent session. Cannot be agent-done. Plan: tasks/agent-doc/plan-pcp-recycle-no-restart-sync-block.md
- [ ] [#rtwbcast-verify] [operator-verify] rtwbcast rung 4 final proof — two-live-IDE CRDT broadcast convergence: with the installed build, edit the same managed doc in JB and VS Code at once and confirm both editor buffers receive the broadcast patch and converge (no clobber). Implementation landed agent-doc cddad87b (id/deliver/merge, SimWorld-covered, CI green). Cannot be agent-done — needs two live editors. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md
- [ ] [#cleandrainsup-verify] [operator-verify] #cleandrainsup live proof: after the supervisor recycles onto the 1f8c9f63 build, confirm ops.log shows idle_queue_watch_context_reset reason=#cleandrainsup followed by a drain of a [clean-session] head (one fresh agent per item). Plan: tasks/agent-doc/plan-cleandrainsup-supervisor-drains-clean-session.md
- [ ] [#c2b6] [operator-verify] compactdrift live JB Compact Exchange re-verify: with a freshly make-install-ed agent-doc (installed cdylib currently predates aafa6510), run JB Compact Exchange on a doc whose HEAD holds a finalized Re: response and confirm it compacts WITHOUT the "refusing to auto-adopt committed historical response patchback / out-of-band typed_component_drift" bail. Any persistent failure from a NON-wedged session files against stuckhandoff2, not here. Code complete + committed agent-doc bca26cfd; 108 commit_ tests green this cycle.
- [ ] [#recyclerestart-agent] [clean-session] Make #recyclerestart-verify agent-verifiable: add automated recycle/restart evidence collection for kill-pane + JB Sync Tmux Layout, including ops.log assertions for no sync-deferred wedge and restart/re-clear proof. Plan: tasks/agent-doc/plan-pcp-recycle-no-restart-sync-block.md
- [ ] [#lazilyqrestore-agent] [clean-session] Make #lazilyqrestore agent-verifiable: add cross-doc lost-queue recovery tooling for foreign-owned tasks/software/lazily-rs.md that reads snapshots/sidecars/doc history, reconstructs deleted queue heads, and emits explicit user-removal proof when no restore is needed.
- [ ] [#suprehotreload-agent] [clean-session] Make #suprehotreload agent-verifiable: add supervisor hot-reload/stale-binary recycle proof logging and a SimWorld/local harness test that proves a JB Run Agent Doc cycle observes the new binary or maps the proof to #recyclerestart-verify/#aazp/#4myd without live operator inspection.
- [ ] [#qmisstrike-regression] [clean-session] Reopen #qmisstrike: moving/reordering do [#staleheaddupcontent] caused agent-doc to strike the id-backed head even though no work was done. Fix finalize/queue consume so id-backed heads can only be struck by explicit --done/--pending-gate/valid id consume proof, never as a side effect of stale free-text/head reconciliation. Plan: tasks/agent-doc/plan-finalize-misstrike-next-head.md
- [ ] [#staleheaddupcontent] [clean-session] Fix stale queue head duplicate-content corruption: a stale agent:queue head can replay old exchange/queue content and corrupt the document. Add stale-head mismatch coverage, refuse stale queue snapshot replay without live-head match or consume/strike proof, and log live/snapshot/stale head identities. Plan: tasks/agent-doc/plan-stale-queue-head-duplicate-content.md
- [ ] [#mrhbacklogqueuesync] [clean-session] Fix tasks/monsterrodholders.md backlog priority queue sync: document has agent:backlog priority queue but eligible backlog items are not materializing in agent:queue. Add diagnostics for skipped backlog ids, regression coverage for backlog-to-queue convergence, and persist queue sync through session-check. Plan: tasks/agent-doc/plan-monsterrodholders-backlog-priority-queue-sync.md
- [ ] [#mrhqueuepreserve] [clean-session] Prevent active-turn queue convergence from deleting user-added queue items in tasks/monsterrodholders.md. Add regression coverage for live queue edits during a running turn, require explicit consume/strike/remove proof before queue deletion, and log the proof source for removed queue nodes. Plan: tasks/agent-doc/plan-monsterrodholders-queue-preservation.md
- [ ] [#jbclearctxsubmit] [clean-session] Fix JB Clear Session Context submit on Codex: calling Clear Session Context for tasks/monsterrodholders.md inserted /clear into the Codex composer but did not submit it. Implement submit-proof + one-shot Enter retry for the clear-session route, with stale-scrollback guards and SimWorld/unit coverage. Plan: tasks/agent-doc/plan-jb-clear-session-context-submit.md
- [ ] [#codexmcpfinalize] [clean-session] Fix Codex MCP finalize transport-close after response capture: agent_doc_finalize can leave session-check interrupted when the MCP transport closes after capture but before write/commit. Add regression coverage that forces the close-after-capture path and proves the next CLI/preflight/write path commits exactly once, clears cycle state, and does not require manual reset. Related observed cycle: 2026-06-15 tasks/agent-doc/agent-doc-bugs2.md.
- [ ] [#ar27-majors] [clean-session] Major-version dependency bumps held back by Cargo.toml constraints in src/agent-doc (each needs a focused API-migration review, NOT a blanket bump): instruction-files 0.2->0.3, notify 7->8, portable-pty 0.8->0.9, pulldown-cmark 0.12->0.13, rusqlite 0.34->0.40, sha2 0.10->0.11, signal-hook 0.3->0.4, similar 2->3, toml 0.8->1.1, ureq 2->3, yrs 0.21->0.27 (CRDT core, highest risk), zip 2->8, generic-array 0.14.7->0.14.9. Do per-crate with its own make check; yrs/rusqlite/toml/zip are the breaking ones. Follow-up from #ar27 (compatible cargo update landed d0f5475d). CLEAN session (rebuild/reinstall re-stales the live supervisor).
- [ ] [#dfrefresh-retire] [clean-session] Retire the dogfood-supervisor-refresh runbook (runbooks/dogfood-supervisor-refresh.md + BUNDLED_RUNBOOKS entry + SKILL.md catalog line + 4 harness mirrors) once #jbrestale (JB Run Agent Doc auto-restarts a stale route-owned supervisor / red-green switchover) AND #recyclerestart (PCP recycle restarts the harness session) both land and prove the binary self-heals a stale supervisor without the manual make+install+restart bootstrap. The runbook is an explicit stopgap for the bootstrap gap (running supervisor predates its own self-recycle fix + is the live agent parent). Acceptance: with both landed, a stale-binary session auto-promotes onto the new build at the next idle/dispatch boundary; delete the runbook + dependent references in one cycle and re-run audit-docs. CLEAN session (skill/runbook surface).
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
