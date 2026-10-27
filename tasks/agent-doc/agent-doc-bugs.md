---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 5c496cf2-d923-4cdb-bff1-73ad7c75c246
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

📤 Queue handoff: 2 reported items awaiting coordinator integration, verification, and closeout.
Main is at `b6b9af6e15` and agent-doc 0.35.532 is installed locally. Release v0.35.532 was published from `71f9801b49`. Post-release main now includes the inline-code-safe, occurrence-ordered queue preset authorization fix for GitHub issue #331 and the positive-evidence-only Remote Dev pane-order fix for GitHub issue #332; both issues are closed, the integrated full suite passed, and the current mainline was installed locally.

<!-- status:sync -->
Released and published agent-doc v0.35.556 from release commit `5cbfdbbc4c`; tag `v0.35.556` is the stable latest release (GitHub Latest, PyPI 0.35.556, stable target channels). JetBrains classic/262/263 0.2.839/0.2.841/0.2.840, VS Code 0.2.83. GH #378, #380 and #381 closed; GH #379 stays open pending Remote Dev proof (todo `#gh379-remotedev-proof`). Post-release `origin/main` is `eebfdfef07` (unreleased, installed locally; CI green at `eebfdfef07`; the `95ae7fe959` red was a real queued-clear race that passes intermittently (`#clearqueuedciflake`: a /clear could be typed into a just-started turn; fix on branch, not yet on main); JetBrains classic/262/263 0.2.862/0.2.863/0.2.864 need an IDE restart, VS Code 0.2.89, Zed 0.1.2): the in-IDE restart-required prompt, no-argument `/agent-doc` via `agent-doc document`, `agent-doc queue repair-stale-completion`, id-less `agent:run` / `agent:loop` named by kind, the GH #376 measured-route ordering fix, the GH #339 shell design with stage R1 complete and the adopted Exact-263 reopen rule, route-outcome / exchange-prefix-repair / session-command-outcome moved into the hot-reloaded native library (JetBrains and VS Code), an edit-proportional native replica flush that also folds remote ops incrementally plus a windowed Kotlin flush-prepare step, the JetBrains editor-tree focus listener deferred until the split tree exists, todo `id=` always winning, the editor frontend/backend parity matrix, VS Code stale-VSIX prompt + measured layout + workspace extensionKind + activation receipt + code-server/Remote-SSH install, the VS Code koffi owned-string free crash fix and koffi shipped in the VSIX for 7 platforms, Zed registration identity + in-place binary handoff + document-only routing, a PreToolUse guard against agents killing the live tmux server, the GH #380 supervisor harness-binary install-dir fallback, GH #381 project-preset queue heads draining as presets, and queue claims that end on a strike and never follow across queues (`#claimfollowstruck`), the PreToolUse tmux guard in every harness user layer, the same install-dir fallback for headless harness launches (`#headlessharnesspath`), claim closure judged across every queue (`#claimclosureallqueues`), no actor write after a provisioning claim launches its start (`#claimpanecasrace`), a hermetic koffi round-trip test (`#vscodenativeceiling`), hermetic codex command-builder tests for CI runners without codex, fenced post-launch route rebinds that never overwrite the owner's actor record (`#dispatchrebindaudit`), a status-sync `agent-doc commit` that lands beside pending subagent completion cells (`#statussynccommitrefusal`), pre-dispatch rebinds that never write Ready over a running owner (`#predispatchreadyoverwrite`), sync binding writes fenced on generation plus actor record revision (`#syncbindingfence`), revision-fenced controller stale closes and prune (`#controllerclosefence`) and route lifecycle writes (`#bindingcallersaudit`), a supervisor stale-Busy self-heal that cannot wedge an admin drain (`#drainbusymask`), hermetic resync prune tests (`#prunetestisolation`), a rootless-document recovered-owner rebind that writes only the document's own store, fenced (`#syncnorootfallback`), claim binding in the document's store (`#claimwrongstore`), isolated tmux test servers sharing one socket with their clients (`#isotmuxtmpdir`), and one standalone todo when a closeout adds it beside --done of the item it cites (`#todoaddonsamecycledone`).
<!-- /status:sync -->

