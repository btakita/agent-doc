---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 8eea85e7-65c9-4fbb-a8b4-f9bc53d90ea0
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

## Status <!-- agent:status patch=replace sync="##sync-release" -->

📤 Queue handoff: 10 reported items awaiting coordinator integration, verification, and closeout.
Main is at `b6b9af6e15` and agent-doc 0.35.532 is installed locally. Release v0.35.532 was published from `71f9801b49`. Post-release main now includes the inline-code-safe, occurrence-ordered queue preset authorization fix for GitHub issue #331 and the positive-evidence-only Remote Dev pane-order fix for GitHub issue #332; both issues are closed, the integrated full suite passed, and the current mainline was installed locally.

<!-- status:sync -->
Released and published agent-doc v0.35.556 from release commit `5cbfdbbc4c` (= `origin/main`); tag `v0.35.556` is the stable latest release (GitHub Latest, PyPI 0.35.556, stable target channels) and is installed locally. JetBrains classic/262/263 0.2.839/0.2.841/0.2.840, VS Code 0.2.83. It adds onboarding Getting started, the queue marker legend, the component-marker fences (live edit, cold start, JetBrains edit-time warning), row-aware editor attach, reliable echoed-head strikes beside a loop queue, doctor/autofix exit codes, and pane-process launches for start/route/fan-out. GH #378 closed; GH #379 awaits the reporter upgrading.
<!-- /status:sync -->

