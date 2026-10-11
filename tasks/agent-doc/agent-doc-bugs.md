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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261011-201227.md`*

Compacted content:
- Archived 9 response topic(s): what #drivecapturedsplice still needs; release status; v0.35.490 released; 6 more
- Prior summary/context: prior compacted content: Archived 29 response topic(s): actionable-review dispatched; v0.35.487 batch status; actiona...

### Re: dispatched GH #240 to a subagent — opus-5.5 · 2026-10-11T16:36-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/240

**Deferred:** this went to a background subagent (claim `subagent:gh240`, worktree `agent-doc-wt/gh240`, based on `23a5d7c0a4`). Issue #240 describes this sequence: a free-text closeout approves consumption against the head as it stood before the write. After the answered head is struck, the consume re-reads the document and also takes the next, unanswered head (`[#<id-a>]: go`) and clears `queue: go`. Session-check then blames the response for the loss. A second closeout driver (the controller's captured-finalize resume beside `respond`) may have committed between the steps. The subagent will first reproduce it with a deterministic test. Then it will bind the consume to the exact head identity approved at proof time, and check the double-driver commit and the session-check misattribution. It adds tests and updates the specs. Following `#gh-fix` ("fix then close"), I'll close the issue after the fix is integrated on main, using the closing comment the subagent drafts.

`#splicectlcommit` and `#clearqueuedliveturn` are still with their subagents.

### Re: Clear Session Context on sdk.md lost both presses — opus-5.5 · 2026-10-11T16:49-04:00

> **Queue prompt:** Clear Session Context on sdk.md did not take effect despite me running it twice. Run Agent Doc was sandwiched in between the 2 Clear Session Context calls.

**Deferred:** I've confirmed it from the haiven-dev ops.log (`doc=sdk`, pane %180), on the queued-clear build I installed at `23a5d7c0a4`:
1. **20:45:19, Clear #1:** deferred as `queued` because `evidence=dispatch_turn_running`. But the actor was `ready`, the pane showed an idle prompt (`⏵⏵ bypass permissions on · ← 4 agents`), and at 20:47:29 the binary itself logged `turn_live=false turn_ended_at=18:19:55`. A dispatch receipt seems to have been stuck at `running` since then. That stale receipt "proved" a live turn, so the clear was deferred when it should have gone through immediately. The deferred clear then waits for that same receipt to settle, which never happens.
2. **20:45:52–58, Run Agent Doc:** dispatched and submitted.
3. **20:45:55, Clear #2:** arrived before the submit and was absorbed as `already_deferred`. The new prompt then superseded the deferred clear, so both presses were lost. No `/clear` was ever sent, and nothing told you so.

This is the same code and the same open question (the turn proven live only by its dispatch receipt) as `#clearqueuedliveturn`. So I forwarded the evidence to that item's subagent (claim `subagent:clearlive`) rather than start a second, conflicting branch. It is to reproduce this exact sequence with tests. Then fix it so a stale `running` receipt can never prove a live turn (the receipt is fenced to its turn and settles when the turn ends), and so a press that arrives after a newer prompt applies to the new turn rather than being silently lost. If a clear is dropped anyway, you'll get a notice. It will also draft a GitHub issue summary.

**Workaround until it lands:** run `agent-doc session interrupt-clear tasks/sdk.md` from `src/haiven-dev`. It interrupts whatever is running in pane %180 (here the pane is idle apart from its 4 background agents) and clears. Or wait for the fix and press Clear once.
<!-- agent:boundary:75cf33a3 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 do [#clearqueuedliveturn]
- 🚧 do [#splicectlcommit]
- 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/240
- Clear Session Context on sdk.md did not take effect despite me running it twice. Run Agent Doc was sandwiched in between the 2 Clear Session Context calls.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] 🚧 [#splicectlcommit] [agent-doc] A strict write --commit on a git-ignored scratch doc under a live editor is routed to the project controller (commit_document_via_controller), which commits against the product repo and refuses the ignored path (commit_skipped_ignored_path + commit_error), so the captured-splice recipe's isolated GIT_DIR never applies. Either give the controller commit an explicit commit-target override or treat an ignored scratch doc as a clean skip instead of commit_error. Evidence: src/agent-doc ops.log doc=xdotool-captured-splice-1791744122 18:42:24Z.
- [ ] 🚧 [#clearqueuedliveturn] [operator-verify] [agent-doc] Live-check the queued Clear Session Context (merged 23a5d7c0a4): press Clear while a route-dispatched turn is running; expect an info notice 'queued … runs when that turn ends', exit 0, then exactly one /clear at the idle boundary, and none if a newer prompt starts first. Specifically confirm the turn-ended projection arrives when the turn is proven live only by its dispatch receipt (the pane %149 case).
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
