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
Main is at `b6b9af6e15` and agent-doc 0.35.532 is installed locally. Release v0.35.532 was published from `71f9801b49`. Post-release main now includes the inline-code-safe, occurrence-ordered queue preset authorization fix for GitHub issue #331 and the positive-evidence-only Remote Dev pane-order fix for GitHub issue #332; both issues are closed, the integrated full suite passed, and the current mainline was installed locally.

<!-- status:sync -->
Latest published release: [v0.35.533](https://github.com/btakita/agent-doc/releases/tag/v0.35.533), published on GitHub at 2026-10-21 13:58:09 UTC (15 assets); PyPI 0.35.533 followed at 2026-10-21 13:59:00 UTC; installed locally (agent-doc 0.35.534). Published JetBrains packages: 0.2.759 (classic), 0.2.760 (IDE 262), and 0.2.761 (IDE 263), published with the release at 2026-10-21 13:58:09 UTC. Current main/source generations remain 0.2.759 (classic), 0.2.760 (IDE 262), and 0.2.761 (IDE 263). VS Code extension 0.2.83.
<!-- /status:sync -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261021-053012.md`*

Compacted content:
- Archived 7 response topic(s): Response; Subagent completed: `subagent:release531` returned work for queue item “release + publish”; Subagent completed: `subagent:sdklegalhistory` returned work for queue item “Run Agent Doc on sdk-legal.md issue: ```...; 4 more
- Prior summary/context: prior compacted content: Archived 29 response topic(s): nested status:sync, reported-item integration, four new subag...

### Re: #gh-fix issue 331 — gpt-5 · 2026-10-21T02:26-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/331

Fixed and closed [GH #331](https://github.com/btakita/agent-doc/issues/331). Queue preset authorization now ignores preset names inside balanced inline-code spans and preserves preset expansion order by textual occurrence instead of map order. The implementation, specification, and CLI/unit regressions landed on `main` as `bef5b08236`.

Verification passed: focused unit and queue-brief CLI regressions each 1/1; the worker's full suite passed 11,378 nextest tests, 1,982 batched tests, 202 VS Code tests, cross-editor simulation, Python bootstrap, all three JetBrains builds, and TLA validation.

### Re: #gh-fix issue 332 — gpt-5 · 2026-10-21T02:26-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/332

Fixed and closed [GH #332](https://github.com/btakita/agent-doc/issues/332). Cold Remote Dev layout observations may still provide a stable fold basis, but they no longer become positive exported column-order authority; order stays unknown until an editor/manual order-bearing signal arrives, and focus escalation carries only a genuinely positive basis. The implementation, JetBrains specification, and regressions landed on `main` as `b6b9af6e15`.

Verification passed: focused controller tests, 810/810 controller-library tests, and 27/27 editor-surface tests; the worker's full suite passed 11,376 nextest tests, 202 VS Code tests, cross-editor simulation, Python bootstrap, all JetBrains builds, and TLA validation.

The two commits were integrated serially from `0b5909927f` and pushed to `origin/main` at `b6b9af6e15`. A single authoritative combined `make check` then passed with exit 0 and zero detected failures. The single batch `make install` also passed, installing agent-doc 0.35.532, VS Code 0.2.83, and JetBrains generations 0.2.759/0.2.760/0.2.761. The latest pre-integration main CI run was green; the new push will receive its own CI run asynchronously.
<!-- agent:boundary:9d4ce1fd -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main
<!-- agent:queue id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/331~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/332~~
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/333
- 📥 release + publish: include issue 331-333
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/334: Can you simulation test the Coder environment?
- 📥 release + publish: include issue 334
- 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/335
- 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/336
- 📥 release + publish: include issue 335-336
- 🚧 In sdk-demo.md, the queue item "- 📥 Why are you telling me to `cd /home/brian/work/btakita/agent-loop/worktrees/haiven-dev/haiven-cli/demo-open-options-20261021` if we are working in `~/work/btakita/worktrees/haiven-dev/haiven-cli/unity-hub-register-option`?" was never struck. Can this fix be a part of the 335-336 release or do we need a new release?
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