Queue markers: 📥 pending · 🟡 claimed · 🚧 running · ☑️ reported · ⚠️ lost · 🔄 needs reconcile · ❔ unknown · ⏭️ skipped · ⏳ awaiting operator · 🙋 operator action · 📌 pinned · 📍 agent pin ([legend](https://github.com/btakita/agent-doc/blob/main/docs/guide/queue-markers.md#queue-markers))
<!-- /agent:status -->
 

## Exchange <!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261027-124849.md`*

Compacted content:
- Archived 38 response topic(s): session resumed after /clear; do [#decisionbacklogstrike]; id-less run + loop, no-arg entrypoint, stale-cell repair; 35 more
- Prior summary/context: prior compacted content: Archived 49 response topic(s): Subagent completed: `subagent:grok_queue_ui` returned work fo...
<!-- agent:boundary:d7b4ac5d -->
<!-- /agent:exchange -->

<!--
-->

## Run <!-- agent:run subagents preset="#spec-test-build-install-commit-push" priority go -->
- ☑️ [#subagent:clearqueuedciflake#1] do [#clearqueuedciflake]
- ☑️ [#subagent:startcwdfallback#1] do [#startcwdfallback]
- do [#clearqueuedciflake]
- do [#startcwdfallback]
<!-- /agent:run -->

## Loop <!-- agent:loop interval="1h" priority -->
- 📥 Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.
<!-- /agent:loop -->

## Todo <!-- agent:todo priority -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#coder-remote-install-02805] Repair Coder Remote Dev sync after the two-sided install: open /home/brian/work/btakita/agent-loop as the backend IDE project root; ensure Client/Gateway and backend both load JetBrains 263 plugin 0.2.824; restart both; on the backend run agent-doc env --json, agent-doc tmux ensure tasks/agent-doc/agent-doc-bugs.md --ide-terminal --json, and agent-doc session status tasks/agent-doc/agent-doc-bugs.md; require client idea.log to show snapshot schema negotiated with geometry=measured, backend idea.log to show accepted ingress with no DELIVERY_FAILED, and .agent-doc/logs/ops.log to show controller_editor_surface_observed client=jetbrains-rd@... surface=remote-client:... followed by pane_layout_desired_published with column_order=editor. If DELIVERY_FAILED No such file or directory persists, fix ControllerSurfaceSnapshotPublisher to resolve the controller from each session document root instead of an unrelated IDE project basePath, expose connected-without-accepted-publication as blocked while preserving the no-guessing fence, and add a regression test.
- [ ] [#gh379-remotedev-proof] Close GH #379 (https://github.com/btakita/agent-doc/issues/379) after proof from its Remote Dev setup (Windows JetBrains Client): on that backend, `agent-doc --version` is 0.35.555 or newer and the IntelliJ IDEA 2026.3 plugin is 0.2.835 or newer; Run Agent Doc then ends exit 0 and ops.log shows `controller_editor_route_measured_layout_resolved` with no `editor route requires the exact visible layout before dispatch`.
- [ ] [#lazily-stale-cell-repair] After the next tasks/software/lazily.md cycle commits it clean (no uncommitted edits, pane prompt_ready=true), run `agent-doc queue repair-stale-completion tasks/software/lazily.md` (dry run, 14 stale completion cells as of 2026-10-26), then `--apply` and `agent-doc commit tasks/software/lazily.md`.
- [ ] [#vscodecodersmoke] Run scripts/vscode-remote-smoke.py on a real Coder workspace (procedure in runbooks/coder-workspace.md): install, register, remote extension host, tmux attach, remoteName, layout sync; then update the VS Code on Coder status cell in editors/SPEC.md section 13.
- [x] [#12t8] Decide whether deleting a claimed queue row while adding a different row at the same position should cancel the claim instead of handing it to the new row as an edit (current #queue-mutation-steering design; raised by the #claimfollowstruck fix). (source: agent:backlog [#claimfollowstruck])
- [ ] [#krwr] Decide whether deleting a claimed queue row while adding a different row at the same position should cancel the claim instead of handing it to the new row as an edit (current #queue-mutation-steering design; raised by the #claimfollowstruck fix).
- [x] [#4rb8] Decide whether ~/.agent-doc/ (which holds state.db) should make $HOME a project root: today any pane under $HOME with no closer .agent-doc/ resolves $HOME as its project root (found by #prunetestisolation; cf. the earlier ~/.tsift-made-$HOME-a-workspace fix). (source: agent:backlog [#prunetestisolation])
- [ ] [#ddj8] Decide whether ~/.agent-doc/ (which holds state.db) should make $HOME a project root: today any pane under $HOME with no closer .agent-doc/ resolves $HOME as its project root (found by #prunetestisolation; cf. the earlier ~/.tsift-made-$HOME-a-workspace fix).
<!-- /agent:todo -->

## Backlog <!-- agent:backlog priority queue -->
- [ ] ☑️ [#clearqueuedciflake] tests/clear_queued_live_turn_tmux.rs::a_repeat_press_rearms_the_queued_clear_behind_the_run_started_before_it failed on main CI 40485993738 (95ae7fe959, make tmux-ci): 'nothing is typed into B's running turn' at :1188 (left 1, right 0), i.e. one keystroke landed in B's running turn. It passed 6/6 locally on eebfdfef07 and on 4e17a4c793 CI. Find the nondeterminism (load/timing in the repeat-press re-arm vs B's run start) and make it deterministic; if the product can type into a running turn under load, fix the product. Never rerun to green.
- [ ] ☑️ [#startcwdfallback] agent-doc-start-io/src/lib.rs:1480 prepare_start_runtime_with_admission falls back to current_dir() for a document with no .agent-doc ancestor (project_root_containing(..).unwrap_or(current_dir())), while start's other paths (:146, :497), the route and sync (#syncnorootfallback) use the canonical parent. Use project_root_or_file_parent so a rootless document starts in the store sync and claim bind it in.
<!-- /agent:backlog -->

## Review <!-- agent:review -->
- [/] [#jetbrainsmarketplaceprofile] [marketplace-approval] [agent-doc] Plugin 35436 is hidden and under initial JetBrains moderation; after approval, replace the basic description with polished copy, add Getting Started, Documentation, and Bugtracker URLs, then verify the public listing.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
- [/] [#gh299-live-263-proof] Live-verify v0.35.519 on JetBrains 263.7031 with changed plugin sources: `agent-doc upgrade` must hot-reload and complete without an IDE restart; close GH #299 only with that proof.
<!-- /agent:review -->

## Icebox <!-- agent:icebox -->
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
