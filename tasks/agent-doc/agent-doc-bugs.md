---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: f08cd727-ebd1-4b95-8305-635688e2d56c
  codex: 01a17616-9ddc-7ee0-86df-a42c74605356
agent: grok
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
presets:
  '#agent-doc-causes': Fix agent-doc causes
  '#audit': Audit agent-doc runtime for dogfooding. Add issues to the backlog + queue.
  '#fix-dataflow': Fix any issue you see at multiple levels of the dataflow.
  '#repo': https://github.com/btakita/agent-doc
  '#self': agent-doc-bugs.md
dogfood:
  path: tasks/agent-doc/agent-doc-bugs.md
  auto: true
queue: go
---

## Status <!-- agent:status patch=replace sync="##sync-release" -->

📤 Queue handoff: 1 reported item awaiting coordinator integration, verification, and closeout.


<!-- status:sync -->
Latest release: agent-doc v0.35.560 (release commit bd971e9b87, tag v0.35.560). The GitHub Release has 24 assets and was published 2026-10-28T20:11:42Z. PyPI agent-doc is 0.35.560. The installed binary is 0.35.560. JetBrains pluginVersions on that release commit stay 0.2.877, 0.2.878, and 0.2.879. Main is 8862b2f9b5. That commit echoes a nested agent:run leaf as its nest vector. make check on the worker tree exited 0 (nextest 12117 passed, 187 skipped). After rebase onto main, 8 nest_vector tests passed and make install exited 0. JetBrains packages on main stay 0.2.880, 0.2.881, and 0.2.882, and VS Code stays 0.2.90. This install found those JetBrains bytes already current. CI for 5291b1f93c is still in progress (run 40774734370). CI for 8862b2f9b5 is in progress (run 40781176636); Book and Pages for that commit succeeded. Those main commits are outside tag v0.35.560.
<!-- /status:sync -->

