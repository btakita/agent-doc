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

### Re: closing the finished GH #240, sdk.md and api.md queue items — opus-5.5 · 2026-10-11T18:31-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/240

`#gh-fix` ("fix then close") is done on both counts:
- **Fixed:** the approved-head fencing in the consume (`f82ea45372`), on main via `3fbe838baa`, installed.
- **Closed:** GH #240 closed when the fix landed on main. I posted the root cause, fix and tests as a [comment](https://github.com/btakita/agent-doc/issues/240#issuecomment-6114223605).

The remaining overlap of two closeout drivers is tracked as `#closeoutleaseresume`, which is in progress with a subagent.

> **Queue prompt:** Clear Session Context on sdk.md did not take effect despite me running it twice. Run Agent Doc was sandwiched in between the 2 Clear Session Context calls.

Fixed in `clearlive` (`37bc4add5a`), on main as `472ec36906`, and installed. The details are in the 18:25 response.

> **Queue prompt:** Clear Session Context on api.md is wedged.

Fixed in the same `clearlive` change, and installed. The background-agent row no longer hides the idle prompt.

### Re: why a corrupted pane looks like tabs were inserted — opus-5.5 · 2026-10-11T18:34-04:00

> **Queue prompt:** I want to also diagnose why the tmux pane goes into a format where tabs are inserted. Once it gets corrupted.

**Deferred:** this went to the same subagent as the frontend.md wedge (claim `subagent:frontendwedge`), because it has the same root cause. My working diagnosis, which the subagent is to confirm with a reproduction:
- **What you're seeing:** a staircase, not real tabs. In img_4.png, each stray `[perf]` line starts one row down, at the column where the previous line ended.
- **Why lines staircase:** Claude Code puts its terminal in raw mode. In raw mode a bare `\n` moves the cursor down a row but does not return it to column 0; normally the terminal adds that return, and raw mode turns it off. An `eprintln!` written straight to the pane emits exactly a bare `\n`, so each line begins where the last one stopped.
- **Why it keeps looking broken:** the foreign bytes also throw off where the TUI thinks its cursor is. Its own partial repaints then land offset, which makes the whole layout look tab-shifted until a full redraw.

**Recovery once it's corrupted:** force a full redraw. Resize the pane, or press Ctrl+L, or run `tmux refresh-client`. The subagent will confirm which of these works for Claude Code.

**The fix:** remove the source (the unconditional `[perf]` print) and close the stderr channel to the pane. The subagent is also checking for other bare-newline writers on paths that inherit the pane, and adding a regression test that fails on today's leak by detecting the staircase on an isolated tmux server. It will also report whether agent-doc should trigger a redraw itself after it detects foreign output.

### Re: #clearnoticeverify — testing it myself — opus-5.5 · 2026-10-11T18:51-04:00

