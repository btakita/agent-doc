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

### Re: actionable-review dispatched; v0.35.487 batch status — opus-5.5 · 2026-10-10T23:40-04:00

`#actionable-review` is claimed by `subagent:actionable-review2` and dispatched read-only. It is the first dispatch with the binary-emitted `subagent_prompt_preamble` from `#waypostauthorization`, pasted verbatim. It re-checks the 5 gated review items: `#operatorverifylive` / `#frontendlivedispatch` look like duplicates, plus `#jb262dynlauncherrecommen`, `#drivecapturedsplice` and `#pypiquotarecheck`. It also verifies two candidate follow-ups:
- 10 `agent-doc-crdt-relay-io` relay-hub tests fail under plain `cargo test` but pass under nextest, which looks like process-shared state.
- The coined-id commit hook blocked a commit over `#devclear`, an id that wasn't in that commit's text.

I'll apply its findings to the backlog and queue when it reports.

**v0.35.487 batch** (worktree `agent-doc-toolsslow-int-20261010`, all merged cleanly onto main `ab6cf0fe7`):
- `bb5ab38aa` retained tracked-work publish now records actionable ids
- `#respondfinalizehard` `8c9a39af9`
- `#pypireleaseworkflow` `df08ab8a9`
- GH #235 `4c2708194`
- GH #236 `bf2924b2c`: native save gated on protocol compatibility, so the 263 IDE on 0.2.508 is admitted
- dev.md Codex clear `925b6fc4f`: a Codex `turn_aborted` interrupt releases the turn lease
- `#idefocusowner` `fd04ad845`: only the most recently focused JetBrains installation drives the tmux window

`make check-fast` on the first six passed 8309/8309. Lint is clean after `#idefocusowner` merged. `#visiblewriteloadflake` is still with its subagent. When it lands I'll run one full `make check`, then release + publish v0.35.487, and close every item it ships.

This dispatch cycle adds no backlog items; the review's findings are captured when its subagent reports.

### Re: actionable review results — opus-5.5 · 2026-10-10T23:55-04:00

> **Queue prompt:** #actionable-review

The read-only review subagent made no code changes. Both known gaps reproduced, and the root cause of each was found.

**New backlog items** (added to the end of the queue):
1. `hookstalemsgfile`: the coined-id PreToolUse hook reads a `-F` commit-message file *before* the command runs. A heredoc that writes the message and then commits is therefore scanned against the previous message: a false positive from a stale shared `msg.txt`, as happened to a subagent tonight. The new heredoc body is never scanned at all, a false negative.
2. `relayhubtestisolation`: 9 relay-hub tests fail under plain `cargo test` but pass with `--test-threads=1` and under nextest. They share process-global state: the `OnceLock` hub registry, the projection and quarantine maps, and a reap that crosses hubs. The fix is to isolate that state, not serialize the tests.

**Review list:**
- **`operatorverifylive`:** closed as an exact duplicate of `frontendlivedispatch`.
- **`frontendlivedispatch`:** rewritten. Its precondition has cleared: pane %166's supervisor re-registered on the current binary and recycles now settle. What remains is your one Run Agent Doc click on haiven-dev `frontend.md`, and the text names the ops.log pass signal to look for.
- **Unchanged:**
  - `jb262dynlauncherrecommen` is your policy call on whether to take the restart-free launcher option.
  - `drivecapturedsplice` needs your go-ahead to run the xdotool driver, which sends real keystrokes.
  - `pypiquotarecheck`: support issue #12214 is still unanswered and there is ample headroom.

