---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: ba846c4c-2e01-4e12-9539-096faec207ee
  codex: 01a12175-b9c0-7db0-af02-218c5501efe7
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#agent-doc-causes': Fix agent-doc causes
  '#do-backlog': /goal do backlog items
  '#gh-fix': fix then close
  '#release': release + publish
  '#self': agent-doc-bugs.md
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

Released agent-doc v0.35.475. v0.35.476 is not yet tagged or published; the actionable review added the release gap and two correctness follow-ups.
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261011-181802.md`*

Compacted content:
- Archived 29 response topic(s): actionable-review dispatched; v0.35.487 batch status; actionable review results; re-trigger, sdk.md correction, two dispatches; 26 more
- Prior summary/context: prior compacted content: Archived 20 response topic(s): queue go; `#runctrlclaude` re-trigger; `#runctrlclaude` press...
<!-- agent:boundary:75cf33a3 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
- ⏳ do [#drivecapturedsplice]: what needs to be done?
- 🚧 release + publish
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#drivecapturedsplice] [operator-verify] Captured-splice stale-replica live proof (plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md). Recipe and regression test shipped in v0.35.466; live proof remains gated because the corrected run reached owner pane %134 but stopped before the retained-delivery marker, edit two, and verifier completion. Rerun the recipe through verify-captured-splice-recovery and require it to pass.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#frontendlivedispatch] [operator-verify] [agent-doc] Live confirmation that Run Agent Doc on haiven-dev frontend.md dispatches (fixes 0.35.483: runfrontenddispatch 3d9fbad86, dispatchreadyselfheal 81b9e1910). Precondition met 2026-10-11: pane %166's supervisor re-registered on the current binary (pid 3851566, healthy/ready) and install_fanout recycles now settle. Remaining: the operator clicks Run Agent Doc on frontend.md; pass = route_dispatch_only_recycle_inflight_ready_after_start outcome=settled or supervisor_recycle_graph_settled reason=dispatch_ready_after_start in haiven-dev ops.log, with no refusal=recycle_ttl_elapsed_supervisor_alive.
- [/] [#jb262dynlauncherrecommen] [recommended] [operator-verify] [agent-doc] Operator decision: jb262dynupgrade (153c72916) chose the accurate-warning option (a 262 package reports declined_by=modular_package and falls back to file replacement + restart). Either keep that as the supported policy (then close this), or authorize a real restart-free launcher for the exact-262 split-mode plugin that swaps backend + frontend (the frontend may run in a separate JetBrains Client process), proven against a live 262 IDE.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-06: 8.363 GiB of 10.0 GiB, 1.637 GiB headroom, 198 releases through 0.35.465, post-cutover mean 35.3 KiB, approximately 48,621 further releases fit; VERDICT headroom fine. The support issue remains open with only the github-actions bot comment and has not changed since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ideriderindependentnav] [recommended] [agent-doc] Fully independent navigation per JetBrains installation: one tmux window or session per IDE installation. The installation identity becomes part of the layout-ownership key, tmux-to-editor focus mirroring goes only to the IDE that owns the window, tmux_focus_state and pane ownership are split per window, and specs plus a two-IDE SimWorld are added. Several days.
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
