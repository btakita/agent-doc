---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: f08cd727-ebd1-4b95-8305-635688e2d56c
  codex: 01a18932-dcae-7493-8021-dd8a09572c34
agent: codex
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
Main is 7b44d8bb28. The published release is v0.35.561, published 2026-10-29T14:24:45Z, at https://github.com/btakita/agent-doc/releases/tag/v0.35.561. origin/main and tag v0.35.561 are 7b44d8bb28. Release run 40891180347 succeeded. PyPI run 40898500134 succeeded and agent_doc-0.35.561-py3-none-any.whl is public. Book run 40891180393 succeeded. Pages run 40891180439 succeeded. CI run 40891180342 is in progress. The installed binary sha256 is 51c35579dad108a4d2cafc7a2fdbed1ed3b01a1cd0f98b6eb023eab0cb6b6634. agent-doc --version is agent-doc 0.35.561. v0.35.560 remains bd971e9b87. 1bbc28a6a9, now an ancestor of main, 1bbc28a6a9 keys a nested free-text leaf by its parent lineage. A unique later sibling is still dispatched as the bare text Second route child gets its own worker. Lineage stays on the authorization and the submitted prompt. The completion cell keeps the child in held_work and the parent stays absent. The coordinator make check on that SHA exited 0 (nextest 12176 passed, 187 skipped). make install of 1bbc28a6a9 exited 0. The installed binary sha256 is 03b775ac10f60cf6c6c11dc32c8d42ce3eeab1a69e45d6e6ce209220be4f4b7f. agent-doc --version is agent-doc 0.35.560. That install recorded no JetBrains plugin byte change, so no JetBrains restart is required. reload_library delivered 0 of 1 editor endpoints, with 1 deferred mid-cycle. e5d6bb0f6a, now an ancestor of main, skips a queue held by pause, including a legacy marker stop, and stays live so after= still waits. Three runs whose first is paused still offer the second and the third. A focused-cycle head is not a pause and still gates. An earlier inline queue that still has owner-available work gates later queues. The worker make check exited 0 (nextest 12173 passed, 187 skipped). make install of e5d6bb0f6a exited 0. That install's binary sha256 was 2b32ca0272cdb65ea290bbddef018b964d4c692d9ef4cfec23687775a70f338b. reload_library delivered 0 of 1 editor endpoints, with 1 deferred mid-cycle. 9c09d2aadc, now an ancestor of main, pauses a queue whose executable heads are all claimed, reported, operator-gated, or group labels, and later subagents queues are still offered in that turn. An after= predecessor that still has that work stays closed. The worker make check exited 0 (nextest 12170 passed, 187 skipped). make install of 9c09d2aadc exited 0. That install's binary sha256 was cc4b7c2431027a4b776ca2e7f02792274e41862a5cd7e78eb456d9133b4eb5e8. reload_library delivered 1 of 1 editor endpoints. 0552d86df0, now an ancestor of main, offers every subagents queue whose after= predecessors are empty, so a claimed agent:run no longer hides a later ready agent:run. An earlier inline queue that still has owner-available work gates later queues. The worker make check exited 0 (nextest 12160 passed, 187 skipped). make install of 0552d86df0 exited 0. That install's binary sha256 was a035a1027d8dc4fb0d900f4014ea38ee6a7d89f4a0de3f6652af3602d3ba816f. reload_library delivered 1 of 1 editor endpoints. 3568802068, now an ancestor of main, submits the latest document diff into a busy Grok turn and reports steering_delivered_to_owner only after tmux accepts that submit. The worker make check exited 0 (nextest 12159 passed, 187 skipped). make install of 3568802068 exited 0. That install's binary sha256 was bbb1284f90959905511b3b33c49edd68684b43d9e0c17d65b71250b4408f88b3. reload_library delivered 1 of 1 editor endpoints. 10c88c343d, now an ancestor of main, reports an unclosed opener as unclosed component: the component has no close `/agent:NAME` (opening marker at line N). A later bare opener stays another opener. The worker make check exited 0 (nextest 12156 passed, 187 skipped). make install of 10c88c343d exited 0. That install's binary sha256 was 0af6484f0f296cb5306cfee9d8d2092eff2cc70722af1c17fa390007f68b35d2. reload_library delivered 0 of 1 editor endpoints, with 1 deferred mid-cycle. cf2469c43b, now an ancestor of main, keeps go, start, pause, and stop off the default agent:run scaffold because go-mode is already the default. The worker make check exited 0 (nextest 12155 passed, 187 skipped). make install of cf2469c43b exited 0. That install's binary sha256 was bbbfc5bfb9e11cbfd7ed2dc58f6e7a6e9ab404824d2f065efc4a0d7df13c497d. 83e423d98d is now an ancestor of main. The previous published release was v0.35.560. CI for 1bbc28a6a9 succeeded (run 40823503416); Book succeeded (run 40823503452); Pages succeeded (run 40823503419). CI for e5d6bb0f6a succeeded (run 40821941589); Book succeeded (run 40821941642); Pages succeeded (run 40821941607). CI for 9c09d2aadc failed (run 40819362750); Book succeeded (run 40819362802); Pages succeeded (run 40819362792). CI for 0552d86df0 succeeded (run 40815027714); Book succeeded (run 40815027831); Pages succeeded (run 40815027835). CI for 3568802068 was cancelled (run 40814610845); Book succeeded (run 40814610830); Pages succeeded (run 40814610906). CI for 10c88c343d succeeded (run 40811577207); Book succeeded (run 40811577166); Pages succeeded (run 40811577138). CI for cf2469c43b succeeded (run 40809926777); Book succeeded (run 40809926784); Pages succeeded (run 40809927282). CI for 83e423d98d was cancelled (run 40808095127); Book succeeded (run 40808095125); Pages succeeded (run 40808095131). CI for 8ae42f4429 was cancelled (run 40807962750); Book succeeded (run 40807962657); Pages failed (run 40807962569). CI for d5b8ccf9f7 was cancelled (run 40804949130); Book succeeded (run 40804949159); Pages succeeded (run 40804949114).
<!-- /status:sync -->