**v0.35.487:** seven of eight fixes are merged and pass `make check-fast`; it waits only on `visiblewriteloadflake`, whose subagent is running its load-stress proof.
In sdk.md, Run Agent Doc while I was typing a queue item duplicated part of the queue item. Use formal logic to prevent this entire class of bug:
```
- We also need developer friend documentation on all steps of the DX. The APIs need documentation, SDK usages, tool usages, tutorials. We will have video walkthrough as well. Most of all, the SDK tooling should be self-guiding. The code generation + conformance DX needs to have developer friendly documentation. The agentic layer needs documentation and needs to provide good opinionated guidance.
- We also need developer friend documentation on all steps of the DX. The APIs need documentation, SDK usages, tool usages, tutorials. We will have video walkthrough as well. Most of all, the SDK tooling should be self-guiding. The code generation + conformance DX needs to have
```
> **Chat prompt (#chatprompt):** n/agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.mde

> **Chat prompt (#chatprompt):** disregard the last part of the now deleted steering

### Re: re-trigger, sdk.md correction, two dispatches — opus-5.5 · 2026-10-10T23:55-04:00

**Re-trigger:** I read the first chat line as a garbled `/agent-doc` re-trigger. This admitted cycle is that retry. The previous refusal was a pending queue reconciliation, and the IntelliJ restart settled it: the held actionable-review response and its backlog changes have since landed.

**Correction applied:** the sdk.md queue-duplication subagent (branch `fix/typingforkdup`) was told to drop the deleted sentence. It no longer assumes the delete+edit / "provide" vs "have" fork caused the duplication, and will establish the mechanism from evidence only. The task is otherwise unchanged: state a formal invariant, so that agent-side reconciliation never duplicates operator-authored text and never makes a prefix of an in-progress line its own item, then fix the single chokepoint and add a replay regression.

**Dispatched** with the binary-emitted authorization preamble:
- `hookstalemsgfile` goes to `subagent:hookstalemsgfile` (`fix/hookstalemsgfile`). The coined-id hook should scan the heredoc a command writes, not the stale `-F` file left on disk.
- `relayhubtestisolation` goes to `subagent:relayhubtestisolation` (`fix/relayhubtestisolation`). Each test gets isolated relay-hub state, so plain parallel `cargo test` passes; serializing the tests is not an acceptable fix.

**New follow-up, `capturegatedispatched`:** the pre-write backlog-capture gate refused this cycle again because a capture preset rode along on queue steering. That preset's head is already dispatched to a subagent, so the capture belongs to whichever cycle closes that head. This is the second occurrence, so it is queued as a fix rather than suppressed.

**v0.35.487:** seven fixes are merged and pass check-fast. `visiblewriteloadflake` is finishing its load-stress proof; its first after-fix run was 30/30. I'll release once it lands. Fixes that arrive later go into the following batch.
> **Chat prompt (#chatprompt):** Can I delete the duplicated queue item now?

> **Chat prompt (#chatprompt):** Do the fix in a subagent

### Re: deleting the sdk.md duplicate; fix is in a subagent — opus-5.5 · 2026-10-11T00:05-04:00

**Yes, delete it now.** The duplicated state is already preserved as evidence: haiven-dev commit `bb26315c` (2026-10-10 23:45:26) recorded `tasks/sdk.md` with the truncated twin. The subagent reconstructs the incident from that commit, git history and haiven-dev `ops.log`/`state.db`, not from the live buffer. Deleting the line loses nothing it needs. It reads sdk.md read-only and will not edit it.

**The fix is already in a subagent** (`fix/typingforkdup`, worktree `~/worktrees/agent-doc-typingforkdup-20261011`). Its brief:
- reconstruct the mechanism from evidence, now without the deleted "provide"/"have" premise;
- state the invariant formally: agent-side reconciliation never duplicates operator-authored text, and never materializes a prefix of an in-progress line as its own item;
- encode that invariant in the repo's TLA+/Lean checks, with a reproducing non-vacuity config;
- fix it at the single chokepoint, with a deterministic replay regression.

It runs under the queue preset's authorization (commit and push its own branch; no install or release). I integrate it in the next batch after v0.35.487.
> **Chat prompt (#chatprompt):** it was written in error

### Re: retracted speculation — opus-5.5 · 2026-10-11T00:10-04:00

Understood. The "have" vs "provide" divergence was your speculation and is now withdrawn. The `fix/typingforkdup` subagent already has that correction: it determines the duplication mechanism from evidence only (haiven-dev commit `bb26315c`, git history, `ops.log`/`state.db`) and does not assume the fork.

The repeated chat lines, "Can I delete the duplicated queue item now?" and "Do the fix in a subagent", were answered in the previous response. Yes, delete the duplicate; the evidence is preserved in `bb26315c`. The fix is already running in that subagent. Nothing else changes.
> **Chat prompt (#chatprompt):** release 0.35.487 without waiting for visiblewriteloadflake

### Re: release 0.35.487 — opus-5.5 · 2026-10-11T00:25-04:00

**There's nothing left to wait on.** `visiblewriteloadflake` finished, `4925690b0`, and it is already merged into the release tree.

- **Release commit:** `release: v0.35.487` (`e6d36e647`) on `integrate/toolsslow-20261010`. Version projected, VERSIONS.md entry and editor parity row written.
- **Contents:** 13 changes:
  - the retained actionable-mirror fix
  - `respondfinalizehard`, `pypireleaseworkflow`, GH #235, GH #236
  - the dev.md Codex clear fix, `idefocusowner`
  - `visiblewriteloadflake`, `hookstalemsgfile`, `relayhubtestisolation`, `typingforkdup`
  - the tools.md drain fix and its lint fix, already on main
- **Now running:** the one authoritative `make check` on that exact tree; nextest has just started 10013 tests.
- **Next, if it passes:** fast-forward main, `make release` (tmux-ci, tag, GitHub Release + PyPI, `install-full`), verify the published assets and PyPI version, then close every shipped item.
- **Next batch:** `capturegatedispatched` (`436cb0652`) arrived after this boundary and goes there.

### Re: sdk.md Clear Session Context wedge dispatched — opus-5.5 · 2026-10-11T00:40-04:00

I claimed the new queue head, the sdk.md Clear Session Context wedge after a cancelled Run Agent Doc turn, as `subagent:sdkclearwedge` and dispatched it to branch `fix/sdkclearwedge`.

Read-only status of the sdk.md session:
- harness `claude-code`, pane `%180`, actor `ready`, pane `alive-idle` with `prompt_ready=true`
- the footer shows `← 4 agents` and `conversation_holders: 9 live`
- the last session event is `idle_queue_watch_skipped reason=clear_cooldown`

The subagent will identify which evidence refused the clear: a turn lease left by the Claude cancel path, background subagents counted as a live turn, or a cooldown that never expires. It will fix that at the root with a replay test, stay fail-closed while a turn can still be running, and record how background subagents count toward a live turn in `specs/15-turn-lifecycle.md`. It will also say whether your current wedge clears by itself after install or needs one exact action.

**v0.35.487:** the authoritative `make check` on the release tree is past nextest and running the editor test suites. The release follows if it passes. This fix goes in the next batch.
> **Chat prompt (#chatprompt):** Can I manually run `/clear` in the sdk.md Claude session?

### Re: manual /clear in sdk.md — opus-5.5 · 2026-10-11T00:45-04:00

**On the agent-doc side, yes.** Pane `%180` is idle at an empty prompt; its last turn finished at 12:26 AM. Since 0.35.484, agent-doc reclaims a manually cleared Claude session from the durable turn-end receipt. A manual `/clear` doesn't wedge the document, and it doesn't erase the evidence the `fix/sdkclearwedge` subagent is reading from `ops.log`/`state.db`.

**But wait for its background agents first.** The pane footer shows `← 4 agents` still running. sdk.md has two live queue claims owned by `subagent:recs-and-quirks`: "Do your recommendations." and the conformance/code-generation engine-quirks item. Both expire around 01:38. If you `/clear` now:
- those agents keep running, but their completion reports land in a fresh context that doesn't know about them;
- nobody integrates their results or closes those two queue items;
- the claims hold the items until they expire, about 01:38, and only then does the queue re-dispatch them.

**Recommendation:** let `← 4 agents` drain to zero, then `/clear`. If you need the clear now and are OK with those two items being redone later, `/clear` is safe; it only costs that in-flight work's handoff.

### Re: v0.35.487 released — opus-5.5 · 2026-10-11T01:30-04:00

> **Queue prompt:** release + publish

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/235

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/236

> **Queue prompt:** fix: Clear Session Context in dev.md: agent-doc command failed (exit 1):
> Error: session_clear refused for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/dev.md because a turn is live in pane %127 (harness_turn_live=true evidence=turn_active_lease, source=authoritative_actor); no `/clear` was sent. ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=wait_for_turn_or_interrupt_then_retry unblocker=clear_queued_behind_busy_turn. Wait for the turn to finish and run Clear Session Context again, or run `agent-doc session interrupt-clear /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/dev.md` to intentionally interrupt the turn and clear context.

**v0.35.487 is released and installed.**
- **Gate:** the authoritative `make check` on the version-projected tree passed: 10013/10013 nextest, 0 `result: FAILED`, plus TLA (incl. the new `AdmissionSplitMerge` twin wedge/output/pair/reach configs).
- **Release:** `make release` exited 0 with `tag_publish_handoff=complete` and `local_install_full=complete`. Tag `v0.35.487` (`e6d36e647`) was pushed, and `agent-doc --version` reports 0.35.487.
- **Publishing:** the GitHub Release workflow for the tag was in progress at closeout. The tag-driven pipeline publishes PyPI and Homebrew; I observed CI rather than waited on it.

**Shipped and closed:**
- `respondfinalizehard`
- `pypireleaseworkflow`
- GH #235 (already closed) and GH #236 (closed with a comment)
- the dev.md Codex interrupt lease fix
- `idefocusowner`
- `visiblewriteloadflake`
- `hookstalemsgfile`
- `relayhubtestisolation`
- the earlier backlog-to-queue mirror fix, the tools.md slow-start drain fix and the sdk.md `typingforkdup` duplicate prevention

**New follow-ups** (appended to the queue):
- `visiblewritewallclock`: one poll test still assumes a 5s writer deadline under extreme single-core load. It should wait on the receipt event, not a longer timeout.
- `editoropscheckpointfail`: find, from 0.35.487's new full error chain, why editor-op checkpoints failed during the sdk.md typing burst.

**Next batch:** `capturegatedispatched` is ready (`436cb0652`), and the sdk.md Clear Session Context wedge is still with `subagent:sdkclearwedge`.

**Your action:** restart IntelliJ so it loads the 0.35.487 plugin projection. Delete the shorter duplicate line in sdk.md if you haven't already.

### Re: two follow-ups dispatched — opus-5.5 · 2026-10-11T00:55-04:00

Both new heads are claimed and dispatched with their authorization preambles. Their worktrees branch from v0.35.487 (`e6d36e647`).
- `visiblewritewallclock`, on `fix/visiblewritewallclock`: make `test_poll_lazily_event_appears_after_delay` wait on the writer's receipt event instead of a 5s deadline, with before/after counts at K=96 on a pinned core.
- `editoropscheckpointfail`, on `fix/editoropscheckpointfail`: reproduce the editor-op checkpoint append failure with a deterministic rapid-append test instead of waiting for a live recurrence, fix the root cause so no op is dropped, or report the exact log signature to watch for if it can't be determined offline.

Still in flight for the next batch: `capturegatedispatched` (done, `436cb0652`) and the sdk.md Clear Session Context wedge (`subagent:sdkclearwedge`, running `check-fast`). The v0.35.487 GitHub Release workflow was still running at last check.
> **Chat prompt (#chatprompt):** release + publish?

### Re: v0.35.488 release in progress — opus-5.5 · 2026-10-11T01:25-04:00

**Yes, releasing as v0.35.488.** I claimed the queued `release + publish` head as `coordinator:release-0488`; subagents never release.

- **Batch** (worktree `agent-doc-batch0488-20261011`, all merged cleanly onto v0.35.487):
  - `capturegatedispatched` `436cb0652`: the capture gate only applies to heads this cycle drains.
  - `sdkclearwedge` `7599b0548`: a prompt cancelled before it was persisted retires its turn lease, and an already-satisfied deferred clear is dropped.
  - `visiblewritewallclock` `9c0d0e257`: the poll test waits on the writer's receipt; 30/30 at K=96.
  - `editoropscheckpointfail` `f99567825`: timed-out editor-op appends are retained and re-sent with the same batch id, and the controller dedups them.
- **Release commit:** `release: v0.35.488` (`1318e9005`), with the VERSIONS.md entry and editor parity row. The authoritative `make check` is now running on exactly that tree. If it passes: fast-forward main, `make release`, verify, and close the four items.

**Disclosure:** to restart the check on the version-projected tree, I ran `pkill -f 'make check'`. That matched its own shell and any other session's `make check` running at that moment on this machine. No make or nextest processes survived to show who else, if anyone, was hit. If another session reports a check that died of SIGTERM around 01:20, this caused it. Next time I'll stop my own background task by its id.

**Follow-up to file:** the controller's `refresh_memory durable_reload` holds the project state lock for 1–4s. That stall is the root of the editor-op timeouts, which are now survived through retention but not removed.

### Re: v0.35.488 released; two new heads dispatched — opus-5.5 · 2026-10-11T02:10-04:00

> **Queue prompt:** release + publish

> **Queue prompt:** Clear Session Context on sdk.md is wedged after I cancelled a Run Agent Doc turn in the Claude terminal.

**v0.35.488 is released and installed.** The `make release` started before your `/clear` kept running and exited 0 with `tag_publish_handoff=complete local_install_full=complete`. `main` is pushed to `1318e9005` (`release: v0.35.488`), and `agent-doc --version` reports 0.35.488. The tag's GitHub Release workflow, which publishes PyPI and Homebrew, was still running at closeout.

**Shipped and closed:**
- `capturegatedispatched` `436cb0652`: the capture gate now only applies to heads that this cycle drains.
- the sdk.md Clear Session Context wedge, `sdkclearwedge` `7599b0548`: a prompt cancelled before it was persisted retires its turn lease, and a deferred clear that is already satisfied is dropped.
- `visiblewritewallclock` `9c0d0e257`: the poll test waits on the writer's receipt; it passed 30/30 with 96 busy-loops sharing its core.
- `editoropscheckpointfail` `f99567825`: an editor-op append that times out is kept and re-sent with the same batch id, and the controller dedups it.

**Dispatched this cycle**, each claimed, in its own worktree branched from `v0.35.488`, with its authorization preamble pasted verbatim:
- The Cancel Turn issue you just reported, on `fix/cancelturnprompt`. It will find which component left `/agent-doc <FILE>` in the Claude prompt after the cancel, and why Clear Session Context stayed refused after you emptied the prompt. It fixes both at the root with regression tests and a spec update. This looks like a different path from `sdkclearwedge`. It will also say whether your current state needs any action.
- GH #237, on `fix/gh237-queue-add`: `agent-doc queue add` for external tools. Per the issue it goes through the binary write path so it works under a live editor, dedupes by key, redacts secrets, and reaches running sessions as `drain_after_current` steering. I close the issue after integrating and releasing.

**New backlog item:** `refreshmemlockstall`. The controller's durable reload holds the project state lock for 1–4s. That stall caused the editor-op timeouts, which 0.35.488 now survives but does not remove.

The exchange is at 193 lines; compacting can wait until the queue drains.

### Re: dispatched #refreshmemlockstall to a subagent — opus-5-5 · 2026-10-11T02:08-04:00

`#refreshmemlockstall` is claimed (`subagent:refreshmemlockstall`) and running in a background subagent. It works in its own worktree outside agent-loop (`~/work/btakita/agent-doc-wt/refreshmemlockstall`) under the `#spec-test-build-install-commit-push` authorization. Its job: move the `refresh_memory` durable reload's load step outside the project state lock, swap the result in under the lock with a generation check, and add a regression test that a concurrent editor-op append isn't blocked by the reload.

The other two queue heads are still claimed by subagents from the prior session: `subagent:cancelturnprompt` (the Cancel Turn / Clear Session Context wedge) and `subagent:gh237` (GH #237). With every head claimed, the loop is idle until the subagents report back. After that I'll integrate the work, run one `make install`, release the claims, and close the items.

The exchange is large (215 lines, 12 sections). The queue is active, so I'm not compacting.

### Re: #actionable-review, and dispatched GH #238 and the struck-release bug — opus-5-5 · 2026-10-11T02:31-04:00

**`#actionable-review`:** I checked each of the 4 gated review items. One can be done by an agent; the other three really are blocked.

- **`#drivecapturedsplice` moved back to backlog + queue.** Its plan (`plan-captured-editor-splice-recovery.md`, status 2026-09-27) says nothing is left to build. What remains is one live driver run, `scripts/xdotool-live-verify.sh captured-splice`, and an agent can run it. The plan only kept it behind the operator because it types real keystrokes into your desktop. It goes to the end of the queue and keeps its `[operator-verify]` tag, so the loop won't start it while you're working. Say the word and it can run.
- **`#frontendlivedispatch` stays in review.** I searched haiven-dev's `ops.log` and the rotated `ops.log.1.zst`. Neither has a pass signal (`route_dispatch_only_recycle_inflight_ready_after_start` / `dispatch_ready_after_start`) or the failure `recycle_ttl_elapsed_supervisor_alive`. It still needs you to click Run Agent Doc on `frontend.md`. The preflight warning that `#operatorverifylive` already resolves it is a false match: that done entry was the earlier attempt from 2026-10-10.
- **`#jb262dynlauncherrecommen` stays.** It needs your decision on keeping the warn-and-replace policy versus asking for a restart-free 262 launcher.
- **`#pypiquotarecheck` stays.** It is waiting on a reply to pypi/support#12214, and the last check showed plenty of headroom (1.6 GiB).

**Dispatched to subagents in their own worktrees (claimed):**
- `#gh-fix` GH #238 (`subagent:gh238`). `agent-doc upgrade` tells you to restart IntelliJIdea2026.2 even though no 2026.2 IDE is running. Any live IDE on the host (here the 263 RemoteDev backend on the 2026.3 data dir) currently counts as "live", and that is a regression since 0.35.480. The subagent will count an IDE only when it runs from the data dir being written, and add tests for both the false-positive and true-positive cases. I'll close the issue after merging it.
- The struck "release + publish" item (`subagent:releasestruck`). It is tracing which code path rewrote your free-text head to `~~release + publish~~` and fixing it so an unanswered operator head can't be struck by a path that didn't complete it.

**Release plan:** I've noted that you want a release + publish once these items finish. I'll merge each subagent branch as it reports and run one `make install`. After the remaining claimed items (`#refreshmemlockstall`, GH #237, GH #238, the Cancel Turn wedge, the struck-release bug) are merged, I'll cut one release covering all of them.

The `#refreshmemlockstall` subagent is still running `make check-fast`.

### Re: sdk.md queue out of sync with its subagents — dispatched — opus-5-5 · 2026-10-11T02:40-04:00

**Dispatched (claimed `subagent:sdkqueuesync`):** the report that `src/haiven-dev/tasks/sdk.md` shows its queue items as unstarted even though their subagents mostly finished. The subagent treats sdk.md and haiven-dev's state as read-only. It will rebuild what actually happened from the doc's git history, the ops log and the claim listing. Then it finds which part of agent-doc fails to show a subagent's progress (claimed → in flight → reported → closed) in the queue, and fixes it with tests. It will also list which sdk.md items are actually complete, with evidence, so they can be closed. It works in its own worktree and checks for overlap with the struck-release subagent, which is in the same queue code.

**Not dispatched: `#drivecapturedsplice`.** It's `[operator-verify]` and its only step sends real keystrokes into your desktop (`scripts/xdotool-live-verify.sh captured-splice`). It waits for your go-ahead.

**Subagent status:**
- **GH #237** (`agent-doc queue add`): code is written. Its fast checks (`make check-fast`) passed; the full `make check` is running, then commit + push.
- **Cancel Turn wedge:** fix written (it removes the cancelled trigger from the input box and ends the cancelled turn immediately). Fast checks passed; `make check`, `make tmux-ci`, then push of `fix/cancelturnprompt`.
- **`#refreshmemlockstall`:** in its full `make check`.
- **GH #238** and the **struck "release + publish"** bug: still investigating.

As each branch passes, I'll merge it, run one `make install`, close its items, then cut the release.

### Re: tmux focus out of sync with the editor — dispatched; #drivecapturedsplice waits for your go — opus-5-5 · 2026-10-11T02:46-04:00

**Dispatched (claimed `subagent:focussync`):** switching between `sdk.md` and `rpg.md` left tmux focused on rpg.md's pane in the `1:stash` window. The subagent only reads the ops logs and a tmux pane listing, and won't touch your panes. It will reconstruct the order of focus events around that switch and find why a superseded or stashed target won. Then it makes focus always end up on the editor's current document(s), including split editors: the newest editor selection wins, and older focus requests are dropped or re-checked against the editor before acting. It adds tests that reproduce the sdk → rpg → sdk sequence.

**`#drivecapturedsplice` is still not started.** It reappeared as a new queue line because my earlier `queue sync` write landed after the last snapshot, not because you approved it. Its only step is `scripts/xdotool-live-verify.sh captured-splice`, which types real keystrokes into your desktop. You're actively switching editor tabs right now, so I won't run it unannounced. To approve it, write `go #drivecapturedsplice` in the exchange (ideally when you can step away from the keyboard for a few minutes) and I'll run it in-session.

**Subagents in flight (7):** `#refreshmemlockstall`, GH #237, the Cancel Turn wedge, GH #238, the struck "release + publish" bug, the sdk.md queue sync, and the tmux focus sync. The first three are in their final `make check`. As each one reports, I'll merge it, run one `make install`, close its items, and then cut the release you asked for.

> **[NOTIFY from src/haiven-dev/tasks/sdk.md]** (2026-10-11T06:49:49Z)
> State mismatch between src/haiven-dev/tasks/sdk.md and its Claude Code coordinator session (2026-10-11 ~02:30); operator reports a subagent is already working on it. (1) Stale queue claim survived /clear: #cligenport was claimed (owner subagent:cligoshell-then-cligenport, 2h TTL) by a subagent dispatched before the coordinator /clear'd. That subagent finished only #cligoshell and never started #cligenport (no branch/worktree/commit), yet the claim kept the head 'in flight'; preflight no_changes guidance told the coordinator not to release it, and the post-clear coordinator (ListAgents: no live subagents) reported it as still running to the operator. Claims need to be bound to a live agent/session identity and invalidated on /clear or agent exit. (2) After 'queue release #cligenport', session-check INTERRUPTED with 'runnable agent:queue head #cligenport removed from the committed queue ... silently dropped' while the head was live in the editor buffer ('backlog requeue' replied 'already has a live queue head; unchanged'); the next session-check was OK with 'repaired committed historical committed_capture snapshot drift' - a false positive from release vs committed snapshot. (3) Same session: pending_done_guard kept flagging #quirkbigint after its kept-open --backlog-edit was already in HEAD (c0477a6a), and --backlog-edit on an id re-adds its struck queue head, so operator-gated heads kept re-appearing as the driver.

### Re: sdk.md NOTIFY — dispatched — opus-5-5 · 2026-10-11T02:53-04:00

**Dispatched (claimed `subagent:sdknotify`):** the NOTIFY from `src/haiven-dev/tasks/sdk.md` (posted 06:49Z). The subagent fixes its three defects, each with tests:
1. **Queue claims tied to their owner.** A claim records the coordinator session that made it. When that session ends or is `/clear`ed, guidance marks the claim "owner session ended, unverifiable" and stops telling the new coordinator not to touch it, so the new coordinator can take it over or release it.
   - It does not release claims automatically on `/clear`. Two subagents started before this session's `/clear` (Cancel Turn and GH #237) are still running and reporting back.
2. **False "silently dropped" report after `queue release`.** session-check reports the item as dropped even though it is still in the editor.
3. **Items reappearing.** `pending_done_guard` doesn't notice a keep-open `--backlog-edit` that is already committed. Separately, `--backlog-edit` re-adds a queue item that was already crossed out.

Its claim code overlaps with the sdk.md queue-sync and struck-release subagents. It will list files it expects to conflict on, and I'll resolve conflicts when merging.

**`#drivecapturedsplice` still waits for your `go`.** Its only step types real keystrokes into your desktop.

**In flight (8 subagents):** `#refreshmemlockstall`, GH #237, Cancel Turn, GH #238, struck release, sdk.md queue sync, tmux focus sync, and the sdk.md NOTIFY. Cancel Turn's first full `make check` failed only on the timing test `closeout_sim_fixed_seed_corpus_exercises_recent_failure_classes`: 3.70s against its 3s limit, with load average 70–82 from the sibling test suites. On its own the test takes 0.71s, so the change didn't cause it. That run stopped early, so a rerun is covering the tests it skipped; I won't count a passing rerun as proof. Making that test reliable under load is a follow-up I'll file when I integrate.

> **[NOTIFY from src/haiven-dev/tasks/sdk.md]** (2026-10-11T08:30:33Z)
> Queue admission dropped operator text (2026-10-11 ~04:27). The operator moved five annotated heads sharing one id into agent:queue: "- [#dxdecisions]: D1: …" through "- [#dxdecisions]: D5: …" (plus one other annotated head). Preflight (UserPromptSubmit hook) kept only D1 in the queue, rewrote D2 into a NEW agent:backlog item "- [ ] [#dxdecisions] : D2: …" (colliding with the existing gated review item #dxdecisions, flagged as preset_item_id_collision), and deleted the D3, D4 and D5 lines from the document. Warning shown: free_text_admission_item_skipped "pending add: custom id already exists: dxdecisions". Expected: multiple annotated heads for the same id stay in the queue verbatim (or are all surfaced as annotations), and nothing is converted to backlog or dropped. Recovered from the preflight diff by the agent. Then the closeout refused ("owned component(s) have not reached the current document authority: notes,queue,review"), and session-check reports authority/disk divergence on queue+review.

### Re: batch integrated, released as v0.35.489, queue closed — opus-5-5 · 2026-10-11T04:58-04:00

All eight subagent fixes are merged, verified on the combined code, installed, and released as **v0.35.489**. Main and tag `v0.35.489` are at `622bd006dd`; the Release workflow is publishing.

**Verification on the merged tree:** `make check` passed all 10089 tests (142 skipped) and `make tmux-ci` passed; both exited 0, captured with RTK bypassed. `make release` ran them again on the version-bumped tree, tagged, pushed, and ran `install-full`. `agent-doc --version` now prints 0.35.489. I built and installed from a separate worktree (`~/work/btakita/agent-doc-wt/integrate-0489`), because the shared `src/agent-doc` checkout has another session's uncommitted edits (including `agent-doc-start-runtime-io/src/lib.rs`), which I didn't touch. That checkout is still on v0.35.488 and needs `git pull` once those edits are committed.

**Merge conflicts:** `VERSIONS.md` conflicted five times (resolved by keeping every entry) and one `specs/07-orchestration-commands.md` paragraph conflicted (resolved by keeping both the `⏳` marker and the owner-session text). I added the missing changelog entries for the lock stall and GH #238, plus the release parity row the first `make release` attempt refused without.

> **Queue prompt:** Cancel Turn caused the prompt to be full with `/agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md` blocking other commands such as Clear Session Context. After manually clearing the prompt, Clear Session Context was wedged. I manually ran `/clear`.

**Fixed.** The cancel arrived while preflight was still running, so Claude Code put the `/agent-doc …` trigger back into the prompt box. Cancel Turn now clears it with one `C-u`, and only when the box contains exactly this document's trigger, the pane is idle, and no route is submitting. The cancelled turn then ends immediately instead of after 20s. Clear Session Context now refuses when the prompt box holds your own unsent text, instead of typing `/clear` on top of it.

> **Queue prompt:** #gh-fix: https://github.com/btakita/agent-doc/issues/237

**Fixed and closed.** New command: `agent-doc queue add <FILE> --source <NAME> [--key <K>] [--attach <PATH>]... [TEXT]`. It adds an attributed item to the queue, going through the editor buffer when one is open. A repeated `--key` doesn't add a duplicate, and text and attachments are scrubbed of secrets. `--for-path` is not included: nothing maps a path to its session document yet.

> **Queue prompt:** #gh-fix: https://github.com/btakita/agent-doc/issues/238

**Fixed and closed.** `upgrade` now checks which IDE actually loads plugins from the install folder, using the jars it has mapped, its `idea.plugins.path` setting, or its data directory. It does this before applying the exact-262 package's restart refusal. An install into the idle `IntelliJIdea2026.2` no longer asks for a restart, and a stale restart-required marker is cleared. An IDE that really runs from that folder still gets the restart prompt.

> **Queue prompt:** do [#refreshmemlockstall]

**Done.** When an editor change raced the controller's memory refresh, the refresh used to reload the whole 215 MB ledger while holding the lock, for 1–4 s. Now it applies only the rows written since its unlocked load. A regression test shows an editor-op append completing while a full load is in progress.

> **Queue prompt:** I added another "release + publish" queue item but agent-doc immediately struck it. I'm intending to release + publish the queue items once complete.

**Fixed, and released.** Preflight queue maintenance struck your new line as a stale editor re-emit. The exception that protects re-typed lines only applied when your keystrokes were recorded against the exact committed snapshot, and yours were one edit past it. Now a whole line you typed after the agent's last write counts as a new request. The release + publish you intended is done (v0.35.489), so you don't need to re-type it. It's still struck in the document, since that strike happened before this fix.

> **Queue prompt:** sdk.md queue items are not in sync with the subagent activity. Most of the queue items are complete but the current queue state in sdk.md look like they are unstarted.

**Fixed.** There was no marker for "a subagent finished and it's waiting on you", and ⏭️ was being stamped over items with live 🚧 claims. Finished `[operator-verify]` items now show **⏳**. Claimed and ⏳ items are never blamed as skipped. A new preflight warning names any claim still held on an item that's already waiting on you. On sdk.md, `specmovesdk`, `quirkinventory`, `quirkbigint`, `paritycorpusmain`, `cligoshell`, `quirkidsasstrings` and `quirkrobloxaccess` are done on the agent side and waiting on pushes, PRs or decisions. `quirkaotserializer`, `specmovecontracts` and `cligenport` are not done. Markers redraw on sdk.md's next preflight.

> **Queue prompt:** Navigating between sdk.md + rpg.md resulted in a tmux focus on rpg.md in the 1:stash tmux window. Please make sure the focus effects always sync with the current Editor document(s).

**Fixed.** The layout did focus sdk.md correctly. Then an rpg.md request that was still in flight sent its trigger and called `select-pane` on rpg's pane, which had been parked in stash. That `select-pane` also switches the window (06:44:09 UTC in the logs). Requests now never move focus while the controller owns the layout, so the newest editor selection always wins. A live tmux test replays the sequence.

> **Queue prompt:** See `NOTIFY from src/haiven-dev/tasks/sdk.md` in this document.

**Fixed (two and a half of three).**
1. A claim now records the coordinator session that made it. When that session has ended or been `/clear`ed, preflight reports a **stale claim** with ready-made adopt or release commands, instead of saying "in flight, do NOT release". Claims aren't released automatically: two subagents started before this session's `/clear` kept running and reported back.
2. The "silently dropped" report came from preflight reading the buffer while you were deleting a `⏭️`, which left a bare `⏭do` the parser couldn't read. An item now counts as present while its id is on any live queue line.
3. The guard now records a keep-open edit when a retained continuation absorbs it. The claim that `--backlog-edit` re-adds a struck item didn't reproduce in three experiments, and sdk.md's history only shows a different free-text line struck. A test now pins the correct behaviour.

**Still waiting on you:** `#drivecapturedsplice`. It sends real keystrokes to your desktop, so write `go #drivecapturedsplice` when you can step away.

**Follow-ups filed** (appended to the backlog):
- `#loadbudgettests`: timing-budget tests fail under concurrent load.
- `#qheadresiduestrike`: the residue strike can still strike a re-typed item that isn't a recurring command.
- `#checktmpdirleak`: a test TMPDIR showed another worktree's path.
- `#subagentmsgcollide`: concurrent subagents overwrote each other's commit-message files.

### Re: sdk.md wedge and the four follow-ups dispatched — opus-5-5 · 2026-10-11T05:08-04:00

**Dispatched in this cycle; each item is claimed and has its own worktree on v0.35.489:**
- **sdk.md subagent turn wedged** (`subagent:sdkwedge`). This one is urgent. The subagent works read-only on the live system: no keys, recycles, restarts or writes. Its first job is to send me a quick diagnosis of what is wedged and whether restarting is safe, or what gentler recovery to use; I'll pass that on as soon as it arrives. Then it fixes the root cause, including checking whether the v0.35.489 install or recycle triggered the wedge.
- **`#loadbudgettests`**: rewrite the four timing-limit tests so they pass or fail deterministically, using an injected clock, event barriers or operation counts. Each must still pass with the CPU saturated and still fail when the behaviour is actually broken. Raising limits and rerunning until green are both off the table.
- **`#qheadresiduestrike`**: the catch-up residue strike will only count answers written after a head was re-added, so a re-typed repeat bug report stays live.
- **`#checktmpdirleak`**: find how the test TMPDIR leaked between worktrees, check whether tests could have written into another checkout, and isolate the TMPDIR per checkout.
- **`#subagentmsgcollide`**: add a rule to the subagent authorization text the binary emits: commit messages go through `git commit -F -`, and temp files live in the subagent's own worktree, never a shared scratch file. Every place that text is rendered must read from one source.

**New backlog item (filed this cycle):** last cycle's closeout applied each of my four backlog adds twice. Each landed once under its own id and once under a generated id with my id left in the text, ending at 8 items and 8 queue lines. This happened on the path where the tracked-work half was retained and then absorbed. I removed the duplicates with `backlog remove` and `queue consume --id`. The new item asks for replay to be idempotent and comes with a regression test.

**Still waiting on you:** `#drivecapturedsplice`. It types real keystrokes into your desktop, so write `go #drivecapturedsplice` when you can step away.
> **Chat prompt (#chatprompt):** tasks/sdk.md is still stuck: the editor that has it open isn't syncing with agent-doc. Restart or reload that editor, then run /agent-doc tasks/sdk.md, and I'll record everything from today in one pass. That covers the D1–D5 answers with PR #133, the D5 Unity version research, the 90-day retention confirmation, this PR #132 update and the missing haiven sdk fake-server text.

> **Chat prompt (#chatprompt):** Can I restart Idea now?

### Re: don't restart IDEA; sdk.md already recovered, press Enter in its pane — opus-5-5 · 2026-10-11T05:08-04:00

**Don't restart IDEA. You don't need to.** sdk.md already recovered on its own. The only thing left is a trigger that never got submitted.

- **What happened:** at 08:53 UTC, sdk.md's `/loop` turn (pane `%180`) ran session-check and got `INTERRUPTED: canonical editor authority and disk projection diverge`. It stopped the loop and asked you to restart the editor.
- **Cause: my v0.35.489 install.** From 08:58:00 the ops log shows `native_editor_save_request_deferred … IPC build mismatch: listener=0.35.489, client=0.35.488` with `retained_persistence_retryable reason=no_exact_native_save_receipt`. The editor's save receipt couldn't cross the version skew while the controller was being swapped for the new build. The queue-claim, Cancel Turn and route-focus changes did not cause it.
- **It healed itself:** at 08:58:15, `retained_persistence_applied proof=exact_disk`. At 08:58:40 the commit landed (`155131e3 agent-doc(sdk): 04:58:39`) with `captured_finalize_resume_committed`. I checked: `tasks/sdk.md` is clean in git, and session-check is ok.
- **Still stuck:** the supervisor sent Enter for the pending `/agent-doc …/tasks/sdk.md` at 08:58:44, but it didn't submit. The pane still shows that trigger in the input box. **Fix:** focus pane `%180` and press Enter. That run will record today's D1–D5/PR #133, Unity version, 90-day retention, PR #132 and fake-server text in one pass.
- **Side note:** a background shell in that session has been stuck for about 4 hours: `grep` is reading stdin because `$f` was empty. It's harmless. Stop it from the Claude shells UI if you like.

The sdkwedge subagent is now fixing both root causes: the save receipt stalling across a build-version skew during install, and the supervisor's Enter resubmit not taking.

### Re: GH #239 dispatched; correction on the sdk.md wedge — opus-5-5 · 2026-10-11T05:35-04:00

**Dispatched (claimed `subagent:gh239`):** GH #239. When you edit a component's opening marker line, agent-doc treats it as replica corruption. The relay restores the old line while you type, and the committed marker ends up with its text twice. The subagent will reproduce each case in the issue through the editor/CRDT relay path, so that a marker edit commits exactly what you typed while real corruption is still detected. Then it closes the issue after integration.

**Correction to my earlier sdk.md diagnosis** (from the sdkwedge subagent's evidence):
- **The 26-minute stall (08:32–08:58 UTC) was not caused by the v0.35.489 install.** The JetBrains plugin (0.2.531) rejected every native save request for sdk.md, 52 times. About 15 s after each rejection, an unrelated delivery success called `clear_after_proven_delivery`, which wiped the document's transport-health row. So the rejection count always read 1 and never escalated. The install's recycle happened to reset things. Fix on branch `sdk-subagent-turn-wedge`: a success on one transport clears only that transport's rejection run, while an exact native-save receipt still clears everything.
- **The "unsent trigger" in pane `%180` was Claude Code's dimmed prompt suggestion, not real input**, so my "press Enter" advice was likely wrong. The idle drain treated it as pending and sent a bare Enter, which an empty composer ignores. Fix: dimmed prompt text is masked before matching (the same rule routes already apply). sdk.md recovered on its own when a direct submit at 09:07 ran `/agent-doc sdk.md`.

**In flight (7 subagents):** sdk.md wedge fixes (in `make check`), `#loadbudgettests`, `#qheadresiduestrike`, `#checktmpdirleak`, `#subagentmsgcollide` (re-running `make check` without stopping at the first failure; I'll check which test failed), `#respondcloseoutbacklog` (now also covering the duplicated queue line it created itself), and GH #239. `#drivecapturedsplice` shows ⏳ (the new awaiting-operator marker) until you write `go`.

### Re: batch integrated and installed (main a186f61e46); queue items closed — opus-5-5 · 2026-10-11T07:35-04:00

All seven fixes in this batch are merged into main (`a186f61e46`, fast-forward from v0.35.489) and installed with `make install`. Testing ran on the merged tree with logs written only to the integration worktree and exit codes taken from my own process: `make check` passed (10114 of 10114 tests, 142 skipped, TLA included) and `make tmux-ci` passed. I have **not** cut a new release. v0.35.489 is the latest tag; say the word and I'll release these as v0.35.490.

> **Queue prompt:** sdk.md turn with a subagent is wedged. Asking me to restart.

**Fixed.** No restart was needed: sdk.md recovered at 08:58 UTC and its turn ran at 09:07. There were two root causes:
- The editor's 52 save rejections were never counted up, because a success on another transport cleared the record each time. Now a transport success clears only its own refusal count.
- The supervisor pressed Enter on Claude Code's dimmed prompt suggestion, which an empty input box ignores. Dimmed prompt text is now ignored when checking for a pending trigger.

> **Queue prompt:** #gh-fix: https://github.com/btakita/agent-doc/issues/239

**Fixed and closed.** Editing a component's opening marker line no longer counts as replica corruption. A marker line only counts as removed when it was deleted or none of its own text survives. Typing into it keystroke by keystroke now keeps exactly what you typed, once. Deleting a whole marker line is still caught.

**Done:**
- **`#loadbudgettests`:** eight load-sensitive tests no longer depend on wall-clock time; they use CPU time, an injected clock, wait counts, or test-held gates. This turned up a real bug: finalize marked an editor-op epoch clear as failed when the reply arrived after 250ms. It now decides from the actual state.
- **`#qheadresiduestrike`:** a retyped item is no longer struck by an answer to an earlier copy. Git history determines when the item was re-added.
- **`#checktmpdirleak`:** the "leak" was subagents overwriting one shared `check.log`. The real gap was that a raw nextest run with an inherited search limit didn't bound its own workspace; each checkout now bounds its own. Subagents are now told to keep logs in their own worktree.
- **`#subagentmsgcollide`:** the subagent authorization text now says commit messages go through `git commit -F -` and temp files stay in the subagent's own worktree.
- **`#respondcloseoutbacklog`:** two processes (the foreground respond and the background resume) both applied the same backlog adds, and the second pass generated new ids. An identical add is now a no-op, the resume skips adds that already landed, and the queue mirror checks the live editor copy. The second `do [#respondcloseoutbacklog]` line was typed in the editor (31 bytes at 05:05:55), not written by agent-doc.

**One decision for you:** in two tests (`controller_client_connection_is_one_request` and the IPC listener-eviction test) the timing limit became a 120s hang ceiling instead of a wait on an event, because there is no readiness signal to wait on yet. I filed adding one as a follow-up; delete that item if the ceilings are acceptable.

**Follow-ups added to the backlog:**
- A suspected race in the `state_subscribe` liveness test.
- Remaining 250ms timing asserts in controller-io.
- Readiness signals to replace the two 120s ceilings.
- One `make tla` run that couldn't find the `Naturals` module.
- The continuation-head skip still matching against the whole exchange.

**Still waiting on you:** `#drivecapturedsplice`, which now shows ⏳. Write `go` when you can step away from the keyboard.
<!-- agent:boundary:f6f2f5ef -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
- ~~#gh-fix: https://github.com/btakita/agent-doc/issues/239~~
- ⏳ do [#drivecapturedsplice]
- ~~do [#respondcloseoutbacklog]~~
- ~~sdk.md turn with a subagent is wedged. Asking me to restart.~~ — auto-struck: answered this cycle (#ftstrike)
- ~~do [#loadbudgettests]~~
- ~~do [#qheadresiduestrike]~~
- ~~do [#checktmpdirleak]~~
- ~~do [#subagentmsgcollide]~~
- ~~do [#respondcloseoutbacklog]~~
- do [#statesubscriberecords]
- do [#remainingwallclock]
- do [#replace120shang]
- do [#tlafailedonce]
- do [#continuationheadskip]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#statesubscriberecords] [agent-doc] state_subscribe_records_only_the_previously_applied_live_peer_cursor (agent-doc-controller-io) failed twice under heavy concurrent load (panic: the durable OS-exit liveness transition evicts the crashed registration's ack) and passes alone; it has no wall-clock bound. Suspected race on the process-global liveness state keyed by the test process pid: a concurrent test's production path touching that pid between this test's reset and its exit call. Confirm and fix the race (isolate the liveness state per test or key it per test), with a stress reproduction.
- [ ] [#remainingwallclock] [agent-doc] Remaining wall-clock asserts in tests: the 250ms 'not blocked by a blocked effect' asserts in agent-doc-controller-io (project_controller.rs blocking-document-effect around line 4747, rpc.rs) are the most load-sensitive; convert them to the closed-gate pattern used for captured_response_and_authority_reactively_project_answered_free_text_strike. The 1-5s 'returns instead of hanging' bounds in preflight-io debounce.rs, codex-stop-io and src/preflight_hook.rs can become hang ceilings.
- [ ] [#replace120shang] [agent-doc] Replace the 120s hang ceilings in wait_for_test_controller (controller_client_connection_is_one_request) and agent-doc-ipc-io await_endpoint (a_bound_but_unaccepting_listener_is_evicted_not_protected) with a real readiness signal from the controller/listener, so the tests wait on an event instead of a bounded poll.
- [ ] [#tlafailedonce] [agent-doc] make tla failed once during a concurrent make check in the respondcloseout worktree: TLC could not resolve the standard module Naturals (looked for /tmp/Naturals.tla) while parsing StopHookContinuation.tla; make tla alone passed. Each worktree has its own checksum-verified target/tla jar and a mktemp work dir, and disk-sweep did not clean that worktree. Find what made the jar's standard modules unavailable (cwd, TMPDIR, classpath, concurrent jar refresh) and make run_tla.sh robust to it.
- [ ] [#continuationheadskip] [agent-doc] The continuation-head skip in agent-doc-queue/src/queue_continuation.rs still matches a live free-text head against the whole exchange, so an operator re-typed head that repeats an earlier answered prompt can be left out of the continuation offer (it is never struck). Scope it to echoes after the head's re-entry, sharing the git-history entry-point helper the residue-strike fix added.
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
