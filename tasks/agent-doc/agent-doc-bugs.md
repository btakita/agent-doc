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

### Re: Clear Session Context on api.md wedged — same stale dispatch receipt — opus-5.5 · 2026-10-11T17:07-04:00

> **Queue prompt:** Clear Session Context on api.md is wedged.

**Deferred:** this is the same bug as `sdk.md`. From the haiven-dev ops.log (`doc=api`, pane %165):
- **The stale record:** the Run Agent Doc turn dispatched at 20:37 was promoted to `running` at 20:37:25 and closed out at 20:38:33, but its dispatch record was never settled.
- **Your presses:** all four Clear presses (21:04:32–57) were deferred or absorbed on that stale `dispatch_turn_running`, with no feedback, so it looked wedged.
- **The pane:** it is at an idle `❯` prompt, with one background subagent still running ("Checking DecisionFeed.jsx…").
- **When it fires:** the deferred clear should go through on its own once the record ages past 30 minutes, at about 17:07 local (21:07Z), which is about now. That `/clear` may cut off the background subagent.

I gave this to the same subagent as the sdk.md item (claim `subagent:clearlive`). Two more requirements are added to its fix:
- Repeat presses while a clear is waiting show visible feedback: what it's waiting on, and how old that evidence is.
- An explicit, tested rule for a Clear when the prompt is idle but background agents or shells are still running.

**Now:** if it hasn't cleared by 17:08, run `agent-doc session interrupt-clear tasks/api.md` from `src/haiven-dev`. It interrupts whatever is running in that pane, which here likely means that background subagent too.

### Re: two integration batches landed and installed; frontend.md wedge dispatched; release held for it — opus-5.5 · 2026-10-11T18:25-04:00

