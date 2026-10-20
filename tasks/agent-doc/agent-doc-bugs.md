---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: ba846c4c-2e01-4e12-9539-096faec207ee
  codex: 01a1545a-f42c-7aa1-b9a0-aeafeba96910
agent: claude
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
Review pass done. frontendlivedispatch closed on live evidence. Two review items remain, both genuinely gated: an operator decision (jb262dynlauncherrecommen) and an external PyPI reply (pypiquotarecheck).

<!-- status:sync -->
Review pass done. frontendlivedispatch closed on live evidence. Two review items remain, both genuinely gated: an operator decision (jb262dynlauncherrecommen) and an external PyPI reply (pypiquotarecheck).

<!-- status:sync -->
Latest published release: [v0.35.528](https://github.com/btakita/agent-doc/releases/tag/v0.35.528) (GH #320 option A: Remote Dev split state reaches the controller; background-drain busy-turn defer, orphan-drain claim re-read, state-propagation spec), published on GitHub at 2026-10-20 07:37:15 UTC (15 assets); PyPI 0.35.528 followed at 2026-10-20 07:38:01 UTC; installed locally (agent-doc 0.35.528). JetBrains packages in v0.35.528: 0.2.723 (classic), 0.2.725 (IDE 262), 0.2.724 (IDE 263), published with the release at 2026-10-20 07:37:15 UTC; installed: IntelliJ IDEA 2026.1 classic 0.2.723, Rider 2026.2 IDE-262 0.2.725. VS Code extension 0.2.83.
<!-- /status:sync -->
<!-- /status:sync -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261020-142741.md`*

Compacted content:
- Archived 49 response topic(s): Subagent completed: `subagent:issue310` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/is...; Subagent completed: `subagent:archivepath` returned work for queue item “re: `agent:done` `archive` relative path. We...; Subagent completed: `subagent:issue309` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/is...; 46 more
- Prior summary/context: prior compacted content: Archived 75 response topic(s): Queue-prompt flood and GH #282–#283; Publication status; rele...
<!-- agent:boundary:268e5d65 -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main
<!-- agent:queue id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
- 📥 There was 1 tmux pane with 2 editor panes.
- 📥 route: a background orphan-drain dispatch types into a pane right after reading its busy-turn cue (route_dispatch_only_busy_active_turn_skip_wait then tmux_text_enter; from GH 319)
- 📥 investigate: preset head upgrade was dispatched 2 s after its claim landed (GH 319 "also seen"; likely claim-visibility timing in the controller state.db read)
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/320
- 🚧 GH 320 follow-up (option B): real Remote Dev split geometry. The 262/263 frontend module publishes the client measured editor-window columns (bump the shared surface snapshot contract) and the backend forwards them into the controller surface Source as editor order; the remembered-order fallback, labelled unknown, stays for classic and one-sided installs. Needs live JetBrains Client verification.
- 🚧 Move Run Agent Doc and Sync Tmux Layout off the --col CLI shell-out onto the Lazily surface path (specs/13-state-backbone.md "State Propagation, Not Effect Inference").
- 📥 steering: the agent's own committed `> **Queue prompt:** <line>` echo blockquotes in agent:exchange were surfaced to the same turn as 5 NEW operator exchange prompts (source=exchange change=added dispatch=subagent) right after closeout on 0.35.527; agent-authored response cells must never be read back as operator steering
- release + publish including PR #317-#320.
<!-- /agent:queue -->

## Queue: Issues
<!-- agent:queue id=issues loop interval=1h -->
- 🚧 Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.
<!-- /agent:queue -->

## Todo

<!-- agent:todo -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [x] [#gq2c] After the haiven-dev supervisor recycles onto the new build, re-run Agent Doc on tasks/sdk.md: bracketed todo ids are admitted again (it currently uses bare ids as a workaround).
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#n7kq] In the sdk.md session: restore the 7 todo ids the 23:30 --todo-edit dropped (#tdclireview, #tdcontracts162, #tdpromotioncallers, #tdinspectclones, #tdfpe202, #tdadr0033, #tdadr0030) with --todo-edit "<exact text>=#tdX <same text>", or accept the ids the preflight backfill assigns
- [x] [#8ph6] In the docs.md session: check off the Google Drive re-auth todo (--todo-done httpsdocsgoogle once backfilled, or --todo-done with its exact text)
- [ ] [#t1v0] In the sdk.md pane, press Clear Session Context again: the 06:03 presses were held behind the open cycle and then dropped by the install restart. Check first that sdk.md's queue has not opened its tdue4cpuset cycle.
- [x] [#yvt3] Manually sync tmux now that the one-pane diagnostics are complete
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