Queue markers: 📥 pending · ⛔ blocked · 🟡 claimed · 🚧 running · ☑️ reported · ⚠️ lost · 🔄 needs reconcile · ❔ unknown · ⏭️ skipped · ⏳ awaiting operator · 🙋 operator action · 📌 pinned · 📍 agent pin ([legend](https://github.com/btakita/agent-doc/blob/main/docs/guide/queue-markers.md#queue-markers))
<!-- /agent:status -->
 

## Exchange <!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261028-174648.md`*

Compacted content:
- Archived 68 response topic(s): do [#clearqueuedciflake], do [#startcwdfallback], [#krwr] decision; [#ddj8] Is `~` as an agent-doc project safe?; Subagent completed: `subagent:krwr` returned work for queue item “[#krwr]: deleting a claimed queue row while adding ...; 65 more
- Prior summary/context: prior compacted content: Archived 38 response topic(s): session resumed after /clear; do [#decisionbacklogstrike]; id...
> **Queue prompt:** Add agent:rules tag, which provides context for each turn in the document. agent:rules takes a list or free text, similar to agent:run. This should also be supported with project.md and be composable. Multiple `agent:rules` elements should be allowed in a document.

**Deferred:** dispatched to `subagent:agentrules` generation 1, worker `01a183a4-8f9b-7bb2-8cce-ae26ac1f190e`, running on `feat/agent-rules` in `/home/brian/worktrees/agent-doc-agentrules` (base `dc7efbcbf6`). Integration, the single `make install`, and closing this row wait for that worker's report.

### Re: agent:rules dispatch — grok-4.7 · 2026-10-28T13:55-04:00

The `agent:rules` row is claimed and running. The worker owns the spec, tests, build, commit, and push of `feat/agent-rules`. This cycle does not implement the tag inline.

The Grok skill was behind the binary (`0.35.557` vs `0.35.559`). `agent-doc skill install --harness grok` updated `.grok/skills/agent-doc` to `0.35.559`. Those skill files are dirty in the agent-loop checkout and are outside this queue item.

No new backlog item. The row is the work, and it stays queued under the running claim.
> **Queue prompt:** release + publish
>   - include #subagent:agentrules

**Deferred:** `subagent:agentrules` generation 1 (worker `01a183a4-8f9b-7bb2-8cce-ae26ac1f190e`) is still running. Its worktree `/home/brian/worktrees/agent-doc-agentrules` is clean at `dc7efbcbf6`, so there is no commit to include. This row stays queued. After that worker reports, this session integrates `feat/agent-rules`, runs the single `make install`, then releases and publishes. A second worker is not dispatched: the row asks to include that claim, and a subagent is not allowed to run `make release`.

### Re: release waits on agent:rules — grok-4.7 · 2026-10-28T13:59-04:00

The running `agent:rules` claim was refreshed through generation 1. It remains the worker bound to `feat/agent-rules`.

`release + publish` is not started this cycle. Publishing now would omit the feature the child line names.
> **Queue prompt:** Add agent:rules tag, which provides context for each turn in the document. agent:rules takes a list or free text, similar to agent:run. This should also be supported with project.md and be composable. Multiple `agent:rules` elements should be allowed in a document.

`agent:rules` is on `main` at `2b7ef088eaaf6cc86152cceb6c1cd98a0979ccb7` (`feat(rules): compose agent:rules into every admitted turn`). `origin/main` fast-forwarded from `dc7efbcbf6`. `make check` exited 0, then `make install` exited 0. The installed binary is `0.35.559`, built from that commit at 2026-10-28 14:53 -04:00. CI for `2b7ef088ea` is in progress: https://github.com/btakita/agent-doc/actions/runs/40748955632

Every `agent:rules` body in the nearest `.agent-doc/project.md` is composed in file order, then every body in the session document. Session rules add to project rules. A list item is one rule. Free text is one rule as written. Preflight puts the list on the cycle contract as `agent_rules`, with `agent_rules_guidance` when the list is non-empty. A content-only edit does not start a turn.

### Re: agent:rules on main — grok-4.7 · 2026-10-28T14:54-04:00

The worker branch `feat/agent-rules` was verified before the fast-forward. `make test` had already passed with 12107 tests and 0 failures. The diff added no private-name denylist hits. The version was not bumped. The next row is still the release request.
> **Queue prompt:** release + publish
>   - include #subagent:agentrules

**Deferred:** dispatched to `subagent:release` generation 4, worker `01a183ed-f565-74d3-afe0-b1942fb321eb`, running in `/home/brian/worktrees/agent-doc-release`. `origin/main` is `2b7ef088eaaf6cc86152cceb6c1cd98a0979ccb7` (`v0.35.559-18-g2b7ef088ea`), which already contains the `agent:rules` commit. The worker verifies that commit and the release procedure. The coordinator rules forbid that worker from `make release`, a version bump, or a tag, so this session runs the release after the worker reports. The row stays queued.

> **Queue prompt:** sdk-demo.md `agent:run` is out of sync. Fix agent-doc causes.

**Deferred:** dispatched to `subagent:sdksync` generation 1, worker `01a183ed-f565-74d3-afe0-b1aa7050daf0`, on `fix/sdk-demo-agent-run` in `/home/brian/worktrees/agent-doc-sdksync`. Spec, tests, commit, and push belong to that worker. Integration and the single `make install` wait for its report.

### Re: both rows are running — grok-4.7 · 2026-10-28T15:13-04:00

Both `agent:run` heads are claimed and worker-bound.

`release + publish` is generation 4 on worker `01a183ed-f565-74d3-afe0-b1942fb321eb`. The child line's `agent:rules` work is already on `origin/main` at `2b7ef088ea`. The worker's report will name the release command, the range since `v0.35.559`, and CI for that SHA. This session publishes after that report.

`sdk-demo.md` is generation 1 on worker `01a183ed-f565-74d3-afe0-b1aa7050daf0`. It owns the agent-doc fix, tests, and the branch push. This cycle does not edit that demo inline.

No new backlog item. Both rows are the work, and both stay queued under their running claims.
> **Queue prompt:** Error on api.md: [route] Sent 2 updates to the agent already running for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/api.md; it will handle them in this turn. No second trigger was injected. steering_delivered=2 sent=explicit ui_outcome_contract=ui-outcome-v1 ui_outcome=steering_delivered_to_owner ui_outcome_class=ok next_action=owner_turn_handles_steering

**Deferred:** dispatched to `subagent:apiroute` generation 1, worker `01a18406-3ec5-7240-98f1-39f3e11d88cd`, on `fix/api-md-route-error` in `/home/brian/worktrees/agent-doc-apiroute`. Spec, tests, commit, and push belong to that worker. Integration and the single `make install` wait for its report.

### Re: api.md route dispatch — grok-4.7 · 2026-10-28T15:39-04:00

The api.md route row is claimed and running. The pasted diagnostic says two updates were delivered to the agent already running for `src/haiven-dev/tasks/api.md`, no second trigger was injected, and the route classed the outcome `steering_delivered_to_owner` / `ok`. The worker owns the agent-doc fix for that path.

`subagent:sdksync` generation 1 is still running on `fix/sdk-demo-agent-run`. Its claim was refreshed. `subagent:release` generation 4 has reported and is waiting for integration: project `0.35.560`, one `make check`, push `main`, then `make release`. This cycle does not cut that release.

No new backlog item. The api.md row stays queued under the running claim.

> **Queue prompt:** release + publish
>   - include #subagent:agentrules

> **Queue prompt:** sdk-demo.md `agent:run` is out of sync. Fix agent-doc causes.

### Re: v0.35.560 is published and the sdk-demo queue fix is on main — grok-4.7 · 2026-10-28T16:50-04:00

agent-doc v0.35.560 is published. Tag `v0.35.560` points at `bd971e9b87` (`release: agent-doc 0.35.560`). `make release` pushed `2b7ef088ea..bd971e9b87` to `main` and pushed the tag. The GitHub Release is a stable public release with 24 assets, published 2026-10-28T20:11:42Z: https://github.com/btakita/agent-doc/releases/tag/v0.35.560. PyPI `agent-doc` is 0.35.560, and the wheel `agent_doc-0.35.560-py3-none-any.whl` is public. The release workflow https://github.com/btakita/agent-doc/actions/runs/40761053526 and the PyPI workflow https://github.com/btakita/agent-doc/actions/runs/40765602042 both completed. The release driver reported `tag_publish_handoff=complete`, `local_install_full=complete`, and `public_publication=complete`.

The sealed batch includes `agent:rules` from ancestor `2b7ef088ea`. On the release tree, nextest ran 12107 tests: 12107 passed and 187 skipped. `make tmux-ci` passed, including `two_ide_lane_claims_start_panes_in_their_own_lanes_end_to_end`. `install-full` installed the tagged binary. `agent-doc --version` reports `agent-doc 0.35.560`.

The sdk-demo `agent:run` mismatch is fixed on main at `343fe163c2` (`fix(queue): match completion cells to a row's own work`), the commit after the tag. A completion cell matches a row only when its recorded item is that row's own work, so a longer tracked child stays off its parent. Regression `completion_cell_for_a_longer_tracked_line_does_not_reconcile_the_parent` passed on that tree. The push was `bd971e9b87..343fe163c2` to `main`. `make install` of that tree exited 0 and held JetBrains pluginVersions at 0.2.877, 0.2.878, and 0.2.879 because the plugin bytes were already current. The release worktree `feat/release-readiness` is clean and matches `origin/main` at `343fe163c2`. Tag `v0.35.560` stays at `bd971e9b87`. CI for `343fe163c2` was in progress as run 40766108880 when status was recorded.

The api.md route claim stays open. `fix/api-md-route-error` has two local commits (`519352f34d`, `5291b1f93c`) and `make check` is still running in that worktree. The branch is unpushed, so it stays out of this closeout.

> **Queue prompt:** Where does this file live? My Coder agent is asking.

**Deferred:** dispatched to `subagent:vscodecodersmoke` generation 1, worker `01a18450-c468-7a82-ac95-36f5f45e17ae`, on `fix/vscode-coder-file` in `/home/brian/worktrees/agent-doc-vscodecodersmoke`.

### Re: Coder file question is with its worker — grok-4.7 · 2026-10-28T17:00-04:00

The row nested under `#vscodecodersmoke` is claimed and running. The worker is resolving `scripts/vscode-remote-smoke.py`, `runbooks/coder-workspace.md`, and the VS Code on Coder cell in `editors/SPEC.md` section 13. It builds and tests in that worktree. The coordinator keeps `make install` and any main integration until the worker reports.

The api.md route claim stays with its existing worker. Its branch is still unpushed.

> **Queue prompt:** Where does this file live? My Coder agent is asking.

### Re: The Coder smoke script is in the agent-doc checkout — grok-4.7 · 2026-10-28T17:04-04:00

The file is `scripts/vscode-remote-smoke.py` in the agent-doc repository. Run it from that checkout, on the workspace host:

```sh
python3 scripts/vscode-remote-smoke.py --project ~/project --doc tasks/plan.md
```

`runbooks/coder-workspace.md` (section "VS Code remote smoke test") and `editors/SPEC.md` section 13 already name that script. The procedure also names `runbooks/coder-workspace.md`. No spec change and no commit. `fix/vscode-coder-file` stayed clean at `343fe163c2`. I re-ran `python3 scripts/vscode-remote-smoke.py --self-test`; it printed `vscode-remote-smoke self-test: ok` and exited 0.

The api.md route suite finished in its own worktree. `make check` with socket `/tmp/adar.7lb83r` exited 0: nextest 12108 passed and 187 skipped, and the full-check receipt was written. `fix/api-md-route-error` is pushed at `5291b1f93c`. That row stays open for integration onto main.

> **Queue prompt:** Error on api.md: [route] Sent 2 updates to the agent already running for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/api.md; it will handle them in this turn. No second trigger was injected. steering_delivered=2 sent=explicit ui_outcome_contract=ui-outcome-v1 ui_outcome=steering_delivered_to_owner ui_outcome_class=ok next_action=owner_turn_handles_steering

> **Queue prompt:** The queue prompt in the response for a nested agent:run item should show the entire nest "vector" such as `root parent: parent: item`.

**Deferred:** dispatched to `subagent:nestvector` generation 1, worker `01a1847a-9095-78f2-85c6-aa4ddfdcbe87`, on `fix/nested-queue-prompt` in `/home/brian/worktrees/agent-doc-nestvector`. That worktree started at `343fe163c2`. Main has since moved to `5291b1f93c`, so the worker rebases onto current `origin/main` before it pushes the feature branch.

### Re: api.md route fix is on main — grok-4.7 · 2026-10-28T17:49-04:00

The route diagnostic was live-owner steering delivery, classed `ok`. The fix is on main at `5291b1f93c` (`fix(route): report live-owner steering as success`, plus the VS Code 0.2.90 package bump). Fast-forward was `343fe163c2..5291b1f93c`. `make check` on that tree exited 0: nextest 12108 passed and 187 skipped. `make install` exited 0. The binary remains `agent-doc 0.35.560`. JetBrains on this commit is 0.2.880, 0.2.881, and 0.2.882. Restart IDEA to load 0.2.882. VS Code 0.2.90 is installed.

CI for `343fe163c2` succeeded: https://github.com/btakita/agent-doc/actions/runs/40766108880. CI for `5291b1f93c` is in progress: https://github.com/btakita/agent-doc/actions/runs/40774734370. Tag `v0.35.560` stays at `bd971e9b87`.

The nested queue-prompt row is claimed and running in its own worktree.

> **Chat prompt (#chatprompt):** has a new version been published?

> **Queue prompt:** Nested .agent-doc/project.md files are not being honored. Agent Doc files in src/haiven-dev should read agent-loop/src/haiven-dev/.agent-doc/project.md and ageunt-loop/.agent-doc/project.md

**Deferred:** dispatched to `subagent:nestedproject` generation 1, worker `01a1849e-c77d-7910-8f66-460ccee632b9`, on `fix/nested-project-md` in `/home/brian/worktrees/agent-doc-nestedproject` at `5291b1f93c`.

### Re: v0.35.560 is still the published release — grok-4.7 · 2026-10-28T18:26-04:00

The published version is still agent-doc v0.35.560. The GitHub Release is the latest one, published 2026-10-28T20:11:42Z at tag `v0.35.560` (`bd971e9b87`). PyPI and the local binary are both `0.35.560`. `origin/main` is `5291b1f93c`, three commits past that tag: the queue completion-cell fix, the live-owner steering fix, and the VS Code 0.2.90 package bump. Those commits are outside the tag.

The nested `project.md` row is claimed and running. A session document under `src/haiven-dev` should read `src/haiven-dev/.agent-doc/project.md` and the workspace-root `.agent-doc/project.md`. The queue line spells the root as `ageunt-loop`.

The nested queue-prompt worker is still editing `fix/nested-queue-prompt` in its own worktree. `#vscodecodersmoke` stays queued for the real Coder workspace run.

> **Queue prompt:** The queue prompt in the response for a nested agent:run item should show the entire nest "vector" such as `root parent: parent: item`.

> **Queue prompt:** release + publish
>   - Include #subagent:nestvector + #subagent:nestedproject

**Deferred:** claimed by `subagent:release` generation 5, worker `01a184a9-ec8a-73a1-85bc-90238454799f`. `#subagent:nestedproject` is not on main, so no release was cut.

### Re: Nested queue prompt is on main — grok-4.7 · 2026-10-28T18:38-04:00

A nested `agent:run` leaf is echoed as its nest vector, `root parent: parent: item`. That fix is on main at `8862b2f9b5` (`fix(queue): echo a nested agent:run leaf as its nest vector`), rebased onto `5291b1f93c` and fast-forwarded `5291b1f93c..8862b2f9b5`. The worker's `make check` exited 0: nextest 12117 passed and 187 skipped. After the rebase, 8 `nest_vector` tests passed. `make install` exited 0. The binary is still `agent-doc 0.35.560`. JetBrains bytes were already current. Tag `v0.35.560` stays at `bd971e9b87`.

CI for `8862b2f9b5` is in progress: https://github.com/btakita/agent-doc/actions/runs/40781176636. Book and Pages for that commit succeeded.

The release row stays open until `#subagent:nestedproject` is on main. That claim is still running, and its worktree has no commit yet.

### Re: Subagent completed: `subagent:release` returned work for queue item “release + publish - Include #subagent:nestvector + #subagent:nestedproject” — agent-doc
<!-- agent-doc-subagent-response sha256=832bb29bb9420f2b50047ebd80f6a1d19b729a2c9e9bc75dc28d645e758eee65 -->
> **Subagent response:**
>
> Release is blocked. No version bump, tag, make release, make install, or release commit was made.
>
> #subagent:nestvector is on origin/main at 8862b2f9b5a0a864a8a932a027aac1286dbfd33c, subject fix(queue): echo a nested agent:run leaf as its nest vector. Installed binary is agent-doc 0.35.560. No tag points at 8862b2f9b5.
>
> #subagent:nestedproject is not on origin/main. Worktree /home/brian/worktrees/agent-doc-nestedproject is clean on fix/nested-project-md at 5291b1f93c with no unique commits. Commits after v0.35.560 are 343fe163c2, 519352f34d, 5291b1f93c, and 8862b2f9b5.
>
> This worker stayed on fix/release-readiness-561 at 8862b2f9b5, clean, no new SHA. Do not cut a partial release until a nested project.md fix is on origin/main.
<!-- agent-doc-subagent-completion key=62faec71d1667704882edf7a79bdfb681b1d4b42c3964caeccfd3c188c1e099a -->

> **Queue prompt:** do [#apikeepsrefusing]

**Deferred:** dispatched to `subagent:apikeepsrefusing` generation 1, worker `01a184ae-2b63-76b0-a028-c568b9888f6f`, on `fix/api-keeps-refusing` in `/home/brian/worktrees/agent-doc-apikeepsrefusing` at `8862b2f9b5`.

### Re: api.md push refusal is with its worker — grok-4.7 · 2026-10-28T18:42-04:00

`#apikeepsrefusing` is claimed and running. The backlog text is that `src/haiven-dev/tasks/api.md` keeps refusing to push, rebase, and publish. The worker will find that refusal, update the spec and tests when the refusal is an agent-doc bug, and push `fix/api-keeps-refusing`. It will not install or cut a release.

The nested `project.md` claim is still running and has no commit on main. The release row stays open until that fix lands. No new version has been published. Tag `v0.35.560` remains `bd971e9b87`.

> **Queue prompt:** When submitting a nested run item, include the parent lineage using the nested prompts. For example, in api.md `https://github.com/haiven-dev/haiven-api/pull/758: #rebase` created `[#subagent:pr767-rebase#1] #rebase` which bound to the wrong PR. If you are only sending `#rebase` as the prompt, the agent does not understand the nesting. The agent should be able to understand the entire tree and the actual prompt list item that is being sent.
>   - Use the AST to guide the agent.

**Deferred:** generation 1 is already running as `subagent:nestedlineage`, worker `01a184af-7e89-7dd2-bc93-f7fa9388a9a0`, on `fix/nested-run-lineage` in `/home/brian/worktrees/agent-doc-nestedlineage` at `8862b2f9b5`. The operator extended this item after dispatch, including the child "Use the AST to guide the agent." That text is in `OPERATOR-STEERING.md` in the same worktree for this worker. The same worker keeps the claim.

> **Queue prompt:** release + publish
>   - Include #subagent:nestvector + #subagent:nestedproject
>   - Include #subagent:nestedlineage

**Deferred:** generation 5 already reported this row blocked. The include list now also names `#subagent:nestedlineage`. The same release claim stays reported. The row stays open until `#subagent:nestedproject` and `#subagent:nestedlineage` are both on main.

### Re: nested lineage stays with its worker — grok-4.7 · 2026-10-28T18:51-04:00

`#subagent:nestedlineage` is claimed and running. A nested run submission has to carry the parent lineage. In the api.md example, the `#rebase` child under `https://github.com/haiven-dev/haiven-api/pull/758` became `[#subagent:pr767-rebase#1] #rebase` and bound to the wrong PR when only `#rebase` was sent. The same worker has the added instruction to use the AST, and that instruction is written in its worktree.

`#subagent:nestedproject` is still on `fix/nested-project-md` at `8862b2f9b5` with uncommitted edits and no commit. `#apikeepsrefusing` is still running on a clean `fix/api-keeps-refusing`. The published release is still agent-doc v0.35.560 at `bd971e9b87`.
> **Queue prompt:** release + publish
>   - Include #subagent:nestvector + #subagent:nestedproject

### Re: release report is blocked — grok-4.7 · 2026-10-28T18:54-04:00

The release worker's report is persisted. Release is blocked. No version bump, tag, `make release`, `make install`, or release commit was made.

`#subagent:nestvector` is on origin/main at `8862b2f9b5`. `#subagent:nestedproject` is not on origin/main. The published release is still agent-doc v0.35.560 at `bd971e9b87`.

The same row also listed `#subagent:nestedlineage`. That work is still running on `fix/nested-run-lineage`, and `#subagent:nestedproject` still has uncommitted edits on `fix/nested-project-md`. A release that includes both waits until those two are on main.
<!-- agent:boundary:22ffb2cb:agent-doc-bugs -->
<!-- /agent:exchange -->

## Run <!-- agent:run subagents preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 [#subagent:apikeepsrefusing#1] do [#apikeepsrefusing]
- ~~Error on api.md: [route] Sent 2 updates to the agent already running for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/api.md; it will handle them in this turn. No second trigger was injected. steering_delivered=2 sent=explicit ui_outcome_contract=ui-outcome-v1 ui_outcome=steering_delivered_to_owner ui_outcome_class=ok next_action=owner_turn_handles_steering~~
- 📥 [#vscodecodersmoke]
  - ~~Where does this file live? My Coder agent is asking.~~
- ~~The queue prompt in the response for a nested agent:run item should show the entire nest "vector" such as `root parent: parent: item`.~~
- 🚧 [#subagent:nestedproject#1] Nested .agent-doc/project.md files are not being honored. Agent Doc files in src/haiven-dev should read agent-loop/src/haiven-dev/.agent-doc/project.md and ageunt-loop/.agent-doc/project.md
- 🚧 [#subagent:nestedlineage#1] When submitting a nested run item, include the parent lineage using the nested prompts. For example, in api.md `https://github.com/haiven-dev/haiven-api/pull/758: #rebase` created `[#subagent:pr767-rebase#1] #rebase` which bound to the wrong PR. If you are only sending `#rebase` as the prompt, the agent does not understand the nesting. The agent should be able to understand the entire tree and the actual prompt list item that is being sent.
  - Use the AST to guide the agent.
- ☑️ [#subagent:release#5] release + publish
  - Include #subagent:nestvector + #subagent:nestedproject
  - Include #subagent:nestedlineage
<!-- /agent:run -->

## Loop <!-- agent:loop interval="1h" priority -->
- 📥 Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.
<!-- /agent:loop -->

## Todo <!-- agent:todo priority -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#coder-remote-install-02805] Repair Coder Remote Dev sync after the two-sided install: open /home/brian/work/btakita/agent-loop as the backend IDE project root; ensure Client/Gateway and backend both load JetBrains 263 plugin 0.2.824; restart both; on the backend run agent-doc env --json, agent-doc tmux ensure tasks/agent-doc/agent-doc-bugs.md --ide-terminal --json, and agent-doc session status tasks/agent-doc/agent-doc-bugs.md; require client idea.log to show snapshot schema negotiated with geometry=measured, backend idea.log to show accepted ingress with no DELIVERY_FAILED, and .agent-doc/logs/ops.log to show controller_editor_surface_observed client=jetbrains-rd@... surface=remote-client:... followed by pane_layout_desired_published with column_order=editor. If DELIVERY_FAILED No such file or directory persists, fix ControllerSurfaceSnapshotPublisher to resolve the controller from each session document root instead of an unrelated IDE project basePath, expose connected-without-accepted-publication as blocked while preserving the no-guessing fence, and add a regression test.
- [ ] [#gh379-remotedev-proof] Close GH #379 (https://github.com/btakita/agent-doc/issues/379) after proof from its Remote Dev setup (Windows JetBrains Client): on that backend, `agent-doc --version` is 0.35.555 or newer and the IntelliJ IDEA 2026.3 plugin is 0.2.835 or newer; Run Agent Doc then ends exit 0 and ops.log shows `controller_editor_route_measured_layout_resolved` with no `editor route requires the exact visible layout before dispatch`.
- [ ] [#vscodecodersmoke] Run scripts/vscode-remote-smoke.py on a real Coder workspace (procedure in runbooks/coder-workspace.md): install, register, remote extension host, tmux attach, remoteName, layout sync; then update the VS Code on Coder status cell in editors/SPEC.md section 13.
- [ ] [#yeja] Choose how to handle ~/.agent-doc (see the #ddj8 answer in agent:exchange): (a) never treat $HOME as a project root, mirroring the tsift fix (icebox #homerootguard), (b) keep agent-doc activity in a child git repo such as ~/notes, (a)+(b) (recommended), or (c) status quo. Separately, ~/.agent-doc holds 1.8G (1.7G in the retired live-buffer/) that you can archive or delete. (source: agent:icebox [#homerootguard])
- [ ] [#automationapiquestions] Answer the automation-API design questions (specs/proposals/automation-api.md on branch design/automationapi)
  - Who is the first real caller: CI, a dashboard, another agent, or a bridge?
  - Local-only, or remote and multi-user (which needs a security spec first)?
  - Should "start a turn" ever be an API operation?
  - Will automation ever write document text, and which components could it own?
  - Plain JSON envelope, or the Lazily `CommandSubmit` envelope?
  - Should API calls launch a controller, and should event subscribers keep it from retiring?
  - What stability promise for v1, and how long a deprecation window?
  - One endpoint per project, or a user-level gateway across projects?
- [ ] [#pmsyncconfirm] Confirm the PM-sync research defaults (specs/proposals/pm-sync-integrations.md, decision 12 and "Still open")
  - GitLab default: union of open MRs (created_by_me, assigned_to_me, reviews_for_me), open issues (created_by_me, assigned_to_me) and items behind pending `mentioned`/`directly_addressed` to-dos. It misses items you only commented on. OK?
  - Jira default: `(assignee = currentUser() OR reporter = currentUser() OR creator = currentUser() OR watcher = currentUser()) AND resolution = Unresolved`. Comments count only through auto-watch, and mentions are not matched. OK?
  - Should we rely on GitHub advanced search ORing PR queries (observed, not documented), with the two-query fallback?
<!-- /agent:todo -->

## Backlog <!-- agent:backlog priority queue -->
- [ ] [#apikeepsrefusing] api.md keeps on refusing to push #rebase and publish #review. How can we make this work?
<!-- /agent:backlog -->

## Review <!-- agent:review -->
- [/] [#jetbrainsmarketplaceprofile] [marketplace-approval] [agent-doc] Plugin 35436 is hidden and under initial JetBrains moderation; after approval, replace the basic description with polished copy, add Getting Started, Documentation, and Bugtracker URLs, then verify the public listing.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
- [/] [#gh299-live-263-proof] Live-verify v0.35.519 on JetBrains 263.7031 with changed plugin sources: `agent-doc upgrade` must hot-reload and complete without an IDE restart; close GH #299 only with that proof.
<!-- /agent:review -->

## Icebox <!-- agent:icebox -->
- [ ] [#qresiduewedgeveto] The `#qresiduewedge` committed-heading replay strike path does not apply `#subagentinlineveto` yet, so it could still close a `subagents` dispatch row that was answered inline (rare).
- [ ] [#lazilyspecagentdocfixtures] agent-doc-specific schemas and fixtures still live in lazily-spec (`schemas/agent-doc-state.json`, `conformance/agent-doc/`, the `editor_route` and `sync_tmux_layout` fixtures): move them into agent-doc or make them generic (automation-API proposal section 5).
- [ ] [#apiversionexempt] `command_plane_submit` refuses any caller whose binary version differs from the controller's; the future public `agent-doc api` command must be exempt or every install breaks automation clients (automation-API proposal, Phase 2).
- [ ] [#trackededitforward] An operator edit to a backlog item or Todo that a claimed queue row references is never forwarded to that worker (`forward_to_owner`); steering watches only exchange and queue edits (`#steerbacklogsource`). Affects backlog and Todo alike.
- [ ] [#tododonestrikerow] `--todo-done <id>` does not strike the matching `do [#id]` queue row in the same closeout the way `--done` does; the next preflight closes it (`source=todo_checked`).
- [ ] [#todopriorityinert] `<!-- agent:todo priority -->` does nothing: no pass sorts the Todo, and Todo `priority=` tokens do not rank `do [#todo]` rows under `agent:run priority`.
- [ ] [#tododupefold] Backlog drops identical same-id duplicates every preflight; the Todo folds them only after `patch:todo` or `--todo-done`.
- [ ] [#echosteermisread] Exchange steering reports the coordinator's own committed `> **Queue prompt:**` echo cells as new operator prompts, on almost every cycle.
- [ ] [#claimmarkersteer] Steering reports a claimed row as operator-edited (dispatch=forward_to_owner) when the only difference is the binary ☑️ claim marker plus nested rows that were there at claim time. It asks to forward to a worker that already finished.
- [ ] [#servegetdisk] serve follow-ups: GET /doc still reads disk, not the authority; a browser save carries no base revision, so it cannot be merged; the editor-attached check races the commit.
- [ ] [#holdroundtrip] The in-flight typing hold adds a controller round-trip to every direct CLI write and can stall up to 60s. session-check should report the quarantine state that the new commit refusal names.
- [ ] [#nestedtodograndchildren] `--todo-add` / `--todo-edit` accept one nesting level only; grandchildren and checkbox children are refused.
- [ ] [#controllersocketlock] From the automation-API design: lock down the controller socket (socket/dir permissions, SO_PEERCRED same-user check) and stop a request that omits binary_version from skipping the stale-binary refusal (agent-doc-controller-io rpc.rs:21823-21833); fix specs/10-security.md:10 ('no network services') now that agent-doc serve can bind non-loopback with bearer tokens. Also from nested sub-prompts: a do [#id] parent with nested free-text children still runs the children as separate heads.
- [ ] [#jbdocdrift] JetBrains doc drift found by #movedsingleplugin: editors/PLUGIN-SPEC.md:33-35 says two disjoint packages (there are three); PLUGIN-SPEC.md:74-79 keeps the old full module-graph replacement wording that contradicts the restated Exact-263 reopen rule (editors/jetbrains/SPEC.md:286-294,701-720); editors/jetbrains-262/README.md:114-118 says the resolver must not choose the exact-262 ZIP (it now picks the ranged ZIP). Also pending operator decision: merge 262+263 into one 262-263.* modular ZIP (option B). And VS Code Restart Agent needs the same --expect-agent check as JetBrains.
- [ ] [#cwdfallbacktail] Leftover cwd uses found by #cwdfallbackrest: (1) agent_doc_git_io::dirs::resolve_pane_cwd falls back to the process cwd for a document outside any git repo, and claim/route record it as the registry entry's cwd (should likely be the canonical parent); (2) agent-doc-sync ResyncTargetMatcher / resolve_absolute_file_path resolve relative registry entries against the process cwd (filter_registry_for_target, targeted prune) instead of the store; (3) terminal::run builds Tmux::default() instead of configured_tmux(), ignoring AGENT_DOC_TMUX_SOCKET and the configured tmux binary (also why terminal has no CLI test).
- [ ] [#compactrequester] From #compactabsorbsedit: controller_compact_document does not log who requested the compact (editor action, CLI, agent); add a requester= field. Also review whether operator edits inside the exchange before the boundary marker should still be archived and committed by compaction.
- [ ] [#orphandrainprobe] The orphan-drain liveness probe (supervisor_pid_for_doc) only matches 'start --route-owned', so a plain 'agent-doc start doc.md' supervisor (which runs an idle watch) reads as orphaned on every tick and only route-time deferral (GH #307, #autoclearorphandrain) prevents a double drive. Decide whether the drain should use start_supervisor_document_from_args (keeping kill paths route-owned only) and its effect on the GH #307 fallback. Also: tmux_session_policy_for_scope / sync_scope_root (agent-doc-sync/src/lib.rs:623) and route session_resolution.rs:41/223 read the cwd pin with no document (likely process/layout level).
- [ ] [#retireddirsgc] Other apparently retired directories under real .agent-doc stores (patches/, plugin-owner/, ack-content/, op-capture/; already forbidden by the hot-path guard) could join #livebuffergc's agent-doc gc --retired-stores list after the same retirement check. Also undecided (#queuerowdup): should a committed plain 'do [#id]' twin of a claimed row be collapsed or flagged (collapsing deletes operator-typed text)?
- [ ] [#homerootguard] Option (a) from the #ddj8 research, pending the operator's choice: never treat $HOME, / or the temp dir as an agent-doc project root (agent-doc-fs/src/lib.rs:44-61 find_project_root plus the JetBrains PatchWatcher.kt:288-297 / TurnStateBridge.kt:295 / CrdtReplicaManager.kt:5104 and VS Code extension.ts:318 root walkers, which also ignore the root ceiling) unless an override such as AGENT_DOC_ALLOW_HOME_ROOT=1 is set or ~/.git exists; move user-level logs to $XDG_STATE_HOME/agent-doc.
- [ ] [#rootlessstorecreate] Since #startcwdfallback, the first start of a document with no .agent-doc ancestor creates .agent-doc/ in the document's parent directory (as the bootstrap paths always did). Readers that skip rootless documents (turn-status, session-actor generation/log paths, start-runtime-io lib.rs:304/339, run.rs:196/404/1012) then find that store; decide whether a rootless store should be created at all or whether those readers should treat it consistently.
- [ ] [#citedworknag] Since #todoaddonsamecycledone, a follow-up todo created in the closeout that resolved its cited item is standalone, so preflight's cited_work_resolved advisory fires on every preflight for it (e.g. #krwr, #ddj8) even though it is outstanding by design. Quieting it needs a creation marker the document does not record. Also: claim still reads the process cwd's registry in find_alive_project_window (--window fallback) and the cwd config in project_tmux_session() (read-only; left to keep admission behavior).
- [ ] [#ownerlifecyclerevive] The owner lifecycle writer (store_lifecycle_actor_record_for_runtime callers, agent-doc-controller-io rpc.rs ~27253) decides from the runtime actor record and writes on generation only, so a same-generation Closed landing in that window could be overwritten by Ready. It is an owner write (kept generation-only by #controllerclosefence/#bindingcallersaudit); decide whether it should refuse to revive a Closed actor. Also unverified from #drainbusymask: a durable Busy with in-memory Ready would still block an admin drain (no writer found that produces it).
- [ ] [#evictdeadcode] evict_cross_document_pane_bindings (agent-doc-controller-io/src/project_controller.rs, fenced by #controllerclosefence) has no callers in the workspace; remove it or wire it.
- [ ] [#predispatchblockedaudit] After #predispatchreadyoverwrite, a Blocked or Closed record on the same pane, and a different-session record on the same pane, still get the pre-dispatch Ready rebind; audit whether a Blocked owner or the route's starting-timeout Blocked latch should be preserved. Related: after a stash rescue the record's window_id stays stale until sync's (now fenced, window-only) refresh repairs it.
- [ ] [#jbropeshadow] Make the JetBrains replica shadow a non-String text (rope or piece table) in CrdtReplicaManager and CrdtReplicaForwarder (knownReplicaText, the next flush base), so a flush stops allocating and copying the whole next shadow; that copy is now the only document-size-dependent part of prepareLocalEditorEditsUtil (about 25 us at 125 KB, 177 us at 500 KB). Gate on make jetbrains-keystroke-bench.
- [ ] [#jbfrontendswap] [agent-doc] Remote Dev frontend implementation swaps for the GH #339 shell design: let the JetBrains Client load the swappable implementation from the backend so frontend changes stop needing a restart. Parked 2026-10-26: it pushes backend code into the local client JVM without a plugin install, which changes the security boundary. The frontend stays shell-only (editors/jetbrains/SPEC.md, "Restart-free implementation generations").
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Done <!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
