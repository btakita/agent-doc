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



<!-- status:sync -->
Latest release: agent-doc v0.35.559 (release commit `5870713c02`; GitHub Latest with 24 assets; PyPI 0.35.559; Homebrew bumped). Main is now at `dc7efbcbf6`, unreleased and installed locally (JetBrains plugins 0.2.877/0.2.878/0.2.879 installed; restart IDEA to load them). Since 559: operator request packets nest the queue_item/request/context values (`dc4fc37fcc`); reported ☑️ rows close for tagged id rows, answered requests and acked open backlog (`0b27368c0e`); editor registration proves from the acknowledged replica text and frontmatter merges keep operator bytes (`b8932c1b90`); a quoted or unrelated deferral phrase no longer holds an exactly echoed row (`0dd4eeb0be`); the repair remedy shows only when no editor replica is attached (`5426def761`); a claim never reopens a struck row whose work is live in the same queue (`1e24cd70dd`); agent component patches survive the response-only rebase, with a warning if one is dropped (`220ba2d791`); Claude Stop holds once for a reported but unclosed queue row (`edc4debc24`); an editor undo of a status:sync block is detected and re-synced (`dffba0edcb`); an operator answer row nested under its request id is accepted and row splitting ignores binary markers (`dc7efbcbf6`). Main CI on `5870713c02` is red in tmux-ci (`two_ide_lane_claims_start_panes_in_their_own_lanes_end_to_end`).
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
<!-- agent:boundary:2e0fc005 -->
<!-- /agent:exchange -->

## Run <!-- agent:run subagents preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 [#subagent:agentrules#1] Add agent:rules tag, which provides context for each turn in the document. agent:rules takes a list or free text, similar to agent:run. This should also be supported with project.md and be composable. Multiple `agent:rules` elements should be allowed in a document.
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
