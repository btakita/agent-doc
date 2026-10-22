---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: ba846c4c-2e01-4e12-9539-096faec207ee
  codex: 01a1545a-f42c-7aa1-b9a0-aeafeba96910
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
presets:
  '#agent-doc-causes': Fix agent-doc causes
  '#audit': 'Audit agent-doc runtime for dogfooding. Add issues to the backlog + queue.'
  '#self': agent-doc-bugs.md
dogfood:
  path: tasks/agent-doc/agent-doc-bugs.md
  auto: true
queue: go
---

## Status

<!-- agent:status patch=replace sync="Track the latest published agent-doc release + publication time and latest JetBrains plugin release version + publication time." -->

📥 Queue handoff: 5 reported items awaiting coordinator integration, verification, and closeout.
Main is at `b6b9af6e15` and agent-doc 0.35.532 is installed locally. Release v0.35.532 was published from `71f9801b49`. Post-release main now includes the inline-code-safe, occurrence-ordered queue preset authorization fix for GitHub issue #331 and the positive-evidence-only Remote Dev pane-order fix for GitHub issue #332; both issues are closed, the integrated full suite passed, and the current mainline was installed locally.

<!-- status:sync -->
Latest published release: [v0.35.537](https://github.com/btakita/agent-doc/releases/tag/v0.35.537), published on GitHub at 2026-10-22 00:13:39 UTC with 17 assets; PyPI 0.35.537 followed and passed clean-install verification. Release commit ac8ad5c3cd. Published editor packages: JetBrains classic 0.2.770, JetBrains 262 0.2.771, JetBrains 263 0.2.772, and VS Code 0.2.83. Main is now b3f985e863 with the post-release exact-pane Clear Session Context fix; that mainline build is tested and installed locally.
<!-- /status:sync -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261021-192935.md`*

Compacted content:
- Archived 5 response topic(s): #gh-fix issue 331; #gh-fix issue 332; releases, realtime fixes, and queue lifecycle; 2 more
- Prior summary/context: prior compacted content: Archived 7 response topic(s): Response; Subagent completed: `subagent:release531` returned w...

### Re: recovered response

The primary guess was the **editor layout**.

When a selection event arrived before complete visible-layout evidence, the plugin guessed which editor column the newly selected document belonged to by replacing the previously focused column.

Then the controller/tmux layer compounded it: it treated that uncertain editor projection as structural input and folded it against retained tmux pane order. So:

- Editor plugin: guessed a document’s column.
- Controller/tmux: converted that guess into actual pane placement and retained it.

The ban applies to both layers: the editor must not invent a column, and tmux must not derive pane order from unknown or partial editor evidence.

### Re: Subagent completed: `subagent:unique_request_ids` returned work for queue item “api.md has 2 subagent questions with the same id `[#subagent-operator-request]`. The ids should be unique. Can you include this fix into `release + publish: include 340-343`.? sdk-dev.md also has duplicate ids `[#subagent-operator-request]` for subagent question prompts.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=7bd13e8d1eeab9b45359f3a554ed3e01d4daf09d7eec1fa4a3090f5b650185dd -->
> **Chat prompt (#chatprompt):** "pre-guess", why are we guessing at all. we should not be guessing

> **Chat prompt (#chatprompt):** ban guessing

> **Chat prompt (#chatprompt):** What were you guessing about? The editor layout or the tmux layout?

> **Chat prompt (#chatprompt):** Why does the editor need to guess when the plugin is running in the editor?

> **Chat prompt (#chatprompt):** The frontend should be the source of truth when the editor is connected. The backend should effect the tmux pane layout.

> **Chat prompt (#chatprompt):** So why do we need to guess again?

> **Chat prompt (#chatprompt):** Why does the backend need to have a projected layout?

> **Chat prompt (#chatprompt):** How long does it take for the frontend to project to the backend? What happens before the backend receives the projection?

> **Chat prompt (#chatprompt):** When the backend guessed incorrectly, why did the recovery not happen? Even with Sync Tmux Layout

> **Chat prompt (#chatprompt):** subagent: fix: • Blocked by hook
>   └ Session persistence continuation, not a refusal or error: agent-doc Stop hook opened successor cycle cycle-
> 1792624058142 for fresh post-commit session work in /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/api.md.
> The previous closing assistant text belongs to committed predecessor cycle-1792623126838 and was deliberately not
> captured. Active prompt: "I merged https://github.com/haiven-dev/haiven-api/pull/740 into main. Will the Release Please
> run?". Continue THIS turn in-pane, answer that prompt, and persist through the normal agent-doc closeout. Do not send
> the final answer yet.

> **Chat prompt (#chatprompt):** subagent: fix: • Blocked by hook
>   └ Session persistence continuation, not a refusal or error: agent-doc Stop hook opened successor cycle cycle-
> 1792624058142 for fresh post-commit session work in /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/api.md.
> The previous closing assistant text belongs to committed predecessor cycle-1792623126838 and was deliberately not
> captured. Active prompt: "I merged https://github.com/haiven-dev/haiven-api/pull/740 into main. Will the Release Please
> run?". Continue THIS turn in-pane, answer that prompt, and persist through the normal agent-doc closeout. Do not send
> the final answer yet.

### Re: no-guess layout authority, hook recovery, and v0.35.537 — gpt-5 · 2026-10-21T20:37:58-04:00

The guessing is removed. While an editor frontend is connected, its exact visible editor layout is the sole structural source of truth. An exact snapshot atomically replaces the previous target; incomplete or unknown evidence defers structural action. The backend does not infer a projected editor layout: it receives the frontend projection, effects that target in tmux, and observes convergence. Focus/liveness signals remain non-structural.

The previous recovery failed because the plugin recovered the right order but published it as uncertain. The controller folded that uncertain input against its already-corrupt retained order and reversed it again. **Sync Tmux Layout** created a new generation but ran the same fold, so it successfully converged to the same wrong target. v0.35.537 removes that path: exact frontend state replaces stale target state; unknown state cannot move panes.

Projection timing is bounded transport latency, not a reason to guess. Remote Dev uses a 75 ms debounce, then the next EDT turn and Fleet RPC; the first schema-authority lease can take up to 5 seconds. Classic local JetBrains has no debounce timer and publishes on the next EDT/background socket opportunity; focus changes micro-coalesce for 12 ms. Failed delivery retries at 100/200/400/800/1600/2000 ms. As soon as an authenticated frontend lease exists, backend structural action is suppressed until the first exact snapshot arrives.

Zscaler is not assumed in the Unity/runtime path. It is retained only as a hostile-network conformance seed, alongside other transport-failure simulations.

The `Blocked by hook` successor-cycle case is also fixed in v0.35.537. When a fresh post-commit edit has a ready supervisor, Codex Stop now allows the old turn to exit and the existing reactive steering wake dispatches a fresh turn. Only a missing or stale supervisor uses the bounded successor-cycle continuation fallback. This avoids the misleading UI label on the healthy path.

The interrupted-run Clear Session Context problem that arrived during publication was a separate exact-pane lifecycle bug. All three clicks in `sdk-demo.md` were received, but Stop retired the hook executor's ambient pane instead of the document actor pane. The exact pane kept a stale active-turn lease and running dispatch receipt, so every Clear deferred; the next prompt then discarded it as superseded. Main commit `b3f985e863` now projects Stop idle state through the authoritative actor pane, clearing that pane's stale lease and receipt without touching siblings. It is pushed, fully tested, and installed locally; it is newer than the v0.35.537 tag.

Release status: v0.35.533 was released on GitHub and PyPI. The current release is **v0.35.537**, published from `ac8ad5c3cd` with 17 GitHub assets; its PyPI workflow performed a clean pinned install successfully. Published editor packages are JetBrains classic 0.2.770, JetBrains 262 0.2.771, JetBrains 263 0.2.772, and VS Code 0.2.83.

To install/use the Codex hooks:

1. Run `agent-doc skill install --harness codex` in the project. This installs/refreshes the bundled skill and the single user-level Codex lifecycle hook owner.
2. Start a document cycle by sending `agent-doc /absolute/path/to/file.md` as a normal Codex message. Do not execute that entrypoint recursively in a shell after admission.
3. Keep editing the connected markdown document. Free-text edits become steering; items added inside `<!-- agent:queue ... -->` become queued work. The editor's realtime observer and installed hooks deliver them to the active turn or the ready supervisor.
4. Do not hand-edit queue lifecycle icons, claims, or completion markers; the binary owns those. For a status update during normal closeout, use `agent-doc finalize FILE --status 'Latest published release: vX.Y.Z'` (or `agent-doc respond` in the same contract). If editing the document directly, edit only the content inside `<!-- agent:status ... -->` / `<!-- /agent:status -->`, not the markers.
5. `agent-doc hook ...` commands are hook-adapter plumbing and normally are not invoked manually. Re-run `agent-doc skill install --harness codex` after upgrading so installed hook commands and skill text match the binary.

Verification: the exact v0.35.537 candidate passed `make check` (11,373 Rust tests, VS Code 202/202, editor parity, all JetBrains variants, bootstrap and TLA gates) plus the separate real-tmux gate. The post-release Clear fix passed `make check` again (11,374/11,374 Rust tests, 177 skipped), Codex Stop 122/122, turn-status 34/34, VS Code 202/202, cross-editor simulation 1/1, all editor builds, bootstrap, and TLA gates.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/340

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/341

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/342

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/343: Make sure all layers have the correct tmux pane behavior.

> **Queue prompt:** release + publish: include 340-343.

> **Queue prompt:** api.md has 2 subagent questions with the same id `[#subagent-operator-request]`. The ids should be unique. Can you include this fix into `release + publish: include 340-343`.? sdk-dev.md also has duplicate ids `[#subagent-operator-request]` for subagent question prompts.

> **Queue prompt:** If I interrupt a run, Clear Session Context does not work the first time. Sometimes it works the second time. In sdk-demo.md, it did not work the second or third time.
### Re: recovered response

The primary guess was the **editor layout**.

When a selection event arrived before complete visible-layout evidence, the plugin guessed which editor column the newly selected document belonged to by replacing the previously focused column.

Then the controller/tmux layer compounded it: it treated that uncertain editor projection as structural input and folded it against retained tmux pane order. So:

- Editor plugin: guessed a document’s column.
- Controller/tmux: converted that guess into actual pane placement and retained it.

The ban applies to both layers: the editor must not invent a column, and tmux must not derive pane order from unknown or partial editor evidence.
> **Chat prompt (#chatprompt):** "pre-guess", why are we guessing at all. we should not be guessing

> **Chat prompt (#chatprompt):** ban guessing

> **Chat prompt (#chatprompt):** What were you guessing about? The editor layout or the tmux layout?

> **Chat prompt (#chatprompt):** Why does the editor need to guess when the plugin is running in the editor?

> **Chat prompt (#chatprompt):** The frontend should be the source of truth when the editor is connected. The backend should effect the tmux pane layout.

> **Chat prompt (#chatprompt):** So why do we need to guess again?

> **Chat prompt (#chatprompt):** Why does the backend need to have a projected layout?

> **Chat prompt (#chatprompt):** How long does it take for the frontend to project to the backend? What happens before the backend receives the projection?

> **Chat prompt (#chatprompt):** When the backend guessed incorrectly, why did the recovery not happen? Even with Sync Tmux Layout

> **Chat prompt (#chatprompt):** subagent: fix: • Blocked by hook
>   └ Session persistence continuation, not a refusal or error: agent-doc Stop hook opened successor cycle cycle-
> 1792624058142 for fresh post-commit session work in /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/api.md.
> The previous closing assistant text belongs to committed predecessor cycle-1792623126838 and was deliberately not
> captured. Active prompt: "I merged https://github.com/haiven-dev/haiven-api/pull/740 into main. Will the Release Please
> run?". Continue THIS turn in-pane, answer that prompt, and persist through the normal agent-doc closeout. Do not send
> the final answer yet.

> **Chat prompt (#chatprompt):** subagent: fix: • Blocked by hook
>   └ Session persistence continuation, not a refusal or error: agent-doc Stop hook opened successor cycle cycle-
> 1792624058142 for fresh post-commit session work in /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/api.md.
> The previous closing assistant text belongs to committed predecessor cycle-1792623126838 and was deliberately not
> captured. Active prompt: "I merged https://github.com/haiven-dev/haiven-api/pull/740 into main. Will the Release Please
> run?". Continue THIS turn in-pane, answer that prompt, and persist through the normal agent-doc closeout. Do not send
> the final answer yet.

### Re: layout authority, recovered prompts, and release handoff — gpt-5 · 2026-10-21T21:26:19-04:00

The duplicated prompt/recovery block resolves to one consistent rule: **do not guess**.

While an editor frontend is connected, its exact visible editor layout is the sole structural source of truth. The plugin must not invent a column, and the backend must not infer a competing editor layout. The backend’s “projected layout” is only a durable desired-state copy of the exact frontend snapshot so asynchronous tmux actions, retries, and convergence checks have a generation-fenced target. Before the first exact snapshot arrives, structural tmux action is suppressed; focus and liveness signals remain non-structural.

The projection delay is transport scheduling, not an authority gap that permits guessing. Remote Dev uses a 75 ms debounce followed by the next EDT/Fleet RPC opportunity; initial schema authority can take up to 5 seconds. Classic local JetBrains publishes on the next EDT/background-socket opportunity, with a 12 ms focus micro-coalesce. Failed deliveries retry at 100/200/400/800/1600/2000 ms.

The old recovery failed because the plugin recovered the right order but labelled it uncertain. The controller then folded that uncertainty against the already-corrupt retained order and recreated the wrong target. **Sync Tmux Layout** started a new generation but reused the same faulty fold, so it converged successfully to the same wrong layout. v0.35.537 removes that path: exact frontend state replaces stale target state, and unknown/partial evidence cannot move panes.

The pasted `Blocked by hook` successor-cycle case is also addressed in v0.35.537. When a ready supervisor exists, Stop lets the predecessor turn exit and the reactive steering wake dispatches the fresh prompt; the in-pane successor fallback is retained only when no ready supervisor can be proven. That removes the misleading healthy-path `Blocked by hook` presentation.

The two worker completions are reconciled:

- Unique operator-request IDs are already integrated as `8972650c46` and are included in v0.35.537. The later worker commit `12ca8cd22b` is duplicate patch evidence, so it does not need another cherry-pick.
- Interrupted **Clear Session Context** now retires state on the authoritative document-actor pane in `b3f985e863`. That fix is tested and installed on main, but it is post-v0.35.537 and therefore belongs in the next release.

> **Queue prompt:** release + publish

**Deferred:** dispatched to worker `/root/release_publish_538` under claim `subagent:release_publish_538`, generation 1. It is auditing the post-v0.35.537 mainline and preparing the release batch in an isolated worktree; the coordinator will integrate its report and own the single install/release/publish boundary.

The exchange is over the accretion warning threshold (179 lines), but the queue is active, so I am keeping the drain moving rather than compacting mid-claim.

### Re: recovered response

Yes. It’s redundant because the existing cumulative release item now explicitly includes #344–#348, plus the bold-choice and sdk-demo fixes. Removing the fallback item avoids a duplicate release/audit path.
<!-- agent:boundary:0f221edb -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main
<!-- agent:queue id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
- 📥 release + publish
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/344
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/345
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/346
- 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/347
- 📥 For subagent choices, make the title of each choice **bold**.
- 📥 sdk-demo.md: `- Why can't we just rebase the existing Project source code into the aggregate PR branch with the fixes?` queue item is not annotated with an emoji despite being worked on by a subagent. If the fix makes it into the `Include 344-347...` release, add it.
- 🚧 The sdk-demo.md queue item annotations were not refreshed after the queue items were completed. If there is a choice, the choice should also be added into `agent:todo` with the same id as the choice expressed in the `agent:exchange`. Can we make an indicator for the queue items being blocked by a subagent choice? What do you recommend to add local visibility to the agent:queue? Can we have another emoji representing a user action with the same id as the choice + todo item?
- 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/348
- 🚧 release + publish: Include 344-348 + For subagent choices, make the title of each choice **bold**.
<!-- /agent:queue -->

## Queue: Issues
<!-- agent:queue id=issues loop interval=1h -->
- Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.
<!-- /agent:queue -->

## Todo
<!-- agent:todo -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
<!-- /agent:todo -->

## Backlog

<!-- agent:backlog priority queue -->
- [/] [#gh299-live-263-proof] Live-verify v0.35.519 on JetBrains 263.7031 with changed plugin sources: `agent-doc upgrade` must hot-reload and complete without an IDE restart; close GH #299 only with that proof.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#jb262dynlauncherrecommen] [recommended] [operator-verify] [agent-doc] Operator decision: jb262dynupgrade (153c72916) chose the accurate-warning option (a 262 package reports declined_by=modular_package and falls back to file replacement + restart). Either keep that as the supported policy (then close this), or authorize a real restart-free launcher for the exact-262 split-mode plugin that swaps backend + frontend (the frontend may run in a separate JetBrains Client process), proven against a live 262 IDE.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