Queue markers: 📥 pending · 🟡 claimed · 🚧 running · ☑️ reported · ⚠️ lost · 🔄 needs reconcile · ❔ unknown · ⏭️ skipped · ⏳ awaiting operator · 🙋 operator action · 📌 pinned · 📍 agent pin ([legend](https://github.com/btakita/agent-doc/blob/main/docs/guide/queue-markers.md#queue-markers))
<!-- /agent:status -->
 

## Exchange <!-- agent:exchange patch=append subagents -->
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

### Re: Subagent completed: `subagent:mergepairbyid` returned work for queue item “document_cell_merge pairs components by name only; multi-queue (`id=`) documents should pair by id/occurrence” — agent-doc
<!-- agent-doc-subagent-response sha256=50fe64eb61300fef1223afbab8825a261ee234aeaac850ae9663603c5e5c01c9 -->
> **Subagent response:**
>
> The merge now pairs each component by its `id=` (or by order of appearance when there is no id) instead of taking the first component with the same name. The fix is committed and pushed. All requested tests and `make check-fast` pass.
>
> Branch: `fix/merge-pair-components-by-id`, built on `origin/fix/gh374-sameline-run-respond` (`e2ee3bad84`), pushed to origin. Commit: `03eb75ff88efb337e205623578862fa0722f3c0c`. Worktree: `/home/brian/work/btakita/agent-doc-wt/mergepairbyid`. Files changed: `agent-doc-merge/src/document_cell_merge.rs` and `specs/02-document-format.md`. No `make install`, release build or version bump.
>
> How pairing works now: a component is matched across base, agent and operator by its name (with `run`/`loop` counted as `queue`), its id, and its position among same-named components on that side. The explicit `id=` value counts as the id; a bare `id`, an empty value or `true` counts as no id. A single queue with no id is treated as `default`, the same id the queue code already uses, so `<!-- agent:run -->` and `<!-- agent:run id=default -->` pair. A component is never merged with a same-named one that has a different id; an unmatched component is handled on its own side (the operator's keeps its items, the agent's is carried over and appended). Each operator marker is tied to the exact component that starts on that line, so the rebuild never looks a component up by name again; if the parser doesn't recognize the component at a marker, the operator's lines are kept as they are (before, those lines were dropped). The agent's new response turns are read from the matched exchange component. The pairing rule is a new paragraph in `specs/02-document-format.md`, right after the #374 `projected_component_name` text.
>
> New regression tests (all 5 fail on the old code and pass with the fix): two_runnable_components_merge_their_items_independently; id_keyed_components_pair_across_an_operator_reorder; components_with_different_ids_never_pair; lone_unkeyed_queue_pairs_with_id_default; repeated_unkeyed_components_pair_by_occurrence.
>
> Test results: `cargo test -p agent-doc-merge -p agent-doc-markdown-ast -p agent-doc-queue` exit 0 (920, 415 and 66 tests, 0 failures). `make check-fast` exit 0: nextest 11520 tests run, 11520 passed, 177 skipped; "No blocking issues found". `cargo clippy -p agent-doc-merge --all-targets -- -D warnings` exit 0.
>
> Open decisions: an agent component the operator has no match for is appended at the end of the document, which now also applies to an id'd queue the operator deleted (kept consistent, not changed). Outcomes and conflict notes still label nodes only as component `queue`, with no queue id, because adding it would change the public `NodeOutcome` struct. No follow-up queue items.
<!-- agent-doc-subagent-completion key=c4a225158fe099940846764cc30a441c50cdd99aed4deebb27eb0a28dad767d1 -->

### Re: Subagent completed: `subagent:retainedgateflake` returned work for queue item “fix nondeterministic project_controller::tests::retained_transition_adapter_does_not_hold_the_reactive_context - the shared blocking_effect_gate::RETAINED_TRANSITION counter is incremented by concurrent tests (passes in isolation, fails under the parallel crate suite)” — agent-doc
<!-- agent-doc-subagent-response sha256=e9953ad388fb7a0c2d8d970740571f06b8163486a44fbbf4bdc29a2b9f5b99ca -->
> **Subagent response:**
>
> The flaky test is fixed, and the cause matched the suspicion: two tests shared one process-wide gate. The fix is committed and pushed on its own branch. Before the fix, 3 of 15 full `cargo test -p agent-doc-controller-io` runs failed on this test; after it, 0 of 15 failed.
>
> Branch: `fix/retained-transition-gate-flake`, pushed to origin. Commit: `30783f0402`. Worktree: `/home/brian/work/btakita/agent-doc-wt/retainedgateflake`.
>
> Root cause: `blocking_effect_gate::RETAINED_TRANSITION` was one open flag plus entered/passed counters for the whole test binary. Two tests use its fixture: this one and `canonicalization_retirement_fences_a_queued_stale_retained_projection`. Each calls `close()`, which resets the counters, and `open()`, which releases any waiting adapter, so either test could release or recount the other's adapter mid-assertion. Both directions happened: the target test failed its `passed()==0` check, and in another run the sibling failed with `CP relay write refused ... recovery=retry_crdt_merge` because the target test's `open()` let the sibling's stale queued write apply before retirement.
>
> Fix: each of the three test gates now only blocks for the one document a test closed it for. The retained-transition and document-effect gates are keyed by `document_hash`, and the strike gate by canonical path. `Gate::close(key)` returns a handle owned by that test, and its counters only see that document's adapter. Dropping the handle opens and releases the gate, and closing a key that is already closed panics. An adapter for a document no test closed passes straight through. No retries, serial marking or timeout changes.
>
> Tests: new `blocking_effect_gate::a_gate_key_is_isolated_from_every_other_tests_key`; the strike test now also asserts `wait_entered() >= 1`. Spec: the `#remainingwallclock` paragraph in `specs/12-deterministic-simulation.md` now requires each gate to belong to the test that closed it and be keyed by that test's document.
>
> Results: full crate suite, 15 runs: before 3 failed on this test, after 0 failed (826 passed each run). The two gate-sharing tests together, 200 runs before: 0 failed. Tests matching `retained` at 32 threads, 60 runs: 0 failed before and after. Gate tests including the new one: all 5 passed. `make check-fast`: exit 0, nextest 4189 run, 4189 passed, 153 skipped. One run in the before set failed on `singleton_launch_claim_rejects_concurrent_holder` instead (unrelated). Neither stress loop reproduced the race even before the fix, so the proof rests on the full-suite counts. One batch of 10 before-runs was discarded because a copied test binary made two `process::tests` discovery tests fail every run.
>
> Files changed: agent-doc-controller-io/src/project_controller.rs; specs/12-deterministic-simulation.md. No `make install`. No follow-up queue items.
<!-- agent-doc-subagent-completion key=e727477b8ae6c2038b453576dfdaac7a624edd71692bbc5b27f4ef54349c07bd -->
### Re: merge pairing, queue mirror sync, gate flake integrated — opus-5 · 2026-10-26T10:29-04:00

> **Queue prompt:** document_cell_merge pairs components by name only; multi-queue (`id=`) documents should pair by id/occurrence

The fix is integrated as `d8eb3ac0ba` and installed. The document-cell merge now pairs each component by name, `id=`, and position among same-named components. `run` and `loop` count as `queue`, and a lone component without an id pairs with `id=default`. Components with different ids are never merged. Each operator marker is tied to the exact component that starts on its line, so lines from a component the parser doesn't recognize are now kept instead of dropped.

The 5 new regressions fail on the old code and pass with the fix. One of them uses this document's own shape: `agent:run id="main"` and `agent:loop id=issues` both holding `#a`. After the rebase, 1,405 merge, markdown-ast, and queue tests passed, and `make check-fast` passed.

The worker left two behaviors for you to decide:

- **Deleted components come back:** an agent component with no operator match is still appended at the end of the document. With id pairing, that now also brings back an id'd queue the operator deleted.
- **Outcome labels:** merge outcome labels still name only `queue`, without the queue id.

> **Queue prompt:** fix nondeterministic project_controller::tests::retained_transition_adapter_does_not_hold_the_reactive_context — the shared blocking_effect_gate::RETAINED_TRANSITION counter is incremented by concurrent tests (passes in isolation, fails under the parallel crate suite)

The fix is integrated as `d14f33a01e`; it changes test code only. The cause was the suspected one. Two tests shared one process-global gate, and each test's `close()` and `open()` reset or released the other test's adapter.

Each test gate is now keyed by its own fixture's document, through a per-test handle that opens when dropped. Before the fix, 3 of 15 full crate runs failed. After it, 0 of 15 failed. After the rebase, the full controller crate passed: 826 passed, 0 failed.

The first push got a GitHub 500 and went through on the second attempt. A different flake seen once during the baseline, `singleton_launch_claim_rejects_concurrent_holder`, is queued as a follow-up and running as `subagent:singletonflake`.

**Queue mirror sync** is integrated as `fc9c0be086` and installed. The Issues queue now shows `⚠️ #2`, `🔄 release + publish`, and `📥` on the onboarding row. Two causes:

- **Frozen markers:** claims are keyed across the whole document, but every projection path painted only the selected queue. All queues are now projected from the same ledgers. A struck row in a loop is never reopened by another queue's claim.
- **Vanished row:** the `do [#claimrowmigrate]` row was removed by the binary at 13:09:26Z. `queue_prompt_done_id` read `subagent` from the owner marker, and the tombstone pass then dropped the live row. The id is now read after stripping lifecycle markers and the owner annotation. That work was already integrated, so its leftover reported claim is harmless.

The worker noted two remaining gaps:

- Marker changes in queues other than the selected one are not copied into the snapshot baseline.
- Claim edit-following still reads only the selected queue's heads.

**`#layoutunprovenliveverify` moved to review.** Its code, `cb2f9068e3`, has shipped since v0.35.552. The remaining step is a live check in your IDE, which a subagent cannot do without driving your live tmux layout. The previous worker's claim timed out. Your check is now the `#layoutliveverify-check` todo.

Next: the `release + publish` you queued after these items.

### Re: cleanup the agent:loop + agent:run items — opus-5 · 2026-10-26T11:03-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/372: When we had the "guessing" architecture between the frontend + the backend, the ends were able to communicate...but had the persistent swapped tmux pane issue. Now the frontend + the backend don't communicate and I cannot Run Agent Doc or Sync Tmux Panes and tmux pane auto-sync is not working. Can we bring back the "good parts" of the architecture that used "guessing" but without the "guessing"? Or is this some state propagation issue, race condition, wait/timeout needed, or state that we are not accounting for?

> **Queue prompt:** In sdk-demo.md using Grok, the queue item was finished but the UI did not update. `- 📥 When I load Boss Room, claim an item, stop Boss Room, start Boss Room, the item is not claimed.`

> **Queue prompt:** On lazily.md, `- ⚠️ [#subagent:corecireachaudit2#2] do [#lzcorecireachaudit]` is blocked and I can't find a reason why.

> **Queue prompt:** release + publish: include issue 372 + sdk-demo.md issue + lazily.md issue

> **Queue prompt:** The sdk-demo.md Grok turn did not use subagents despite `agent:queue` having `subagents`.

> **Queue prompt:** tmux pane auto sync is not working. Sync Tmux Panes works.

> **Queue prompt:** In the Claim for Tmux Pane template, make each `##` header on the same line as the open tag such as: `## Run <!-- agent:run priority subagents -->`. This is to reduce extra spaces in the document to allow for more information to be added in the view pane. `## Queue` should be renamed to `## Run`.

> **Queue prompt:** Rename "## Completed / Reaped" to "## Done"

> **Queue prompt:** The queue item in this document `- 🚧 [#subagent:release_batch_prep3#3] release + publish: include issue 372 + sdk-demo.md issue + lazily.md issue` seems to be stale. If it is stale, fix agent-doc causes.

> **Queue prompt:** Two errors popped up in Idea: ![img_8.png](img_8.png)

> **Queue prompt:** `- ⚠️ [#subagent:jb263_feed#1] Should we have the `-263.xml` in `jetbrains-263.xml`? I saw the selection in the list. I selected it. I still see Agent Doc in the Cannot Upgrade Plugins. ![img_7.png](img_7.png)` should have an actionable todo item. With the todo item id appended to the queue item for followup. [#jb263-feed-verify]

> **Queue prompt:** Run Agent Doc on sdk-demo.md is wedged after I started a new session via `/clear`. Fix agent-doc causes. Fix any issue you see at multiple levels of the dataflow.: [route] explicit send for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md is blocked: 1 pending steering item(s) remain durable, but no live owning turn was proven and no second trigger was injected. The idle supervisor re-evaluates stale-empty-preflight recovery on a bounded cadence; no second send is required. Otherwise follow the named recovery (blocker: closeout recovery blocked [open_cycle]: missing proof: open cycle must finish, be replayed, or be explicitly queued behind; recommended: captured response baseline no longer matches current document for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md. If an editor has this document open, reattach it so the authority re-bootstraps from the editor's text: `agent-doc admin reload-lib` (or reopen the document tab); do not run `reset --from-current` while the editor shows text the authority lacks, because it saves the authority over the editor. With no editor attached, rebuild the cold recovery projection without clearing session state: `agent-doc reset --from-current --preserve-session /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md`; resume durable checkpoint cycle=cycle-1792824204227 phase=write_applied capture_id=cycle-1792824204227; the canonical write is already applied; retained commit recovery owns the terminal boundary; do not resubmit finalize or write for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md). steering_delivered=0 sent=explicit ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=run_recovery_command recovery_command=agent-doc admin reload-lib

> **Queue prompt:** Run Agent Doc on sdk-demo.md (Grok): We should be able to support `## Run <!-- agent:run priority subagents -->`. Also the error message should mention the tag `agent:run`, not Queue, to more easily identify where and what the issue is in the document. Do this for all error messages mentioning tags. Also, line 229 is not the correct line. The agent:run was on line 185...it is now on 186 due to me placing `## Run` on a separate line in attempt to make the turn run.

> **Queue prompt:** Run Agent Doc on sdk-demo.md still wedged even after I corrected the issue on the ran agent-doc version:

> **Queue prompt:** Run Agent Doc on sdk-demo.md: [route] explicit send for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md is blocked: 1 pending steering item(s) remain durable, but no live owning turn was proven and no second trigger was injected. The idle supervisor re-evaluates stale-empty-preflight recovery on a bounded cadence; no second send is required. Otherwise follow the named recovery (blocker: closeout recovery blocked [open_cycle]: missing proof: open cycle must finish, be replayed, or be explicitly queued behind; recommended: captured response baseline no longer matches current document for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md. If an editor has this document open, reattach it so the authority re-bootstraps from the editor's text: `agent-doc admin reload-lib` (or reopen the document tab); do not run `reset --from-current` while the editor shows text the authority lacks, because it saves the authority over the editor. With no editor attached, rebuild the cold recovery projection without clearing session state: `agent-doc reset --from-current --preserve-session /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md`; resume durable checkpoint cycle=cycle-1792824204227 phase=write_applied capture_id=cycle-1792824204227; the canonical write is already applied; retained commit recovery owns the terminal boundary; do not resubmit finalize or write for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md). steering_delivered=0 sent=explicit ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=run_recovery_command recovery_command=agent-doc admin reload-lib

> **Queue prompt:** The agent:loop is getting items sent to it, but the items should have been sent into `agent:run`.

> **Queue prompt:** The queue is full of items with ☑️. Are there complete? Why are they not struck?

> **Queue prompt:** tmux pane auto-sync not working in this agent-doc project instance

> **Queue prompt:** [#hj2r]: verified

> **Queue prompt:** Integrate all ☑️ items.

> **Queue prompt:** The backlog items are marked with 🚧 and is out of sync. The work has been completed in the queue.

> **Queue prompt:** backlog/queue/icebox items should always backlink to todo items so the todo items can be cleared once complete. This may mean the todo item(s) block the queue/backlog/icebox item(s).

> **Queue prompt:** Read the feedback from the JetBrains agent-doc plugin submission. Fix any issues.

> **Queue prompt:** The queue items in sdk-demo.md are out of sync. Will the agent-doc causes be fixed in the next version? If not, fix agent-doc causes.

> **Queue prompt:** fix nondeterministic agent-doc-controller-io process::tests singleton_launch_claim_rejects_concurrent_holder — failed once in 15 full parallel crate runs during the retained-gate flake baseline (2026-10-26); find the shared state or timing it depends on

> **Queue prompt:** cleanup the agent:loop + agent:run items.

The queue cleanup is done. Every ☑️ row in `agent:run` (Queue: Main) whose worker reported back is quoted above. Each one is closed, and its reported claim is retired.

- **Earlier handbacks:** the GH #372 batch, the sdk-demo, Grok, and lazily reports, the tmux template and Done-rename work, the JetBrains feedback and feed checks, the backlog/Todo backlink and queue-routing fixes, and the `Integrate all ☑️ items` pass were integrated and shipped in v0.35.552, as the 03:27 closeout recorded.
- **Today's handbacks:** they are integrated on `main` and in v0.35.554.
  - The singleton launch-claim flake is `1a6bfb4fa7`. Its cause was a fork-inherited socket handle held until the child exec'd. Neighbour stress went from 2 failures in 100 runs to 0 in 100.
  - The gate-flake and merge-pairing rows were echoed by the previous closeout. Preflight strikes them.

Five rows stay open:

- **`do [#grokqueuesubagents]`:** its backlog item is closed with `--done` in this closeout.
- **`do [#release-promotion-concurrency]`:** its backlog item was already archived. The orphaned row is cleared with `queue consume --id` right after this closeout.
- **`do [#jb263-feed-verify]` (two rows):** these point at your open IntelliJ 2026.3 todo. They stay until you confirm the live update. You can delete the duplicate row you added. The other row keeps the single claim.
- **`🔄 release + publish`:** this is the v0.35.554 release now in its Release workflow. It closes when publication is verified.
- **`agent:loop` (Run: Issues):** loop rows close only through their own pass (GH #361/#364), so I did not strike them. `📥 do [#layoutunprovenliveverify]` is in review, waiting on your `#layoutliveverify-check`. The onboarding item is real pending work.

Still running: `subagent:ghostflake` and `subagent:socketexitflake`, two more controller-test flakes found during the singleton baseline.

### Re: v0.35.554 cancelled; fenced-marker queue regression — opus-5 · 2026-10-26T11:22-04:00

**v0.35.554 is cancelled.** At your choice, I cancelled its Release run mid-publish. The run had already finished three things:

- the editor packages, with JetBrains 0.2.828/0.2.829/0.2.830 uploaded to Marketplace
- the two Linux gnu builds
- the stable-channel promotion for those two targets

So `stable-x86_64-unknown-linux-gnu` and `stable-aarch64-unknown-linux-gnu` pointed at 0.35.554. I rolled both back to 0.35.553 through the promote-or-roll-back workflow. Runs `40346945712` and `40346950773` succeeded, and I re-read both pointers afterwards. All six target channels, GitHub Latest, PyPI, and Homebrew are on 0.35.553. v0.35.554 stays a GitHub pre-release. The fix will ship as v0.35.555.

**The regression.** One of the pasted sdk-demo reports in `agent:run` Main has a fenced code block. A line inside it is exactly `<!-- agent:run priority subagents -->`. Some component scanner treats that line as a real queue opener.

The 11:03 cleanup closeout quoted 26 finished rows, with these results:

- **Rows after the fenced marker** struck correctly.
- **Rows before it** did not strike. They now show 🔄, and their claims were retired.
- **Two rows already struck came back as ☑️:** `release-promotion-concurrency`, which `queue consume --id` had struck, and `lazily_block_reason`.

The steering hook then offered the resurrected `🔄 In sdk-demo.md…` row for a new subagent. I did not dispatch it, or any other resurrected row, because that would redo finished work. Today's suspects are `5fe17385b5` (shared opener rule), `fc9c0be086` (cross-queue projection), and `d8eb3ac0ba` (id/occurrence pairing).

The fenced-marker investigation worker is finding the cause and fixing it. Its own worktree is `fix/fenced-marker-queue-scan`, and it may not write to this document. It also owns your new question about `agent:run` and `agent:loop` still being out of sync. Until it lands, I'm not doing any more strikes or cleanup in this document.

**Corrections to earlier responses.** I referred to `#layoutliveverify-check` and `#claimrowmigrate-check`. agent-doc filed those todos under their backlog ids instead, `#layoutunprovenliveverify` and `#claimrowmigrate`. The 10:12 response heading also said 10:53 by mistake.

Still running: the tmux auto-sync delay worker and the two controller-test flake workers.

### Re: tmux pane auto-sync delay — opus-5 · 2026-10-26T11:52-04:00

> **Queue prompt:** tmux pane auto-sync has a large delay. Is this due to the upgrade being installed? Either way, this should be fixed.

No, the installs are not the cause. They are what eventually synced the panes. On IntelliJ 2026.3 and Rider 2026.2, switching tabs never triggered a tmux sync. A tab switch reached tmux only at the next install, Run Agent Doc, or split-layout change. This is now fixed in `9f7591405b`, which is installed. It takes effect after you restart both IDEs.

**Evidence, all from logs:**

- **The controller is fast.** Once it receives a layout, tmux matches it within about a second.
- **Every automatic sync followed an install.** Today's syncs at 14:21:17Z, 14:26:42Z, and 15:02:37Z each landed exactly when the plugin finished reloading the native library after an install. The plugin re-sends the layout after every reload, so each install looked like the moment sync caught up.
- **The real delays:**
  - The 12:49Z switch to `monsterrodholders.md` reached tmux at 13:09Z, 20 minutes later, at the next install.
  - The 15:05:50Z switch to `sdk-demo.md` arrived only when you ran Run Agent Doc at 15:06:19Z.
- **The tab-switch handler never runs in the modular plugin.** `[layout-sync] selection file=` appears 39 times in the 2026.1 IDE log and 0 times in the 2026.2 and 2026.3 logs.

**Cause:** The 2026.2 and 2026.3 builds use a modular plugin package. Since Oct 9, its build has generated a startup file that strips the tab-switch, split-focus, and activation hooks. The Oct 24 change `69e2426089` correctly kept the new local editor bridge from taking over layout, but it assumed those old hooks still covered a local IDE.

**Fix:**

- The generated startup file keeps those hooks, and the build fails if any of them goes missing.
- A Remote Dev client that owns the layout still blocks this path, so Remote Dev behaves as before.
- New test: `ModularLifecycleLocalSelectionIngressTest` fails 3/3 on the old build and passes with the fix.
- Spec: `editors/SPEC.md` §4 now requires every package to keep a live automatic sync trigger. A re-sync that happens only as a side effect of an install or Run does not count.

**Verification:** The worker's run passed 71 modular-plugin tests plus `make jetbrains-262-check` and `make jetbrains-263-check`. I bumped the package generations to JetBrains 2026.2 0.2.831 and 2026.3 0.2.832, and the coordinator's `make check-fast` passed.

Until you restart, Sync Tmux Panes or Run Agent Doc syncs immediately. After restarting, run the `#autosync-restart-verify` check.

### Re: fenced-marker regression, ghost and socket flakes — opus-5 · 2026-10-26T11:47-04:00

> **Queue prompt:** The agent:run + agent:loop in this document are still out of sync? There are many items in ☑️ state which have been around before our last release.

The cause was not the fenced block, and it did not come from today's commits. A rule added on 2026-10-22 (`059dda5846`) made any marker-projection pass reopen a struck row if a report receipt or a "Subagent completed" cell for it still existed. The 11:03 cleanup strikes did land. Preflight struck 24 rows at 15:06:35Z, and the next projection reopened every one that still had a cell (`🔄`) or a receipt (`☑️`). The 🔄 rows lined up with the fence only because the cells still in the exchange belong to the rows above it.

The fix is integrated as `6439b1d93d` and installed:

- **Strikes stay struck.** A receipt, a completion cell, or a reported or lost attempt no longer reopens a struck row. Only a worker that is still dispatching or running does. All three projection paths now use one predicate.
- **Fenced markers are now plain content everywhere.** The document-cell merge was the one component scanner that did not skip fenced code. A fenced `<!-- /agent:run -->` ended the queue early, and pasted multi-line items were emitted twice. There is now one shared fence-aware scan.

Six regressions cover this document's exact shape, and the affected crates ran 2,533 tests with 0 failures.

On Main versus Issues: a strike in `agent:run` deliberately never strikes the `agent:loop` copy, because loop rows close only through their own pass (GH #361/#364). Once Main's claims ended, the loop copies correctly repainted as `📥`. The `🔄 release + publish` in both queues is a stale completion cell from the old v0.35.551 release.

Two follow-ups are dispatched:

- `subagent:cellretire`: retire completion cells and receipts on every strike path.
- `subagent:rowidentity`: key queue-row identity by the work text instead of the owner tag.

Next I re-strike the finished rows. Their echoes already exist, so preflight's residue strike picks them up, and the orphaned `release-promotion-concurrency` row goes through `queue consume --id` again.

> **Queue prompt:** fix nondeterministic agent-doc-controller-io rpc::tests recording_an_editor_exit_reaps_the_relay_ghost_member — failed 6 of 60 full parallel crate runs in the singleton-flake baseline (2026-10-26), panic "a live editor registers a replica"

This is integrated as `5bc4f16446`. The test used the test binary's own pid as its editor. Sibling exit tests mark that pid as exited in the shared liveness record, so registering a replica was refused. The test now starts its own child editor process and reaps it only after the shared record is cleared.

The natural flake did not reproduce, with 0 failures in 342 runs. A deterministic control (marking the pid exited first) panics every time on the old code and passes with the fix. After the rebase, the controller crate passed: 827 passed, 0 failed.

> **Queue prompt:** fix nondeterministic agent-doc-controller-io rpc::tests controller_exits_when_its_public_socket_is_unlinked_under_a_surviving_root — failed 1 of 30 full parallel crate runs (2026-10-26)

This is integrated as `1e4156719f`. The test deletes its controller's `.agent-doc` and recreates `state.db`. A controller writer from another test, its exit watcher, was still committing on the deleted file. It deleted the new file's `state.db-journal` by name, which raised SQLITE_IOERR_DELETE_NOENT (5898).

The fixture now builds the ledger in a staging directory and renames it into place. A targeted probe went from 846 failures in 2,000 runs to 0 in 2,000. The cross-test exit-watcher writes are their own follow-up, `subagent:exitwatcheriso`.

GH #376 is fixed in `d67ad7d6ac`, which is now going through `check-fast` and install. A zero-column Run Agent Doc now uses, or waits for, the JetBrains Client's measured layout.

### Re: GH #376 measured-layout route — opus-5 · 2026-10-26T11:51-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/376

[GH #376](https://github.com/btakita/agent-doc/issues/376) is fixed in `82d790fe1d`, installed, and closed.

The reported error message came from a controller that started at 09:28, before the #373 fix. That reporter ran 0.35.553 with plugin 0.2.827 on a Remote Dev backend.

A real defect was still on main. On a modular Remote Dev backend, the JetBrains Client measures the layout, and that measurement wins. So Run Agent Doc sends a zero-column route, and the measured columns reach the controller separately, sometimes about a second later. The route never used that measurement, even when it was already there. Without a live pane, the route refused with a "frontend plugin missing" remedy, which was wrong.

What the fix does:

- **Uses the measurement.** Before the #373 fallback, a zero-column route takes the newest measured layout from the same IDE install.
- **Waits only for a measurement that is still coming.** The wait wakes when the measurement lands, and the route's existing `--wait-for-ready` budget bounds it. It adds no polling and no new timeout.
- **Skips the wait for a live pane.** That route just focuses the pane.
- **Never guesses.** A measurement from another IDE install is never used. If nothing arrives in time, the #373 path runs unchanged.
- **The plugin now says when to wait.** It sends `layout_authority = "frontend_measured"` so the controller knows a measurement is coming.

Plugin generations are classic 0.2.833, 2026.2 0.2.834, and 2026.3 0.2.835.

All three new controller regressions fail with the fix disabled. One covers this report's order, with the route sent first and the measurement after it. After the rebase, the editor-surface and controller crates passed 991 tests, and `make check-fast` passed.

One limitation: a plugin older than 0.2.833 does not send the new field. Its route still uses a measurement that has already arrived, but it will not wait for one in flight. Update the plugin on the Remote Dev backend.

### Re: exit watcher scoped to its own root — opus-5 · 2026-10-26T12:00-04:00

> **Queue prompt:** isolate the controller exit watcher from other tests' editor pids: every in-process test controller reaps dead pids registered by other tests' documents in the process-wide liveness state and writes their exit records into its own root's state.db (found by the socket-unlink flake fix, 2026-10-26)

The fix is integrated as `b2ffc4e907` and installed. The controller exit watcher now reaps a dead editor pid only when one of that pid's registered documents has the controller's root as its nearest project root. It also records exits only into that root's own `state.db`.

The same cross-root write was in three places:

- the poll tick
- exit-record retention
- startup hydration, which now recovers only dead pids opened by its own journal

The spec clause is `#exitwatcherrootscope`.

In practice only tests were affected. A production process runs one controller for one root, and both editor plugins send a document registration with every open.

There are two new regressions. In the first, controller B's tick runs while A's dead pid is still listed, and B never creates a `state.db`. The second covers hydration. With the filters removed, both fail. After the rebase, the controller-io lib passed 834 tests twice, and `make check-fast` passed.

The worker hit an unrelated flake once in 5 lib runs, `plugin_publications_project_into_the_publishing_clients_navigation_lane`. It is queued and running as `subagent:laneplaneflake`. Your strikethrough report is running as `subagent:strikeformat`. The row body starts with an inline-code backtick and holds unbalanced code spans, so the `~~…~~` delimiters collide with them.

### Re: v0.35.555 and coordinator handoff — opus-5 · 2026-10-26T13:51-04:00

> **Chat prompt (#chatprompt:26bba7ddac60):** Is the new release out for Coder to install?

> **Chat prompt (#chatprompt:37d938926795):** Is 555 published for Coder?

**v0.35.555** (`f11ad5932f`) is out for Coder's CLI. Its release gate passed: `make check` ran 11,553 tests. The x86_64 Linux gnu and musl stable channels already point to 0.35.555, so `agent-doc upgrade` gets it. To install by script, pin the version with `AGENT_DOC_VERSION=0.35.555`, because `install.sh` follows Latest. Release run `40368074628` was still building macOS and Windows at handoff. Until it finishes, GitHub still marks 0.35.555 as a pre-release and PyPI is on 0.35.553.

On Coder, run `agent-doc plugin install jetbrains` on the backend. The Client and the backend both need 2026.3 plugin 0.2.835, then a restart. That fixes #377 and brings the #376 layout wait.

v0.35.554 was never published. It stays a pre-release, and 0.35.555 replaces it.

**Coordinator handoff (state at 13:51).**

- **Queue dispatch for this document is PAUSED** with `admin queue pause`, receipt 7. I paused it so the `issues` loop's pass 6 could not run 15 resurrected rows.
- **Loop-resurrection cause:** it was the loop's own pass restore, not anything after v0.35.553. The fix is `ddd960d632`, which is installed: loops never receive inserted items, and they restore only rows that are still struck in that loop.
- **Operator action:** in `## Run: Issues`, delete every row except the Watch row. Do not strike them. Then resume with `agent-doc admin queue resume tasks/agent-doc/agent-doc-bugs.md --observed-generation <gen>`, using the gen from `agent-doc admin inspect`.
- **On main and installed:** everything in v0.35.555, plus `ddd960d632`.
- **Ready to integrate, in this order.** Each needs a rebase, `check-fast`, a push, then one `make install`.
  - `fix/queue-strike-format` is already rebased, and `check-fast` passed with 11,567 tests.
  - The others are `feat/any-heading-component-prefix`, `docs/onboarding-quickstart`, `feat/queue-marker-legend`, `fix/doctor-fresh-doc-healthy`, and `fix/start-shell-readiness`.
- **Held for your naming decision:** `feat/skill-no-arg-pane-document` adds `agent-doc resolve-pane-document`. Confirm or rename the command.
- **GH #377:** the same bug as #375, with no code change needed. Close it when 0.35.555 becomes Latest.
- **Not yet dispatched:**
  - the two follow-ups after strike-format lands, ending a Reported claim on every strike path and the one-time stale-cell repair
  - `doctor` exiting 0, to build on the doctor branch
  - the load flake in `retained_capture_repairs_legacy_welded_queue_close_before_validation`
  - the route-owned and fan-out panes, which still type their commands like `start` used to (suggested by the start-shell-readiness worker)
- **Released back to pending:** the Marketplace publishing item. It needs your accounts.
- **Your live check:** `#layoutunprovenliveverify`.

### Re: restoring agent:todo — opus-5 · 2026-10-26T14:08-04:00

> **Chat prompt (#chatprompt:56b1e180781c):** Can I clear the agent:loop?

> **Chat prompt (#chatprompt:c49c7d43312f):** I'm getting confused and am adding things into it.

> **Chat prompt (#chatprompt:28d31809a03b):** I thought agent-doc supports the header prefix

> **Chat prompt (#chatprompt:1099dfb245f8):** Can we fix this already. I've requested it to be fixed 3 times

> **Chat prompt (#chatprompt:c1c9bca05b51):** Do this in a subagent if work is needed

> **Chat prompt (#chatprompt:3571ab3b7b93):** fixed

> **Chat prompt (#chatprompt:7069dee140c9):** actually, my todo items were deleted

> **Chat prompt (#chatprompt:735ee16cdb34):** I think agent-doc collapsed agent:todo into agent:loop

> **Chat prompt (#chatprompt:68197c136733):** Please restore agent:todo

> **Chat prompt (#chatprompt:a768ac89a291):** with the items that were in agent:todo

> **Chat prompt (#chatprompt:ecce9f51e753):** and fix the agent-doc causes in a subagent

This closeout restores the `agent:todo` component with all six items from the last commit, `48796d91bd`, each under its original id and text. They are `#ahfe`, `#ctg7`, `#coder-remote-install-02805`, `#jb263-feed-verify`, `#layoutunprovenliveverify`, and `#autosync-restart-verify`. All six are restored unchecked, as they were.

You reported that the tmux panes are syncing. If that covers `#layoutunprovenliveverify` and `#autosync-restart-verify`, check those two off.

**The header prefix is supported.** `568aa7102e` was installed at about 13:55, and `## Queue: Main <!-- agent:run … -->` parses. The parse error was a separate problem. The loop opener was being closed by `<!-- /agent:todo -->`, so the Todo opener, its items, and the loop's own close marker had all disappeared.

**Cause:** A background subagent on `fix/todo-collapsed-into-loop` is finding which actor removed the Todo region, your editor buffer or a binary write, and which code path did it. It will fix agent-doc so that rewriting one component can never absorb or delete another component's markers or body. A component must end only at its own close tag, and a mismatched close must fail closed.

**Rule for the two queues.** `agent:loop` holds only the hourly Watch row. Everything else goes at the bottom of `## Queue: Main`.

Queue dispatch for this document is still paused (receipt 7), and I resume it after this closeout lands.

### Re: v0.35.555 published; closing integrated rows — opus-5 · 2026-10-26T14:34-04:00

> **Chat prompt (#chatprompt:e5c924aa461d):** Is 555 ready for Coder to install?

**Yes.** v0.35.555 is the published Latest release, PyPI serves 0.35.555, and release run `40368074628` succeeded. On Coder:

1. Run `agent-doc upgrade`.
2. Run `agent-doc plugin install jetbrains` on the backend.
3. Make sure the JetBrains Client and the backend both have 2026.3 plugin 0.2.835, then restart both.

GH #377 is closed. Queue dispatch is resumed (receipt 8).

> **Queue prompt:** release + publish

> **Queue prompt:** release + publish: Issue 376 fix

> **Queue prompt:** The strikethrough in the first current queue item is malformed.

> **Queue prompt:** fix nondeterministic agent-doc-controller-io rpc::tests plugin_publications_project_into_the_publishing_clients_navigation_lane — await_pane_layout_plane_seen(rider_frame.plane_version, Duration::ZERO) returned false in 1 of 5 full lib runs (rpc.rs:43910), found during the exit-watcher root-scope fix, 2026-10-26

> **Queue prompt:** retire a queue row's subagent completion cell and report receipt on every strike path (preflight residue strike, orphan `queue consume --id`, controller completion), not only closeout `queue consume`, so a stale cell can never paint 🔄 on a later row with the same text (found by the fenced-marker fix, 2026-10-26)

> **Queue prompt:** key merge/overlay queue-row identity by the work identity, not the first `[#...]` token: a claimed row`s owner tag `[#subagent:x#N]` makes it a different node from its bare struck copy, so a stale editor copy with a different marker can resurrect a struck row (found by the fenced-marker fix, 2026-10-26)

> **Queue prompt:** agent-doc should support the header being on the same line as the open tag: e.g. `## Queue: Main ` as a prefix to the `agent:run` open tag.

> **Queue prompt:** Items are being added by this agent + agent-doc into `agent:loop` in error. The items should go into `agent:run`.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/377

> **Queue prompt:** `agent:loop` in this document should only have `- 📥 Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.`. agent-doc is treating `agent:loop` as the kitchen sink which is wrong. The items belong in `agent:run`.

> **Queue prompt:** [#jb263-feed-verify]: done?

> **Queue prompt:** [#layoutunprovenliveverify]: [#autosync-restart-verify]: The tmux panes are syncing.

The rows quoted above are closed:

- **Releases:** v0.35.555 satisfies both `release + publish` rows.
- **Integrated on main:** strike format `0e796855c4`, lane flake `c210b89833`, cell retirement `187d7821f8`, row identity `1323f54c87`, any-heading prefix `568aa7102e`, and loop misroute `ddd960d632`. The loop now holds only the Watch row.
- **GH #377:** closed.
- **`#layoutunprovenliveverify` and `#autosync-restart-verify`:** your confirmation that the tmux panes sync closes both todos.

**`[#jb263-feed-verify]: done?`:** that is your IntelliJ 2026.3 check, and only you can confirm it. If the "Cannot update plugin" warning has cleared on 2026.3 at 0.2.835, check that todo off.

**The todo collapse was an editor action.** A single 4,596-byte delete at 17:54:52Z, logged right after JetBrains `[Next Edit] Document was changed`, removed the todo region. agent-doc's fault was adopting that broken buffer as canonical after the install recycle. The fix is `d8794daf9a`, pushed and installing:

- It never adopts a buffer with broken markers, and it shows an `⚠ agent-doc:` diagnostic that names the broken markers and how to repair them.
- Merge refuses documents that do not parse.
- A component ends only at its own close tag.

Three gaps it left are queued, including one where a running controller can still revert your text.

**Still to integrate:** onboarding `3f33ab1f96`, marker legend `bd0847efca`, doctor fresh-doc `9f0ebc1e16`, and start shell-readiness `9317ccc50c`. The no-arg skill (`resolve-pane-document`) is held for your naming decision.

### Re: #378 fixed, #379 triaged, jb263 queue row — opus-5 · 2026-10-26T15:17-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/378

[GH #378](https://github.com/btakita/agent-doc/issues/378) is fixed in `ce2eeb0fae`, installed, and closed. The reported error was already fixed on main: the reporter ran 0.35.553, before GH #374 and the any-heading change.

The worker found two real bugs:

- **A loop above the run:** when an `agent:loop` sits above the `agent:run`, `--backlog-add` never reached the run. The `go` check, the id comparison, and the baseline copy all read the loop. All three now use one target picker.
- **No queue at all:** a document with no queue failed every controller commit with "component `queue` was not found".

Two follow-ups are queued:

- About 40 call sites that read "the first queue".
- The `queue: start` front-matter sync, which writes onto the loop marker.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/379

[GH #379](https://github.com/btakita/agent-doc/issues/379) is not a new bug. The reporter still ran CLI 0.35.553 with plugin 0.2.826/0.2.827 at 18:47Z, after v0.35.555 was published at 17:23Z. The issue now has upgrade and restart steps, and the `ops.log` lines that tell fixed from not-fixed. It stays open until the reporter confirms.

> **Queue prompt:** do [#jb263-feed-verify]

The worker for that row confirmed the `#jb263-feed-verify` todo exists. What remains is your live IntelliJ 2026.3 check, which stays open as the todo, so this queue row is closed.

**Since v0.35.555, on main and installed** (not yet released):

- `d8794daf9a`: an editor buffer with broken markers is never adopted, and a diagnostic shows the problem.
- `655eacb624`: a live edit that removes a marker is fenced instead of reverted.
- `ce2eeb0fae`: GH #378.

The strike-ends-reported-claim fix (`fb54c8f6ab`) is integrating.

**Still running:** the stale-completion-cell repair command, and the Idea attach error, which now has your `docs.md` repro.

**Decision for you:** should a backlog strike also close claimed free-text rows? Today it skips rows that show `☑️ [#owner#N]`.

### Re: updated coordinator handoff — opus-5 · 2026-10-26T15:26-04:00

> **Chat prompt (#chatprompt:5e96560b27d6):** Can I clear this session?

Yes, you can clear it. This replaces the 13:51 handoff.

**Released:** v0.35.555 is the published Latest, on GitHub, PyPI, and every stable channel. GH #377 and #378 are closed. GH #379 is not a new bug; the reporter has upgrade steps, so close it once they confirm.

**On main and installed** since 0.35.555, not yet released:

- `ddd960d632`: loops never receive inserted items or resurrect removed rows.
- `0e796855c4`: a single strike formatter.
- `568aa7102e`: any heading may prefix a component opener.
- `d8794daf9a`: a broken-marker buffer is never adopted.
- `655eacb624`: a live marker removal is fenced, not reverted.
- `ce2eeb0fae`: GH #378.
- `fb54c8f6ab`: strikes end finished claims.

**Integrating at handoff:** `fix/attach-ambiguous-retained-after-fence` (`369da0c3e0`, plugins 0.2.836/0.2.837/0.2.838). A detached `check-fast`, push, and install was running, with its log in `~/work/btakita/agent-doc-wt/ia-check.log`. It answers your `docs.md` item, so close that item once `INSTALL_EXIT=0` appears. Restart the IDEs to load the plugin.

**Ready but held for your naming decision:**

- `feat/skill-no-arg-pane-document`: `agent-doc resolve-pane-document` for bare `/agent-doc`.
- `feat/repair-stale-completion-evidence`: `agent-doc queue repair-stale-completion`. Dry run first, then `--key 340307c5605e3f613ff24c04f798ee2c67df4551a40e926dae1e41857d098a84` for the old `release + publish` cell.

**Still to integrate, in order:**

1. `docs/onboarding-quickstart`
2. `feat/queue-marker-legend`
3. `fix/doctor-fresh-doc-healthy`
4. `fix/start-shell-readiness`

**Queued follow-ups** for the next session to dispatch:

- `doctor` exiting 0, built on the doctor branch.
- The route-owned and fan-out pane launch, built on start-shell.
- The load flake `retained_capture_repairs_legacy_welded_queue_close_before_validation`.
- Todo-collapse gaps 1 and 3.
- The two attach follow-ups, built on `369da0c3e0`.
- The ~40 "first queue" call sites.
- The `queue: start` loop-marker sync.

**Decisions for you:**

- The two command names above.
- Whether a backlog strike should also close claimed free-text rows.
- `#jb263-feed-verify`: your 2026.3 check.

### Re: queue integration batch and subagent dispatch — opus-5 · 2026-10-26T15:45-04:00

> **Queue prompt:** end a Reported queue claim on every strike path (orphan-id strike, controller projection, preflight done-id and backlog strikes), not only at the next closeout prune, so a later identical row can never pick up ☑️ from the claim ledger (found by the completion-cell retirement fix, 2026-10-26)

On main as `2ebf740bcc` (`#strikeretires`), and installed. Every strike path now ends the struck row's Reported/Lost claim attempt, so a later identical row starts at 📥.

> **Queue prompt:** We need to work on making agent-doc easy to onboard. Sanat, a coworker is confused about how to get started.

Integrated as `0e5c3249bb`. The README now opens with a tested **Getting started** path (install, `agent-doc init`, first turn from an editor, a harness, or a terminal). The commit also fixes four first-run failures the pass found. Sanat can start from the README's Getting started section.

> **Queue prompt:** `agent-doc doctor` reports outcome=blocked (queue_continuation) on a brand-new document with no queue; it should report healthy (found by the onboarding pass, 2026-10-26)

Integrated as `9258237699`: a fresh document with no queue now reports healthy. The exit-code follow-up was dispatched this cycle (see below).

> **Queue prompt:** `agent-doc start` types its command into a new tmux pane before the shell is ready (zsh with no ~/.zshrc lost the leading `/`); wait for shell readiness (found by the onboarding pass, 2026-10-26)

Integrated as `44a20db27c`: the re-exec'd `start` now runs as the pane's own process, so a slow shell can no longer swallow the leading `/`. The route-owned and fan-out panes are the dispatched follow-up.

> **Queue prompt:** What does `🔄` mean? Is it documented? The prefixes meanings should be in the help command response and in each document.

Integrated as `338b8b3805`. `🔄` means **needs reconcile**: the row has finished-work evidence but no current claim, and it is not dispatched again. One generated legend (`agent_doc_queue_icons::legend`) now feeds `agent-doc queue --help`, `agent-doc queue legend`, the book page `docs/guide/queue-markers.md`, the README's Queue markers table, and a one-line legend at the end of each document's `agent:status` (`queue-legend=off` hides it). A marker without a legend entry does not compile.

> **Queue prompt:** an already-running controller that receives an operator edit removing a component marker still restores the last valid canonical and pushes it back into the editor, reverting operator text; keep the editor text, refuse adoption, and show the ⚠ agent-doc marker diagnostic instead (gap 2 of the todo-collapse fix d8794daf9a, 2026-10-26)

On main as `655eacb624` and installed. A live marker-removing edit is fenced: the editor text stays, the last valid canonical is never pushed back, and the ⚠ marker diagnostic shows.

> **Queue prompt:** Idea error: Agent Doc could not attach the open editor replica for agent-doc-bugs.md. Cause: ambiguous-retained-projection. The editor buffer and controller canonical have diverged from their last settled ancestor. Copy any unsaved text, close this editor tab, run `agent-doc repair <file>` while it is detached, and reopen it. The editor buffer and disk were left unchanged.

On main as `f155fe0076` and installed (`INSTALL_EXIT=0`). A canonical that only lags the buffer, or only adds a claim projection, now re-attaches without the ambiguity hold. Restart the IDE to load JetBrains 0.2.838: the live IDE still runs the superseded 0.2.835 jar, and only a restart can swap it.

> **Queue prompt:** I deleted a queue item in docs.md and added some of the deleted queue item prompt into an existing queue item: Agent Doc could not attach the open editor replica for agent-doc-bugs.md. Cause: ambiguous-retained-projection. The editor buffer and controller canonical have diverged from their last settled ancestor. Copy any unsaved text, close this editor tab, run `agent-doc repair <file>` while it is detached, and reopen it. The editor buffer and disk were left unchanged

Same cause and fix as the item above: `f155fe0076`, installed. Your edit made the buffer lead the canonical, which the old attach treated as ambiguous. The fixed attach re-attaches it. It needs the IDE restart for the 0.2.838 plugin.

> **Queue prompt:** release + publish

**Deferred:** this release waits until the seven subagent fixes dispatched below land. It is the same request as the release row at the end of the queue, and both will be answered by one release. The 🔄 marker is the leftover completion cell from the v0.35.551 release handback.

> **Queue prompt:** release + publish: Include fixes for Issue 378-379 and the current 📥 items.

**Deferred:** this waits for the seven 📥 fixes below, which are now 🚧 running. GH #378 is already on main as `ce2eeb0fae`. GH #379 had no code change: its reporter has the upgrade steps. Once the workers report, I integrate them in one batch, run one `make check`, and cut the release from the coordinator. Subagents are not allowed to release, so this row is not dispatched to one.

> **Queue prompt:** one-time repair: retire stale subagent completion cells and report receipts left behind by strikes made before the completion-cell retirement fix, or made by hand in the editor, in this document and other session documents (found by the completion-cell retirement fix, 2026-10-26)

**Deferred:** the branch `feat/repair-stale-completion-evidence` (`66325a2db1`) is ready and adds `agent-doc queue repair-stale-completion`. It waits only for your decision on the command name.

> **Queue prompt:** In the agent terminal, running the `agent-doc` skill (`/agent-doc` on Claude + Grok + Opencode and `agent-doc` in Codex) without any arguments should run agent-doc on the claimed file for the tmux pane.

**Deferred:** the branch `feat/skill-no-arg-pane-document` (`81c3de55d5`) is ready and adds `agent-doc resolve-pane-document`. It waits only for your decision on the command name.

**Integrated this cycle.** I cherry-picked the four ready branches onto `f155fe0076`. README and SPEC conflicted only in prose. I kept the onboarding Getting started layout plus the legend's Queue markers section, and kept HEAD's newer `#strikeretires` sentence plus the new `#queuemarkerlegend` bullet. Then `make check-fast` passed (`CHECK_EXIT=0`), the batch was pushed as `f155fe0076..44a20db27c`, and `make install` finished (`INSTALL_EXIT=0`). CI for `f155fe0076` and `2ebf740bcc` was still running at closeout, and I did not wait for it.

**Dispatched this cycle** (each claimed at generation 1, 🚧 running in its own worktree under `~/work/btakita/agent-doc-wt/`):

| Worker | Branch | Base |
|---|---|---|
| `doctorexit` | `fix/doctor-exit-nonzero` | doctor-healthy `9f0ebc1e16` |
| `routelaunch` | `fix/route-fanout-launch-as-pane-process` | start-shell `9317ccc50c` |
| `retainedflake` | `fix/retained-capture-welded-close-flake` | `f155fe0076` |
| `jbmarkercheck` | `fix/jb-detached-op-marker-check` | `f155fe0076` |
| `coldstartfence` | `fix/cold-start-marker-fence` | `f155fe0076` |
| `attachsimworld` | `test/attach-relayhub-simworld` | `f155fe0076` |
| `attachrowmerge` | `fix/attach-row-aware-merge` | `f155fe0076` |

**Review gate.** You checked `#layoutunprovenliveverify`, so this closeout resolves that gate.

**Decisions for you:** the two command names above, and whether a backlog strike should also close claimed free-text rows. Separately, restart IntelliJ IDEA 2026.3 so it loads plugin 0.2.838.

The exchange is 1011 lines. Compacting it would help, but only when you ask, because the queue is still draining.

### Re: release v0.35.556 — opus-5 · 2026-10-26T17:13-04:00

> **Queue prompt:** release + publish: Include fixes for Issue 378-379 and the current 📥 items.

**v0.35.556 is released and published.** Release commit `5cbfdbbc4c`, tag `v0.35.556`, is `origin/main`. It is the GitHub Latest release, PyPI serves 0.35.556, the stable target channels were promoted, and `make release` finished its local `install-full` (`RELEASE_EXIT=0`). Full `make check` passed on the version-projected tree first. Editor packages: JetBrains classic, 2026.2, and 2026.3 are 0.2.839, 0.2.841, and 0.2.840; VS Code stays 0.2.83.

- **GH #378** is in the release (`ce2eeb0fae`) and closed.
- **GH #379** needed no code change. The reporter ran 0.35.553 with plugin 0.2.827; the fixes shipped in 0.35.554/0.35.555 and are in this release too. The issue stays open until they confirm after upgrading.
- **Every 📥 item queued before the release is in it:** the seven dispatched fixes, the `autofix` follow-up, and the closeout-strike fix described below.

Two release-gate notes: `make editor-generation-bump` skips the JetBrains 2026.2 package, so I bumped it with `check_plugin_versions.py --bump "JetBrains 262"`. The first `make check` stopped at the editor parity gate until I added the 0.35.556 row to `editors/release-parity.tsv`.

> **Queue prompt:** `agent-doc doctor` exits 0 even when its outcome is blocked or recoverable; exit non-zero so scripts and CI can act on it (found by the doctor fresh-document fix, 2026-10-26)

Released in v0.35.556 (`143e8bb106`). `agent-doc doctor` exits 0 ok, 3 recoverable, 4 operator action, 5 blocked, and 1 when doctor itself fails; the report still prints for every outcome.

> **Queue prompt:** `agent-doc autofix` exits 0 even when invariants stay recoverable, operator or blocked after the plan/apply; give it the same exit status as `agent-doc doctor` (found by the doctor exit-status fix, 2026-10-26)

Released in v0.35.556 (`fae26ad2d7`). `agent-doc autofix` uses the same codes. After an `--apply` that ran a repair it re-diagnoses and exits with what remains; the report schema is now 2, with `outcome` beside `doctor_outcome`.

> **Queue prompt:** route-owned auto-start (agent-doc-route-io/src/startup.rs route.auto_start) and parallel fan-out panes (src/parallel.rs) still type their commands into a just-split pane; launch them as the pane process via agent_doc_tmux_io::launch_command_in_pane, checking route_owned_start_enter input logging and the fan-out $(passage ...) env prefix (found by the start shell-readiness fix, 2026-10-26)

Released in v0.35.556 (`18509487ac`). Route-owned auto-start and parallel fan-out panes run their command as the pane process, and the fan-out `$(passage ...)` prefix is expanded only inside the pane. The old `route_owned_start_enter` event is now `route_owned_start_launch`. One difference: a launched command gets the tmux session environment plus the launcher's `PATH`, not your shell's rc.

> **Queue prompt:** fix nondeterministic agent-doc-document-realtime-io tests::retained_capture_repairs_legacy_welded_queue_close_before_validation — waits on a background delivery thread and failed under check-fast load once (2026-10-26, any-heading worker); find what it races, do not rerun to green

Released in v0.35.556 (`187067ef1a`). The race was in the test: it expected one settle call to succeed, which only happened if the simulated editor projected inside a window of about 20ms. The test now settles, runs the editor delivery to completion, and settles again. No production change.

> **Queue prompt:** JetBrains plugin: run the component-marker check on the buffer before recording a detached-editor op, so an edit that deletes a component open/close marker warns the operator at edit time (the detached op path carries only offsets and lengths) (gap 1 of the todo-collapse fix, 2026-10-26)

Released in v0.35.556 (`55bb175b0a`; JetBrains classic/262/263 0.2.839/0.2.841/0.2.840). Before recording an edit, the plugin runs the shared marker check, so deleting a component marker shows the `⚠ agent-doc:` warning at edit time. Your text is never changed. Restart the IDE to load it.

> **Queue prompt:** cold controller start with no editor handover loads the document from disk even when disk has broken component markers; fence it against the last snapshot or HEAD and show the marker diagnostic (gap 3 of the todo-collapse fix, 2026-10-26)

Released in v0.35.556 (`ef3a09a34f`). A cold controller start no longer adopts disk text whose markers broke since the snapshot or `HEAD`. It fences the text like a live edit: disk is never rewritten, commits are refused, the warning shows, and a repairing edit lifts the fence. The same fence covers an out-of-band rebuild at the commit barrier.

> **Queue prompt:** relay-hub / SimWorld coverage for editor attach: the structure-quarantine-with-durable-ingest canonical lag and the library-reload-then-refused-swap re-registration sequence (found by the ambiguous-retained-projection attach fix 369da0c3e0, 2026-10-26)

Released in v0.35.556 (`3a08e5095d`). Four relay-hub tests drive the reload-then-refused-swap and structure-quarantine lag sequences through the real hub. They found no new defect, and they still pass beside the row-aware merge below.

> **Queue prompt:** editor attach still holds when canonical both lags the buffer and has a real controller addition the buffer lacks (e.g. a claim marker plus a response); add a row-aware attach merge (found by the ambiguous-retained-projection attach fix, 2026-10-26)

Released in v0.35.556 (`6c19ec64db`). When the controller lags your buffer and also added a claim marker or a response, attach keeps every buffer byte and rolls the controller's additions onto it. It still holds on a true same-row conflict.

**Closeout-strike fix (`2a8a3e6d25`, released).** My previous closeout quoted eight finished rows, and none was struck; it also consumed a `release + publish` row I had marked Deferred. Two causes. First, the loop-queue exclusion numbered components with a parser that skips the exchange's boundary marker, so after the boundary every `agent:run` Main row looked like a loop row and was never struck. Finalize, preflight, and the controller projection all shared that bug. Second, a `--review-resolve` closeout's strike had a free-text budget of 1, so it fell through to the leading free-text head without checking deferral. Both are fixed with regression tests, and this cycle's preflight on 0.35.556 struck the eight rows.

> **Queue prompt:** one-time repair: retire stale subagent completion cells and report receipts left behind by strikes made before the completion-cell retirement fix, or made by hand in the editor, in this document and other session documents (found by the completion-cell retirement fix, 2026-10-26)

**Deferred:** the branch `feat/repair-stale-completion-evidence` (`66325a2db1`) adds `agent-doc queue repair-stale-completion`. It waits only for your decision on the command name.

> **Queue prompt:** In the agent terminal, running the `agent-doc` skill (`/agent-doc` on Claude + Grok + Opencode and `agent-doc` in Codex) without any arguments should run agent-doc on the claimed file for the tmux pane.

**Deferred:** the branch `feat/skill-no-arg-pane-document` (`81c3de55d5`) adds `agent-doc resolve-pane-document`. It waits only for your decision on the command name.

**Follow-ups the workers found** (not queued yet, so you can choose):

- `agent-doc commit` refuses to commit a status update while a binary-owned subagent completion cell is in the exchange, so the status-sync step cannot finish between closeouts.
- The controller projection and the session-check guard count only live claims for a reported `☑️` row, while finalize and preflight also count reported attempts. So a reported row can be struck by a mention instead of an exact echo.
- The session-check residue guard may not exclude loop rows.
- `cold_start_preserve_pane` in the controller's supervisor replacement still types its command into a pane you kept.
- Row-aware attach: carry the claim marker forward in the claim-only case too, and pair free-text rows that have no `[#id]`.

**Decisions for you:** the two command names above. Separately, restart IntelliJ IDEA 2026.3: it still runs plugin 0.2.835, and 0.2.840 is installed.
<!-- agent:boundary:3ec64169 -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main <!-- agent:run id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
- ~~release + publish~~
- ~~end a Reported queue claim on every strike path (orphan-id strike, controller projection, preflight done-id and backlog strikes), not only at the next closeout prune, so a later identical row can never pick up ☑️ from the claim ledger (found by the completion-cell retirement fix, 2026-10-26)~~
- ☑️ [#subagent:cellrepair#1] one-time repair: retire stale subagent completion cells and report receipts left behind by strikes made before the completion-cell retirement fix, or made by hand in the editor, in this document and other session documents (found by the completion-cell retirement fix, 2026-10-26)
- ~~We need to work on making agent-doc easy to onboard. Sanat, a coworker is confused about how to get started.~~
- ~~`agent-doc doctor` reports outcome=blocked (queue_continuation) on a brand-new document with no queue; it should report healthy (found by the onboarding pass, 2026-10-26)~~
- ☑️ [#subagent:doctorexit#1] `agent-doc doctor` exits 0 even when its outcome is blocked or recoverable; exit non-zero so scripts and CI can act on it (found by the doctor fresh-document fix, 2026-10-26)
- ☑️ [#subagent:autofixexit#1] `agent-doc autofix` exits 0 even when invariants stay recoverable, operator or blocked after the plan/apply; give it the same exit status as `agent-doc doctor` (found by the doctor exit-status fix, 2026-10-26)
- ~~`agent-doc start` types its command into a new tmux pane before the shell is ready (zsh with no ~/.zshrc lost the leading `/`); wait for shell readiness (found by the onboarding pass, 2026-10-26)~~
- ☑️ [#subagent:routelaunch#1] route-owned auto-start (agent-doc-route-io/src/startup.rs route.auto_start) and parallel fan-out panes (src/parallel.rs) still type their commands into a just-split pane; launch them as the pane process via agent_doc_tmux_io::launch_command_in_pane, checking route_owned_start_enter input logging and the fan-out $(passage ...) env prefix (found by the start shell-readiness fix, 2026-10-26)
- ~~What does `🔄` mean? Is it documented? The prefixes meanings should be in the help command response and in each document.~~
- ☑️ [#subagent:retainedflake#1] fix nondeterministic agent-doc-document-realtime-io tests::retained_capture_repairs_legacy_welded_queue_close_before_validation — waits on a background delivery thread and failed under check-fast load once (2026-10-26, any-heading worker); find what it races, do not rerun to green
- ☑️ [#subagent:noargskill#1] In the agent terminal, running the `agent-doc` skill (`/agent-doc` on Claude + Grok + Opencode and `agent-doc` in Codex) without any arguments should run agent-doc on the claimed file for the tmux pane.
- ~~an already-running controller that receives an operator edit removing a component marker still restores the last valid canonical and pushes it back into the editor, reverting operator text; keep the editor text, refuse adoption, and show the ⚠ agent-doc marker diagnostic instead (gap 2 of the todo-collapse fix d8794daf9a, 2026-10-26)~~
- ☑️ [#subagent:jbmarkercheck#1] JetBrains plugin: run the component-marker check on the buffer before recording a detached-editor op, so an edit that deletes a component open/close marker warns the operator at edit time (the detached op path carries only offsets and lengths) (gap 1 of the todo-collapse fix, 2026-10-26)
- ☑️ [#subagent:coldstartfence#1] cold controller start with no editor handover loads the document from disk even when disk has broken component markers; fence it against the last snapshot or HEAD and show the marker diagnostic (gap 3 of the todo-collapse fix, 2026-10-26)
- ~~Idea error: Agent Doc could not attach the open editor replica for agent-doc-bugs.md. Cause: ambiguous-retained-projection. The editor buffer and controller canonical have diverged from their last settled ancestor. Copy any unsaved text, close this editor tab, run `agent-doc repair <file>` while it is detached, and reopen it. The editor buffer and disk were left unchanged.~~
- ☑️ [#subagent:attachsimworld#1] relay-hub / SimWorld coverage for editor attach: the structure-quarantine-with-durable-ingest canonical lag and the library-reload-then-refused-swap re-registration sequence (found by the ambiguous-retained-projection attach fix 369da0c3e0, 2026-10-26)
- ☑️ [#subagent:attachrowmerge#1] editor attach still holds when canonical both lags the buffer and has a real controller addition the buffer lacks (e.g. a claim marker plus a response); add a row-aware attach merge (found by the ambiguous-retained-projection attach fix, 2026-10-26)
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/378~~
- ~~I deleted a queue item in docs.md and added some of the deleted queue item prompt into an existing queue item: Agent Doc could not attach the open editor replica for agent-doc-bugs.md. Cause: ambiguous-retained-projection. The editor buffer and controller canonical have diverged from their last settled ancestor. Copy any unsaved text, close this editor tab, run `agent-doc repair <file>` while it is detached, and reopen it. The editor buffer and disk were left unchanged~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/379~~
- 📥 release + publish: Include fixes for Issue 378-379 and the current 📥 items.
<!-- /agent:run -->

## Loop: Issues <!-- agent:loop id="issues" interval="1h" priority -->
- 📥 Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.
<!-- /agent:loop -->

## Todo <!-- agent:todo priority -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#coder-remote-install-02805] Repair Coder Remote Dev sync after the two-sided install: open /home/brian/work/btakita/agent-loop as the backend IDE project root; ensure Client/Gateway and backend both load JetBrains 263 plugin 0.2.824; restart both; on the backend run agent-doc env --json, agent-doc tmux ensure tasks/agent-doc/agent-doc-bugs.md --ide-terminal --json, and agent-doc session status tasks/agent-doc/agent-doc-bugs.md; require client idea.log to show snapshot schema negotiated with geometry=measured, backend idea.log to show accepted ingress with no DELIVERY_FAILED, and .agent-doc/logs/ops.log to show controller_editor_surface_observed client=jetbrains-rd@... surface=remote-client:... followed by pane_layout_desired_published with column_order=editor. If DELIVERY_FAILED No such file or directory persists, fix ControllerSurfaceSnapshotPublisher to resolve the controller from each session document root instead of an unrelated IDE project basePath, expose connected-without-accepted-publication as blocked while preserving the no-guessing fence, and add a regression test.
- [ ] [#jb263-feed-verify] In IntelliJ IDEA 2026.3, configure only https://github.com/btakita/agent-doc/releases/latest/download/agent-doc-jetbrains-263.xml, refresh Plugins, update Agent Doc from 0.2.816 to 0.2.824 or newer, restart the IDE, and verify the “Cannot update plugin” warning clears; if it does not, capture the configured repository entry and relevant idea.log update-check lines.
- [x] [#layoutunprovenliveverify] In the IDE, open the cross-root sdk-demo.md and agent-doc-bugs.md together, run Sync Tmux Layout, and confirm it recreates the stale sdk-demo pane and converges to two panes without focusing an ownership-unproven pane; restart the IDE first so JetBrains 0.2.828+ is loaded. (source: agent:review [#layoutunprovenliveverify])
- [x] [#autosync-restart-verify] Restart IntelliJ IDEA 2026.3 and Rider 2026.2 so JetBrains 0.2.832/0.2.831 load, then switch editor tabs: tmux should follow within about a second with no install or Run Agent Doc in between (idea.log shows `[layout-sync] selection file=`).
<!-- /agent:todo -->

## Backlog <!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review <!-- agent:review -->
- [/] [#jetbrainsmarketplaceprofile] [marketplace-approval] [agent-doc] Plugin 35436 is hidden and under initial JetBrains moderation; after approval, replace the basic description with polished copy, add Getting Started, Documentation, and Bugtracker URLs, then verify the public listing.
- [/] [#jb262dynlauncherrecommen] [recommended] [operator-verify] [agent-doc] Operator decision: jb262dynupgrade (153c72916) chose the accurate-warning option (a 262 package reports declined_by=modular_package and falls back to file replacement + restart). Either keep that as the supported policy (then close this), or authorize a real restart-free launcher for the exact-262 split-mode plugin that swaps backend + frontend (the frontend may run in a separate JetBrains Client process), proven against a live 262 IDE.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
- [/] [#gh299-live-263-proof] Live-verify v0.35.519 on JetBrains 263.7031 with changed plugin sources: `agent-doc upgrade` must hot-reload and complete without an IDE restart; close GH #299 only with that proof.
<!-- /agent:review -->

## Icebox <!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Done <!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