> **Queue prompt:** do [#clearnoticeverify]: Test yourself

> **Operator note:** Test yourself

**Deferred:** this went to a background subagent (claim `subagent:clearnotice`). It will drive your running IntelliJ and real tmux server, but only against things it creates: a git-ignored scratch session doc (`agent_doc_scratch: true`) and that doc's own pane. It won't touch your existing panes or documents, and it won't run `make install`.
1. **Plugin check:** first it confirms the running IDE has the queued-clear plugin build (0.2.537 / 0.2.538). If it doesn't, it reports that rather than restarting the IDE.
2. **Notice checks:** it presses Clear during a running turn, expecting the "queued … runs automatically when that turn ends" notice and exactly one `/clear` at idle. Then it presses twice, expecting the re-armed wording with `evidence_age_secs`.
3. **Background agents:** in a real Claude Code pane with a background agent running (`← N agents`) at an idle prompt, it presses Clear and records what happens to that agent: killed, orphaned, or still running into the cleared context.

It will quote each notice verbatim with screenshots, fix any defect it finds on its own branch with tests, and clean up its scratch pane and tab afterwards.

### Re: is #clearnoticeverify verified; 2/3|1/3 layout during the test — opus-5.5 · 2026-10-11T19:12-04:00

> **Queue prompt:** [#clearnoticeverify]: verified?

> **Operator note:** verified?

**Deferred:** not yet verified. The subagent is still in its live test and hasn't reported. I've asked it to report the notice evidence as soon as it has it: the verbatim wording, screenshots, and what happens to background agents. I'll close this item with that evidence.

> **Queue prompt:** The tmux panes are not laid out correctly. 2/3|1/3. There were 3 tmux panes during your [#clearnoticeverify] test.

**Deferred:** this went to the same test subagent (claim `subagent:clearnotice`), since its scratch doc and pane triggered it. Right now window `0:0 agent-doc` holds a single pane, `%186`, likely the scratch doc's pane, and your other doc panes are in `0:1 stash`. The subagent will do three things:
1. Capture the tmux layout, the IDE's editor split, and the sync/layout ops.log lines from the test window.
2. Work out why three panes landed at 2/3 | 1/3 instead of matching the editor split, and fix that on its branch if it's an agent-doc defect.
3. When the test is done, close only its own scratch tab and pane so your layout returns to normal.

If the layout is still off once that tab closes, Sync Tmux Layout in the IDE (or `agent-doc sync`) should restore it.

### Re: v0.35.491 released — pane bleed, queued clears, lease, layout — opus-5.5 · 2026-10-11T20:52-04:00

> **Queue prompt:** frondend.md end of turn wedges session...with the formatting messed up ![img_4.png](img_4.png)

Fixed in `frontendwedge` (`c7515155fb`) and released in v0.35.491.
- **The source:** the `frontend.md` supervisor had been started by hand, so it never redirected its stderr. The redirect only ran for supervisors agent-doc's routing had started. Its 17-second steering check printed `[perf] find_code_ranges` into the pane each time.
- **Why it wedged:** the text left in the empty input box read as a draft you hadn't sent, so the queue refused to dispatch anything. It wasn't a separate hang.
- **The fix:**
  - Every TUI supervisor now sends stderr to its log, and does so again first thing when a `make install` makes it re-launch itself.
  - The `[perf]` line is gone.
  - Tests on a separate tmux server fail if any byte reaches the pane.
- **This pane:** the `frontend.md` supervisor fixes its own stderr at its next automatic reload. It may print its reload line into the pane one last time. After that, zoom the pane in and out once (prefix + `z` twice) to clear the text left on screen.

> **Queue prompt:** I want to also diagnose why the tmux pane goes into a format where tabs are inserted. Once it gets corrupted. ![img_5.png](img_5.png)

Diagnosed and fixed together with the item above. They aren't tabs but a staircase. Claude Code runs its terminal in raw mode, so a plain line break moves the cursor down a row without returning to column 0, and each stray line starts where the last one ended. The stray bytes also throw off Claude Code's idea of where its cursor is, so its own redraws land shifted. img_5 showed the supervisor's own hot-reload messages leaking alongside the `[perf]` lines. To recover a garbled pane, zoom it in and out or resize it. `tmux refresh-client` doesn't help, because tmux's copy of the screen still holds the stray text.

> **Queue prompt:** The tmux panes are not laid out correctly. 2/3|1/3. There were 3 tmux panes during your [#clearnoticeverify] test.

Fixed in `clearnotice` (`b38af9cd70`) and released. The test's scratch doc opened in a separate IntelliJ window. The plugin counted editors from every IDE window, so it mirrored a phantom third column. It now mirrors only the window you're looking at. The scratch pane is closed, and `0:0` matches your editor again.

> **Queue prompt:** do [#closeoutleaseresume]

Done (`leaseresume`, `6cbf7f51f7`, released). The captured-finalize resume now takes the closeout lease before its legacy repair path changes anything. If another closeout holds the lease, it retries when that lease is released. The test that reproduced the GH #240 overlap fails without the fix.

> **Queue prompt:** do [#clearsupersedenotice]

Done (`supnotice`, `698f36c11d`, released). When a newer prompt drops a queued Clear, IntelliJ and VS Code show a "Queued clear not sent" notice with a Clear Session Context button, once per dropped clear. The notice is pushed to the editor; nothing polls for it.

> **Queue prompt:** do [#flakyintegrate0494]

**Deferred:** the fix is done on branch `flaky0494` (`9dda7abd8d`, `make check` exit 0), but it isn't in v0.35.491; I'll merge it next batch.
- **The lost failure can't be recovered:** the disk sweep had already cleaned that worktree's build output.
- **Two causes of this kind of failure fixed:**
  - `build.rs` watched the workspace root, including `target/`, so every cargo command relinked the test programs. A second cargo command could delete them under a running test run: 65 failures before the fix, none after.
  - Four `already_applied_*` tests waited on a controller started from scratch and ignored a missing receipt. They failed 5 of 5 under heavy load before the fix and passed 5 of 5 after.
- **Results are now kept:** every `make check` and `make test` keeps its output and test report in `tmp/test-runs/`.
- **Still open:** one slow test hitting the 3-minute limit under very heavy load, and three more test helpers that ignore receipt errors, filed as flakyreceipthelpers.

> **Queue prompt:** release + publish

Released v0.35.491.
- **Verification:** the release gate's `make check` passed (10225/10225) and `make tmux-ci` passed. The tag `v0.35.491` is pushed (`06fda54e10`), and main contains it.
- **Local install:** `install-full` is complete; agent-doc is 0.35.491, the JetBrains plugin 0.2.541 (262: 0.2.542), and the VS Code extension 0.2.81.
- **CI:** the GitHub Release run is in progress; I checked it once and didn't wait. PyPI follows from it.
- **Two release slip-ups on my side, both caught before tagging:** the release needed an editor parity row, which I added. And I had written the release log inside the worktree, which changed its fingerprint and made the gate refuse; I moved it out.
- **Also in this release:** the queued-clear work, GH #240, the scratch-doc closeout, and the verifier's CRDT-forward shape.

**From the #clearnoticeverify live test, two things for you, filed:**
- **idleclearrestart:** Clear at an idle prompt doesn't type `/clear`. It restarts Claude, which killed a running background agent at 12 of 40 ticks, with no report. Do you want idle Clear to type `/clear` like the queued path?
- **idlecleartriggerinject:** after each idle Clear, the supervisor sent an unrequested `/agent-doc <file>` turn (~13s each). Filed as a bug.

I also filed annotateddeferstrike: a deferred queue line carrying your note was struck twice today, because the required `> **Operator note:**` quote sits between the echo and `**Deferred:**`.

### Re: dispatched the 0.35.491 follow-ups; merging flaky0494 — opus-5.5 · 2026-10-11T21:00-04:00

> **Queue prompt:** do [#idlecleartriggerinject]

**Deferred:** this went to a background subagent (claim `subagent:clrinject`, branch `clrinject`, based on v0.35.491). After an idle Clear's fresh restart, nothing should be dispatched unless real work is pending (queue continuation, an unanswered prompt, or a retained capture). It will add regression tests that run only on an isolated tmux server.

> **Queue prompt:** do [#annotateddeferstrike]

**Deferred:** this went to a background subagent (claim `subagent:deferstrike2`, branch `deferstrike2`). It will make the closeout recognize the deferral in a deferred annotated head's answer, which runs echo, then the operator-note quote, then the deferral paragraph. That covers `do [#id]: note`, `[#id]: note`, and free-text heads. The annotation must still be answered. Today's two reproductions become regression tests, and the runbook will spell out the answer shape.

> **Queue prompt:** do [#flakyreceipthelpers]

**Deferred:** this went to a background subagent (claim `subagent:rcpthelpers`, branch `rcpthelpers`, built on `flaky0494`). It will reproduce and fix the three simulated-editor listeners that ignore receipt errors. It will also profile the slow `finalize_integration` test; the time limit won't be raised.

> **Queue prompt:** do [#flakyintegrate0494]

**Deferred:** I'm integrating `flaky0494` myself (claim `coordinator:integrate-flaky`, worktree `integrate-0497` off main). The authoritative `make check` is running, with its log and exit code outside the repo. If it passes, I'll push it to main, run `make install`, and close this item.

`#idleclearrestart` is still yours to decide: should Clear at an idle prompt type `/clear` instead of restarting Claude? The restart kills its background agents. No subagent can make that call for you.

### Re: #flakyintegrate0494 integrated — opus-5.5 · 2026-10-11T21:25-04:00

> **Queue prompt:** do [#flakyintegrate0494]

Done. `flaky0494` is merged to main as `8e7ef63ce6` and installed.
- **First run failed:** the first authoritative `make check` on the merge failed (exit 2) on `agent-doc-preflight-command-io progress::tests::crossing_the_deadline_at_a_phase_boundary_refuses_by_name`. The new per-run test record kept the output: the test gave itself a 40ms budget, which the safety margin turns into a deadline of about 0ms, and expected its first phase to start inside it. With three subagents running test suites at once, the first phase started 88ms in. It was a wall-clock race in the test, not a product bug.
- **Fixed, not rerun:** the test now starts with a one-hour budget, checks that the first phase is inside it, then moves the installed run's deadline to "now" to cross the boundary. No sleep, no timing dependency.
- **Second run passed:** `make check` exit 0, 10226/10226.
- **What `flaky0494` itself contributes:**
  - `build.rs` no longer watches the workspace root, so cargo stops relinking every test program and deleting them under a running test run.
  - The `already_applied_*` tests use an already-running controller and record receipts strictly.
  - Every `make check` and `make test` keeps its output and test report in `tmp/test-runs/`, which is how this failure was caught and named.

The original lost failure from integrate-0494 stays unidentified, since its output was gone before this existed. The remaining receipt-dropping helpers and the slow test are with `#flakyreceipthelpers`.
<!-- agent:boundary:75cf33a3 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 do [#idlecleartriggerinject]
- 🚧 do [#annotateddeferstrike]
- 🚧 do [#flakyreceipthelpers]
- ⏳ do [#idleclearrestart]
- ~~[#clearnoticeverify]: verified?~~
- ~~The tmux panes are not laid out correctly. 2/3|1/3. There were 3 tmux panes during your [#clearnoticeverify] test.~~
- ~~frondend.md end of turn wedges session...with the formatting messed up ![img_4.png](img_4.png)~~
- ~~release + publish~~
- ~~do [#closeoutleaseresume]~~
- ~~do [#flakyintegrate0494]~~
- ~~do [#clearsupersedenotice]~~
- ~~I want to also diagnose why the tmux pane goes into a format where tabs are inserted. Once it gets corrupted. ![img_5.png](img_5.png)~~
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] 🚧 [#idlecleartriggerinject] [agent-doc] After Clear Session Context at an idle prompt (delivery=supervisor_restart_fresh), the supervisor injects an unrequested /agent-doc <file> turn (auto_trigger_inject), a no-op costing ~13s of model time each time; seen twice in the 2026-10-11 clearnotice live test (evidence: agent-doc-wt/clearnotice/tmp/evidence). Do not auto-dispatch after an operator Clear unless queue work is actually pending.
- [ ] 🚧 [#annotateddeferstrike] [agent-doc] A deferred annotated queue head is struck anyway: for an annotated head the closeout requires the operator-note answer quote, which then sits between the queue-prompt echo and the deferral paragraph, so the deferral is not recognized, the head is consumed and its claim released (2026-10-11, twice on the clearnoticeverify heads). Recognize a deferral paragraph that follows the operator-note answer, with a regression test.
- [ ] 🚧 [#flakyreceipthelpers] [agent-doc] Follow-up from flaky0494: simulated-editor socket listeners in agent-doc-test-support/src/lib.rs:781, agent-doc-write-converge-io/src/convergence_fixture_tests.rs:170 and agent-doc-commit-io/tests/git.rs:104 drop receipt errors the way the fixed already_applied_* tests did (may flake under load); and finalize_integration reopened_pending_only_bare_tag_add_applied_twice_lands_exactly_once hits the 180s nextest limit under load ~80 (performance, do not raise the limit). Integrate branch flaky0494 first.
- [ ] [#idleclearrestart] [operator-verify] [agent-doc] Decide: Clear Session Context at an idle prompt restarts the Claude process (supervisor_restart_fresh) instead of typing /clear, killing running Claude background agents/shells with no report (2026-10-11 live test: agent stopped at 12/40 ticks). Queued clears type /clear. Should idle Clear type /clear too?
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
