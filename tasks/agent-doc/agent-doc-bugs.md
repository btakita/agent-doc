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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261011-033612.md`*

Compacted content:
- Archived 20 response topic(s): queue go; `#runctrlclaude` re-trigger; `#runctrlclaude` pressed again; 17 more
- Prior summary/context: prior compacted content: Archived 48 response topic(s): actionable review; queue subagent dispatch (claude coordinato...
<!-- agent:boundary:2c9385e2 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#removeobsoleteicebox]~~
- 🚧 do [#respondfinalizehard]
- 🚧 do [#pypireleaseworkflow]
- 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/235
- 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/236
- 🚧 fix: Clear Session Context in dev.md: agent-doc command failed (exit 1):
Error: session_clear refused for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/dev.md because a turn is live in pane %127 (harness_turn_live=true evidence=turn_active_lease, source=authoritative_actor); no `/clear` was sent. ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=wait_for_turn_or_interrupt_then_retry unblocker=clear_queued_behind_busy_turn. Wait for the turn to finish and run Clear Session Context again, or run `agent-doc session interrupt-clear /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/dev.md` to intentionally interrupt the turn and clear context.
- ~~Run Agent Doc on tools.md was slow to go.~~
- 🚧 release + publish
- 🚧 do [#visiblewriteloadflake]
- 🚧 do [#idefocusowner]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] 🚧 [#visiblewriteloadflake] [agent-doc] Fix flaky agent-doc-write-runtime-io visible_write_content_snapshot_tests (test_lazily_visible_write_event_read, test_poll_lazily_event_*): under host load (load average around 50) they time out with 'timed out after 2.0s waiting for project controller response' at ipc.rs:170 (record_lazily_visible_write_candidate); seen in 2 of about 6 full runs on 2026-10-10. Fix the nondeterminism instead of rerunning: warm the controller before the timed RPC, or charge cold-start launch separately from CONTROLLER_RPC_TIMEOUT, so a busy runner cannot decide the outcome. Prove it under load stress. (Promoted from icebox visiblewriteloadtimeout.)
- [ ] 🚧 [#idefocusowner] [agent-doc] Two JetBrains IDEs on one project: only the most recently focused IDE drives the shared tmux agent-doc window, which stops the layout from flipping between IDEA and Rider (both installed on this machine). About 1 day. Builds on idearidereditors 9796d8c8a (installation-scoped editor-surface client families). Fully independent per-IDE navigation stays in the icebox as ideriderindependentnav. (Promoted from icebox ideriderstopgap.)
- [ ] 🚧 [#respondfinalizehard] respond/finalize hard-fails with 'queue consume: no prompt to consume' (then write --commit fails 'failed to compose completed tracked-work target') when the cycle's quoted Queue prompt / --done target was already struck/consumed earlier (all queue heads struck, item already in agent:done). The response write landed and the cycle wedged at write_applied until a bare 'agent-doc write --commit' recovered it as AlreadyApplied. Consuming an already-consumed head must be idempotent. Repro 2026-10-10 cycle-1791675315432.
- [ ] 🚧 [#pypireleaseworkflow] PyPI release workflow races the GitHub Release: the PyPI job polls at most 30 minutes for the Release's SHA256SUMS, but v0.35.485's cross-platform builds took about 33 minutes, so publish failed at 21:45:30Z and the Release went up at 21:47:24Z (run 38086853278; a rerun of the failed job published it). Trigger PyPI from the release:published event, or from the Release workflow after its assets upload, instead of a fixed wall-clock poll.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#operatorverifylive] #frontendlivedispatch [operator-verify] [agent-doc] Live confirmation that Run Agent Doc on haiven-dev frontend.md dispatches again. The fixes shipped in 0.35.483: runfrontenddispatch 3d9fbad86 and dispatchreadyselfheal 81b9e1910. The existing wedge (started 4351 / settled 4350) is still in place because pane %166's supervisor (pid 1989646, started about 15h ago on the pre-0.35.483 binary, last heartbeat 03:49Z) has not re-registered, so its lease has no ready_registered_at stamp, and install_fanout recycle requests are ignored while the old recycle is in flight. Unblocker: restart the frontend.md session in pane %166, or let that supervisor re-register. Then click Run Agent Doc and look for route_dispatch_only_recycle_inflight_ready_after_start outcome=settled, or supervisor_recycle_graph_settled reason=dispatch_ready_after_start, in haiven-dev ops.log, with no refusal=recycle_ttl_elapsed_supervisor_alive.
- [/] [#frontendlivedispatch] [operator-verify] [agent-doc] Live confirmation that Run Agent Doc on haiven-dev frontend.md dispatches again. The fixes shipped in 0.35.483: runfrontenddispatch 3d9fbad86 and dispatchreadyselfheal 81b9e1910. The existing wedge (started 4351 / settled 4350) is still in place because pane %166's supervisor (pid 1989646, started about 15h ago on the pre-0.35.483 binary, last heartbeat 03:49Z) has not re-registered, so its lease has no ready_registered_at stamp, and install_fanout recycle requests are ignored while the old recycle is in flight. Unblocker: restart the frontend.md session in pane %166, or let that supervisor re-register. Then click Run Agent Doc and look for route_dispatch_only_recycle_inflight_ready_after_start outcome=settled, or supervisor_recycle_graph_settled reason=dispatch_ready_after_start, in haiven-dev ops.log, with no refusal=recycle_ttl_elapsed_supervisor_alive.
- [/] [#jb262dynlauncherrecommen] [recommended] [operator-verify] [agent-doc] Operator decision: jb262dynupgrade (153c72916) chose the accurate-warning option (a 262 package reports declined_by=modular_package and falls back to file replacement + restart). Either keep that as the supported policy (then close this), or authorize a real restart-free launcher for the exact-262 split-mode plugin that swaps backend + frontend (the frontend may run in a separate JetBrains Client process), proven against a live 262 IDE.
- [/] [#drivecapturedsplice] [operator-verify] Captured-splice stale-replica live proof (plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md). Recipe and regression test shipped in v0.35.466; live proof remains gated because the corrected run reached owner pane %134 but stopped before the retained-delivery marker, edit two, and verifier completion. Rerun the recipe through verify-captured-splice-recovery and require it to pass.
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
