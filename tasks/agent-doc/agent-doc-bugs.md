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
---

## Status

<!-- agent:status patch=replace -->

Released agent-doc v0.35.475. v0.35.476 is not yet tagged or published; the actionable review added the release gap and two correctness follow-ups.
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261010-172930.md`*

Compacted content:
- Archived 48 response topic(s): actionable review; queue subagent dispatch (claude coordinator takeover); actionable-review dispatch; 45 more
- Prior summary/context: prior compacted content: Archived 31 response topic(s): retained closeouts, queue integrity, GH #181–#184, and v0.35....
<!-- agent:boundary:047c1cfd -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#operatorverifylive] #frontendlivedispatch [operator-verify] [agent-doc] Live confirmation that Run Agent Doc on haiven-dev frontend.md dispatches again. The fixes shipped in 0.35.483: runfrontenddispatch 3d9fbad86 and dispatchreadyselfheal 81b9e1910. The existing wedge (started 4351 / settled 4350) is still in place because pane %166's supervisor (pid 1989646, started about 15h ago on the pre-0.35.483 binary, last heartbeat 03:49Z) has not re-registered, so its lease has no ready_registered_at stamp, and install_fanout recycle requests are ignored while the old recycle is in flight. Unblocker: restart the frontend.md session in pane %166, or let that supervisor re-register. Then click Run Agent Doc and look for route_dispatch_only_recycle_inflight_ready_after_start outcome=settled, or supervisor_recycle_graph_settled reason=dispatch_ready_after_start, in haiven-dev ops.log, with no refusal=recycle_ttl_elapsed_supervisor_alive.
- [/] [#frontendlivedispatch] [operator-verify] [agent-doc] Live confirmation that Run Agent Doc on haiven-dev frontend.md dispatches again. The fixes shipped in 0.35.483: runfrontenddispatch 3d9fbad86 and dispatchreadyselfheal 81b9e1910. The existing wedge (started 4351 / settled 4350) is still in place because pane %166's supervisor (pid 1989646, started about 15h ago on the pre-0.35.483 binary, last heartbeat 03:49Z) has not re-registered, so its lease has no ready_registered_at stamp, and install_fanout recycle requests are ignored while the old recycle is in flight. Unblocker: restart the frontend.md session in pane %166, or let that supervisor re-register. Then click Run Agent Doc and look for route_dispatch_only_recycle_inflight_ready_after_start outcome=settled, or supervisor_recycle_graph_settled reason=dispatch_ready_after_start, in haiven-dev ops.log, with no refusal=recycle_ttl_elapsed_supervisor_alive.
- [/] [#jb262dynlauncherrecommen] [recommended] [operator-verify] [agent-doc] Operator decision: jb262dynupgrade (153c72916) chose the accurate-warning option (a 262 package reports declined_by=modular_package and falls back to file replacement + restart). Either keep that as the supported policy (then close this), or authorize a real restart-free launcher for the exact-262 split-mode plugin that swaps backend + frontend (the frontend may run in a separate JetBrains Client process), proven against a live 262 IDE.
- [/] [#jetbrains262distribution] [agent-doc] Provide and verify a two-sided production distribution channel for the exact-262 modular plugin before calling v0.35.480 generally installable: publish classic and 262 as non-overlapping updates of the same Marketplace plugin ID, or ship an equivalent tested custom-repository feed, while preserving fail-closed behavior for one-sided installation. Verify clean 262 Remote Dev installation reaches backend and Client/Gateway, 242–261 receive only classic, mismatch remains fail closed, and the release workflow asserts both ranged updates.
- [/] [#gh218remoteproof] [operator-verify] GH 218: PR 222 is MERGED (01:25Z 2026-10-10, merge cc6e458c6) at head 34842a23a, CI run 38002449411 green. Run the real IDEA 2026.2.3 (262) Remote Dev acceptance matrix against prerelease jetbrains-262-preview-34842a2 (asset agent-doc-jetbrains-262-0.2.511.zip, id 626772959, 2556136 bytes, sha256 2ef328588d1491446f1a45a83775ee220c13507f5cdd9bca4f5c2111f9e864a7), or against the 0.2.514 ZIP of the next published release. Verify both downloaded copies with make verify-jetbrains-262-preview BACKEND_ZIP=... CLIENT_GATEWAY_ZIP=... first, and install the same ZIP on backend and Client/Gateway. Check: detached-only isolated terminal; same-document main-wins read-only placeholder with byte-identical main; focus/sync/resize causes no main mutation; close/rejoin; backend restart/client reconnect before the first main publication; two simultaneous thin clients; capability loss/one-sided install fails closed. Post results on 218 citing that tag and commit, then close 218. jetbrains-262-preview-c39de1b is superseded.
- [/] [#drivecapturedsplice] [operator-verify] Captured-splice stale-replica live proof (plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md). Recipe and regression test shipped in v0.35.466; live proof remains gated because the corrected run reached owner pane %134 but stopped before the retained-delivery marker, edit two, and verifier completion. Rerun the recipe through verify-captured-splice-recovery and require it to pass.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or make pypi-quota-check reporting a ceiling in reach. Rechecked 2026-10-06: 8.363 GiB of 10.0 GiB, 1.637 GiB headroom, 198 releases through 0.35.465, post-cutover mean 35.3 KiB, approximately 48,621 further releases fit; VERDICT headroom fine. The support issue remains open with only the github-actions bot comment and has not changed since 2026-09-10. Nothing to act on or delete. If the grant lands, record it and drop this item; deletion stays reserved for a ceiling the authenticated check reports.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#recommendedtwojetbrains] #ideriderstopgap [recommended] [agent-doc] Two JetBrains IDEs on one project: only the most recently focused IDE drives the shared tmux agent-doc window, which stops the layout from flipping between IDEA and Rider. About 1 day. Builds on idearidereditors 9796d8c8a (installation-scoped editor-surface client families).
- [ ] [#recommendedfullyindepend] #ideriderindependentnav [recommended] [agent-doc] Fully independent navigation per JetBrains installation: one tmux window or session per IDE installation. The installation identity becomes part of the layout-ownership key, tmux-to-editor focus mirroring goes only to the IDE that owns the window, tmux_focus_state and pane ownership are split per window, and specs plus a two-IDE SimWorld are added. Several days.
- [ ] [#recommendedwriteruntime] #visiblewriteloadtimeout [recommended] [agent-doc] agent-doc-write-runtime-io visible_write_content_snapshot_tests (test_lazily_visible_write_event_read, test_poll_lazily_event_*) fail under host load (load average around 50) with 'timed out after 2.0s waiting for project controller response' at ipc.rs:170 (record_lazily_visible_write_candidate). Seen in 2 of about 6 full runs on 2026-10-10. Make the fixture warm the controller before the timed RPC, or charge cold-start launch separately from CONTROLLER_RPC_TIMEOUT, so a busy runner cannot decide the outcome.
- [ ] [#ideriderstopgap] [recommended] [agent-doc] Two JetBrains IDEs on one project: only the most recently focused IDE drives the shared tmux agent-doc window, which stops the layout from flipping between IDEA and Rider. About 1 day. Builds on idearidereditors 9796d8c8a (installation-scoped editor-surface client families).
- [ ] [#ideriderindependentnav] [recommended] [agent-doc] Fully independent navigation per JetBrains installation: one tmux window or session per IDE installation. The installation identity becomes part of the layout-ownership key, tmux-to-editor focus mirroring goes only to the IDE that owns the window, tmux_focus_state and pane ownership are split per window, and specs plus a two-IDE SimWorld are added. Several days.
- [ ] [#visiblewriteloadtimeout] [recommended] [agent-doc] agent-doc-write-runtime-io visible_write_content_snapshot_tests (test_lazily_visible_write_event_read, test_poll_lazily_event_*) fail under host load (load average around 50) with 'timed out after 2.0s waiting for project controller response' at ipc.rs:170 (record_lazily_visible_write_candidate). Seen in 2 of about 6 full runs on 2026-10-10. Make the fixture warm the controller before the timed RPC, or charge cold-start launch separately from CONTROLLER_RPC_TIMEOUT, so a busy runner cannot decide the outcome.
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
