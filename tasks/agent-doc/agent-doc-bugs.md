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

📤 Queue handoff: 10 reported items awaiting coordinator integration, verification, and closeout.
Main is at `b6b9af6e15` and agent-doc 0.35.532 is installed locally. Release v0.35.532 was published from `71f9801b49`. Post-release main now includes the inline-code-safe, occurrence-ordered queue preset authorization fix for GitHub issue #331 and the positive-evidence-only Remote Dev pane-order fix for GitHub issue #332; both issues are closed, the integrated full suite passed, and the current mainline was installed locally.

<!-- status:sync -->
Latest published agent-doc release: v0.35.546, published on GitHub at 2026-10-23 05:23:26 UTC. It remains a stable, non-draft release with 21 assets, including platform binaries, per-archive checksum sidecars, all three JetBrains compatibility ZIPs, the VSIX, and aggregate manifests. The locally installed CLI still reports 0.35.546.

The current unreleased integration candidate is `integrate/thrash-compact-20261023` at pushed head `910ea714aafd0ffe289d794ee400e8e193899f6b`. Its authoritative combined `make check` passed and the coordinator-owned local install completed. It includes retained-pull backoff, ambiguous-attach pause handling, archive-aware compact closeout, explicit lazily source substitution, per-archive upgrade and installer verification with GNU or musl selection, recurring-loop auto-strike protection, explicit single-peer Remote Dev installation scope, and retry with paired diagnostics for normally completed empty Remote Dev ingress. JetBrains package generations are classic 0.2.801, 262 0.2.804, and 263 0.2.805.

GitHub issues #363, #364, and #366 are closed as completed; #365 is closed as not planned after its original one-sided-install diagnosis was superseded by later evidence. The remaining Coder deployment action is to install modular 0.2.805 on both Client or Gateway and backend, restart both peers, and verify the paired ingress diagnostics. No newer release has been tagged or published. The previously recorded Homebrew token gap, JetBrains Marketplace onboarding gap, and separate v0.35.547 follow-up branch remain unresolved and must be reconciled before the next release.
<!-- /status:sync -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261023-152449.md`*

Compacted content:
- Archived 53 response topic(s): Release v0.35.544 published and release workflow hardened; Continuation after release closeout; recovered response; 50 more
- Prior summary/context: prior compacted content: Archived 15 response topic(s): recovered response; Subagent completed: `subagent:unique_requ...
<!-- agent:boundary:1d86f43f -->
<!-- /agent:exchange -->

<!--
-->

## Queue: Main
<!-- agent:queue id="main" subagents preset="#spec-test-build-install-commit-push" priority go -->
- ☑️ [#subagent:gh363-fix#1] do [#gh363]
- ☑️ [#subagent:gh355-cleanup-rerun#3] #gh-issue https://github.com/btakita/agent-doc/issues/355
- ❔ Compact Exchange on sdk-demo.md error: Agent Doc could not attach the open editor replica for sdk-demo.md. Cause: ambiguous-retained-projection. The editor buffer and controller canonical have diverged from their last settled ancestor. Copy any unsaved text, close this editor tab, run `agent-doc repair <file>` while it is detached, and reopen it. The editor buffer and disk were left unchanged.
- ❔ agent-doc process is thrashing in the background along with Idea.
- ❔ Investigate the pre-existing JetBrains LazilyGenerationFenceTest stale-disposal failure on untouched main after the lazily-kt 0.46.0 update.
- ❔ Compact Exchange in sdk-demo.md compacted but did not complete the cycle. There is uncommitted compact summary content in the exchange.
- ❔ #gh-fix https://github.com/btakita/agent-doc/issues/364
- ❔ #gh-fix https://github.com/btakita/agent-doc/issues/365
- ❔ #gh-fix https://github.com/btakita/agent-doc/issues/366
- ❔ Coder instance still has backend/frontend communication issues
- 📥 #gh-fix https://github.com/btakita/agent-doc/issues/367
<!-- /agent:queue -->

## Queue: Issues

<!-- agent:queue id=issues loop interval=1h -->
- ☑️ [#subagent:gh363-fix#1] do [#gh363]
<!-- /agent:queue -->

## Todo
<!-- agent:todo -->
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, once idea.log shows `[native] hot-reloaded libagent_doc`, Run Agent Doc should complete (ops.log `editor_route_superseded_editor_admitted`, route output ends with the advisory line). 2026-10-15 operator: did not work (believed); watch for bug reports before reopening GH #267.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#coder-remote-install-02805] Install agent-doc-jetbrains-263 0.2.805 on both the Coder Client or Gateway and backend, then restart both peers and verify paired ingress deferred or accepted and completed or connected diagnostics.
<!-- /agent:todo -->

## Backlog
<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review
<!-- agent:review -->
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
