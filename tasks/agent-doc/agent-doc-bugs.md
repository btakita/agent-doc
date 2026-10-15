---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: ba846c4c-2e01-4e12-9539-096faec207ee
  codex: 01a13cac-54da-7403-9456-6fc418818870
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
Review pass done. frontendlivedispatch closed on live evidence. Two review items remain, both genuinely gated: an operator decision (jb262dynlauncherrecommen) and an external PyPI reply (pypiquotarecheck).
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append subagents -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261015-024625.md`*

Compacted content:
- Archived 4 response topic(s): agent-doc fixes, release, and tmux auto-sync; GitHub issue 259 closeout; GitHub issue 260 closeout; 1 more
- Prior summary/context: prior compacted content: Archived 17 response topic(s): strict-template failure and steering stall; retained closeout...

### Re: dispatched #gh-fix 261–264 + release batch — opus-5.5 · 2026-10-14T22:49-04:00

I picked up the four `#gh-fix` queue items and gave each one to its own background subagent. They all use the queue preset `#spec-test-build-install-commit-push`. Every subagent works in its own worktree outside agent-loop, branched from `origin/main` at v0.35.503. The local `src/agent-doc` checkout is 662 commits behind `origin/main`, so nothing branches from it.

| item | subagent | worktree / branch | scope |
|---|---|---|---|
| GH #261 | `subagent:gh261` | `~/work/btakita/agent-doc-wt/gh261` · `fix/gh261` | After promotion, the old controller stops its watchdog and orphan-drain ticks. Its drain gets a time limit so idle pooled connections can't hold it open, and its frozen hub stops answering reads as current. |
| GH #262 | `subagent:gh262` | `~/work/btakita/agent-doc-wt/gh262` · `fix/gh262` | The fallback watches Remote Dev `CONTROLLER` sessions (`ClientKind.REMOTE`), logs which sessions it observes at install, and gets a test that a `CONTROLLER` session is observed |
| GH #263 | `subagent:gh263` | `~/work/btakita/agent-doc-wt/gh263` · `fix/gh263` | Process start time comes from `/proc/<pid>/stat` `starttime` + `btime`, not the `/proc/<pid>` mtime. Every call site moves to one shared helper, and the tests that faked the mtime are rewritten. |
| GH #264 | `subagent:gh264` | `~/work/btakita/agent-doc-wt/gh264` · `fix/gh264` | Component scope uses only top-level body ranges, so nested `agent:salient-response` no longer panics. A panic in a controller request comes back as an error that names the document. |

Subagents test in their own worktrees (targeted tests, clippy, `make check-fast`) and push their own branches. They do not run `make install`, close issues, or touch `main`.

**`release + publish` (added while this turn was running):** I claimed it for myself (`coordinator:release-after-gh261-264`) instead of giving it to a subagent. It can only run after the four fixes are merged, and subagents may not cut releases. Next steps once the subagents report:

1. Merge `fix/gh261`…`fix/gh264` into `main` one at a time.
2. Run one full `make check`, then one `make install`.
3. Push `main`, close #261–#264 with the fix commits, and release v0.35.504.
4. Release the queue claims and close the items.

### Re: Subagent completed — agent-doc: `subagent:gh262` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/262”; coordinator integration and closeout are pending.
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=b0207481330a0cca237e9ba4c3fc75f6dcabb3d58fca9a2fca77c4dd101cb501 -->

### Re: Subagent completed — agent-doc: `subagent:gh264` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/264”; coordinator integration and closeout are pending.
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=235197fb29a2b8aa99889c8cd2d40bea9f5f810c8004a4c8fd89beb377468851 -->

### Re: Subagent completed — agent-doc: `subagent:gh263` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/263”; coordinator integration and closeout are pending.
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=a60233b7c14ac95ec8b58f4707e265a6b96c0541600746e475077e13edc18940 -->

