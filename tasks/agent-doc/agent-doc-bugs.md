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

📥 Queue handoff: 4 reported items awaiting coordinator integration, verification, and closeout.

Main is at fa3a54b1aa and installed locally: every GH 326 phase (3, 3b, 4, 5, 5b, 5c), the Codex queued-clear fix and four capture-gate fixes. Unreleased; queue "release + publish" to tag v0.35.531. The main queue is empty.

<!-- status:sync -->
Latest published release: [v0.35.530](https://github.com/btakita/agent-doc/releases/tag/v0.35.530) (GH #324 extract re-run safety, GH #325 done-archive resolver, GH #326 phases 1–2 hot-reloaded editor derivations, GH #327 loop queues restore consumed items, live-supervisor drain readiness, queue auto-clear baseline, Claude crash-restart resume id, live-session run guard, closeout ordering, GH #317 regression coverage), published on GitHub at 2026-10-20 20:54:49 UTC (15 assets); PyPI 0.35.530 followed at 2026-10-20 20:55:37 UTC; installed locally (agent-doc 0.35.530). JetBrains packages in v0.35.530: 0.2.735 (classic), 0.2.736 (IDE 262), 0.2.737 (IDE 263), published with the release at 2026-10-20 20:54:49 UTC. VS Code extension 0.2.83.
<!-- /status:sync -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261020-232532.md`*

Compacted content:
- Archived 29 response topic(s): nested status:sync, reported-item integration, four new subagents; Subagent completed: `subagent:gh321` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issue...; Subagent completed: `subagent:gh322` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issue...; 26 more
- Prior summary/context: prior compacted content: Archived 49 response topic(s): Subagent completed: `subagent:issue310` returned work for que...

### Re: Response

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/328

Implemented and closed GitHub issue #328. Queue subagent dispatches now support optional `model` and `effort`, resolved in queue → preset → frontmatter order and omitted when unset. The resolved policy is included in briefs/preflight launch guidance and persisted through claim/start attribution.

> **Queue prompt:** [#n7kq]: Is this moot?

Yes. Commit `f88bcde1` restored all seven IDs immediately after the destructive edit. Two remain open in `sdk.md`, two moved intact to `sdk-dev.md`, and three were completed and reaped. Restoring them again would duplicate or resurrect work, so `#n7kq` is marked done without changing either SDK document.

> **Queue prompt:** [#t1v0]: look through the logs to validate if this works or not

Validated: Clear Session Context succeeded through Codex's `supervisor_restart_fresh` path. The later “projection dropped” event was only the redundant queued `/clear`. The SDK actor is ready, its latest cycle is committed, and no `tdue4cpuset` cycle is open. Do not clear again; the separate CPU-sharing decision remains open. `#t1v0` is marked done.

> **Queue prompt:** Can we have a setting for ultra-efficient usage of tokens? Perhaps regular clearing of session context in the queue would help? auto-clear should be an optional `attribute` in `agent:queue`. The `clear` attribute should default to clear per turn when not in subagent mode. When in subagent-mode, `clear` at the next idle spot? `clear` should be set in the default template.

Implemented a canonical bare `clear` queue attribute. Direct queues clear after each completed turn; `subagents` and `fan-out` queues defer clearing until the next safe idle point after live claims settle. Existing supervisor safety gates are reused, `disable-auto-clear` remains a compatible winning opt-out, and default/generated templates now include `clear`.

Both changes were integrated on `main` at `bca2b67240`. The combined `make check` passed, `make install` succeeded, the integration worktree is clean, and installed agent-doc remains version `0.35.530`. The persistent release status was already current, so it was not rewritten.
<!-- agent:boundary:811a7087 -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main
<!-- agent:queue id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/328~~
- ~~[#n7kq]: Is this moot?~~ — auto-struck: answered this cycle (#ftstrike)
- ~~[#t1v0]: look through the logs to validate if this works or not~~ — auto-struck: answered this cycle (#ftstrike)
- ~~Can we have a setting for ultra-efficient usage of tokens? Perhaps regular clearing of session context in the queue would help? auto-clear should be an optional `attribute` in `agent:queue`. The `clear` attribute should default to clear per turn when not in subagent mode. When in subagent-mode, `clear` at the next idle spot? `clear` should be set in the default template.~~ — auto-struck: answered this cycle (#ftstrike) 
<!-- /agent:queue -->

## Queue: Issues
<!-- agent:queue id=issues loop interval=1h -->
- 🚧 Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.
<!-- /agent:queue -->

## Todo
<!-- agent:todo -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [x] [#n7kq] In the sdk.md session: restore the 7 todo ids the 23:30 --todo-edit dropped (#tdclireview, #tdcontracts162, #tdpromotioncallers, #tdinspectclones, #tdfpe202, #tdadr0033, #tdadr0030) with --todo-edit "<exact text>=#tdX <same text>", or accept the ids the preflight backfill assigns
- [x] [#t1v0] In the sdk.md pane, press Clear Session Context again: the 06:03 presses were held behind the open cycle and then dropped by the install restart. Check first that sdk.md's queue has not opened its tdue4cpuset cycle.
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
