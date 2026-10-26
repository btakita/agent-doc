---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 6bd4c579-6308-4846-b5b6-af24bb0c65f8
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

📤 Queue handoff: 3 reported items awaiting coordinator integration, verification, and closeout.
Main is at `b6b9af6e15` and agent-doc 0.35.532 is installed locally. Release v0.35.532 was published from `71f9801b49`. Post-release main now includes the inline-code-safe, occurrence-ordered queue preset authorization fix for GitHub issue #331 and the positive-evidence-only Remote Dev pane-order fix for GitHub issue #332; both issues are closed, the integrated full suite passed, and the current mainline was installed locally.

<!-- status:sync -->
Released and published agent-doc v0.35.556 from release commit `5cbfdbbc4c`; tag `v0.35.556` is the stable latest release (GitHub Latest, PyPI 0.35.556, stable target channels). JetBrains classic/262/263 0.2.839/0.2.841/0.2.840, VS Code 0.2.83. It adds onboarding Getting started, the queue marker legend, the component-marker fences (live edit, cold start, JetBrains edit-time warning), row-aware editor attach, reliable echoed-head strikes beside a loop queue, doctor/autofix exit codes, and pane-process launches for start/route/fan-out. GH #378 closed; GH #379 awaits the reporter upgrading. Post-release `origin/main` is `c4518134ca` (unreleased, installed locally): the in-IDE restart-required prompt for restart-only plugin upgrades (JetBrains 0.2.842/0.2.844/0.2.843).
<!-- /status:sync -->

Queue markers: 📥 pending · 🟡 claimed · 🚧 running · ☑️ reported · ⚠️ lost · 🔄 needs reconcile · ❔ unknown · ⏭️ skipped · ⏳ awaiting operator · 🙋 operator action · 📌 pinned · 📍 agent pin ([legend](https://github.com/btakita/agent-doc/blob/main/docs/guide/queue-markers.md#queue-markers))
<!-- /agent:status -->
 

## Exchange <!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261026-215049.md`*

Compacted content:
- Archived 49 response topic(s): Subagent completed: `subagent:grok_queue_ui` returned work for queue item “In sdk-demo.md using Grok, the queue item ...; Subagent completed: `subagent:lazily_block_reason` returned work for queue item “On lazily.md, `- ⚠️ [#subagent:corec...; Subagent completed: `subagent:tmux_auto_sync` returned work for queue item “tmux pane auto sync is not working. Sync ...; 46 more
- Prior summary/context: prior compacted content: Archived 7 response topic(s): JetBrains Marketplace registration; JetBrains CLI installer an...
<!-- agent:boundary:3ec64169 -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main <!-- agent:run id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
- ☑️ [#subagent:autoupgrade#1] do [#fixautoupgrade]: I like **Keep restart-only + prompt** as a fallback solution. We should attempt to auto-upgrade though. **Build the GH #339 architecture** seems like a reasonable solution sfor most cases. Not sure what **Re-qualify on 263.7307.44** would imply. Is **Re-qualify on 263.7307.44** safe and how effective would it be? Would **Re-qualify on 263.7307.44** be more likely to successfully run auto-upgrade than **Build the GH #339 architecture**? Your recommendation of **Keep restart-only + prompt** + **Shrink the restart count** seems reasonable if it performs well.
- ☑️ [#subagent:cellrepair#1] one-time repair: retire stale subagent completion cells and report receipts left behind by strikes made before the completion-cell retirement fix, or made by hand in the editor, in this document and other session documents (found by the completion-cell retirement fix, 2026-10-26)
- ☑️ [#subagent:noargskill#1] In the agent terminal, running the `agent-doc` skill (`/agent-doc` on Claude + Grok + Opencode and `agent-doc` in Codex) without any arguments should run agent-doc on the claimed file for the tmux pane.
<!-- /agent:run -->

## Loop: Issues <!-- agent:loop id="issues" interval="1h" priority -->
- 📥 Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.
<!-- /agent:loop -->

## Todo <!-- agent:todo priority -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#coder-remote-install-02805] Repair Coder Remote Dev sync after the two-sided install: open /home/brian/work/btakita/agent-loop as the backend IDE project root; ensure Client/Gateway and backend both load JetBrains 263 plugin 0.2.824; restart both; on the backend run agent-doc env --json, agent-doc tmux ensure tasks/agent-doc/agent-doc-bugs.md --ide-terminal --json, and agent-doc session status tasks/agent-doc/agent-doc-bugs.md; require client idea.log to show snapshot schema negotiated with geometry=measured, backend idea.log to show accepted ingress with no DELIVERY_FAILED, and .agent-doc/logs/ops.log to show controller_editor_surface_observed client=jetbrains-rd@... surface=remote-client:... followed by pane_layout_desired_published with column_order=editor. If DELIVERY_FAILED No such file or directory persists, fix ControllerSurfaceSnapshotPublisher to resolve the controller from each session document root instead of an unrelated IDE project basePath, expose connected-without-accepted-publication as blocked while preserving the no-guessing fence, and add a regression test.
- [ ] [#jb263-feed-verify] In IntelliJ IDEA 2026.3, configure only https://github.com/btakita/agent-doc/releases/latest/download/agent-doc-jetbrains-263.xml, refresh Plugins, update Agent Doc from 0.2.816 to 0.2.824 or newer, restart the IDE, and verify the “Cannot update plugin” warning clears; if it does not, capture the configured repository entry and relevant idea.log update-check lines.
- [ ] [#xj75] Choose the JetBrains restart-free upgrade direction in the [#subagent-operator-request-autoupgrade-263-path] packet in agent:exchange: keep restart-only + prompt, re-qualify on IU 263.7307.44, build the GH #339 shell, or shrink the restart count
- [ ] [#subagent-operator-request-autoupgrade-263-path] Answer the blocking subagent question for tracked work jb262dynlauncherrecommen for queue item ‘Fix the auto-upgrade not working...the IDE needed to be restarted.’ in agent:exchange: The exact-263 package is restart-only by spec (GH #339 closed with that contract; `#jb262dynlauncherrecommen` is the same open decision for 262). (choices: Keep restart-only + prompt, Re-qualify on 263.7307.44, Build the GH #339 architecture, Shrink the restart count) (source: agent:review [#jb262dynlauncherrecommen])
<!-- /agent:todo -->

## Backlog <!-- agent:backlog priority queue -->
- [ ] ☑️ [#fixautoupgrade] 🙋☑️ [#subagent:autoupgrade#1] Fix the auto-upgrade not working...the IDE needed to be restarted. [#subagent-operator-request-autoupgrade-263-path]: would **Shrink the restart count** have performance issues due to extra serialization?
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