Queue markers: 📥 pending · ⛔ blocked · 🟡 claimed · 🚧 running · ☑️ reported · ⚠️ lost · 🔄 needs reconcile · ❔ unknown · ⏭️ skipped · ⏳ awaiting operator · 🙋 operator action · 📌 pinned · 📍 agent pin ([legend](https://github.com/btakita/agent-doc/blob/main/docs/guide/queue-markers.md#queue-markers))
<!-- /agent:status -->
 

## Exchange <!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261030-003656.md`*

Compacted content:
- Archived 89 response topic(s): agent:rules dispatch; release waits on agent:rules; agent:rules on main; 86 more
- Prior summary/context: prior compacted content: Archived 68 response topic(s): do [#clearqueuedciflake], do [#startcwdfallback], [#krwr] dec...

### Re: Actionable context, cross-run dedupe, and Todo sync — gpt-5 · 2026-10-29

> **Queue prompt:** Should these run items be struck? If yes, fix agent-doc causes.: What is actionable about those items? Can agent-doc express what needs to be done? agent-doc should add a child agent:run item to make closing it actionable. It's ok if the actionable item is blocked. The main thing is we should have a goal of completing the agent:run items and never leave a agent:run item blocked.

Fixed. Queue settlement is now deepest-first: a plain context parent closes after all executable descendants finish; if unresolved lifecycle, owner, or operator evidence remains, agent-doc removes inert parent lifecycle state and projects one concrete executable child carrying the remaining action. The child may be blocked, but the prompt tree no longer ends as an inert blocked/lost parent.

The contamination guard now compares against real pre-write provenance, so a pre-existing operator context row quoted in `### Re:` is not misclassified as copied assistant prose.

Implemented in `20d9d7c984`; full worker `make check` passed 12,217 Rust tests plus editor/plugin suites.

> **Queue prompt:** agent-doc issues in api.md:: `- 🚧 [#subagent:api-contracts-pr170-closed#1] do [#closed]` was duplicated in the `agent:run id="main"` from where it was originally defined in `agent:run id="contracts-prs"` under `https://github.com/haiven-dev/haiven-contracts/pull/170`.

Fixed. Same-closeout, preflight, and one-shot backlog mirroring now check tracked identity across all non-repeating sibling run sections. An existing nested `do [#closed]` in `contracts-prs` prevents a duplicate top-level mirror in `main`, preserving its original ancestry.

Implemented in `9ebb0b2e5b`; four focused regressions and the worker’s full `make check` passed.

> **Queue prompt:** sync agent:todo with current decision state. Mark any agent:todo items done that were already answered.: [#gh379-remotedev-proof]: done

Done. The operator’s explicit assertion is sufficient to close the local Todo; `#gh379-remotedev-proof` is checked off by this closeout.

> **Queue prompt:** sync agent:todo with current decision state. Mark any agent:todo items done that were already answered.: [#coder-remote-install-02805]: done? It seems like the Coder instance doesn't have `If DELIVERY_FAILED No such file or directory persists` anymore. Will file another gh issue if it comes up again.

Done. This is an operator-accepted symptom closure rather than a fresh replay of every original positive proof. `#coder-remote-install-02805` is checked off; a recurrence can become a new GitHub issue as requested.

> **Queue prompt:** sync agent:todo with current decision state. Mark any agent:todo items done that were already answered.: [#ahfe]: done

Done. The explicit operator assertion supersedes the older watch-state note. `#ahfe` is checked off; this changes only the local Todo and does not mutate GH #267.

> **Queue prompt:** sync agent:todo with current decision state. Mark any agent:todo items done that were already answered.: [#subagent-operator-request-decision-annotation-syntax]: already decided?

Not yet. `Other` was resolved as the answer value and the reserved request id as the identity, but scope remains open:

- Option (2): only reserved `[#subagent-operator-request-…]` IDs are decisions.
- Option (3): any cited open Todo/operator-request ID can be a decision.

Choose (2) or (3); the Todo remains open.

> **Queue prompt:** sync agent:todo with current decision state. Mark any agent:todo items done that were already answered.: [#subagent-operator-request-inbox-terminal-reading]: already decided?

Not yet. The worker-complete `☑️` was not an operator choice. Choose:

- **Start a new turn:** `📥` is a valid ready leaf that can remain for a later turn.
- **The subprompt is in a bad state:** closeout must not commit that `📥`; it removes or reclassifies it in the same turn.

The Todo remains open.

Merged verification across both fixes passed for queue, preflight, queue-I/O, workflow, and session-check crates. One `make install` completed successfully; no editor package bytes changed, and no JetBrains restart is required. The integrated source is pushed to `origin/main` at `9ebb0b2e5b`.
<!-- agent:boundary:7bbc0262 -->
<!-- /agent:exchange -->

## Run <!-- agent:run subagents preset=#spec-test-build-install-commit-push go -->
- If I have a long list of agent:run items, it is difficult to see what was worked on recently. What are some UI affordances that we can have to quickly distinguish what was recently address in a turn? Should we annotate with a timestamp? Does the Editor provide a way to temporarily highlight recently worked on items, where navigating the cursor to the row would remove the highlight? [#subagent-operator-request-recent-run-affordance]
  - ~~If we use Timestamp annotation, the plugin can have highlights for the most recent, the 2nd most recent, the 3rd most recent. If there's a way to make the Recency glyph numeric, the recency order can be provided.~~ — auto-struck: answered this cycle (#ftstrike) ⟦addressed-at:2026-10-30T00:07:43.000Z⟧
- 🙋 If a decision with an id was made, annotate the prompt with the decision id. In game-sessions.md, there are multiple usages of a single decision id. Make sure that all instances of the decision id are annotated with the decision. Update the state of the prompt item appropriately. Restart work on the prompt if the prompt was blocked by the made decision across all instances of the blocking decision. [#subagent-operator-request-decision-annotation-syntax]
  - ~~The decision id is often a short phrase such as `**Other**`, correct? Or do you need a surrogate key for the decision id?~~ — auto-struck: answered this cycle (#ftstrike) ⟦addressed-at:2026-10-30T00:07:43.000Z⟧
- ☑️ [#subagent:inboxterminal#1] In api.md, there are leftover `  - 📥 #address` and `  - 📥 #rebase: #address` subprompts. The `📥` state of a turn since it is ambiguous. Does this mean the operator needs to start a new turn? or Does this mean the subprompt is in a bad state? If the subprompt is in a bad state, should we have a reliable error emoji? I mainly want `📥` to be unambiguous. Ideally, the turn won't terminate with prompts being in the `📥` state.
- Sync Issues on sdk-demo.md
  - ~~Sync Tmux Pane: Sync failed: sync_tmux_layout refused before layout publication: focused document `/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md` is absent from the desired columns~~ — auto-struck: answered this cycle (#ftstrike) ⟦addressed-at:2026-10-30T00:07:43.000Z⟧
  - ~~Run Agent Doc: editor route layout refused by the main-window owner /home/brian/work/btakita/agent-loop: sync_tmux_layout refused before layout publication: focused document `/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk/sdk-demo.md` is absent from the desired columns~~ — auto-struck: answered this cycle (#ftstrike) ⟦addressed-at:2026-10-30T00:07:43.000Z⟧
- Should these run items be struck? If yes, fix agent-doc causes.
  - ~~`⚠️ [#subagent:recentrunverify#2]`~~ — auto-struck: answered this cycle (#ftstrike) ⟦addressed-at:2026-10-30T00:33:09.000Z⟧
  - ~~`- 🙋⚠️ [#subagent:decisionsurrogate#2]`~~ — auto-struck: answered this cycle (#ftstrike) ⟦addressed-at:2026-10-30T00:33:09.000Z⟧
  - ~~`- Sync Issues on sdk-demo.md`~~ — auto-struck: answered this cycle (#ftstrike) ⟦addressed-at:2026-10-30T00:33:09.000Z⟧
  - ~~`- If I have a long list of agent:run items, ...`~~ — auto-struck: answered this cycle (#ftstrike) ⟦addressed-at:2026-10-30T00:33:09.000Z⟧
  - ~~What is actionable about those items? Can agent-doc express what needs to be done? agent-doc should add a child agent:run item to make closing it actionable. It's ok if the actionable item is blocked. The main thing is we should have a goal of completing the agent:run items and never leave a agent:run item blocked.~~ — auto-struck: answered this cycle (#ftstrike) ⟦addressed-at:2026-10-30T00:53:25.000Z⟧
- agent-doc issues in api.md:
  - ~~`- 🚧 [#subagent:api-contracts-pr170-closed#1] do [#closed]` was duplicated in the `agent:run id="main"` from where it was originally defined in `agent:run id="contracts-prs"` under `https://github.com/haiven-dev/haiven-contracts/pull/170`.~~ — auto-struck: answered this cycle (#ftstrike) ⟦addressed-at:2026-10-30T00:53:25.000Z⟧
- sync agent:todo with current decision state. Mark any agent:todo items done that were already answered.
  - ~~[#gh379-remotedev-proof]: done~~ — auto-struck: answered this cycle (#ftstrike) ⟦addressed-at:2026-10-30T00:53:25.000Z⟧
  - ~~[#coder-remote-install-02805]: done? It seems like the Coder instance doesn't have `If DELIVERY_FAILED No such file or directory persists` anymore. Will file another gh issue if it comes up again.~~ — auto-struck: answered this cycle (#ftstrike) ⟦addressed-at:2026-10-30T00:53:25.000Z⟧
  - ~~[#ahfe]: done~~ — auto-struck: answered this cycle (#ftstrike) ⟦addressed-at:2026-10-30T00:53:25.000Z⟧
  - 🙋☑️ [#subagent:decisiontodosync#1] [#subagent-operator-request-decision-annotation-syntax]: already decided?
  - 🙋☑️ [#subagent:inboxtodosync#1] [#subagent-operator-request-inbox-terminal-reading]: already decided?
<!-- /agent:run -->

## Loop <!-- agent:loop interval="1h" priority -->
- 📥 Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.
<!-- /agent:loop -->

## Todo <!-- agent:todo priority -->
- [x] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [x] [#coder-remote-install-02805] Repair Coder Remote Dev sync after the two-sided install: open /home/brian/work/btakita/agent-loop as the backend IDE project root; ensure Client/Gateway and backend both load JetBrains 263 plugin 0.2.824; restart both; on the backend run agent-doc env --json, agent-doc tmux ensure tasks/agent-doc/agent-doc-bugs.md --ide-terminal --json, and agent-doc session status tasks/agent-doc/agent-doc-bugs.md; require client idea.log to show snapshot schema negotiated with geometry=measured, backend idea.log to show accepted ingress with no DELIVERY_FAILED, and .agent-doc/logs/ops.log to show controller_editor_surface_observed client=jetbrains-rd@... surface=remote-client:... followed by pane_layout_desired_published with column_order=editor. If DELIVERY_FAILED No such file or directory persists, fix ControllerSurfaceSnapshotPublisher to resolve the controller from each session document root instead of an unrelated IDE project basePath, expose connected-without-accepted-publication as blocked while preserving the no-guessing fence, and add a regression test.
- [x] [#gh379-remotedev-proof] Close GH #379 (https://github.com/btakita/agent-doc/issues/379) after proof from its Remote Dev setup (Windows JetBrains Client): on that backend, `agent-doc --version` is 0.35.555 or newer and the IntelliJ IDEA 2026.3 plugin is 0.2.835 or newer; Run Agent Doc then ends exit 0 and ops.log shows `controller_editor_route_measured_layout_resolved` with no `editor route requires the exact visible layout before dispatch`.
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
- [ ] [#subagent-operator-request-decision-annotation-syntax] Pick how a made decision id annotates prompts and whether that restarts them. Worker 01a188dc-ebd3-7711-87c6-67034f7b9536 made no commit. (1) Blocker-close stamp: ⟦decision:<id>⟧ on live do [#id] rows when a stays-open or after= blocker leaves the open set; the last blocker returns the row from ⛔ to 📥. (2) Request-answer stamp and requeue: only [#subagent-operator-request-…] ids, token ⟦decision:<id>=<answer>⟧, every citing row returns to 📥 and its claim is released. (3) Cited-id answer stamp and requeue: any [#id] naming an open todo or operator request, made when that todo is checked or any instance has a nested answer; every unstruck citing row is stamped, loses ☑️ ⛔ 🙋, and returns to 📥. (4) Stamp only: same detection and token as (3), with no lifecycle change and no release. (5) Other.
- [ ] [#subagent-operator-request-inbox-terminal-reading] Which reading should a closeout adopt when a nested #address or #rebase: #address row would stay at 📥? Worker 01a188df-8bd1-7bf1-938a-d7bb1d907c05 made no commit. Branch fix/inbox-not-terminal stays at 7b44d8bb28. (1) Start a new turn: the nested row stays its own pending head, 📥 means ready to run, and another row's closeout may commit while this row is still 📥. (2) The subprompt is in a bad state: do not commit that 📥; treat the row as a sub-prompt or failed work and remove the pending marker in the same turn. The worker prompt ended before the later sentences about a reliable error emoji, making 📥 unambiguous, and not ending a turn while prompts are 📥.
<!-- /agent:todo -->

## Backlog <!-- agent:backlog priority queue -->
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