### Re: Subagent completed — agent-doc: `subagent:gh261` returned work for queue item “#gh-fix https://github.com/btakita/agent-doc/issues/261”; coordinator integration and closeout are pending.
Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.
<!-- agent-doc-subagent-completion key=7dce203b65ae7d8dcdb756b4de8fd2e0e3f45efa09612bfc4582397b23830773 -->
> **Chat prompt (#chatprompt):** can we use formal logic to ensure the tmux auto-sync class of error does not occur?

### Re: formal guarantee for the tmux auto-sync wedge — opus-5.5 · 2026-10-14T23:50-04:00

Yes. The repo already has the model this needs. What missed the bug is the link between that model and the code.

**Why the existing model stayed green.** `formal/tla/PassiveTmuxSyncNet.tla` already models editor focus → tmux over the adversarial `NetChannel` (messages can be delayed, reordered, dropped, duplicated or cut by a reconnect). It proves the liveness property `VisibleEventuallyMatchesEditor == <>[](visible = desired)`. It also has wedge configs that TLC must refute: `AdvanceOnEffect=FALSE` (F14/F15) and `SeqFence=FALSE`. But the model has no *retire/forget* transition. The editor can only send `Focus(doc, seq)`. The regression added an action the model does not contain: the plugin called `forgetEditorSurface` on its own frame when a reply came back after a newer observation. On the controller side, `ControllerEditorSurfaceGraph::observe` rejects a retired endpoint permanently at its unchanged per-process generation. TLC proved liveness for a protocol that no longer matched the shipped one.

**What would prevent this class of error:**

1. **Model the retirement channel.** Add `Retire(frame, gen)` messages, a per-process generation that only a reload changes, the controller's `retired && gen <= current` fence, and frontend handoff. Add the property *a live endpoint is never permanently muted*: `LiveEndpointNeverMuted == \A e : [](Live(e) /\ Refocused(e) => <>Accepted(e))`. The existing `<>[](visible = desired)` must hold with retirement enabled. Following the repo's pattern, add a knob `RetireOnNewerReply`, set TRUE in a `PassiveTmuxSyncNetRetireWedge.cfg`. `scripts/run_tla.sh` asserts that every wedge violates its property, so TLC has to reproduce the 02:54Z wedge as a counterexample, which proves the model can see this bug class. Add a Reach config so the fixed model isn't green only because it never retires anything.
2. **Make retirement recoverable by construction.** The safest rule removes the class rather than detecting it: a retirement is terminal only for a generation that has already ended. A client whose live frame is rejected with `rejected=retired_endpoint` re-registers under a fresh generation. Then no client-side mistake can mute a live endpoint for longer than one round trip, and that recovery is itself an action in the model. The fix now on `fix/tmux-autosync` removes the bad trigger and names the rejection reason, but a future bad trigger could still wedge it.
3. **Bind the model to the code.** This is the step whose absence let the bug ship.
   - **Action-set conformance test:** enumerate every RPC and plugin call that changes surface-graph state (`observe`, `forget`/retire, handoff) and assert each one is listed against a model action. Adding an unmodeled transition then fails `make check` instead of silently voiding the proof.
   - **Trace validation:** replay `controller_editor_surface_observed/forgotten` sequences from SimWorld runs, and from the 00:45Z and 02:54Z wedge windows in `ops.log`, through a checker for the model's next-state relation. The real wedge trace becomes a permanent regression fixture.
   - **Runtime monitor of the liveness property:** if the same connected endpoint gets `accepted=false rejected=retired_endpoint` K times in a row, warn, or heal by forcing re-registration. A model-to-code gap then shows up within seconds instead of after an operator notices that tabs stopped syncing.

**Limits.** TLC checks a bounded model, so it proves the protocol, not the Kotlin or Rust. The guarantee is only as strong as the conformance step in (3). A model is not proof of the code until something mechanically ties the code's transitions to it. Lean already holds the wait-machine and authority-ladder proofs. It could also prove the pure `ControllerEditorSurfaceGraph::observe` fence function, but the cross-process wedge belongs in TLA+.

If you want this, I'd queue it as one item after the v0.35.504 release: (1) the TLA+ extension with wedge and reach configs, (2) re-registration on `retired_endpoint`, (3) the action-set conformance test, trace fixtures for the two wedge windows, and the runtime monitor.
<!-- agent:boundary:72605a80 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
- #gh-fix https://github.com/btakita/agent-doc/issues/261
- #gh-fix https://github.com/btakita/agent-doc/issues/262
- #gh-fix https://github.com/btakita/agent-doc/issues/263
- #gh-fix https://github.com/btakita/agent-doc/issues/264
- tmux pane auto-sync not working
- Add a `priority` attribute to `agent:review`.
- Is `agent:todo` implemented? I don't see it being used. Please fix. There should be todo's when the operator needs to take an action. If a agent:todo is not in the document, agent-doc should add it.
- do your formal logic suggestions
- release + publish
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
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