> **Queue prompt:** do [#splicectlcommit]

Done (branch `splicectl`, `341d2d142a`, on main via `3fbe838baa`).
- **Scratch flag:** a document can declare `agent_doc_scratch: true`. If it's git-ignored, the controller-routed commit now ends in a clean skip (`commit_skipped_ignored_scratch`). It never stages the file, and it no longer logs `commit_error`. An undeclared ignored document is still refused, exactly as before.
- **Ignored-path closeout:** it no longer waits forever in capture recovery (new terminal outcome `ClosedIgnoredPath`). It also no longer fails on an unrelated stale submodule pointer.
- **Recipe:** it now declares its scratch doc instead of relying on GIT_DIR.
- **Not yet proven live:** the post-closeout session-check under a real editor. The next live captured-splice run will show it.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/240

Fixed and closed (branch `gh240`, `f82ea45372`, on main via `3fbe838baa`). The issue closed automatically when the commit landed, and I posted the explanation as a comment. The closeout now records exactly which head it approved, and the consume takes the leading head only if it is still that same head. Otherwise it does nothing, the other head stays live, and `queue: go` stays set. The regression test reproduces the incident without the fix. I filed the remaining risk, two closeout drivers overlapping, as closeoutleaseresume.

> **Queue prompt:** Clear Session Context on sdk.md did not take effect despite me running it twice. Run Agent Doc was sandwiched in between the 2 Clear Session Context calls.

> **Queue prompt:** Clear Session Context on api.md is wedged.

Both fixed, together with `#clearqueuedliveturn` below (branch `clearlive`, `37bc4add5a`, on main as `472ec36906`, now installed). It was one defect with three parts:
1. **The stale receipt:** after a routed Run Agent Doc turn, the dispatch receipt stayed `running` for 30 minutes after the turn ended, so the pane looked live. The turn-end hook (`turn-status idle`) now settles it.
2. **Lost presses:** a repeat press re-arms the queued clear instead of being absorbed. A run you started before pressing Clear becomes the clear's turn instead of cancelling it. A run that's accepted but hasn't started yet counts as a live turn.
3. **Background agents hid the prompt:** Claude's background-agent row (`◯ general-purpose …`) hid the `❯` prompt from the idle check, which is why api.md never fired even after the 30 minutes. That row is now treated as screen decoration.

The notice also now shows `evidence_age_secs`. By the spec, an idle prompt with background agents running is not a live turn.

> **Queue prompt:** do [#clearqueuedliveturn]

Done as an automated live check instead of a manual one. `tests/clear_queued_live_turn_tmux.rs` runs the real `agent-doc start` supervisor on a private tmux server against a fake `claude`. It covers four cases: a turn proven live only by its receipt clears exactly once after Stop, a newer prompt drops the clear, a repeat press re-arms it, and Clear→Run→Clear delivers both clears in order. I ran that suite on the integrated tree: 4/4 passed. Disabling any one of the five fixes makes at least one of those tests fail. What still needs your eyes is filed as clearnoticeverify: the IntelliJ notice wording, and what `/clear` does to running background agents. Separately, a newer prompt dropping a queued clear still only logs it without telling you, filed as clearsupersedenotice.

**Integration and verification:**
- **Batch 1 (`splicectl` + `gh240`):** I made a mistake here. I pushed `3fbe838baa` to main before reading `make check`'s result. That first run had stopped after 1 failure in 101 of 10205 tests. The rerun on the same commit passed all 10205, so the failure is nondeterministic. I deleted the first log before reading it, so I can't name the test. I filed flakyintegrate0494 to capture it when it recurs. I didn't rerun it away. After the passing rerun, I ran `make install` (exit 0).
- **Batch 2 (`clearlive`):** `make check` exit 0, 10215 tests passed. The live tmux suite passed 4/4. Pushed `472ec36906` and ran `make install` (exit 0).
- One note: the clear subagent briefly started the real `claude` program by accident, inside its private tmux server in a temp directory, then killed it within seconds. Your real tmux server, panes and IDE were not touched.

> **Queue prompt:** frondend.md end of turn wedges session...with the formatting messed up ![img_4.png](img_4.png)

**Deferred:** this went to a background subagent (claim `subagent:frontendwedge`, worktree `agent-doc-wt/frontendwedge` off `472ec36906`). The screenshot shows repeated `[perf] find_code_ranges: Nms` lines written straight over the Claude Code composer, footer and status line. That means a process at end of turn, likely a hook or a supervisor child, still has the pane's terminal as its stderr, and the stray text in the composer plausibly defeats ready-prompt detection. The subagent will find what emits it and route it away from the pane, add tests, and check whether the wedge is only the polluted composer or a separate hang.

> **Queue prompt:** release + publish

**Deferred:** claimed by me (`coordinator:release-0491`). Main has both batches. I'm holding the tag until the frontend.md wedge fix lands, so v0.35.491 doesn't ship a known bug that breaks the pane. Then: `make release-version`, the `VERSIONS.md` entry, `make install-full`, and `make release`.

### Re: what scrambles the tmux pane — opus-5.5 · 2026-10-11T18:28-04:00

> **Chat prompt (#chatprompt):** What causes the tmux session formatting to get messed up?

Two things combine, and only one is new.

1. **What prints the text (old):** `agent-doc-element/src/element.rs:1028`. `find_code_ranges`, the markdown scan that finds code spans so component markers inside code aren't parsed, ends with `eprintln!("[perf] find_code_ranges: {}ms")` whenever a call takes 1ms or more. It's a library function, so it runs in every agent-doc process: the CLI, hooks, the supervisor and the editor's native library. On a large session doc nearly every call takes over 1ms, so it prints constantly. It has been there about 4 months (`95ea1b7aec`). Normally that output goes to a log file or a hook's captured stderr, so nobody sees it.
2. **Where it goes (the likely new part):** a process whose stderr is the pane's own terminal. The repo rule (`#restartstderrbleed`) is that the supervisor sends its stderr to `.agent-doc/logs/supervisor-stderr.log`, and nothing it spawns inherits the pane. The text in your screenshot came from a process that broke that rule. Those writes go straight to the terminal, behind Claude Code's back. Claude Code's TUI draws by moving the cursor and repainting its own regions, so foreign bytes land wherever the cursor happens to be. That's why the text shows up inside the composer (`❯ [perf] …`), on the separator line, after the branch name, and after `← 4 agents`, and why the layout looks shredded until a full redraw. A composer with `[perf] …` in it also stops looking like an idle prompt, which can make agent-doc treat the turn as still running. That's the likely "wedge".

**Suspected trigger:** I ran `make install` of the clear-fix batch at about 18:10. Install recycles every supervisor onto the new binary at its next idle boundary, and your screenshot is from 18:16. So the prime suspect is that the hot-reloaded supervisor, or the newly expanded `turn-status idle` hook path, ended up with its stderr on the pane. That's not confirmed yet. I sent the location and this lead to the frontend.md subagent. It will fix both layers: no unconditional perf printing in library code, and no process with stderr on the pane. If the stray text is the only problem, a redraw (resizing the pane, or Ctrl+L) clears it for now. If the session still won't accept input, the wedge is separate, and the subagent is checking for that too.

I also dispatched the three new follow-ups to their own subagents: `#closeoutleaseresume` (the resume takes the closeout lease), `#flakyintegrate0494` (keep test output from every run and hunt the nondeterministic failure), and `#clearsupersedenotice` (tell you when a newer prompt drops a queued clear).
<!-- agent:boundary:75cf33a3 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#clearqueuedliveturn]~~
- ~~do [#splicectlcommit]~~
- 🚧 #gh-fix https://github.com/btakita/agent-doc/issues/240
- 🚧 Clear Session Context on sdk.md did not take effect despite me running it twice. Run Agent Doc was sandwiched in between the 2 Clear Session Context calls.
- 🚧 Clear Session Context on api.md is wedged.
- frondend.md end of turn wedges session...with the formatting messed up ![img_4.png](img_4.png)
- release + publish
- do [#closeoutleaseresume]
- do [#flakyintegrate0494]
- do [#clearsupersedenotice]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#closeoutleaseresume] [agent-doc] The controller's captured-finalize resume can fall back to the legacy repair path, which strikes answered queue heads (agent-doc-repair-io/src/lib.rs ~1094) without first acquiring the closeout lease respond holds; the resume's start gate (agent-doc-supervisor/src/idle_watch.rs:37) does not check the lease either. Two closeout drivers can overlap (GH #240 incident). Make the resume acquire the closeout lease before any repair mutation, with a regression test.
- [ ] [#flakyintegrate0494] [agent-doc] A full make check on integrate-0494 (merge of splicectl + gh240, 3fbe838baa) stopped at 1 failure after 101/10205 tests; the rerun on the same commit passed 10205/10205. The first log was deleted before reading, so the test is unknown. Capture it on recurrence (keep nextest junit/output from every authoritative run) and fix the nondeterminism; never rerun to green.
- [ ] [#clearsupersedenotice] [agent-doc] When a newer prompt drops an operator-queued Clear Session Context (idle_queue_watch_deferred_clear_superseded), the operator gets no notice; surface it in the editor like the queued notice.
- [ ] [#clearnoticeverify] [operator-verify] [agent-doc] Eyeball the queued Clear Session Context notices in IntelliJ after 472ec36906 (queued / re-armed / evidence_age_secs wording) and confirm /clear with running Claude background agents behaves as you expect.
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
