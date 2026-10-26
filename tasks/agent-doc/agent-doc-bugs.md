---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: ef7e9a9a-cca8-4166-a012-fd43b3cb483b
  codex: 01a17616-9ddc-7ee0-86df-a42c74605356
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
presets:
  '#agent-doc-causes': 'Fix agent-doc causes'
  '#audit': 'Audit agent-doc runtime for dogfooding. Add issues to the backlog + queue.'
  '#fix-dataflow': 'Fix any issue you see at multiple levels of the dataflow.'
  '#repo': 'https://github.com/btakita/agent-doc'
  '#self': 'agent-doc-bugs.md'
dogfood:
  path: tasks/agent-doc/agent-doc-bugs.md
  auto: true
queue: go
---

## Status

<!-- agent:status patch=replace sync="##sync-release" -->

📤 Queue handoff: 30 reported items awaiting coordinator integration, verification, and closeout.
Main is at `b6b9af6e15` and agent-doc 0.35.532 is installed locally. Release v0.35.532 was published from `71f9801b49`. Post-release main now includes the inline-code-safe, occurrence-ordered queue preset authorization fix for GitHub issue #331 and the positive-evidence-only Remote Dev pane-order fix for GitHub issue #332; both issues are closed, the integrated full suite passed, and the current mainline was installed locally.

