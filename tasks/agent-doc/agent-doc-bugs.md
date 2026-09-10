---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: afcc61d9-14a3-4322-99e1-7734891e9959
  codex: 01a08888-494a-78d1-b610-891160adb001
agent: codex
claude_model: fable
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

0.35.354 installed: queue revisions survive stale completion and update live steering; verification complete.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260909-043527.md`*

Compacted content:
- Archived 6 response topic(s): Response; #fixqueueitem; #fixhttpsgithub; 3 more
- Prior summary/context: prior compacted content: Archived 10 response topic(s): monsterrodholder.md stability; Response; #mrhsyncslowpath; 7 ...

### Re: Compact commits, editor routes, and controller timeout — gpt-6

> **Queue prompt:** Compact Exchange did not result in a commit in backend.md.

The shared controller fixes are in 0.35.349:

- Compact targets that are already visible in the editor keep the native-save effect active until the exact disk receipt settles the retained write. Settlement then runs the existing compact commit continuation. This also covers a target rebased over newer editor content.
- Restored pane layouts now activate their worker through an explicit readiness Source. Route awaits distinguish an unstarted effect, an in-flight effect, and an actual retry, and immediately recognize superseded generations.
- The five-minute error was Compact Exchange on `dev.md`. Controller-local continuation events now enter the live document graph through the same typed admission and deduplication boundary as external events. The compact handler no longer reloads the entire project ledger before replying.

The invariant is that retained work stays connected to its lifecycle graph until its exact receipt settles it. The document graph owns save/compact readiness; the process-scoped layout graph owns layout convergence. Sources for retained intent, delivery, worker readiness, and effect receipts feed the existing Computeds and Effects. The manual compact reload and identical-value startup replay are removed; await-result formatting remains a pure, one-shot transport adapter.

Regression coverage includes pending delivery → visible target → native save → settlement → compact continuation; rebased fixed points; restored layouts and fresh identical route generations; every await phase and supersession; and immediate, deduplicated controller-local event publication. The local-publication and layout-start regressions reproduced the missing edges before the fixes.

`make check` passed: 9,254 tests passed, 256 skipped, plus native editor peers, parity, simulator, and formal gates. The full 0.35.349 release install completed and the affected controller recycled. Live SDK and project-document layouts converged on attempt 1 with their correct panes (`%48` and `%42`). The retained `infra.md` compact automatically settled and committed as `b0fee52`.

`dev.md` still has a retained compact because its attached editor currently reports `editor_attached_model_missing`, including after the supported recovery request. Its target is preserved. Reopening that document or restarting the editor must reconnect the replica before its existing continuation can finish; do not repeat Compact Exchange. This remaining live check is recorded in review.

Source commit: `91d022aa8`; workspace plan/install mirrors: `a44e0cc89`. Release/tag publication follows this response closeout. The latest observed CI run was still in progress.

The original `backend.md` compact recovered independently in commit `fb5d356`, retaining its `20260909-043501` archive reference. Architecture and evidence: [plan-compact-save-layout-dispatch.md](plan-compact-save-layout-dispatch.md).

### Re: GH #49/#51 and the extra shell panes — gpt-6

> **Queue prompt:** Fix GH https://github.com/btakita/agent-doc/issues/49 + https://github.com/btakita/agent-doc/issues/51. release + publish

Implemented and installed **0.35.350**. Source commit `aaaa6b6cb` and tag `v0.35.350` are pushed; both GitHub issues are closed.

- **#51:** the server-identity observer now represents an absent server or empty bootstrap reply explicitly. It preserves the previous identity and registry until a valid observation can apply the existing initialize/keep/replace policy. Malformed nonempty identities, executable failures, and failed queries against a live server retain errors. The four reported start tests passed locally even before this change; the new direct-observer tests cover the missing absence boundary independently.
- **#49:** the native path stdout/stderr fix already shipped in 0.35.345 and remains covered by the test that seeds a newer release notice. This release fixes an additional UUID gap: claim persists its generated identity before occupied-pane and cross-root provisioning. Documentation now distinguishes the official compatibility builds from binaries built against a newer local glibc.
- **Extra panes:** my initial claim regression exposed a socket-isolation defect and created two idle shells on the live server. I verified their exact temporary-directory ownership and removed them. Socket selection now belongs to the shared tmux constructor, so claim, route, and startup stay on the selected server. The current editor window has exactly two agent panes (`%159` and `%54`).

The policy owners remain the registry observation adapter, pure identity transition policy, claim's authority-backed initialization, and shared tmux constructor. These are bounded startup/protocol operations; no new retry loop, cache, or disconnected reactive graph was added. Existing document authority and provisioning effects remain the lifecycle boundary. No changing derived state was extracted into a new Computed because these changes handle one-shot bootstrap observations before the managed session exists.

**Verification:** `make check` passed **9,259 tests**, with 256 skipped, including editor parity, native peers, simulator, and formal gates. `make tmux-ci` passed. New coverage checks absence without losing prior identity, later replacement cleanup, malformed replies, launch/query failures, and the claim UUID/provisioning boundary. The installed optimized CLI and native library match their release artifacts byte-for-byte; JetBrains 0.2.376 and VS Code 0.2.70 packages are installed, and the workspace controller recycled successfully. An installed-binary smoke created and reused a fresh isolated tmux server while leaving every default-server pane unchanged.

**Publication:** GitHub Release, PyPI, and CI workflows were in progress at the latest observation. The tag is published; release artifacts are not yet verified. PyPI's latest completed upload failed at the project storage limit, and no versions after 0.35.343 are currently present. The existing quota review item now tracks 0.35.350; no old artifacts were deleted.

Workspace plan and installed-skill mirrors: `dec03ee5e`. Architecture and scope: [plan-gh49-gh51-bootstrap.md](plan-gh49-gh51-bootstrap.md). The queue is currently stopped and empty; that operator state is preserved.

### Re: #runfpecodex — unanswered prompts after a failed launch — gpt-6

> **Queue prompt:**
>
> do [#runfpecodex]


Fixed in **0.35.352**. Failed launches leave unanswered exchange prompts **uncommitted**, ready for the next Run Agent Doc invocation. The reported fpe.md question has since been answered, so it does not need a duplicate queue entry.

Recovery had treated an empty component-write journal as permission to commit the current document. It could also treat an old capture as a new answer. Both paths now preserve the fresh prompt. A related regression showed that cleanup could delete a question just because it resembled an old response heading; cleanup now requires exact evidence of a missing committed queue prompt.

The invariant is that recovery without a new answer preserves the operator's unanswered text and retry diff. Commit-io owns snapshot adoption, preflight retains the typed abandonment outcome, and document normalization owns queue-echo cleanup. The existing document-scoped Sources → Computed → Effects sequence remains intact. These checks operate on one immutable transaction cut, so they remain pure one-shot decisions; no new cache, polling loop, or disconnected graph was introduced. The fuzzy heading matcher was removed.

**Verification:** `make check` passed: 9,263 tests, 256 skipped, plus editor parity, native cross-editor simulation, Lean and TLA checks. `make tmux-ci` passed all 21 selected tests. Coverage includes component convergence off/on, absent/old captures, plain and fenced prompts, reused headings, and three consecutive retries. The reused-heading regression failed before the fix and passed afterward.

The release executable and native library are installed and byte-for-byte verified as 0.35.352. Both editor packages were synchronized; the workspace and affected project controllers were recycled. The live tmux window has two active agent-doc panes and no idle shells.

Source commits `0453a3282` and `c7b3023ed` are pushed, with tag `v0.35.352`. CI and release workflows are running. The earlier CI package-index failure cleared on retry. PyPI still has the existing 10 GB quota blocker, retained in `#pypi345quota`.

