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
  '#repo': 'https://github.com/btakita/agent-doc'
  '#self': agent-doc-bugs.md
dogfood:
  path: tasks/agent-doc/agent-doc-bugs.md
  auto: true
queue: go
---

## Status

<!-- agent:status patch=replace sync="##sync-release" -->

Main is at `b6b9af6e15` and agent-doc 0.35.532 is installed locally. Release v0.35.532 was published from `71f9801b49`. Post-release main now includes the inline-code-safe, occurrence-ordered queue preset authorization fix for GitHub issue #331 and the positive-evidence-only Remote Dev pane-order fix for GitHub issue #332; both issues are closed, the integrated full suite passed, and the current mainline was installed locally.

<!-- status:sync -->
✅ Release v0.35.548 was published from `56046b3880` and verified end to end. The stable, non-draft GitHub release has 21 assets; all 11 release jobs passed, including six platform builds, plugins, Homebrew, PyPI, and finalization. Issues #368–#371 are closed.

Remote `main` is at `9e366edd73`. Its post-release per-target stable upgrade-channel implementation is pushed, and CI, Book, and Pages workflows all passed. The feature is not part of v0.35.548; it will activate after a future release containing the resolver.

This host reports agent-doc CLI 0.35.548. Installed editor packages are IntelliJ classic 0.2.806, Rider/262 0.2.807, and VSCodium 0.2.83. The v0.35.548 release also publishes JetBrains 263 package 0.2.808; it is not installed on this host.
<!-- /status:sync -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261024-001113.md`*

Compacted content:
- Archived 15 response topic(s): Release v0.35.547, queue repair, and project agent configuration; Release availability and upgrades; Subagent completed: `subagent:queue-state-live-repair` returned work for queue item “Several of this queue's items we...; 12 more
- Prior summary/context: prior compacted content: Archived 53 response topic(s): Release v0.35.544 published and release workflow hardened; Co...
<!-- agent:boundary:4528051c:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main
<!-- agent:queue id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
<!-- /agent:queue -->

## Queue: Issues

<!-- agent:queue id=issues loop interval=1h priority -->
- ~~do [#helpregisterjetbrains]~~
- ~~After the idea plugin, the agent-doc Plugin formatting stopped working and the commands don't show up. The plugin manager says the Agent Doc plugin is incompatible.~~
- 📥 Register agent-doc at https://plugins.jetbrains.com/plugin/add#intellij. Create a nice profile.
- 📥 In the JetBrains plugin, provide a command to install agent-doc binary on the system. This will be for people who install the agent-doc plugin.
<!-- /agent:queue -->

## Todo
<!-- agent:todo -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#coder-remote-install-02805] Install agent-doc-jetbrains-263 0.2.808 on both the Coder Client or Gateway and backend, then restart both peers and verify paired ingress deferred or accepted and completed or connected diagnostics.
- [ ] [#helpregisterjetbrains] Complete [#helpregisterjetbrains]: create the JetBrains Marketplace vendor/listing with a hidden bootstrap upload, create a permanent token, and add it to btakita/agent-doc as JETBRAINS_MARKETPLACE_TOKEN. (source: agent:review [#helpregisterjetbrains])
- [ ] [#hj2r] Restart IntelliJ IDEA 2026.2, then verify Agent Doc formatting and its Tools/context-menu commands are available.
<!-- /agent:todo -->

## Backlog
<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review
<!-- agent:review -->
- [/] 🚧 [#helpregisterjetbrains] [operator-verify] Complete the first manual JetBrains Marketplace upload and add JETBRAINS_MARKETPLACE_TOKEN; then rerun v0.35.548 Release and verify the three compatibility-ranged updates.
- [/] [#jb262dynlauncherrecommen] [recommended] [operator-verify] [agent-doc] Operator decision: jb262dynupgrade (153c72916) chose the accurate-warning option (a 262 package reports declined_by=modular_package and falls back to file replacement + restart). Either keep that as the supported policy (then close this), or authorize a real restart-free launcher for the exact-262 split-mode plugin that swaps backend + frontend (the frontend may run in a separate JetBrains Client process), proven against a live 262 IDE.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-12: pre-cutover 131 releases 8.361 GiB (0.35.113..0.35.343), post-cutover 91 releases 3.19 MiB (mean 35.9 KiB), about 47,722 further releases fit; VERDICT headroom fine. The support issue is still open with one comment, unchanged since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
- [/] [#gh299-live-263-proof] Live-verify v0.35.519 on JetBrains 263.7031 with changed plugin sources: `agent-doc upgrade` must hot-reload and complete without an IDE restart; close GH #299 only with that proof.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped
<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