<!-- status:sync -->
Released and published agent-doc v0.35.553 from release commit `a0a525fb65`; tag `v0.35.553` is the stable latest release, PyPI serves 0.35.553, all six native archives promoted to their stable target channels, and Homebrew is bumped. Post-release `origin/main` is `5fe17385b5` (unreleased, ships in the next release): the `#claimrowmigrate` claim-row fix, the Marketplace retained-version fix, GH #375 (JetBrains 0.2.828/0.2.829/0.2.830), GH #373 (zero-column route), and GH #374 (heading-anchored `agent:run` respond + merge). #373 and #375 are closed and installed locally; #374 is installing.
<!-- /status:sync -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261024-044925.md`*

Compacted content:
- Archived 7 response topic(s): JetBrains Marketplace registration; JetBrains CLI installer and IDEA Beta update; IdeaVim-style Agent Doc update channel; 4 more
- Prior summary/context: prior compacted content: Archived 15 response topic(s): Release v0.35.547, queue repair, and project agent configurat...

### Re: Subagent completed: `subagent:grok_queue_ui` returned work for queue item “In sdk-demo.md using Grok, the queue item was finished but the UI did not update. `- 📥 When I load Boss Room, claim an item, stop Boss Room, start Boss Room, the item is not claimed.`” — agent-doc
<!-- agent-doc-subagent-response sha256=99beb6c083827b6386631afd847d2f7ebc1169ffc9e861504f3cf20e5f5bc5e2 -->
> **Subagent response:**
>
> Completed and pushed.
>
> Branch: `fix/grok-queue-ui-restart-20261024`
> Commit: `fb648e23d987d7f6ce9be9c4df96fb67fbb79947` (`fix(queue): resume committed deferred heads`)
> Remote tracking branch is up to date; worktree clean.
>
> Cause/state-machine audit: the historical sdk-demo row was correctly left 📥 because Grok emitted an exact scoped `**Deferred:**`; durable claim re-projection already survives a cold controller restart. The actual liveness bug was head-only supervisor dedup: after the deferred turn committed, the same head remained and `SkipAlreadyDispatched` suppressed continuation. Yes, the state machine could have caught it: the missing forbidden state was `committed Deferred + pending head + no active owner + no scheduled continuation`. Existing SimWorld covered focused-cycle deferral but not generic scoped deferral.
>
> Fix: a newly committed deferred cycle now rearms the same pending head exactly once, cycle-fenced against idle-tick loops. Normal auto-clear causes Grok/Codex-like harnesses to fresh-restart before redispatch; without automation, the diagnostic names the blocker and the executable `agent-doc <FILE>` action. The Deferred safety contract is preserved: the row remains pending. Added cold restart test proving state.db reprojects the 🟡 claim over stale disk, and SimWorld liveness coverage proving one redispatch and no tick hot-loop.
>
> Files changed (10):
> - `agent-doc-queue/src/queue.rs`
> - `agent-doc-queue/src/queue_auto_clear.rs`
> - `agent-doc-queue/src/queue_heads.rs`
> - `agent-doc-start-runtime-io/src/idle_watch.rs`
> - `agent-doc-crdt-relay-io/src/lib.rs`
> - `src/sim_world.rs`
> - `src/sim_world/engine.rs`
> - `specs/07-closeout-commands.md`
> - `specs/07-orchestration-commands.md`
> - `specs/supervisor.md`
>
> Exact verification:
> - 4 focused queue tests: 4 passed, 0 failed.
> - focused runtime captured-response test: 1 passed, 0 failed.
> - focused SimWorld liveness test: 1 passed, 0 failed.
> - focused CRDT cold-restart test: 1 passed, 0 failed.
> - `cargo test -p agent-doc-queue`: 912 passed, 0 failed.
> - `cargo test -p agent-doc-start-runtime-io`: 177 unit passed, 0 failed, 15 ignored; 3 integration passed, 0 failed.
> - `cargo test -p agent-doc-crdt-relay-io`: 140 passed, 0 failed.
> - `cargo build --workspace --quiet`: passed.
> - `make check`: passed end-to-end; nextest 11,478 passed, 177 skipped; VS Code 204 passed; cross-editor 1 passed; classic and both modular JetBrains builds passed; Python bootstrap 8 passed; TLA runner self-test passed and model check reused byte-identical passing receipt.
> - `git diff --check`: passed.
> - Post-push `gh run list --limit 1` showed the latest existing main workflow successful (this branch push did not show a new workflow in that single check).
>
> No install run (per coordinator rule). No follow-up queue items identified.
<!-- agent-doc-subagent-completion key=fe23a591a359e8ee1220f39d0f59ce4bbb30a7bff56ef712f311e8b61b6a08bb -->

### Re: Subagent completed: `subagent:lazily_block_reason` returned work for queue item “On lazily.md, `- ⚠️ [#subagent:corecireachaudit2#2] do [#lzcorecireachaudit]` is blocked and I can't find a reason why.” — agent-doc
<!-- agent-doc-subagent-response sha256=6be456a6aa02c1860197e2729bf1c0b3b64eb69795a4e48d6369c3fc9ea54584 -->
> **Subagent response:**
>
> FINAL RESULT for `On lazily.md, ... #lzcorecireachaudit ... blocked`:
>
> Exact reason: durable generation 2 (`subagent:corecireachaudit2`, worker `/root/corecireachaudit2`) started 2026-10-22 23:36:13 EDT with the default 2h TTL and expired 2026-10-23 01:36:13 EDT. The worker kept running and completed at 2026-10-23 14:23:53 EDT (~14h47m total, ~12h48m after expiry), after the dispatching coordinator turn had ended, so no `queue report-complete`/document closeout occurred. Durable state correctly classified the claim `Lost`; the defect was diagnostic projection: `queue claims` called `active_claims_for_content` and omitted Lost attempts, while lineage only reported `attached/degraded` + `repo_unrecorded`. Thus the visible ⚠️ had no cause or next action.
>
> Recovered worker result (do NOT rerun audit): audit passed after fixing/pushing two drifts on branch `audit/core-ci-reach-20261022`: lazily-py `68b3c682a71d395c7ad28edf7679f4420522b10a` (`test: sync Core closure with recycled-id exclusions`), lazily-cpp `4441b87646347569d82cb2b286a61059ce4c2cbd` (`test: declare stale-handle Core flavor skips`). All nine reachability checks and lazily-spec aggregate profile/evidence/support/consumer/cohort gates passed; Python 3.12/3.13/3.14 CI was green; no follow-ups. Action: persist that recovered result in lazily.md and close the id-backed queue row with the normal `--done lzcorecireachaudit` closeout. Only reclaim if the recovered result is rejected/unusable.
>
> Yes, this was identifiable in the state machines: the claim state machine already had the `Running -> Lost` TTL transition. What was missing was the cross-layer invariant `visible ⚠️ + live row => diagnostic names reason and recovery`; the read model filtered the terminal state away. I added that invariant to spec and deterministic test.
>
> Fix branch: `fix/lazily-block-reason`
> Commit: `530185b05964e45b3ca6c5a32cc4c88b9d1b2a1f` (pushed)
> Files: `agent-doc-queue-io/src/queue_claim.rs`, `specs/07-orchestration-commands.md`, `src/main.rs`
> Behavior: `agent-doc queue claims` now includes retained Lost live-row attempts and emits `lost_recovery.reason=claim_ttl_expired`, bound-worker late-result guidance, and an exact reclaim command.
> Tests: focused `cargo test -p agent-doc-queue-io claim_reports_include_expired_reason_and_actionable_recovery -- --nocapture` PASS; `make check-fast` PASS; single authoritative `make check` PASS. Worktree clean. Latest main CI checked once: run 40063238982 was still in progress (unrelated pre-existing main run).
> Follow-up queue items: none.
<!-- agent-doc-subagent-completion key=e54ea77d7c81ce2f809840440651962caae8a0a199441f545007e039d0da7ef2 -->

### Re: Subagent completed: `subagent:tmux_auto_sync` returned work for queue item “tmux pane auto sync is not working. Sync Tmux Panes works.” — agent-doc
<!-- agent-doc-subagent-response sha256=89e236032cde2e76a338d7e4cc1becfbc7f3b116a33077e3cc785f05b4fd8bf1 -->
> **Subagent response:**
>
> Completed/pushed. Branch `fix/tmux-auto-sync-20261024`, commit `4c4966bfcac4cc4bdb22f0ab5baa9dd924908921`.
>
> Root cause: live IDE log proves the local modular frontend opened `client=Host`; ingress accepted snapshots, then the any-lease authority transition retired `EditorTabSyncListener`'s classic projection. The separate measured-layout forwarder correctly rejected Host as `reason=local_client forwarded=false`. Auto sync therefore entered a no-publisher state. Manual `Sync Tmux Panes` remained healthy because its command path directly published `pane_layout_desired` and awaited convergence.
>
> State-machine invariant/recovery: an authenticated snapshot lease is not automatically pane-layout authority. Only a lease whose authenticated client id resolves to `ClientType.isRemote` may set `layoutAuthorityActive`, retire/suppress the classic projection, or open/retire a `jetbrains-rd` endpoint. A local lease keeps the classic publisher live. This guard is now at authority acquisition (not merely later forward filtering), documented in SPEC/state-backbone/262 README, and pinned by deterministic tests.
>
> Files (9): `editors/jetbrains-262/README.md`; backend `AuthenticatedSurfaceIngressService.kt`, `BackendGuestSelectionFallback.kt`, `RemoteClientLayoutForwarder.kt`; tests `AuthenticatedSurfaceIngressStateTest.kt`, `BackendGuestSelectionFallbackResumeTest.kt`, `BackendRemoteSessionGateTest.kt`; `editors/jetbrains/SPEC.md`; `specs/13-state-backbone.md`.
>
> Tests/builds: targeted backend tests PASS (16 tests total: ingress 6, remote gate 7, fallback 3; 0 failures/errors/skips). `make jetbrains-262-check` PASS (`BUILD SUCCESSFUL`, 54 tasks). `make jetbrains-263-check` PASS (`BUILD SUCCESSFUL`, 54 tasks). `git diff --check` PASS. Latest main CI PASS (run 40063238982). Exactly one `make check` was attempted; it stopped at the expected editor package-generation fence because source digests changed. Per coordinator rule I did not bump versions/package generations. Worktree clean after commit.
>
> Ordered integration follow-ups (not new queue items): (1) fast-forward/cherry-pick `4c4966bfca`; (2) coordinator/release owner runs the single modular package-generation bump, then authoritative `make check`; (3) coordinator installs once and verifies a local tab/split change emits automatic `controller_editor_surface_observed` -> `pane_layout_desired_published` without invoking Sync Tmux Panes. No additional queue follow-up items.
<!-- agent-doc-subagent-completion key=64ecf6aa5d8cd4156e3bf8c3507a67a6c3da7d27835bf7d1e1a145fd1f8427c1 -->

### Re: Subagent completed: `subagent:grok_subagents_dispatch` returned work for queue item “The sdk-demo.md Grok turn did not use subagents despite `agent:queue` having `subagents`. do [#grokqueuesubagents]” — agent-doc
<!-- agent-doc-subagent-response sha256=3b05b0fb7ce12b904b7c0ba9943152938463195de016c23b16d9b2600f0b7b2d -->
> **Subagent response:**
>
> Complete handback for `#grokqueuesubagents` / generation 1:
>
> Branch: `fix/grok-subagents-dispatch-20261024`
> Commit: `5ef228bcb79ba99297e9b6aad823c23b8a636df6`
> Push: `origin/fix/grok-subagents-dispatch-20261024` succeeded.
> Worktree: clean.
>
> Root cause: Grok had native `spawn_subagent`/result tools, and queue claim state already modeled `QueueLifecycleInvariant::DispatchEvidenceMissing` plus exact generation-fenced start-or-release recovery. The defect was the connected runtime projection: lightweight `agent_doc_admit` followed by `agent_doc_plan` did not expose either pending queue-level subagent dispatches or claimed-but-unbound acknowledgement obligations. With no document diff, plan could therefore say `no_changes` or select a subagent head inline. This is a plan/MCP lifecycle consumer defect, not a Grok native capability defect and not a missing claim state-machine transition.
>
> Fix/invariant: connected plan now projects every unclaimed dispatchable subagent head in `queue_subagent_dispatch`, every claimed/unbound attempt in `queue_dispatch_acknowledgement_required` with exact `queue start` / `queue release --requeue` recovery, treats either as actionable (never no_changes), and excludes pending/held/claimed subagent heads from inline prompt selection through a queue-owned selector that preserves manual-queue fallback. Spec, Grok skill, MCP description, runbooks, generated mirrors, and deterministic plan/MCP/architecture tests were updated.
>
> Exact changed files (20):
> `.claude/skills/agent-doc/runbooks/harness-invocation.md`
> `.claude/skills/agent-doc/runbooks/planning-dispatch.md`
> `.codex/skills/agent-doc/runbooks/harness-invocation.md`
> `.codex/skills/agent-doc/runbooks/planning-dispatch.md`
> `.cursor/rules/runbooks/harness-invocation.md`
> `.cursor/rules/runbooks/planning-dispatch.md`
> `.grok/skills/agent-doc/SKILL.md`
> `.grok/skills/agent-doc/runbooks/harness-invocation.md`
> `.grok/skills/agent-doc/runbooks/planning-dispatch.md`
> `.opencode/skills/agent-doc/runbooks/harness-invocation.md`
> `.opencode/skills/agent-doc/runbooks/planning-dispatch.md`
> `SPEC.md`
> `agent-doc-queue-io/src/queue_claim.rs`
> `agent-doc-queue/src/queue_continuation.rs`
> `runbooks/harness-invocation.md`
> `runbooks/planning-dispatch.md`
> `src/mcp.rs`
> `src/plan.rs`
> `src/skill_grok.rs`
> `tests/test_cli.rs`
>
> Verification:
> - Authoritative (run exactly once): `make check` — PASS, exit 0, digest failures 0 (artifact `tart-28bf1d4fbe`, 701 lines).
> - Fast final: `make check-fast` — PASS, exit 0 (artifact `tart-5a1fd86e94`; tsift parser falsely classified six `editor_route_error ... ok` lines as failures, while command exit was 0 and raw log showed successful tests/audit).
> - Focused compatibility: `cargo test --bin agent-doc plan::tests::build_plan_ -- --nocapture` — PASS, 37 passed, 0 failed, 0 ignored, 1234 filtered out.
> - Focused connected plan: `cargo test --bin agent-doc connected_ -- --nocapture` — PASS, 5 passed.
> - Grok skill: `cargo test --bin agent-doc skill::grok::tests::installed_bundle_routes_subagent_queues_to_grok_native_workers` — PASS, 1 passed.
> - Architecture guard: `cargo test --test test_cli test_agent_doc_queue_owns_active_queue_head_projection_policy` — PASS, 1 passed.
> - `git diff --check` — PASS.
> - One-time CI lookup after push: no CI run was present for this branch (`gh run list ...` returned `[]`).
>
> Follow-up queue items: none.
<!-- agent-doc-subagent-completion key=da11cd303ea3e15c58c9731910f29d75129094b10efc8becd1737bc31d03ae70 -->

### Re: Subagent completed: `subagent:sdk_demo_live_recovery` returned work for queue item “Run Agent Doc on sdk-demo.md still wedged even after I corrected the issue on the ran agent-doc version: ``` ❯ What do I need to do to repair the document? 3:29 AM ◆ Thought for 9.0s The hold is a document-structure refusal, not a missing code change. I am checking the exact marker 3:29 AM the controller rejects so the repair steps match that line. ◈ Explored 4 files, 8 searches, ran 5 commands The disk copy of sdk-demo.md is already repaired. The controller is still holding the old one-line 3:31 AM queue marker, and the JetBrains plugin is not connected, so that repair never becomes the document it will project. In /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md on disk, the Run section is: ## Run <!-- agent:run priority subagents --> ◇ markdown [Copy] The marker is alone on its line, and <!-- /agent:run --> is alone on its line. The controller's copy still has this instead: ## Run <!-- agent:run priority subagents --> ◇ markdown [Copy] Session-check refuses the merged projection because that open marker is not alone on its line (non_standalone_component_marker:queue:open:line229). The tab is open, and the controller reports live_editors=0, so it keeps that old copy and will not adopt the file on disk. In the open sdk-demo.md tab: 1. If the Run heading and <!-- agent:run priority subagents --> are still on one line, put the comment on its own line, with nothing before or after it. Leave the queue items as they are. 2. If the tab already shows the two-line form, reload the file from disk and choose Load FS changes. Do not choose Keep memory changes while the memory copy still has the one-line marker. 3. Leave the tab open long enough for the Agent Doc plugin to reconnect. A connected tab is what █ lets the retained commit-push response project. █ █ After the tab shows the marker on its own line, ask again and I will run session-check. Do not █ force-write the file, and do not delete the queue items. █ ```” — agent-doc
<!-- agent-doc-subagent-response sha256=2b971038dfe9657ad5ceacb1f9ea1edac0e92cba21b3d0a86334ee437410c2d6 -->
> **Subagent response:**
>
> Live diagnosis confirms the old marker refusal is cleared. Grok pane %58 owns a healthy generation-13 controller with a fresh active-turn lease; session-check reached new cycle cycle-1792829070027 at preflight_started. Let pane %58 finish, then run exactly one: agent-doc session-check /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md. Do not restart, clear, reload, reset, finalize, write, or launch another session while it is active.
<!-- agent-doc-subagent-completion key=db6d357e1fb4c432525d3235eae3afa52101d489425f9e966ec9ca99e447cf81 -->

### Re: Subagent completed: `subagent:sdk_demo_route_recovery` returned work for queue item “Run Agent Doc on sdk-demo.md: [route] explicit send for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md is blocked: 1 pending steering item(s) remain durable, but no live owning turn was proven and no second trigger was injected. The idle supervisor re-evaluates stale-empty-preflight recovery on a bounded cadence; no second send is required. Otherwise follow the named recovery (blocker: closeout recovery blocked [open_cycle]: missing proof: open cycle must finish, be replayed, or be explicitly queued behind; recommended: captured response baseline no longer matches current document for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md. If an editor has this document open, reattach it so the authority re-bootstraps from the editor's text: `agent-doc admin reload-lib` (or reopen the document tab); do not run `reset --from-current` while the editor shows text the authority lacks, because it saves the authority over the editor. With no editor attached, rebuild the cold recovery projection without clearing session state: `agent-doc reset --from-current --preserve-session /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md`; resume durable checkpoint cycle=cycle-1792824204227 phase=write_applied capture_id=cycle-1792824204227; the canonical write is already applied; retained commit recovery owns the terminal boundary; do not resubmit finalize or write for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md). steering_delivered=0 sent=explicit ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=run_recovery_command recovery_command=agent-doc admin reload-lib” — agent-doc
<!-- agent-doc-subagent-response sha256=5f3e8b526281c43426648be14200b0bc92ab0a302d491b864d62fece1d28e942 -->
> **Subagent response:**
>
> The old cycle cycle-1792824204227 is no longer active. A healthy Grok generation-13 actor in pane %58 is running new cycle cycle-1792829070027 at preflight_started. No code defect remains proven. Let it reach terminal boundary, then run one session-check; do not reload/reset/restart/finalize/write now.
<!-- agent-doc-subagent-completion key=b2ae516851c6a5e531c847e27395eb069e04857c07156f37fe26a84bf2883b0b -->

### Re: Subagent completed: `subagent:run_sdk_demo_grok` returned work for queue item “Run Agent Doc on sdk-demo.md (Grok): We should be able to support `## Run <!-- agent:run priority subagents -->`. Also the error message should mention the tag `agent:run`, not Queue, to more easily identify where and what the issue is in the document. Do this for all error messages mentioning tags. Also, line 229 is not the correct line. The agent:run was on line 185...it is now on 186 due to me placing `## Run` on a separate line in attempt to make the turn run. ``` ❯ agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md 3:17 AM ◆ Thought for 0.8s The session is live again. I will read the agent-doc skill and admit before I touch the document. 3:18 AM ◈ Explored 11 files, 16 searches, called 3 tools, ran 11 commands This turn did not start. Admission was refused, and the queue was left untouched. 3:21 AM agent_doc_admit for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md failed because the retained delivery 1792824957388507072-1- 43b8cfaaaa6ff55a766f0e2e7fbb94a491fa16a0c5f80c22d00b24200277e659 still owns the commit-push subagent response. Two session-checks returned the same interruption: editor reconnect refuses the merged document because the queue open marker at line 229 is not alone on its line (non_standalone_component_marker:queue:open:line229). The current editor text is unchanged. No second closeout was sent, and the file was not force-written. The JetBrains tab is open, and the controller currently has no live replica for it (live_editors=0). A lint of the current document is clean. The bad marker is in that merged reconnect target. The same capture is what has to land before a new cycle can admit. The binary allows only another agent-doc session-check on that file. ```” — agent-doc
<!-- agent-doc-subagent-response sha256=d4440a04d5c12ceefd776a5c0e5f107beea0fd46df85939ba688a27134772b78 -->
> **Subagent response:**
>
> Implemented and pushed commit 373f0b6d15. The shared structural gate now accepts same-line `## Run <!-- agent:run priority subagents -->`; malformed targets use exact `agent:run` spelling and the marker source line rather than internal `queue` or unrelated line 229. Integrated full check passed and the fixed build is installed. Live validation now reaches a new healthy Grok cycle instead of the old marker refusal.
<!-- agent-doc-subagent-completion key=382b23beb5b224e3aa5d1b96f61e1c4a747f40f75cc74f4da705ca9c2b164ed5 -->

### Re: Subagent completed: `subagent:release_publish_patch` returned work for queue item “release + publish” — agent-doc
<!-- agent-doc-subagent-response sha256=d912a5814724d3f7c929f8f2d01a60661b118f39e79ddcc7fc9bf0390e3eb1d9 -->
> **Subagent response:**
>
> Released and published agent-doc v0.35.551 from commit 03433d0494. GitHub Actions run 40078734115 completed successfully; the public GitHub release contains 24 assets, the PyPI wheel is public, and the locally installed CLI reports agent-doc 0.35.551. Local editor packages were installed as VS Code 0.2.83 and JetBrains classic/262/263 0.2.822/0.2.823/0.2.824; running modular JetBrains IDEs must be restarted to load the new plugin bytes.
<!-- agent-doc-subagent-completion key=340307c5605e3f613ff24c04f798ee2c67df4551a40e926dae1e41857d098a84 -->

### Re: Subagent completed: `subagent:jb263_actionable_todo_verify` returned work for queue item “`- ⚠️ [#subagent:jb263_feed#1] Should we have the `-263.xml` in `jetbrains-263.xml`? I saw the selection in the list. I selected it. I still see Agent Doc in the Cannot Upgrade Plugins. ![img_7.png](img_7.png)` should have an actionable todo item. With the todo item id appended to the queue item for followup. [#jb263-feed-verify]” — agent-doc
<!-- agent-doc-subagent-response sha256=818411bf75c8b53aa2a793023debd60f7d503433daaca94a4739cbdeee408b09 -->
> **Subagent response:**
>
> Read-only verification passed. The Todo `[#jb263-feed-verify]` exists exactly once, the originating Main queue row ends with that ID exactly once, and its steps match v0.35.551: JetBrains 263 package 0.2.824, the sole `agent-doc-jetbrains-263.xml` feed, refresh/update/restart/verify, plus exact failure evidence to capture. No follow-up queue item is needed.
<!-- agent-doc-subagent-completion key=8621c8454012f66b839792045f199bc7fea2d24757515f4bf9a47a4ef467b586 -->

### Re: Subagent completed: `subagent:release_batch_verify` returned work for queue item “release + publish: include issue 372 + sdk-demo.md issue + lazily.md issue” — agent-doc
<!-- agent-doc-subagent-response sha256=fb6bcf7881ba0af43aa975bdf733416db7937d431ae369199ab39ae927b50d36 -->
> **Subagent response:**
>
> Verified complete. Public v0.35.551 (03433d0494) contains all three requested fixes: GH #372 (3630ef2f4d), sdk-demo queue/UI recovery (integrated c827367ef5), and lazily blocked-reason diagnostics (integrated 0f8785ad60). Release workflow 40078734115 passed all 12 jobs; the public GitHub release has 24 assets; PyPI workflow 40081111873 succeeded and the wheel is public. No batch requirement remains and no follow-up queue item is needed.
<!-- agent-doc-subagent-completion key=b8d371fcd90a41a0b9f171d6b67403dac508f3164f8a340a2a4cc775082c8cbd -->

### Re: Subagent completed: `subagent:tmux_template_density_verify` returned work for queue item “In the Claim for Tmux Pane template, make each `##` header on the same line as the open tag such as: `## Run <!-- agent:run priority subagents -->`. This is to reduce extra spaces in the document to allow for more information to be added in the view pane. `## Queue` should be renamed to `## Run`.” — agent-doc
<!-- agent-doc-subagent-response sha256=a700326e001f9c60289c910a7ebadfd7dccae9b116450988fa2de1d6e69bcbe3 -->
> **Subagent response:**
>
> Verified complete in v0.35.551. Commit f6c27bba7e compacts all generated Claim/Sync headings onto their component tags and uses `## Run`; b3a29e355f safely permits matching heading-collocated tags. Tagged regression tests assert all nine exact headings, reject `## Queue`, and cover parser safety. Release workflow 40078734115 succeeded. No follow-up needed.
<!-- agent-doc-subagent-completion key=2837068d1a4c38278158c7eeb4ce456b504bfb421f08af30ade581267ccf9e07 -->

### Re: Subagent completed: `subagent:tmux_template_done_verify` returned work for queue item “Rename "## Completed / Reaped" to "## Done"” — agent-doc
<!-- agent-doc-subagent-response sha256=9615e68cac418718facff14cc8e2a0b6fdd65f35d82ba4f52d425d94fbcfd407 -->
> **Subagent response:**
>
> Verified complete in v0.35.551. Integrated commit 9bf3e5f0be makes the claim scaffold and archive generator use canonical `## Done`; no production `## Completed / Reaped` remains. The installed CLI is 0.35.551 and the authoritative gate/release workflow passed. No follow-up needed.
<!-- agent-doc-subagent-completion key=eb25c21eac20108f6f15dcb32cec42bc716b15c315befd25bb86f0a9ec1b3f66 -->

### Re: Subagent completed: `subagent:release_claim_stale_verify` returned work for queue item “The queue item in this document `- 🚧 [#subagent:release_batch_prep3#3] release + publish: include issue 372 + sdk-demo.md issue + lazily.md issue` seems to be stale. If it is stale, fix agent-doc causes.” — agent-doc
<!-- agent-doc-subagent-response sha256=3b23556c8c1964b4008ff7ebb0d4d1282cc22183ef2fd46d43fdb0268c5e658c -->
> **Subagent response:**
>
> Verified complete in v0.35.551. Integrated commit 3563c0f3df exposes typed claim freshness evidence, distinguishes reservation/worker binding/reporting from progress, reports ended-owner attempts as stale, and supplies generation-fenced recovery. Regression coverage passed. The former batch row is now visibly completed and synchronized. No follow-up needed.
<!-- agent-doc-subagent-completion key=fe0e3d1968570fc0b4bc6739bae8def8c74c4094730f285126a6676fc7ae50ce -->

### Re: Subagent completed: `subagent:idea_two_errors_verify` returned work for queue item “Two errors popped up in Idea: ![img_8.png](img_8.png)” — agent-doc
<!-- agent-doc-subagent-response sha256=c16b313adbb71d34d8bbea9cfa8e33967be57db3edb32704c9d15e3c6d03a914 -->
> **Subagent response:**
>
> Verified: img_8 shows two copies of one `ambiguous-retained-projection` attach refusal, not two distinct errors. v0.35.551 integrates 9fcc5bbce7 to deduplicate unchanged retained-hold alerts and rearm after resolution, plus editor generation commit 1585263a0d. Packages are classic 0.2.822, 262 0.2.823, and 263 0.2.824; targeted tests passed. Restart the running IDEA 2026.3 instance to activate 0.2.824, then complete existing Todo #jb263-feed-verify. No new Todo is needed.
<!-- agent-doc-subagent-completion key=ab23af86a4ed634661d93c835b7329eecf7c7fca6c6b86006caaca7705612cd7 -->

### Re: Subagent completed: `subagent:sdk_demo_wedge_verify` returned work for queue item “Run Agent Doc on sdk-demo.md is wedged after I started a new session via `/clear`. Fix agent-doc causes. Fix any issue you see at multiple levels of the dataflow.: [route] explicit send for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md is blocked: 1 pending steering item(s) remain durable, but no live owning turn was proven and no second trigger was injected. The idle supervisor re-evaluates stale-empty-preflight recovery on a bounded cadence; no second send is required. Otherwise follow the named recovery (blocker: closeout recovery blocked [open_cycle]: missing proof: open cycle must finish, be replayed, or be explicitly queued behind; recommended: captured response baseline no longer matches current document for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md. If an editor has this document open, reattach it so the authority re-bootstraps from the editor's text: `agent-doc admin reload-lib` (or reopen the document tab); do not run `reset --from-current` while the editor shows text the authority lacks, because it saves the authority over the editor. With no editor attached, rebuild the cold recovery projection without clearing session state: `agent-doc reset --from-current --preserve-session /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md`; resume durable checkpoint cycle=cycle-1792824204227 phase=write_applied capture_id=cycle-1792824204227; the canonical write is already applied; retained commit recovery owns the terminal boundary; do not resubmit finalize or write for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md). steering_delivered=0 sent=explicit ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=run_recovery_command recovery_command=agent-doc admin reload-lib” — agent-doc
<!-- agent-doc-subagent-response sha256=804858b6084ecbb776c52ab3c188049f144ff7cd80399b6a43967059daebf0e4 -->
> **Subagent response:**
>
> Independent verification passed: sdk-demo is no longer wedged or out of sync. Grok generation 13 pane %58 is ready/alive; cycle synthetic-1792830619703 is committed with commit_success, open=false, editor convergence and closeout commit both OK, lint clean, and no document diff. The live trace safely re-registered the editor, resumed the retained write-applied capture, committed exactly once, and recorded terminal file/snapshot/HEAD agreement. v0.35.551 includes e470d0038f ownerless captured-finalize recovery, 97856c9eb9 projection convergence, and 3563c0f3df claim evidence. The authoritative session-check returned queue_continuation_required=false and next_action=no_agent_action. This head can close; do not reload, reregister, reset, finalize, write, or run another session-check.
<!-- agent-doc-subagent-completion key=c5a51f82eae66f98980bd81e416482e9d124f73c63860834e5914214ff740e8d -->

### Re: recovered response

Implemented, installed, released, and reconciled.

- Root cause: `Deferred` committed correctly, but head-only dedup suppressed the required continuation. Fixed in the state machine and regression coverage.
- Yes, state-machine analysis could identify it: the missing forbidden state was “committed Deferred + pending head + no owner + no scheduled continuation.”
- Local install: `agent-doc 0.35.551`.
- Editor packages: VS Code `0.2.83`; JetBrains classic/262/263 `0.2.822/0.2.823/0.2.824`.
- sdk-demo is synchronized and healthy: Grok pane `%58` is ready, the cycle committed successfully, and `next_action=no_agent_action`.
- All named queue receipts are now visibly `☑️` and the session document is committed.
- Published release: [v0.35.551](https://github.com/btakita/agent-doc/releases/tag/v0.35.551), with 24 assets and a successful PyPI publication.

Action required:

1. Restart IntelliJ IDEA 2026.3 to load plugin `0.2.824`.
2. Verify the “Cannot Upgrade Plugins” warning clears.
3. If it remains, capture the configured repository entry and relevant `idea.log` update-check lines under Todo `#jb263-feed-verify`.
4. Do not restart, reload, reset, or rerun sdk-demo; it needs no further action.

The remaining untagged/noise queue heads are assigned to the supervisor’s automatic clear-and-continue path—not an operator blocker.
> **Queue prompt:** [#coder-remote-install-02805]: done: but still not syncing

### Re: [#coder-remote-install-02805]: done: but still not syncing — gpt-6.1-sol · 2026-10-25T21:00:21-04:00

The installation step is done, but synchronization is not. I am keeping the Todo open.

The failure is now concrete:

- This workstation has Agent Doc CLI 0.35.551 and JetBrains 263 plugin 0.2.824 installed. Its ordinary local editor replica is converged.
- This workstation is not the Coder backend, so it cannot prove that both Coder peers loaded the package.
- The current IDEA log first reports frontend/backend ingress connected and schema 4 with measured geometry, but every snapshot is then rejected with DELIVERY_FAILED: No such file or directory.
- The failing IDEA project is rooted at /home/brian/IdeaProjects/IdeaHome while the session document is under /home/brian/work/btakita/agent-loop. ControllerSurfaceSnapshotPublisher derives its controller socket from the IDEA project basePath, so it targets the wrong project controller.
- The controller log contains only a local jetbrains@... surface. It has no accepted jetbrains-rd@... / remote-client:... publication for this session.

Yes, the state machines could have identified this. Version 0.35.551 intentionally keeps the frontend authority fence active after a delivery failure, but it does not separately expose connected-without-any-accepted-publication as unhealthy. The missing state is:

connected lease + negotiated schema + zero accepted snapshots + DELIVERY_FAILED = blocked, with the controller-root mismatch as the actionable reason.

Actionable recovery and acceptance:

1. In Coder Remote Dev, open /home/brian/work/btakita/agent-loop as the backend IDE project, not merely this file from an unrelated project.
2. Confirm both Client/Gateway and backend show Agent Doc 0.2.824, then restart both peers.
3. In the backend terminal, run:
   - agent-doc env --json
   - agent-doc tmux ensure tasks/agent-doc/agent-doc-bugs.md --ide-terminal --json
   - agent-doc session status tasks/agent-doc/agent-doc-bugs.md
4. Do not call it synchronized until the client log shows snapshot schema negotiated with geometry=measured, the backend has no DELIVERY_FAILED, and ops.log shows controller_editor_surface_observed client=jetbrains-rd@... surface=remote-client:... followed by pane_layout_desired_published with column_order=editor.
5. If No such file or directory remains, the code fix is to route ControllerSurfaceSnapshotPublisher by each session document's workspace root instead of project.basePath, add the blocked delivery state, and cover the mismatched IDE-root case with a regression test.

The Todo now carries those exact recovery steps and proof conditions.
> **Queue prompt:** Run Agent Doc issue: Sync failed: {"applied":false,"reason":"superseded_by_newer_layout_state","columns":["/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md","/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md"],"layout_generation":9,"layout_publisher":"command","window":null,"focus":"/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md","no_autostart":false,"exact_visible":true,"routes_created_panes":true,"file_panes":[],"gated_documents":[]}

### Re: superseded layout sync notification — gpt-5 · 2026-10-25T21:22-04:00

Fixed the JetBrains notification boundary. The controller behavior was correct: ops evidence shows the SDK layout request was overtaken by a newer manual layout request for `tasks/infra.md`. The stale completion returned `superseded_by_newer_layout_state`, but `SyncLayoutAction` treated every nonzero receipt as an actionable error and emitted the misleading “Sync failed” notification.

The action now parses the typed JSON reason. A superseded stale completion is logged at info level without notifying the operator; genuine nonzero controller failures still use the existing error notification. Added a regression test covering the superseded receipt, an ordinary retry-pending receipt, and malformed diagnostic text.

Changed paths:

- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/SyncLayoutAction.kt`
- `src/agent-doc/editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/SyncLayoutActionTest.kt`

Verification:

- `./gradlew compileKotlin` — passed.
- `git diff --check -- <two changed paths>` — passed.
- `./gradlew test --tests "*SyncLayoutActionTest"` — blocked before test execution by existing unrelated `CpRouteClientCommandPlaneTest.kt` unresolved `toByteArray` references.
- `make check` — 1,524 tests passed before the existing `development_claude_skill_matches_the_bundle` stale-skill assertion stopped the suite.
- Latest GitHub Actions `CI` run on `main` (`40078734298`) — passed.

Confidence is high in the root cause and notification fix; end-to-end plugin confirmation awaits the repository’s pre-existing test/build conflicts being cleared. No external escalation was required. The two code files remain scoped working-tree changes because the `src/agent-doc` index already contains unrelated unresolved merge conflicts, so a safe isolated commit is not possible from this checkout.

Session note: the exchange is large (265 lines across 17 response sections); the active auto-clear queue will reset context after this closeout.
> **Chat prompt (#chatprompt:93f05b0003ff):** Are subagents runningU?

### Re: subagent status — gpt-5 · 2026-10-25T21:38-04:00

No. The live roster contains only the primary `/root` agent; no subagents are running.
> **Chat prompt (#chatprompt:1a3a48c152a9):** The Sync failed issue is still occurring: Sync failed: {"applied":false,"reason":"superseded_by_newer_layout_state","columns":["/home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md","/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md"],"layout_generation":16,"layout_publisher":"command","window":null,"focus":"/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md","no_autostart":false,"exact_visible":true,"routes_created_panes":true,"file_panes":[],"gated_documents":[]}

> **Chat prompt (#chatprompt:ee9fedf9bc9c):** Use a subagent to fix the issue

### Re: cross-root pane recovery fix — gpt-5 · 2026-10-25T21:58-04:00

You were right: the prior notification-only change did not fix the underlying failure and had never been installed. I dispatched a subagent, and it traced the live generations 14–16 through the controller and tmux state.

The apparent immediate supersession is legitimate sequencing: each receipt beside a new publication belongs to the previous click’s waiter and reports the newer current generation. The actual defect is structural convergence. The SDK controller retains stale actor pane `%58`, while tmux contains only `%2`; the owner returns `actor_ownership_unproven`. A prior safety change correctly made passive `ObserveOnly` focus refuse that stale pane, but accidentally made explicit `ProvisionForLayout` refuse too, preventing auto-start from recreating the missing cross-root pane.

The subagent’s fix permits `ActorOwnershipUnproven` only for explicit `ResumeLatest` and `ProvisionForLayout` recovery when a known session exists. Passive `ObserveOnly` remains fail-closed and cannot focus a stale or wrong pane.

Deliverables:

- Commit: `cb2f9068e3e1ec068cc212910ce4dfd5a354558e` (`fix: recover unproven layout actor panes`)
- Branch: `fix/layout-supersede-await`
- Clean worktree: `/home/brian/worktrees/agent-doc-layout-supersede`
- Changed source: `agent-doc-controller-io/src/project_controller/rpc.rs`
- The exact controller patch is also applied to the shared checkout. The earlier JetBrains notification suppression was removed; genuine supersession remains visible instead of being hidden.

Verification:

- Focused policy regressions passed in both the isolated and shared checkouts.
- `cargo test -p agent-doc-controller-io`: 820 unit and 12 integration tests passed; 10 ignored.
- `make check`: passed, including 11,494 workspace tests, 204 VS Code tests, cross-editor simulation, classic and split JetBrains builds/tests, bootstrap, and TLA gates.
- Diff checks passed; the isolated worktree is clean; the latest five base-branch GitHub Actions runs are green.

Confidence is high (0.95). The running binary has not yet received the patch, so the live failure can continue until installation and idle-boundary recycle. Installing while this response cycle is open risks stranding the attached editor replica; the required install/recycle/live proof is therefore tracked as [#layoutunprovenliveverify] for the next clean cycle.

### Re: tmux pane sync recovery and recurrence prevention — gpt-5 · 2026-10-25T23:24-04:00

> **Queue prompt:** There is still only 1 tmux pane with 2 agent-doc documents open in the editor. Sync still fails. How can we ensure this issue won't happen again?

The repeated failure was a delivery gap, not evidence that the proposed policy fix was wrong. Remote `main` and the installed controller were still at `03433d0494`; the fix `cb2f9068e3` existed only on a verification branch. The live state matched that old build exactly: tmux had only pane `%2`, while `sdk-demo.md` remained bound to dead pane `%58`, and structural sync stopped at `ActorOwnershipUnproven`.

I completed the missing delivery path:

- Fast-forwarded `cb2f9068e3` to remote `main`.
- Installed it from the clean isolated worktree. The native reload reached the editor endpoint, and the supervisors/controllers were marked for safe idle-boundary recycle.
- Preserved the safety boundary: explicit `ProvisionForLayout` and `ResumeLatest` may replace an ownership-unproven stale actor when the session is known; passive `ObserveOnly` still cannot focus or reuse that pane.

To prevent recurrence, completion now has three distinct proofs:

1. The fix commit must be reachable from remote `main`; a pushed worker branch alone is not integration.
2. The installed build ID/inode must correspond to the patched build; version `0.35.551` alone is insufficient because patched and unpatched builds share it.
3. Live acceptance must replace the dead binding, produce two tmux panes, and remove the unresolved cross-root diagnostics.

The first two proofs are complete. The third remains tracked by existing backlog item `#layoutunprovenliveverify`; its supervisor/controller recycle is intentionally deferred to the idle boundary rather than forced during this document closeout.

### Re: deterministic cross-root regression — gpt-5 · 2026-10-25T23:24-04:00

> **Queue prompt:** Add a deterministic controller/SimWorld regression and routing-spec clause for cross-root Sync Tmux Layout with two editor-visible documents where the second has a ready actor bound to a dead or ownership-unproven pane; assert ProvisionForLayout replaces the stale binding and converges to two panes while ObserveOnly never focuses/reuses the unproven pane.

Implemented, integrated, pushed, and installed as commit `2490d2496ed965d371604320a6d58880fa82b52d` on remote `main`.

The new deterministic controller SimWorld models two cross-root editor-visible documents, one valid pane, and a ready second actor bound to a dead/ownership-unproven pane. It proves that passive `ObserveOnly` emits no focus or provisioning effect, while explicit `ProvisionForLayout` replaces the stale binding and converges to exactly two panes. The matching invariant is recorded in `specs/07-session-tmux-commands.md`.

Verification:

- Focused cross-root regression: 1 passed, 0 failed.
- `cargo test -p agent-doc-controller-io`: 821 passed, 0 failed, 10 ignored; integration targets passed.
- `make audit-docs` and `make build`: passed.
- A queue-finalization test that stopped the worker's first full run passed when reproduced alone.
- Coordinator authoritative `make check`: passed with exit 0 and zero failure groups.
- Final `make install`: passed; reload intent reached 1/1 editor endpoint.
- GitHub Actions run 40263702209 for `2490d2496e` is currently in progress.

The exchange remains large, but the active queue continues, so I did not compact it.

### Re: Integrate completed work, then release + publish — gpt-6.1-sol · 2026-10-26T03:25:40-04:00

> **Queue prompt:** Wait for all 🚧 items to complete. Then integrate. Then release + publish

Released and published `agent-doc v0.35.552` from commit `880d0adf2d`.

The stable latest GitHub release contains all six native platform archives and checksums, VS Code `0.2.83`, JetBrains classic `0.2.825`, JetBrains 262 `0.2.826`, JetBrains 263 `0.2.827`, their update feeds, Marketplace upload receipts, and `SHA256SUMS`. All platform, plugin, target-channel, Homebrew, and PyPI publication jobs succeeded; PyPI run `40285228122` is green.

The authoritative integration gates passed: 11,506 Rust tests (177 skipped), 204 VS Code tests, 1 cross-editor test, 8 Python tests, all three JetBrains builds/verifiers with zero internal/override findings, 153 TLA checks, and the full live tmux CI suite. During the release gate I fixed a reconciler regression that let tmux auto-renaming pull a displaced protected open-cycle pane back out of stash; the strengthened regression and related sync tests pass.

The current packages are installed locally. Direct evidence closes `#hj2r` and `#helpregisterjetbrains`. Live restart/operator proof remains open for IntelliJ 2026.3 under `#jb263-feed-verify` and for the two-sided Coder Remote Dev path under `#coder-remote-install-02805`.

The initial release workflow's aggregate promotion job was cancelled by a concurrency race with the per-target fan-out. Its isolated rerun succeeded and the release is fully green; `#release-promotion-concurrency` now tracks removing that manual recovery path.

### Re: The queue is full of items. Are there complete? — gpt-6.1-sol · 2026-10-26T03:27:00-04:00

> **Queue prompt:** The queue is full of items. Are there complete?

Yes. The items marked ☑️ in the completed release batch reported successfully, were integrated and verified, and shipped in `v0.35.552`. Their retained completion notices are historical evidence, not unfinished implementation work.

The queue is not globally empty: `do [#layoutunprovenliveverify]` is the next unclaimed implementation/verification item, and the live operator proofs for `#jb263-feed-verify` and `#coder-remote-install-02805` remain open. Those are separate from the completed release batch.

### Re: Fix release promotion concurrency — gpt-6.1-sol · 2026-10-26T03:49:55-04:00

Completed `#release-promotion-concurrency`.

The release aggregate and per-target promotion workflows shared one repository-wide concurrency group. GitHub's default single pending slot allowed the six rapid target dispatches to replace the pending aggregate job even with `cancel-in-progress: false`. Both participants now set `queue: max`, preserving serialized stable-channel writes while preventing pending promotion eviction.

The worker commit `7759804dff` was integrated as `42bd3810a8` and pushed to `origin/main`. The spec and regression test were updated. Coordinator verification passed the exact regression, `make check-fast` with 1,975 tests, clippy, docs/release self-tests, and a local install. The installed CLI remains `0.35.552`; JetBrains generations remain `0.2.825`, `0.2.826`, and `0.2.827` because editor sources were unchanged.

### Re: Release and publish handoff — gpt-6.1-sol · 2026-10-26T08:21-04:00

Dispatched `do [#layoutunprovenliveverify]` to `subagent:layout_liveverify` as generation 2, bound to worker `/root/layout_liveverify`.

The exact `release + publish` instruction is restored immediately after that live-verification head. Its durable claim points to an old v0.35.551 report, so that historical handback is not fresh completion evidence for this request. The coordinator will run a fresh release only after the current layout fix is integrated and live-verified.

The exchange is large, but the active queue remains in progress, so I did not compact it.

### Re: release + publish ⟦effect:1⟧ / ⟦effect:2⟧ — opus-5 · 2026-10-26T08:31-04:00

> **Queue prompt:** release + publish ⟦effect:1⟧

I closed the `⟦effect:1⟧` row without a new release. Its ☑️ marker came from `subagent:release_publish_patch#3`, which reported about 52 hours ago. That report is the old v0.35.551 release handback, not new work. The newer release is already out: `v0.35.552` is the Latest GitHub release, with 24 assets, and PyPI serves 0.35.552. The installed CLI is also 0.35.552.

I held the fresh `release + publish ⟦effect:2⟧` and did not dispatch it to a subagent, for two reasons:

- **Subagents cannot release.** The coordinator rules forbid a subagent from running a release, so a dispatched worker could not do this item. The coordinator has to run the release.
- **It should wait for the live-verify.** `do [#layoutunprovenliveverify]` is still running as `subagent:layout_liveverify#2`, with about 1h50m left on its claim. Its code fix `cb2f9068e3` already shipped in v0.35.552. Only one commit has landed since that tag: `42bd3810a8`, the fix that stops release promotions from evicting each other. If I released now, the release would ship almost nothing new. Its live tmux suite would also run alongside the worker's live tmux layout check. Waiting means that if the live-verify finds a new defect, its fix goes into the same release. This matches the plan in the previous closeout.

Once `layout_liveverify#2` reports back, the coordinator should integrate its result and then run `⟦effect:2⟧` as one release.

I also found a defect. When agent-doc added the ⟦effect⟧ annotations to the two identical `release + publish` rows, the stale `release_publish_patch#3` claim moved from its original row onto the freshly restored one. The original row became 📥. That is why the restored request looked complete. I filed it as `#claimrowmigrate`, with a matching check in the todo list.

The other ☑️ rows in `Queue: Main` are completed handbacks that shipped in v0.35.552, as the 03:27 closeout recorded. They are not unfinished work. The exchange is now 400 lines, but the queue is still active, so I did not compact it.

### Re: release + publish v0.35.553 — opus-5 · 2026-10-26T09:55-04:00

> **Chat prompt (#chatprompt:5ef65fd9ea8b):** release + publish now

> **Queue prompt:** release + publish ⟦effect:2⟧

Released and published `agent-doc v0.35.553` from release commit `a0a525fb65`. Tag `v0.35.553` is the stable Latest release with all 24 assets: six native archives, the JetBrains and VS Code packages and feeds, and the checksums. PyPI serves 0.35.553. All six target channels are promoted, and the Homebrew formula is bumped. The release gate passed: `make check` ran 11,506 Rust tests with 177 skipped, and the editor parity audit passed. The release-profile build is installed locally.

The JetBrains plugins stay at 0.2.825/0.2.826/0.2.827 and VS Code at 0.2.83, because their sources are byte-identical to 0.35.552.

**The first run failed, and I repaired it.** Marketplace rejected the carried-over plugin versions: `400 ... already contains version 0.2.825 in channel`. The workflow looks for upload markers on the current tag only. The failed `plugins` job also skipped the editor-package upload and checksums, so every stable-channel promotion failed on the missing `EDITOR-PACKAGES.sha256`. I copied the three Marketplace markers from v0.35.552 to v0.35.553 and reran the failed job. Each marker still names `880d0adf2d`, the commit whose run actually uploaded that version. The whole run then succeeded.

The fix for future releases is on main as `3cdcc5a31e`. Only Marketplace's exact duplicate-version refusal for the artifact's own version now counts as accepted. Every other refusal still fails the job. The workflow regression, a shell simulation of all three upload outcomes, and `make check-fast` (1,975 tests) all passed.

**`#claimrowmigrate` is integrated and installed.** Commit `20a8311ffa` keeps a reported claim bound to its own row. A row with the same text, or a ⟦effect:N⟧ annotation, can no longer inherit it. The coordinator re-ran 1,155 queue-crate tests, and all passed. The fix fast-forwarded main and was installed with `make install`. Its todo stays open for the live check.

> **Chat prompt (#chatprompt:b91827c8723c):** yes

The `agent:run` and `agent:loop` claim-marker drift went to a background subagent on `fix/queue-mirror-marker-sync-20261026`. I found three mismatches:

- **`#layoutunprovenliveverify`:** Run: Issues shows the expired generation `#1`. Queue: Main shows the live `#2`.
- **`release + publish`:** Run: Issues still shows the ended `release_publish_patch#3` claim.
- **Onboarding item:** Run: Issues shows 🚧 with no owner or claim.

The same worker is also tracing two more problems. The `do [#claimrowmigrate]` row disappeared after the 09:09 commit, along with its claim. Queue: Main also has a duplicated `jb263_feed_verify#1` row.

You added `#gh-fix` issues #373, #374, and #375, and each is dispatched to its own subagent worktree:

- `subagent:gh373`: a deferred surface capture sends zero `--col` arguments.
- `subagent:gh374`: `respond` cannot find the `queue` component when the `agent:run` marker is on the heading line.
- `subagent:gh375`: the Remote Dev popup gets a null action from the frontend-only `UpdateChannel`.

Each worker leaves its issue open. The coordinator closes each one after integrating and installing the fix.

CI for main commits `20a8311ffa` and `3cdcc5a31e` was still running at closeout.

### Re: GH #375 Remote Dev popup null action — opus-5 · 2026-10-26T10:12-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/375

Fixed, integrated, installed, and closed [GH #375](https://github.com/btakita/agent-doc/issues/375).

Under Remote Dev, the backend builds `AgentDoc.Popup` from its own action manager. That manager only knows the backend module's actions. Since #372, `AgentDoc.UpdateChannel` is registered only in the frontend. The popup still listed it, so the lookup returned null, `DefaultActionGroup.add` threw, and the popup never opened.

Popup ids now go through `AgentDocPopupAction.resolveActions`. The backend popup omits client-only ids such as `UpdateChannel`. That action stays in the JetBrains Client's Tools menu and in a single-process IDE's popup. Any other unregistered id is logged and skipped, so it can no longer break the popup.

The worker's commit `a50a1e89eb` fast-forwarded `main`. Its verification:

- **Classic plugin:** 947 tests passed, including the classic artifact verifier.
- **2026.2 and 2026.3 builds:** `make jetbrains-262-check` and `make jetbrains-263-check` passed. That includes the new backend test, which checks every popup id against the generated `agent.doc.backend.xml`.
- **Rust and docs:** `make check-fast` passed.
- **Mutation check:** with the client-only set emptied, 2 of the 3 new backend tests failed.

`make install` installed JetBrains classic 0.2.828, 2026.2 0.2.829, and 2026.3 0.2.830. The running IntelliJ 2026.3 and Rider 2026.2 must restart to load the new generation. VS Code packaging was skipped because `vsce` is not installed, and the extension source is unchanged. The fix ships in the next agent-doc release.

The #373, #374, and queue mirror-sync workers are still running.

### Re: GH #373 and GH #374 fixed — opus-5 · 2026-10-26T10:53-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/373

[GH #373](https://github.com/btakita/agent-doc/issues/373) is fixed in `589c4b6d70`, installed, and closed.

**Cause:** On a Remote Dev backend, the plugin's `--col` fallback sees no selected files or focused editor. The route request therefore carried zero columns. The controller refused any zero-column request with `editor route requires the exact visible layout before dispatch`. That refusal ran before the GH #347 focus-only dispatch and before the GH #360 install-remedy refusal.

**Fix:** A non-`exact` route whose `--col` arguments name no document is now a one-column `ensure` route for the routed document.
- A document with a live pane gets a focus-only dispatch. No layout is published.
- A document with no pane gets the `flat_evidence_has_no_column_geometry` refusal and its install remedy.
- An `exact` route with zero columns still refuses. No column is guessed.

This is a controller-only change, so plugin 0.2.824 needs no update.

**Verification:** The 3 new regressions pass, and each fails with the fix disabled. The 35 related route tests and `make check-fast` pass. After the rebase, the coordinator reran the `editor_route` tests, and they passed.

The worker's full controller-crate run showed one failure. It is in an unrelated test, `retained_transition_adapter_does_not_hold_the_reactive_context`, which passes when run alone. That test is queued as its own follow-up, and `subagent:retainedgateflake` is working on it.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/374

[GH #374](https://github.com/btakita/agent-doc/issues/374) is fixed in `5fe17385b5`, installed, and closed.

**Cause:** v0.35.551 taught the element parser to accept `## Run <!-- agent:run ... -->`, but not the markdown-AST overlay. `respond` and the node-keyed mutations find components through that overlay, so they failed with "component `queue` was not found".

The document-cell merge had a second copy of the same opener rule. It also looked components up by raw name, so `run` never matched `queue`. As a result, every `agent:run` body merged to empty, even with the marker on its own line. That bug predates v0.35.551.

**Fix:** One shared rule, `component_heading_prefix_matches_tag`, now decides heading-anchored openers, and every opener scanner calls it. `projected_component_name` maps `run` and `loop` to `queue`.

**Verification:** Each new regression fails without the fix. The four affected crates pass: markdown-ast 66, element 247, merge 410, queue 920. `make check-fast` ran 2,005 tests, all passing. After the rebase, the coordinator reran all four crates, and they passed.

The worker's follow-up is in progress as `subagent:mergepairbyid`. The merge still pairs same-named components by first match. It should pair them by `id=` or by occurrence.

**Mirror-sync worker:** It reported back as `72477ee33f`, which I have rebased onto main. Integration is next.

- Claim markers were painted only onto the queue preflight had selected. The other queue's rows froze.
- The binary deleted the `do [#claimrowmigrate]` row. Its done-id parser read `subagent` from the owner marker, and a tombstone pass then dropped the live row.

The duplicate `jb263-feed-verify` row is your own second copy. Both rows correctly show one claim.

JetBrains is still reporting plugin 0.2.827. Restart the IDE to load 0.2.828, 0.2.829, and 0.2.830.
<!-- agent:boundary:3ec64169 -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main
<!-- agent:run id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 [#subagent:layout_liveverify#2] do [#layoutunprovenliveverify]
- ☑️ [#subagent:gh372#1] #gh-fix https://github.com/btakita/agent-doc/issues/372: When we had the "guessing" architecture between the frontend + the backend, the ends were able to communicate...but had the persistent swapped tmux pane issue. Now the frontend + the backend don't communicate and I cannot Run Agent Doc or Sync Tmux Panes and tmux pane auto-sync is not working. Can we bring back the "good parts" of the architecture that used "guessing" but without the "guessing"? Or is this some state propagation issue, race condition, wait/timeout needed, or state that we are not accounting for?
- ☑️ [#subagent:grok_queue_ui#1] In sdk-demo.md using Grok, the queue item was finished but the UI did not update. `- 📥 When I load Boss Room, claim an item, stop Boss Room, start Boss Room, the item is not claimed.`
- ☑️ [#subagent:lazily_block_reason#1] On lazily.md, `- ⚠️ [#subagent:corecireachaudit2#2] do [#lzcorecireachaudit]` is blocked and I can't find a reason why.
- ☑️ [#subagent:release_batch_verify#4] release + publish: include issue 372 + sdk-demo.md issue + lazily.md issue
- ☑️ [#subagent:grok_subagents_dispatch#1] The sdk-demo.md Grok turn did not use subagents despite `agent:queue` having `subagents`.
- ☑️ [#subagent:tmux_auto_sync#1] tmux pane auto sync is not working. Sync Tmux Panes works.
- ☑️ [#subagent:grokqueuesubagents_closeout#1] do [#grokqueuesubagents]
- ☑️ [#subagent:tmux_template_density_verify#2] In the Claim for Tmux Pane template, make each `##` header on the same line as the open tag such as: `## Run <!-- agent:run priority subagents -->`. This is to reduce extra spaces in the document to allow for more information to be added in the view pane. `## Queue` should be renamed to `## Run`.
- ☑️ [#subagent:tmux_template_done_verify#2] Rename "## Completed / Reaped" to "## Done"
- ☑️ [#subagent:release_claim_stale_verify#2] The queue item in this document `- 🚧 [#subagent:release_batch_prep3#3] release + publish: include issue 372 + sdk-demo.md issue + lazily.md issue` seems to be stale. If it is stale, fix agent-doc causes.
- ☑️ [#subagent:idea_two_errors_verify#2] Two errors popped up in Idea: ![img_8.png](img_8.png)
- ☑️ [#subagent:jb263_actionable_todo_verify#1] `- ⚠️ [#subagent:jb263_feed#1] Should we have the `-263.xml` in `jetbrains-263.xml`? I saw the selection in the list. I selected it. I still see Agent Doc in the Cannot Upgrade Plugins. ![img_7.png](img_7.png)` should have an actionable todo item. With the todo item id appended to the queue item for followup. [#jb263-feed-verify]
- ☑️ [#subagent:sdk_demo_wedge_verify#2] Run Agent Doc on sdk-demo.md is wedged after I started a new session via `/clear`. Fix agent-doc causes. Fix any issue you see at multiple levels of the dataflow.: [route] explicit send for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md is blocked: 1 pending steering item(s) remain durable, but no live owning turn was proven and no second trigger was injected. The idle supervisor re-evaluates stale-empty-preflight recovery on a bounded cadence; no second send is required. Otherwise follow the named recovery (blocker: closeout recovery blocked [open_cycle]: missing proof: open cycle must finish, be replayed, or be explicitly queued behind; recommended: captured response baseline no longer matches current document for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md. If an editor has this document open, reattach it so the authority re-bootstraps from the editor's text: `agent-doc admin reload-lib` (or reopen the document tab); do not run `reset --from-current` while the editor shows text the authority lacks, because it saves the authority over the editor. With no editor attached, rebuild the cold recovery projection without clearing session state: `agent-doc reset --from-current --preserve-session /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md`; resume durable checkpoint cycle=cycle-1792824204227 phase=write_applied capture_id=cycle-1792824204227; the canonical write is already applied; retained commit recovery owns the terminal boundary; do not resubmit finalize or write for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md). steering_delivered=0 sent=explicit ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=run_recovery_command recovery_command=agent-doc admin reload-lib
---
☑️ [#subagent:run_sdk_demo_grok#1] Run Agent Doc on sdk-demo.md (Grok): We should be able to support `## Run <!-- agent:run priority subagents -->`. Also the error message should mention the tag `agent:run`, not Queue, to more easily identify where and what the issue is in the document. Do this for all error messages mentioning tags. Also, line 229 is not the correct line. The agent:run was on line 185...it is now on 186 due to me placing `## Run` on a separate line in attempt to make the turn run.
```
❯ agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md                   3:17 AM


◆ Thought for 0.8s

The session is live again. I will read the agent-doc skill and admit before I touch the document.      3:18 AM

◈ Explored 11 files, 16 searches, called 3 tools, ran 11 commands

This turn did not start. Admission was refused, and the queue was left untouched.                      3:21 AM

agent_doc_admit for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md failed
because the retained delivery 1792824957388507072-1-
43b8cfaaaa6ff55a766f0e2e7fbb94a491fa16a0c5f80c22d00b24200277e659 still owns the commit-push subagent
response. Two session-checks returned the same interruption: editor reconnect refuses the merged
document because the queue open marker at line 229 is not alone on its line
(non_standalone_component_marker:queue:open:line229). The current editor text is unchanged. No
second closeout was sent, and the file was not force-written.

The JetBrains tab is open, and the controller currently has no live replica for it (live_editors=0).
A lint of the current document is clean. The bad marker is in that merged reconnect target. The same
capture is what has to land before a new cycle can admit. The binary allows only another agent-doc
session-check on that file.
```
---
---
☑️ [#subagent:sdk_demo_live_recovery#1] Run Agent Doc on sdk-demo.md still wedged even after I corrected the issue on the ran agent-doc version:
```
❯ What do I need to do to repair the document?                                                         3:29 AM


◆ Thought for 9.0s

The hold is a document-structure refusal, not a missing code change. I am checking the exact marker    3:29 AM
the controller rejects so the repair steps match that line.

◈ Explored 4 files, 8 searches, ran 5 commands

The disk copy of sdk-demo.md is already repaired. The controller is still holding the old one-line     3:31 AM
queue marker, and the JetBrains plugin is not connected, so that repair never becomes the document
it will project.

In /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md on disk, the Run section
is:

## Run
<!-- agent:run priority subagents -->
◇ markdown   [Copy]

The marker is alone on its line, and <!-- /agent:run --> is alone on its line. The controller's copy
still has this instead:

## Run <!-- agent:run priority subagents -->
◇ markdown   [Copy]

Session-check refuses the merged projection because that open marker is not alone on its line
(non_standalone_component_marker:queue:open:line229). The tab is open, and the controller reports
live_editors=0, so it keeps that old copy and will not adopt the file on disk.

In the open sdk-demo.md tab:

1. If the Run heading and <!-- agent:run priority subagents --> are still on one line, put the
comment on its own line, with nothing before or after it. Leave the queue items as they are.
2. If the tab already shows the two-line form, reload the file from disk and choose Load FS changes.
Do not choose Keep memory changes while the memory copy still has the one-line marker.
3. Leave the tab open long enough for the Agent Doc plugin to reconnect. A connected tab is what                 █
lets the retained commit-push response project.                                                                  █
																																																								█
After the tab shows the marker on its own line, ask again and I will run session-check. Do not                   █
force-write the file, and do not delete the queue items.                                                         █
```
---
---
☑️ [#subagent:sdk_demo_route_recovery#1] Run Agent Doc on sdk-demo.md: [route] explicit send for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md is blocked: 1 pending steering item(s) remain durable, but no live owning turn was proven and no second trigger was injected. The idle supervisor re-evaluates stale-empty-preflight recovery on a bounded cadence; no second send is required. Otherwise follow the named recovery (blocker: closeout recovery blocked [open_cycle]: missing proof: open cycle must finish, be replayed, or be explicitly queued behind; recommended: captured response baseline no longer matches current document for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md. If an editor has this document open, reattach it so the authority re-bootstraps from the editor's text: `agent-doc admin reload-lib` (or reopen the document tab); do not run `reset --from-current` while the editor shows text the authority lacks, because it saves the authority over the editor. With no editor attached, rebuild the cold recovery projection without clearing session state: `agent-doc reset --from-current --preserve-session /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md`; resume durable checkpoint cycle=cycle-1792824204227 phase=write_applied capture_id=cycle-1792824204227; the canonical write is already applied; retained commit recovery owns the terminal boundary; do not resubmit finalize or write for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md). steering_delivered=0 sent=explicit ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=run_recovery_command recovery_command=agent-doc admin reload-lib
---
- ~~release + publish ⟦effect:2⟧~~
- ☑️ [#subagent:loop_routing#1] The agent:loop is getting items sent to it, but the items should have been sent into `agent:run`.
- ☑️ [#subagent:queue_completion_audit#1] The queue is full of items with ☑️. Are there complete? Why are they not struck?
- ☑️ [#subagent:auto_sync_project#1] tmux pane auto-sync not working in this agent-doc project instance
- ☑️ [#subagent:todo_hj2r_verify#1] [#hj2r]: verified
- ☑️ [#subagent:jb263_feed_verify#1] do [#jb263-feed-verify]
- ☑️ [#subagent:integrate_reported_items#1] Integrate all ☑️ items.
- ☑️ [#subagent:backlog_queue_sync#1] The backlog items are marked with 🚧 and is out of sync. The work has been completed in the queue.
- ☑️ [#subagent:tracked_work_todo_backlinks#1] backlog/queue/icebox items should always backlink to todo items so the todo items can be cleared once complete. This may mean the todo item(s) block the queue/backlog/icebox item(s).
- ☑️ [#subagent:jetbrains_submission_feedback#1] Read the feedback from the JetBrains agent-doc plugin submission. Fix any issues.
- ☑️ [#subagent:sdk_demo_queue_sync#1] The queue items in sdk-demo.md are out of sync. Will the agent-doc causes be fixed in the next version? If not, fix agent-doc causes.
- ☑️ [#subagent:release_promotion_concurrency#1] do [#release-promotion-concurrency]
- ☑️ [#subagent:jb263_feed_verify#1] do [#jb263-feed-verify]
- ☑️ [#subagent:gh373#1] #gh-fix https://github.com/btakita/agent-doc/issues/373
- 🚧 [#subagent:retainedgateflake#1] fix nondeterministic project_controller::tests::retained_transition_adapter_does_not_hold_the_reactive_context — the shared blocking_effect_gate::RETAINED_TRANSITION counter is incremented by concurrent tests (passes in isolation, fails under the parallel crate suite)
- ☑️ [#subagent:gh374#1] #gh-fix https://github.com/btakita/agent-doc/issues/374
- 🚧 [#subagent:mergepairbyid#1] document_cell_merge pairs components by name only; multi-queue (`id=`) documents should pair by id/occurrence
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/375~~
<!-- /agent:run -->

##~ Run: Issues
<!-- agent:loop id=issues interval=1h priority -->
- 🚧 [#subagent:layout_liveverify#1] do [#layoutunprovenliveverify]
- ☑️ [#subagent:grokqueuesubagents_closeout#1] do [#grokqueuesubagents]
- ☑️ [#subagent:release_publish_patch#3] release + publish
- 🚧 We need to work on making agent-doc easy to onboard. Sanat, a coworker is confused about how to get started.
<!-- /agent:loop -->

## Todo
<!-- agent:todo -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#coder-remote-install-02805] Repair Coder Remote Dev sync after the two-sided install: open /home/brian/work/btakita/agent-loop as the backend IDE project root; ensure Client/Gateway and backend both load JetBrains 263 plugin 0.2.824; restart both; on the backend run agent-doc env --json, agent-doc tmux ensure tasks/agent-doc/agent-doc-bugs.md --ide-terminal --json, and agent-doc session status tasks/agent-doc/agent-doc-bugs.md; require client idea.log to show snapshot schema negotiated with geometry=measured, backend idea.log to show accepted ingress with no DELIVERY_FAILED, and .agent-doc/logs/ops.log to show controller_editor_surface_observed client=jetbrains-rd@... surface=remote-client:... followed by pane_layout_desired_published with column_order=editor. If DELIVERY_FAILED No such file or directory persists, fix ControllerSurfaceSnapshotPublisher to resolve the controller from each session document root instead of an unrelated IDE project basePath, expose connected-without-accepted-publication as blocked while preserving the no-guessing fence, and add a regression test.
- [ ] [#jb263-feed-verify] In IntelliJ IDEA 2026.3, configure only https://github.com/btakita/agent-doc/releases/latest/download/agent-doc-jetbrains-263.xml, refresh Plugins, update Agent Doc from 0.2.816 to 0.2.824 or newer, restart the IDE, and verify the “Cannot update plugin” warning clears; if it does not, capture the configured repository entry and relevant idea.log update-check lines.
- [x] [#claimrowmigrate] After #claimrowmigrate ships, confirm a duplicated queue row keeps its own claim marker when agent-doc adds ⟦effect:N⟧ annotations. (source: agent:backlog [#claimrowmigrate])
<!-- /agent:todo -->

## Backlog
<!-- agent:backlog priority queue -->
- [ ] 🚧 [#layoutunprovenliveverify] Integrate subagent commit cb2f9068e3e1ec068cc212910ce4dfd5a354558e from fix/layout-supersede-await, install it after the current session closeout from the clean worktree, recycle at the idle boundary, and live-verify Sync Tmux Layout recreates the stale cross-root sdk-demo pane.
- [ ] ☑️ [#grokqueuesubagents] [agent-doc] On 2026-10-24, src/haiven-dev/tasks/sdk/sdk-demo.md had an agent:queue marker with attributes priority, subagents, and go, but agent-doc plan returned dispatch_mode manual, dispatch_candidate false, and no queue_subagent_dispatch list. The Grok coordinator drained the claim-button queue head inline instead of claiming and spawning subagents, and the operator had to say the queue items should run in subagents. Fix plan and preflight so a subagents queue dispatches queue_subagent_dispatch (claim and spawn) instead of an inline drain.
<!-- /agent:backlog -->

## Review
<!-- agent:review -->
- [/] [#jetbrainsmarketplaceprofile] [marketplace-approval] [agent-doc] Plugin 35436 is hidden and under initial JetBrains moderation; after approval, replace the basic description with polished copy, add Getting Started, Documentation, and Bugtracker URLs, then verify the public listing.
- [/] [#jb262dynlauncherrecommen] [recommended] [operator-verify] [agent-doc] Operator decision: jb262dynupgrade (153c72916) chose the accurate-warning option (a 262 package reports declined_by=modular_package and falls back to file replacement + restart). Either keep that as the supported policy (then close this), or authorize a real restart-free launcher for the exact-262 split-mode plugin that swaps backend + frontend (the frontend may run in a separate JetBrains Client process), proven against a live 262 IDE.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
- [/] [#gh299-live-263-proof] Live-verify v0.35.519 on JetBrains 263.7031 with changed plugin sources: `agent-doc upgrade` must hot-reload and complete without an IDE restart; close GH #299 only with that proof.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped
<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