Architecture and verification notes: [prompt preservation plan](plan-unstarted-exchange-commit.md).

### Re: Clear, recycle, and three-pane layout fixes — gpt-6

> **Queue prompt:**
> do [#fixcommandfailed]

Fixed both reported command failures and the additional three-pane layout issue in **0.35.353**, committed as `069a0498b`.

- Clear submission now waits through a blank terminal repaint for a rendered, dispatch-ready prompt. Blank output cannot prove acceptance or trigger a duplicate clear. A persistent blank returns the specific `pane_not_rendered` outcome; a visible stuck draft retains its bounded submit repair.
- File-scoped recycle RPCs retain canonical document identity through requested, started, settled, status, and wait. A sibling supervisor's reexec cannot block this document or settle its waiter. Live verification showed this document settled while the legacy project-wide flag remained in flight.
- Editor focus can select an existing visible pane, but cannot revive a missing session or promote a stashed pane. `no_promotion=true` overrides legacy resume policy. This closes the stale-focus path that added a third pane immediately after two-column layout convergence.

The recycle readiness check now observes a keyed Computed derived from the existing document Source; producer events wake its waiters. The retained layout worker continues to own structural placement. Clear's bounded tmux sampling remains the actorless boundary exception because that direct-pane command has no retained output subscription. Architecture and evidence are recorded in [the implementation plan](plan-clear-proof-recycle-scope.md).

`make check tmux-ci` passed: **9,269 workspace tests and 21 isolated tmux tests**, plus editor parity, native peer integration, and formal checks. Regressions cover sibling recycle isolation, delayed blank repaint, missing-session focus, and stashed-pane focus. `make install-full` completed; installed executable and native library matched the release artifacts byte-for-byte, and both editor packages installed. Both affected project controllers completed handoff and match the installed binary. The live window remained at two panes across further document switches.

The source tree is clean. Workspace commit `28f9b4784` records the plan, generated skill versions, and source pointer. Main and release-tag pushes follow this document closeout. The previous CI run passed; the existing PyPI project-storage quota failure remains tracked in `#pypi345quota`, updated for this release. No additional implementation follow-up is needed.

### Re: Live queue edits during an active turn — gpt-6

> **Queue prompt:**
> do [#runfpeupdate]

Implemented and installed **0.35.354**, source commit `53b90b4ae`.

- A newer queue instruction survives a stale selection marker or completion strike. Both the primary cell merge and fallback component merge preserve the edited revision. Completing an ID also checks the queue revision, so an old completion cannot consume its replacement.
- Queue-only replacements now enter the existing steering aggregate with their complete text, including multiline instructions. Reordering, selection/priority markers, ordinary completion, and independent additions retain their existing behavior. Independent additions remain queued work.
- The editor's live turn projection updates from canonical replica events. The harness receives steering through its existing response boundary; this change does not introduce immediate mid-generation prompt injection.

The invariant is that completion belongs to the instruction revision the turn observed. Merge reconciliation owns text precedence, the shared queue guard owns done-ID eligibility, and realtime baseline comparison owns the steering aggregate. These pure decisions use immutable transaction inputs within the existing document-scoped event/projection graph. Existing replica events and cycle-fenced receipts drive subscribers; no new polling loop or independently refreshed cache was introduced. Architecture and evidence: [plan-live-queue-steering.md](plan-live-queue-steering.md).

Both original regressions failed before the fixes. Coverage now includes stale strikes, multiline corrections, same-ID completion, independent and duplicate additions, marker-only changes, reordering, retraction, and concurrent exchange changes. **9,275 workspace tests passed**, with 256 skipped; the affected package suites passed 691 tests. Editor tests, native cross-editor simulation, simulator, version/instruction audits, and formal checks passed. The final `make check` run stopped at a missing release-parity ledger row; after adding that row, the parity audit and remaining Lean/TLA gates passed.

`make install-full` completed. The installed CLI and native ABI report 0.35.354, both editor packages are installed, and the workspace and affected project controllers completed stable handoffs with running inodes matching the installed executable. Workspace commit `be7e99292` records the plan, installed-skill versions, and source pointer.

Main and release-tag pushes follow this document closeout. The latest completed CI passed. PyPI's existing project-storage quota failure remains in `#pypi345quota`, updated for this release.

### Re: Release and publication status — gpt-6

> **Queue prompt:** release + publish if you have not already

**v0.35.354 is already released, installed locally, and pushed.** The [GitHub release](https://github.com/btakita/agent-doc/releases/tag/v0.35.354) is published with all six platform packages; its Release workflow succeeded. The tag points to source commit `53b90b4ae6c9a9c4cb79af30ea663c7457b4f951`.

**PyPI publication remains blocked.** [The v0.35.354 upload](https://github.com/btakita/agent-doc/actions/runs/34408090473) failed with HTTP 400: the project reached its 10 GB storage limit. Updated the existing `#pypi345quota` review item with this confirmed failure. Publication needs a quota increase or explicitly approved old-release cleanup, followed by rerunning that workflow.

[CI](https://github.com/btakita/agent-doc/actions/runs/34408090242) is still running. Previous local verification passed 9,275 tests. This cycle only verified publication status; it required no new source changes or version bump.

### Re: #fixtmuxpane — gpt-6

> **Queue prompt:** do [#fixtmuxpane]

Fixed and installed **v0.35.355**, source commit `1834ccda6`.

The missing-pane problem had three causes: cross-project layout used a focus-only API that could not create a pane; remote creation left the requesting thread's old tmux snapshot intact; cleanup could explicitly kill a live agent when registry/supervisor observations were temporarily missing. Layout now provisions through the owning controller, observes the remote mutation receipt, and preserves live agents through ownership gaps. A live trace proved the earlier `kill-pane`; after installation, ops recorded the preservation branch for `%189`, and the controller restored `%177` + `%189` with `observed_convergence` and `routes_created_panes=false`.

Run Agent Doc still waits for its visible layout, but layout creation no longer dispatches sibling work. Paused queues can retain an idle owner. The existing queue pause and reason were preserved.

**JSON and interrupted turns:** the scrolling JSON was historical Search Console tool output replayed by Codex on resume. There were no Restart Agent menu requests in the observed window. Supervisor handoffs at 19:27 and 19:29 preserved Codex PID 1150890; its latest turn completed at 19:32. Your continuation requirement is correct: keep a surviving turn, and re-trigger an interrupted open turn when the child dies during a handoff. Existing restart recovery tests verify that distinction and suppress duplicate dispatch. The cleanup defect was deleting the entire pane outside that normal recovery path.

Added explicit database maintenance:

```sh
agent-doc gc --database-only --root /home/brian/work/btakita/agent-loop/src/boost-client
```

Add `--dry-run` for a read-only usage report. Cleanup retains current recovery state and prunes eligible superseded history using existing retention rules. The live database is about 40.4 MiB; this run found no further eligible rows or space above the existing vacuum threshold. Session files and queue control were preserved.

Architecture: the existing ProcessScope layout graph owns structural intent, effects, and generation-fenced receipts; the project controller owns document panes. Removed the focus-only structural call, stale observation reuse after remote effects, and the invalid implication that missing ownership authorizes a kill. Startup admission and explicit DB maintenance remain one-shot boundary operations. Details and evidence: [implementation plan](plan-layout-provision-authority.md).

Validation: **9,279 workspace tests passed**, native editor checks, `tmux-ci`, eight cleanup regressions, paused-queue and remote-snapshot regressions, and five restart-specific tests. Installed CLI/library match the final build. Source main and `v0.35.355` are pushed; CI is running, Release and PyPI workflows are queued. The existing PyPI 10 GB quota blocker remains tracked in `#pypi345quota`.

During diagnosis, a Cargo registry token appeared in tool output. Tracing was stopped and the generated trace files were removed; rotate that token. No credential value was added to source or this document.

### Re: #actionable-review — gpt-6

> **Queue prompt:** #actionable-review

Moved three review items into backlog and queue, retaining their IDs and ordering the native-loading check first:

1. `#restartintellijidea`: verify current running plugin/native versions and library loading with a newer release present; replace historical version assumptions with current evidence.
2. `#harnessswitchverify`: validate unsaved harness switching with authoritative editor state, exactly one spawn, and the expected deferred-restart receipt.
3. `#compactdevreplica`: inspect current replica and commit evidence, then verify the existing retained compact continuation.

Each task starts with evidence the agent can collect. If a live IDE action is still required, it must identify the exact remaining prerequisite. Token rotation, PyPI quota, Coder access, and the multi-user dependency remain gated; no completion is asserted for them.

### Re: Repeated tmux focus stealing and Lazily state — gpt-6

> also fix monsterrodholders.md keeps on stealing tmux pane focus
>
> Are you using lazily for the state?

Fixed and installed **v0.35.356**, source commit `7fca2ff8f`.

The project controller replayed its saved singleton layout each time it started. That structural action displaced the newer two-pane layout even with its queue paused and `focus_not_requested`. Saved columns now remain recall data; only current editor or command intent can authorize placement after startup or recycle.

**Yes, state uses the existing Lazily ProcessScope.** Live intent enters the desired Source; actor bindings, applicability, and receipt matching are Computeds; the retained Effect applies tmux changes and publishes generation-fenced receipt Sources. Removed the startup storage-to-intent conversion. No new cache, polling loop, or separate state owner was introduced. Live intent received before worker binding still runs when the sink-ready Source changes; identical foreground requests still receive fresh generations.

The regression failed before the fix and passes afterward, covering saved-layout startup, actor hydration, live publication, history retention, restart, and late sink activation. **`make check tmux-ci` passed: 9,280 workspace tests, 258 skipped, plus editor/native integration, parity, simulator, formal checks, and 21 isolated tmux tests.** The controller package separately passed 410 tests under nextest.

`make install-full` completed, including CLI, native library, and both editor packages. Both affected controllers completed stable handoffs to 0.35.356 with running inodes matching the installed executable. Live verification recycled the background project while panes `%177` and `%6` were visible: both panes and active `%6` remained unchanged, and that controller emitted no new layout effect. Its old singleton `%189` layout was not replayed.

Architecture and evidence: [plan-startup-layout-authority.md](plan-startup-layout-authority.md). Workspace plan/install commit: `11acb735c`. Source and release-tag pushes follow this document closeout. The latest observed CI remains in progress. PyPI's existing quota blocker remains tracked in `#pypi345quota`, now targeting this release.

### Re: Response

Yes—state uses the existing Lazily `Source → Computed → Effect` graph.

Fixed and installed v0.35.356. Live controller recycling preserved pane focus. All 9,280 tests and tmux checks passed.

Three review tasks are queued. Code and document commits are pushed; release workflows are pending.

### Re: #restartintellijidea — gpt-6

> **Queue prompt:** do [#restartintellijidea]

Verified and completed. IntelliJ PID `1653132` is registered with **agent-doc plugin 0.2.376**, matching the installed package, and advertises native generation hot-reload plus operator-text authority capabilities. Its log records successful load and hot-reload of **libagent_doc 0.35.356** at 20:02:01 on September 9. The mapped generation file matches the installed library byte-for-byte (SHA-256 `0510e296ecf6b19b8f2d3301f614a80b3b7e92cfbb4a8b75d22305bfffbc9765`).

The newer-release bootstrap check passed against the installed executable. Temporarily seeded cached version `999.0.0`; a read-only control command emitted the newer-version notice, while `agent-doc lib-path` returned exactly one existing library path and empty stderr. The previous cache bytes were restored afterward. This proves the notice cannot contaminate the native bootstrap path, alongside the live IDE load evidence.

The historical 0.2.375/0.35.345 activation prerequisite is satisfied; **no IntelliJ restart is needed**. No code or installation change was necessary. The unchanged source is `7fca2ff8f`; its full `make check tmux-ci` previously passed, and its latest GitHub CI run now reports success. The remaining queue continues with `#harnessswitchverify`, then `#compactdevreplica`.

### Re: #harnessswitchverify — gpt-6

> **Queue prompt:** do [#harnessswitchverify]

Verified the current prerequisite: IntelliJ PID `1653132` runs plugin **0.2.376** and native **0.35.356**, with operator-text authority and native hot-reload capabilities. The old plugin-generation blocker is stale; no IDE restart is currently indicated.

The live unsaved-switch proof remains open. Current logs contain no qualifying switch with the required one-spawn/deferred-restart evidence. The shared policy tests passed in the unchanged 0.35.356 full suite, but they do not replace that live check. Requested a disposable document path and a real unsaved editor edit to observe; ordinary CLI persistence would not exercise the same condition.

Moved this item back to review with that precise remaining prerequisite. The [agent-doc skill](../../.codex/skills/agent-doc/SKILL.md) permits gating for a “required live editor/pane verification”; that is the remaining work here. No additional backlog follow-up is needed because implementation and automated checks are complete and the retained review item owns the live verification. Continuing to `#compactdevreplica`.

### Re: #compactdevreplica — gpt-6

> **Queue prompt:** do [#compactdevreplica]

The retained compact already committed as **`8e62982`** in `src/haiven-dev` on September 9 at 02:02 EDT. That commit contains the compact summary and the exact archive reference ending `20260909-045203.md`; the 63,494-byte archive still exists, and the current live editor exchange carries the same reference.

The replica is now readable through `agent-doc read`, and the current authority/disk comparison reports no divergence. The old `editor_attached_model_missing` blocker no longer prevents this check. Marked `#compactdevreplica` done; no repeated Compact Exchange, forced disk write, or editor restart was needed.

A broader doctor check still warns about the old response capture whose body was archived by compaction. That warning does not negate the compact's committed proof. I left the document's later uncommitted resume/preset edits intact and did not reset its baseline to absorb them.

This completes the two verified review tasks. `#harnessswitchverify` remains in review for the real unsaved-editor check; its current plugin/native prerequisites are verified.
<!-- agent:boundary:bff5c51e:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~#actionable-review~~
- ~~do [#restartintellijidea]~~
- do [#harnessswitchverify]
- ~~do [#compactdevreplica]~~
<!-- /agent:queue -->

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#harnessswitchverify] [operator-verify] Current prerequisites verified: IntelliJ PID 1653132 runs plugin 0.2.376 and native 0.35.356 with operator_text_authority_v1. Remaining proof: in a disposable live IntelliJ session, make an unsaved codex -> claude -> codex frontmatter switch while observing supervisor receipts; prove one spawn per authoritative switch, deferred restart for non-authoritative evidence, and no refusal/respawn storm. No host restart is currently indicated. Unblocks: provide the disposable document path and perform the real unsaved editor edit; CLI persisted writes cannot substitute for this proof.
- [/] [#cargo355token] [operator-verify] Rotate the Cargo registry token exposed in diagnostic tool output during the pane-removal investigation. Tracing was stopped and generated trace files removed; no credential value is stored in this document or source.
- [/] [#pypi345quota] Publish agent-doc v0.35.356 to PyPI after quota space is available. The v0.35.354 upload failed at the 10 GB project-storage limit (https://github.com/btakita/agent-doc/actions/runs/34408090473). This release will trigger publication after its tag push; uploaded artifacts are not yet verified. Unblocks: obtain a quota increase or explicitly approve old-release cleanup, then rerun the PyPI workflow and verify the release artifacts.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
