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

📤 Queue handoff: 1 reported item awaiting coordinator integration, verification, and closeout.
Main is at `b6b9af6e15` and agent-doc 0.35.532 is installed locally. Release v0.35.532 was published from `71f9801b49`. Post-release main now includes the inline-code-safe, occurrence-ordered queue preset authorization fix for GitHub issue #331 and the positive-evidence-only Remote Dev pane-order fix for GitHub issue #332; both issues are closed, the integrated full suite passed, and the current mainline was installed locally.

<!-- status:sync -->
Latest published agent-doc release: v0.35.541, published on GitHub at 2026-10-22 10:14:33 UTC. JetBrains plugin packages published with that release at the same time: classic 0.2.787, 262 0.2.789, and 263 0.2.788 (latest numeric JetBrains package: 0.2.789). VS Code extension 0.2.83 was also published with the release. Release commit 34e41d9e22ba590124f2561630ec03111c5535c5 is on main, PyPI has agent_doc-0.35.541-py3-none-any.whl, and the Homebrew tap is updated by commit 28a351f.
<!-- /status:sync -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261022-143613.md`*

Compacted content:
- Archived 15 response topic(s): recovered response; Subagent completed: `subagent:unique_request_ids` returned work for queue item “api.md has 2 subagent questions with ...; no-guess layout authority, hook recovery, and v0.35.537; 12 more
- Prior summary/context: prior compacted content: Archived 5 response topic(s): #gh-fix issue 331; #gh-fix issue 332; releases, realtime fixes...

### Re: Subagent completed: `subagent:gh352` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/352” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=1083830e51e7cc54c9c31f8caeadf254bd76ae84e447ebec54371887c18d871c -->

### Re: Subagent completed: `subagent:release_publish` returned work for queue item “release + publish” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=03901def1f034bb4d759efe9753be5be61d240b96e28eddb447b45c818347734 -->

### Re: Subagent completed: `subagent:queue_loop2` returned work for queue item “The `agent:queue loop` does not seem to be reliably firing.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=b414cf439892a9bbff920926f06114b12068b3f57956f618af686d22c0826b66 -->

### Re: Subagent completed: `subagent:sched_recovery` returned work for queue item “Make the process-wide queue scheduler replace a disconnected worker and surface failed refresh enqueue receipts instead of discarding `mpsc::Sender::send` errors; its `OnceLock` handle currently cannot recover if the scheduler thread exits.” — agent-doc
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=7aff199ca08e468d96d55e25d7aa669fc37911d1d8a22f67f0b797e3c0325e2e -->
<!-- agent:boundary:0f221edb -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main
<!-- agent:queue id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
- 📤 [#subagent:gh352#1] #gh-fix https://github.com/btakita/agent-doc/issues/352
- 📤 [#subagent:release_publish#2] release + publish
- 📤 [#subagent:queue_loop2#2] The `agent:queue loop` does not seem to be reliably firing.
- 📤 [#subagent:sched_recovery#1] Make the process-wide queue scheduler replace a disconnected worker and surface failed refresh enqueue receipts instead of discarding `mpsc::Sender::send` errors; its `OnceLock` handle currently cannot recover if the scheduler thread exits.
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
