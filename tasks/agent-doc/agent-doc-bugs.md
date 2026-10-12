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

### Re: is v0.35.491 published — opus-5.5 · 2026-10-11T21:45-04:00

> **Chat prompt (#chatprompt):** Have we published the release?

Yes, v0.35.491 is fully published:
- **GitHub Release:** run 38188587357 succeeded. The release was published at 01:00:43Z (not a draft), with 13 assets.
- **PyPI:** the PyPI workflow succeeded, and 0.35.491 is the latest version on PyPI.
- **Local install:** agent-doc 0.35.491 is installed, plus the later `flaky0494` merge (`8e7ef63ce6`).

Not in the release yet: `deferstrike2` (the annotated-deferral fix) and `clrinject` (no auto-trigger after an idle Clear) are both finished, with `make check` green. `#flakyreceipthelpers` is still running. All three go into the next batch and release.

### Re: agent-doc thrashing in the background — opus-5.5 · 2026-10-11T22:05-04:00

> **Queue prompt:** do [#fixthrashingbackground]

**Deferred:** this went to a background subagent (claim `subagent:thrash`, branch `thrash` off the installed `8e7ef63ce6`). What I measured at 22:01:
- **Load and controllers:** the 1-minute load average is 32. The haiven-dev project controller sits at about 48% CPU with 1.0 GB of memory, and the agent-loop controller at about 14% with 1.5 GB.
- **Supervisors:** about 30 route-owned supervisors each idle at 3–15% CPU; `tasks/betterstack.md`'s is at 14.6%.
- **What drives it:** in the last 20k lines of haiven-dev's ops.log, 9577 are `controller_crdt_current_text`. That's a full-document read through the controller, and 6897 of them are `source=session-check:midturn_steering`. Every supervisor re-checks its document for mid-turn steering every ~17s even when idle and nothing changed. The rest come from the idle-queue head probe (1211) and `durable_buffer_state` (1074), spread across every open doc.

The subagent will make that work driven by events and document revisions instead of a fixed timer:
- Steering checks run only while a turn is live and the document actually changed.
- Queue-head checks run only on queue or document change.
- No full-text read serves as the per-tick probe.

It will also check the controllers' 1–1.5 GB for unbounded growth, measure CPU before and after on an isolated harness, and add a regression test that fails if an idle supervisor starts polling full text again. It won't touch any live process.

### Re: idle-Clear auto-trigger and annotated deferral fixes integrated — opus-5.5 · 2026-10-11T23:55-04:00

> **Queue prompt:** do [#idlecleartriggerinject]

Done. `clrinject` (`753924c04b`) is merged to main as `2af67f4ebb` and installed. Its full `make check` passed (exit 0, 10236/10236).
- **Cause:** an idle Clear restarts the agent through the same "fresh restart" request as any other restart, and the restart loop always set `auto_trigger_next_launch = true`. That produced the stray `/agent-doc <file>` turn ~10s after each `session_clear_sent delivery=supervisor_restart_fresh`.
- **Fix:** a Clear restart now records its origin (`clear:fresh`). It re-triggers only if a cycle is still open or an unanswered prompt is pending. If the pending-work check itself fails, it re-triggers anyway, since a wasted turn beats a stranded prompt. Each decision is logged as `operator_clear_restart_trigger`.
- **Unchanged:** queue heads stay with the idle-queue watch, so there's no second dispatcher. Restart Agent, crash recovery and recycle resume still re-trigger.
- **Tests:** a source-check test fails if the unconditional `true` comes back.

> **Queue prompt:** do [#annotateddeferstrike]

Done. `deferstrike2` (`6a5d171d43`) is in the same batch, installed.
- **Cause, free-text heads:** the deferral check looked only one block past the echo, so it stopped at the operator-note quote.
- **Cause, `do [#id]` heads:** the deferral check never ran at all; the `### Re: #id` heading counted as completion.
- **Fix:** a deferral paragraph after the operator-note answer now keeps the head queued and its claim held. The note must still be answered, and an explicit `--done`, gate or review-resolve still closes it.
- **Tests and docs:** today's two reproductions are regression tests, and disabling either half of the fix makes them fail. The runbook and every installed harness copy document the shape: echo, then operator-note quote, then `**Deferred:**`.

**Next batch:** `rcpthelpers` (`#flakyreceipthelpers`) and `thrash` (`#fixthrashingbackground`) are merged in `integrate-0499`, and its `make check` is running.

### Re: background thrashing and receipt-flake fixes integrated — opus-5.5 · 2026-10-12T00:10-04:00

> **Queue prompt:** do [#fixthrashingbackground]

Done. `thrash` (`c7614811f4`) is merged to main as `e31a862648` and installed. The batch's `make check` passed (exit 0, 10242/10242).
- **Main cause:** the orphaned-empty-preflight reclaim check (`#gh179`) ran every 30 ticks (about every 17s) and was always treated as urgent. So every idle supervisor did a full pass each time: queue head, steering wake, frontmatter, all full-document reads. All supervisors ran in step, because they had been recycled together. It now runs only when something can be reclaimed: an open cycle, a request in flight, or unreadable cycle state.
- **Three more fixes:**
  - The turn id for log lines is read only just before a dispatch, instead of a controller round trip every 500ms.
  - The steering wake remembers when it first saw a half-typed queue item, so the item settles once instead of being re-checked forever. With an editor attached, such an item would also never have woken the session; that bug is fixed too.
  - Cycle-state reads use a new `document_closeout_projection` command. For `fpe.md` that returns the needed 34 KB instead of 5.1 MB of history copied under the controller lock, and older controllers fall back to the full read.
- **Smaller fixes:** queue-head checks on documents with no live editor read disk directly, and the orphan-drain sweep skips its `/proc` scan unless a dispatch is possible.
- **Measured** (isolated tmux, 4 idle supervisors, 2 minutes): per-supervisor CPU fell from 960 to 415 ms/min, controller CPU from 2030 to 890 ms/min, and ops.log lines from 84 to 0.
- **Memory:** the controllers' 1–1.5 GB is document history kept by design, not a leak.
- **What's left:** mostly the supervisor's terminal-screen rendering, not document reads.

> **Queue prompt:** do [#flakyreceipthelpers]

Done. `rcpthelpers` (`0c7e665d41`) is in the same batch, installed.
- **Listeners:** the simulated-editor listeners share one strict fixture, `SimulatedEditorListener`, using an already-running controller. Under one-core load with 64 busy loops they went from failing in 3 of 3 runs to passing 5 of 5.
- **Finalize test:** it dropped from 64s to 6.5s on an idle machine. The controller now wakes as soon as a client connects instead of sleeping up to 50ms per request; this is a production change. And the cleanup sweep opens other projects' databases read-only; tests had been writing to your real databases.
- **Corroboration:** both receipt tests that timed out on the `thrash` branch passed in this batch. The full suite also ran in 136s, down from about 290s.
- **Left:** under very heavy load (32 busy loops on one core) the finalize test is genuinely CPU-bound, about 6.4s of CPU, and can still pass its 180s limit. I didn't raise the limit.

I removed the thrash worktree, which reclaimed 2.1 GB of scratch files.

`#idleclearrestart` is still yours: should Clear at an idle prompt type `/clear` instead of restarting Claude, which kills its background agents?

### Re: is the release published — opus-5.5 · 2026-10-12T00:15-04:00

> **Chat prompt (#chatprompt):** Is the relese published?

v0.35.491 is published: the GitHub Release went out at 01:00:43Z, and PyPI's latest is 0.35.491.

The fixes integrated since then are **not** released yet. Five batches have landed on main since the `v0.35.491` tag, and are installed locally only:
- `flaky0494`: the build no longer relinks every test program on each cargo command, and every test run is now recorded in `tmp/test-runs/`.
- `deferstrike2`: a deferred queue line carrying your note stays queued.
- `clrinject`: an idle Clear no longer sends an unrequested `/agent-doc` turn.
- `rcpthelpers`: strict receipt fixtures for the simulated-editor tests, plus the controller's faster accept-loop wake.
- `thrash`: idle supervisors and controllers stop burning CPU.

Each batch passed its own full `make check`. They would ship as v0.35.492: version bump, `VERSIONS.md` entry, parity row, `make release`.

### Re: #idleclearrestart — idle Clear will type /clear — opus-5.5 · 2026-10-12T00:20-04:00

> **Queue prompt:** do [#idleclearrestart]: Yes, Idle clear should type `/clear`.

> **Operator note:** Yes, Idle clear should type `/clear`.

**Deferred:** this went to a background subagent (claim `subagent:idleclear`, branch `idleclear` off `e31a862648`), with your decision. An idle Clear on a responsive pane will type the harness's own `/clear` into the live agent, through the same guarded pane-input path the queued clear already uses: the ready-prompt check, the protected-input guard and the settle receipt. It will no longer restart the Claude process, so background agents and shells survive.
- **Restarts that stay:** only those with a real reason, such as a harness with no native clear command, an unresponsive agent, or an explicit Restart Agent. Those are documented.
- **Unchanged:** one clear per press, the queued and re-armed behavior, `interrupt-clear`, and no auto-trigger afterwards unless work is pending.
- **Live test:** on an isolated tmux server with a fake harness, it will check that an idle Clear types `/clear` into the same child process (same PID) and that a background job keeps running.

### Re: build/test turnaround; sdk.md manual-Clear not operated — opus-5.5 · 2026-10-12T05:10-04:00

> **Queue prompt:** How can we optimize the agent-doc build process + test process to reduce the turnaround time?

**Deferred:** this went to a background subagent (claim `subagent:turnaround`, branch `turnaround`). It will answer with measurements, not guesses:
- **What it times:** each `make check` stage, cold vs warm builds, per-crate and per-binary test time from the nextest reports, link vs compile time, the linker, debug-info and codegen settings, and how much of the check is non-Rust (Gradle, Lean, TLA, sim) and whether those stages run one after another.
- **What it lands:** the safe wins with before/after timings, for example running independent stages in parallel, skipping unchanged non-Rust stages by fingerprint, and a faster dev linker. It will not weaken any check.

For reference, today's `make check` dropped from about 290s to 136s for the test portion after the controller accept-loop fix, and `install-full` takes about 556s.

> **Queue prompt:** sdk.md issue: Run Agent Doc, then `/clear` in the Claude Code terminal: Clear command is not operated.

**Deferred:** the haiven-dev ops.log for pane %180 confirms it:
1. **09:07:19 and 09:07:26:** your Clear presses were correctly deferred behind the live turn (`clear_epoch=92`).
2. **09:07:35:** Run Agent Doc dispatched a new turn.
3. **09:07:46:** as that turn started, the supervisor logged `idle_queue_watch_deferred_clear_satisfied ... settled=true (#sdkclearwedge)` and retired the clear. No `/clear` was ever sent. The start of the new turn was wrongly counted as proof the context had been cleared.

Since that's the same clear-delivery code the `#idleclearrestart` change is rewriting, I gave it to that subagent (claim `subagent:idleclear`) rather than starting a conflicting branch. The fix: "satisfied" must require real proof (a delivered `/clear` receipt or a fresh harness session). A run started after the press either keeps the clear queued or drops it with the "not sent" notice, never silently. It will be covered by a regression test for this exact sequence.

**Now:** the clear didn't happen. Press Clear Session Context again once this sdk.md turn ends, or type `/clear` yourself at the idle prompt.

### Re: GH #241 and #ideriderindependentnav dispatched — opus-5.5 · 2026-10-12T05:25-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/241

**Deferred:** this went to a background subagent (claim `subagent:gh241`, branch `gh241`). The issue: in Remote Dev, when one of two editors for an open document is removed, the IDE fires `fileClosed`. The plugin's close handler then deregistered the document's only CRDT replica while the file stayed open, and nothing re-registered it for 4h48m, so a committed response never reached the editor buffer.

The fix will do two things:
- Deregister only when no editor for that file remains open in any client.
- Have an "editor open, no live replica" state trigger re-registration, so a missed deregister heals itself.

It will add Kotlin and SimWorld regression tests and bump the plugin versions. Following `#gh-fix` ("fix then close"), I'll close the issue with its drafted comment once the fix is integrated on main.

> **Queue prompt:** do [#ideriderindependentnav]

**Deferred:** this went to a background subagent (claim `subagent:ridernav`, branch `ridernav`). The goal is fully independent navigation per JetBrains installation, with one tmux window per IDE:
- The installation identity becomes part of the layout-ownership key.
- Focus mirroring goes only to the IDE that owns the window.
- `tmux_focus_state` and pane ownership are split per window.

It's estimated at several days, so the subagent first writes a phase plan. Then it lands as many independently verified phases as it can, each with `make check` passing, keeping the current focused-IDE behavior as the one-IDE fallback. I'll file the remaining phases as separate backlog ids.

Still running: `#idleclearrestart` (with the sdk.md false-"satisfied" clear) and the build/test turnaround work. The exchange is large (371 lines); I'll compact only if you ask.

### Re: closing completed subagents — opus-5.5 · 2026-10-12T05:35-04:00

> **Chat prompt (#chatprompt):** Can we close the completed subagents?

Done.
- **Subagents:** I stopped the 5 completed ones still listed in this session: the splice verifier, the queued-clear live check, the flaky-test hunt, the live IntelliJ clear-notice test, and the receipt helpers. The 4 running ones are untouched: `idleclear`, `turnaround`, `gh241` and `ridernav`.
- **Worktrees:** I removed the 20 finished worktrees for today's subagent and integration branches. That's every branch I'd confirmed was merged into `origin/main` with no uncommitted tracked changes. Their local branches are deleted too; the commits are all on main.
- **Disk:** free space went from 879G to 1.1T (`rcpthelpers` and `integrate-0499` alone held about 80 GB of build output each).

### Re: actionable review sweep — opus-5.5 · 2026-10-12T05:45-04:00

> **Queue prompt:** #actionable-review

I checked all 3 review items against current evidence. **None can be acted on by an agent right now, so nothing moved to the backlog or queue:**
- **`#frontendlivedispatch`** (operator-verify): the haiven-dev ops.log has no `route_dispatch_only_recycle_inflight_ready_after_start outcome=settled` or `supervisor_recycle_graph_settled reason=dispatch_ready_after_start` for `frontend`, and no `recycle_ttl_elapsed_supervisor_alive` refusal either. The pass needs you to click Run Agent Doc in your live `frontend.md` pane. Doing that from here would inject a prompt into your session, so it stays with you.
- **`#jb262dynlauncherrecommen`:** your policy decision. Keep the accurate-warning fallback for 262 packages, or authorize a real restart-free launcher for the 262 split-mode plugin. It stays in review until you choose.
- **`#pypiquotarecheck`:** rechecked now, still waiting on PyPI. Usage is 8.36 GiB of pre-cutover plus 3.19 MiB of post-cutover releases; about 47,722 more releases fit; verdict "headroom fine — do NOT delete releases". pypi/support#12214 is still open with no change since 2026-09-10. I updated the item with these numbers.

### Re: #actionable-review closed — opus-5.5 · 2026-10-12T05:55-04:00

> **Queue prompt:** #actionable-review

**Add actionable review items into backlog + queue:** done. **Zero items moved**, because none of the 3 review items is actionable by an agent:
- `#frontendlivedispatch` needs you to click Run Agent Doc in the live `frontend.md` pane.
- `#jb262dynlauncherrecommen` is your policy decision.
- `#pypiquotarecheck` waits on an external PyPI support reply; it was rechecked 2026-10-12, headroom fine.

Details are in the 05:45 response. The review list is unchanged at 3 gated items.
❯ Run Agent Doc + editing during the turn in infra.md resulted in duplication + corruption of the queue items:
```
- 🚧 do [#fpeseverityparse]
- do - [from:actionable-review key=stggrafanaowner] do [#stggrafanaowner]
[#sbxdeeplevers]
- do [#stgrefreshonly]
- do [#stggrafanaowner]do [#sbxdeeplevers]
- do [#stgrefreshonly]
- do [#stggrafanaowner]
```

### Re: infra.md queue duplication and corruption — opus-5-5 · 2026-10-12T06:05-04:00

This report is now queue item `#fpeseverityparse`. I claimed it and handed it to a background subagent working on branch `fpeparse` in its own worktree. It will only read infra.md (its ops.log and git history) to work out which writer caused each symptom:
- the doubled `do - do -` prefixes;
- the `[from:actionable-review key=...]` provenance tag left visible in the queue text;
- two items joined onto one line (`do [#stggrafanaowner]do [#sbxdeeplevers]`);
- the repeated tail.

The likely suspects are the actionable-review queue mirror, the 🚧 in-progress marker rewrite, and how queue lines are spliced when your edits land mid-turn. The subagent will fix the cause, add a regression test that replays the same interleaving, and update the spec. I'll integrate it with the next batch. Until then, infra.md's queue still holds the corrupted lines, and nothing touches that document.

Other work still running:
- **v0.35.492:** `make release` is still running its checks. Nothing is tagged yet.
- **idleclear:** being reworked, and it will also fix the red tmux test on `main`.
- **Build/test turnaround:** still in progress.
- **gh241:** ready and waiting for the next batch.
- **Rider/IDE independent navigation (`#ideriderindependentnav`):** the agent stopped while waiting on its own `make check`, so I restarted it to finish and report.

The exchange is long: 433 lines across 23 responses. I'm leaving it alone while the queue is active.

### Re: dispatch status — opus-5-5 · 2026-10-12T06:12-04:00

**New queue item, lazily.md:** you reported that Run Agent Doc, then Ctrl+C in Codex, then Clear Session Context left the Clear wedged. I claimed it and handed it to a subagent working in its own worktree on branch `codexclearwedge`. The subagent will only read lazily.md, its pane, and the logs; it won't send keys or clear anything there. It will find the state the Clear waits on that never settles, and check whether the earlier Claude-side fix for the same sequence (`#runctrlclaude`) never covered Codex. The fix gets a regression test and a spec update.

**`#ideriderindependentnav`:** phase 1 is done on branch `ridernav` (`50039bc7af`), and its `make check` passed (10254 tests). It changes no behavior yet. It lays the groundwork: each IDE gets its own "lane", and focus ownership is now tracked per project and per lane. The design gives each IDE its own tmux session, because tmux shares a session's current window across every attached terminal. The plan is `docs/plan-ide-independent-nav.md` on that branch. Four phases remain:
- **P2:** per-lane layout and an opt-in `editor_navigation` setting. This is the switch that actually turns on independent navigation.
- **P3:** editor wiring.
- **P4:** a document open in both IDEs.
- **P5:** rollout and cleanup.

I'll file P2–P5 as backlog ids when I merge `ridernav`, so they queue behind it rather than starting from `main` before it lands.

**Still running:**
- The v0.35.492 release is still in its checks; nothing is tagged yet.
- The subagents for `#fpeseverityparse`, `idleclear` and build/test turnaround are still working.
- `gh241` and now `ridernav` are waiting for the next merge batch.

### Re: tools.md Codex restart, batch status — opus-5-5 · 2026-10-12T06:40-04:00

**New queue item, tools.md:** you switched the agent from claude to codex, then pressed Restart Agent, and the restart wedged on "No saved session found with ID 915c25bf…". I claimed it and handed it to a subagent working on branch `codexresumeid` in its own worktree. It will only read tools.md and its pane, never edit or restart them.

The likely cause is that agent-doc stores one resume id per document, regardless of harness. After the switch it passed Claude's session id to `codex resume`. The fix covers two things:
- key the stored resume id by harness, so an id recorded under the other harness is ignored;
- treat Codex's "No saved session found" error as a stale id and start a fresh session instead of wedging.

**Batch status:** five fixed branches are merged together, and `make check` plus `make tmux-ci` are running on the result:
- `idleclear`: an idle Clear types `/clear` instead of restarting the agent. It also fixes the red tmux test on `main`.
- `gh241`
- `ridernav`: phase 1 of IDE-independent navigation.
- `codexclearwedge`: the lazily.md wedge where Clear was stuck behind Codex Ctrl+C.
- `fpeparse`: the duplicated and joined queue lines in infra.md.

If both checks pass, I'll push to `main`, install, and re-cut v0.35.492. The first attempt stopped before tagging, so nothing was published. The build/test turnaround subagent is still working, and it will go in a later batch.

### Re: batch merged and installed — opus-5-5 · 2026-10-12T07:20-04:00

Five branches are merged to `main` as `c2f516d914`. `make check` passed (10266 tests), `make tmux-ci` passed, and `make install` has run. The tmux test that was failing CI on `main` passes again.

> **Queue prompt:** do [#idleclearrestart]: Yes, Idle clear should type `/clear`.

> **Operator note:** Yes, Idle clear should type `/clear`.

Done (`582b6fea1e`). Clear Session Context at an idle prompt now types `/clear` into the running Claude process (`/new` for OpenCode), so background agents and shells keep running. It still restarts in three cases, and the reply and ops log name which one applied:
- no child process is alive;
- the supervisor's restart/quit prompt or a permission prompt owns the input;
- the harness is Codex, because Codex's `/clear` exits the process.

The live test also exposed a related bug: a queued Clear could wait forever on a quiet pane. That's fixed too. A queued Clear now sends the idle watch a per-document wake signal.

> **Queue prompt:** sdk.md issue: Run Agent Doc, then `/clear` in the Claude Code terminal: Clear command is not operated.

I checked the logs, and the clear was not wrongly marked done:
- Claude Code dropped your 09:07:10 Run Agent Doc prompt before saving it.
- Both Clear presses landed inside the 20-second wait before a dropped prompt is given up on, so they were queued.
- You typed `/clear` by hand about 3 seconds before the queue would have sent it.

The missing wake signal (fixed above) could have held it even longer. A regression test now replays this sequence. Why Claude Code dropped that prompt is still unproven: an extra Enter that agent-doc sent at 09:07:11 correlates with the drop, but the logs don't show it caused it.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/241

Fix then close: fixed in `15650ff081`, and GH #241 is now closed with a comment. When one of two editors for an open document is removed, the JetBrains plugin no longer deregisters the replica the remaining editor still uses. Plugin versions are JetBrains 0.2.547 and 262 0.2.548.

> **Queue prompt:** do [#ideriderindependentnav]

Phase 1 is done (`50039bc7af`) and changes no behavior yet. Focus ownership is now tracked per project and per navigation lane. Each IDE will get its own tmux session, because tmux shows the same current window to every terminal attached to a session. The plan is in `docs/plan-ide-independent-nav.md`, and I filed the remaining phases as `ridernavp2a`, `ridernavp2b`, `ridernavp3`, `ridernavp4` and `ridernavp5`. They depend on each other in that order, so I'll run them one at a time rather than in parallel.

> **Queue prompt:** do [#fpeseverityparse]

Done (`52c9398612`). When merging concurrent edits, agent-doc identified a `queue add` line by the first `[#id]` in its text. So it collided with your own line for the same id, which produced the `do - do -` prefixes, joined lines and duplicated tail. Inbound lines are now identified by their `[from:...]` attribution. A regression test replays the infra.md interleaving through the real write path. infra.md's queue still has the corrupted lines from that turn; the fix doesn't rewrite them.

> **Queue prompt:** lazily.md issue: Run Agent Doc then Ctrl+c in the Codex terminal then Clear Session Context: Clear Session Context is wedged

Done (`c2149c15f9`). Clear was stuck for about 2 minutes. Codex runs preflight at its first tool call, so a Ctrl+C one second in left the dispatch record half-open, and the earlier Claude-side fix only closed records that preflight had already started. An ended turn now closes that half-open record too. One remaining edge case: a Codex turn that ends normally without ever running preflight would still hit the wait, which I judged rare.

**Release:** v0.35.492 is re-cut on this tree with an updated VERSIONS.md entry, and `make release` is running. Still in flight: the tools.md Codex resume-id wedge and the build/test speed-up.

### Re: lazily.md Codex continuation, release status — opus-5-5 · 2026-10-12T08:05-04:00

**New queue item, lazily.md "Blocked by hook" continuation:** I claimed it and gave it to a subagent on branch `codexstopcont`. It will only read lazily.md, its pane and the logs. The message points at a real problem. Session accretion is at `block` (924 exchange lines), yet Codex's Stop hook keeps continuing the queue in the same session, and it says the context reset it requested will not happen because automatic clearing is disabled. Under the drain contract, the supervisor is supposed to reset context between queue items. For Codex that reset has to be a fresh restart at the idle point between items, because Codex's `/clear` exits the process. The subagent will:
- confirm the cause from the logs;
- make the hook hand over to that reset instead of continuing in place;
- shorten the instruction text the pane shows you.

**v0.35.492:** tagged (`a27ed22e2c`) and installed locally. The GitHub Release build is still running. PyPI publishes after it, so I'm keeping "release + publish" open until both are confirmed.

**Next batch:** `turnaround` (faster `make check`) and `ridernavp2a` are merged and in `make check` / `make tmux-ci`. Also in progress: `ridernavp2b`, and the tools.md Codex resume-id wedge.

### Re: v0.35.492 published, next batch landed — opus-5-5 · 2026-10-12T08:25-04:00

> **Queue prompt:** release + publish

v0.35.492 is published:
- **GitHub Release:** the Linux, Windows and macOS archives, the JetBrains plugins 0.2.547 and 262 0.2.548, and the VS Code extension 0.2.81.
- **PyPI:** has 0.35.492.
- **Local:** installed.

It ships idle Clear typing `/clear`, the Codex Ctrl+C Clear fix, the infra.md queue-merge fix, GH #241, the thrashing fix, and navigation-lane phase 1. The first attempt stopped before tagging on the stale tmux test, so nothing broken was published. The Book workflow failed on the follow-up commit because GitHub could not fetch its Pages artifact. The separate mdBook Pages deploy for the same commit succeeded.

Next batch: three branches are merged to `main` as `84f0605de2`. `make check` passed (10268 tests) and `make tmux-ci` passed, and it is installed. The first `make check` attempt failed only because GitHub returned a 504 on the pinned TLA jar download. I seeded the jar from another worktree after checking its SHA-256 matched, then reran `make check`.

> **Queue prompt:** How can we optimize the agent-doc build process + test process to reduce the turnaround time?

Measured and landed (`259a1af585`). `make check` went from about 919s to about 504s, and no check was dropped. The 504s included a full TLA run, which is now skipped when TLA inputs are unchanged. The speedups:
- The controller-heavy test binaries now run concurrently instead of one after another: 329s down to about 153s.
- TLA runs 8 checks in parallel, down from about 185s to about 90s. A content receipt skips it entirely when `formal/tla/`, the runner script and the JVM are unchanged.
- All the Rust test steps share one build. One core-crate edit now recompiles 256 units instead of 479, and `sim-medium` dropped from 66s to about 5s.
- Dev debug info is now line tables only. Incremental build CPU went from 733s to 429s, and `target/` shrank from 44G to 20G. `CARGO_PROFILE_DEV_DEBUG=2` brings back full debug info.
- The JetBrains build now reuses the gradle build cache.

Further options, not done:
- running the gradle/node/lean/TLA steps alongside the Rust tests (opt-in, because the controller tests are load-sensitive);
- splitting the 639-test `agent_doc_controller_io` binary;
- trimming wall-clock waits in the slowest nextest tests.

The turnaround work also found that `make check`'s clippy lints only the root crate. I filed that as `clippyworkspacegap`.

> **Queue prompt:** tools.md: Changing agent frontmatter from claude to codex: Restart Agent: Codex session restart is wedged: ERROR: No saved session found with ID 915c25bf-68e3-4706-844d-48cb86db4cfa. Run `codex resume` without an ID to choose from existing sessions.

Fixed (`4ca45b5238`). `915c25bf…` was Claude Code's session id. Claude and Codex share one hook ledger whose rows didn't record which harness wrote them. After the switch, Codex adopted Claude's id and ran `codex resume` on it. Changes:
- Each ledger row now records its harness.
- A Codex resume id that has a Claude transcript starts a fresh Codex session instead. This also covers tools.md's frontmatter, which still carries that id.

The wedge itself came from the existing stale-id recovery: its frontmatter clear stalled behind a compaction. That clear now gives up after 5s and relaunches anyway.

The tools.md pane is still on Codex's error screen. Its supervisor should start a fresh Codex session once it restarts on the new binary. That supervisor also had a thread spinning at about 80% CPU, which I filed as `sessionactorspin`.

> **Queue prompt:** do [#ridernavp2a]

Done (`416c2dba1d`). The controller's pane layout is now kept per IDE navigation lane, with no behavior change. P2b, which turns on independent navigation behind the `editor_navigation` setting, is with a subagent now. P3–P5 stay queued behind it.

Still in progress: the lazily.md Codex continuation fix, and `ridernavp2b`. The exchange is long (528+ lines); I'm leaving it alone while the queue is active.

### Re: batch 5 landed — opus-5-5 · 2026-10-12T09:35-04:00

Four branches are merged to `main` as `f43d19c039`. `make check` passed (10281 tests), `make tmux-ci` passed, and `make install` has run.

> **Queue prompt:** lazily.md issue
> ```
> • Blocked by hook
>   └ Queue continuation, not an error: agent-doc kept the active `agent:queue auto` moving for /home/brian/work/btakita/
> agent-loop/tasks/software/lazily.md. Codex labels every Stop continuation `Blocked by hook`; here that label means the
> next queue item is being handed back to this turn. The next queue prompt is "do [#lzcoreprofile]". Continue THIS turn
> in-pane: answer that prompt in /home/brian/work/btakita/agent-loop/tasks/software/lazily.md and persist with `agent-doc
> finalize /home/brian/work/btakita/agent-loop/tasks/software/lazily.md` (or `agent-doc write --commit /home/brian/work/
> btakita/agent-loop/tasks/software/lazily.md`). Do NOT run `agent-doc /home/brian/work/btakita/agent-loop/tasks/software/
> lazily.md` from this pane — that re-invokes the owner pane and hits the recursive-direct-invocation deadlock guard, and
> do not send the final answer yet. A background context reset was requested because session accretion is block
> (exchange_lines=924, response_sections=31, recent_committed_cycles=2, recent_noop_closeouts=0), but automatic context
> clearing is disabled; only an explicit operator clear may reset the session.
> ```

Fixed (`4115f8f6ce`). The logs show no loop: the queue moved on from `#lzcoreprofile` to `#lzcoreclaims` as it should. The problem was the message itself.
- Whenever a reset reason existed, it printed a sentence about a "background context reset" that the agent could not act on.
- That same code path silently swapped the MCP instructions for CLI-only ones, and lazily.md uses MCP.

The continuation is now one short paragraph, capped at 320 characters for its fixed wording. MCP sessions keep their MCP guidance. The suppressed reset reason now goes to ops.log only.

A clear would not have helped here anyway. Accretion `block` measures the size of the document's exchange, and that stayed `block` even after your Clear restarted Codex fresh. Compacting lazily.md's exchange is what reduces it.

One decision is yours, filed as `bgclearpolicy`. Automatic clearing between queue items has been off for every harness since June 30, but the agent instructions still say the supervisor clears context between items. We should either re-enable it or correct the wording.

> **Queue prompt:** do [#clippyworkspacegap]

Done (`1bfe961f38`). `make check` now runs clippy with `--workspace`, so all 144 crates are linted. The workspace-wide run turned up no lints. I confirmed the old root-only run missed a lint planted in another crate, and that the new run catches it. A Makefile test fails if `--workspace` is ever dropped. The cost is about 29 seconds more on a cold clippy run.

> **Queue prompt:** do [#sessionactorspin]

Done (`e1d1496531`). The 80% CPU was a quadratic CRDT merge, not a polling loop:
- A compaction cut tools.md from 75 KB to 3 KB.
- The queued resume-id clear then rebased onto the compacted text, deleting the removed text one character at a time.
- Each single-character delete rebuilt the whole document order.

Replayed on the exact inputs (the hashes matched ops.log), the old code ran more than 8 minutes before I stopped it. The fix deletes the whole range in one call and takes 0.46s; the resulting CRDT state is identical. New tests fail at 60s if the per-character loop comes back.

> **Queue prompt:** do [#ridernavp2b]

Done (`449846030b`). There is now an opt-in setting, `editor_navigation = "per_installation"` in `.agent-doc/config.toml`:
- Each IDE gets its own lane session, named `<pinned session>-<installation>` and marked as agent-doc's.
- Each lane syncs and cancels independently of the other lanes.
- A pane never moves between lanes. A document whose pane is in another lane is dropped from the sync (`cross_lane_layout_dropped`), and focus on it is refused (`owned_by_other_lane`).
- An existing session agent-doc did not create is never adopted.

The default `shared` mode is unchanged, and its existing tests pass without edits. P3, which wires the editors to their lanes, is with a subagent now and is built on this branch. P4 and P5 follow it.

Also filed: `stopreopenflake`, a Codex stop-hook test that fails when run in parallel with the rest of its crate, so its cause gets fixed instead of being rerun until it passes.

### Re: residue repair, sdk.md item, multi-line queue heads — opus-5-5 · 2026-10-12T09:50-04:00

The previous closeout committed, but `session-check` then reported the lazily.md item as still active, even though my response quoted it. The item spans several lines, and the echo did not strike it. I ran the repair the binary printed (`preflight`, then `commit`), which marked it done. `session-check` is OK now.

Your new sdk.md item (the Stop hook holding `agent:queue auto` paused on `intent_id=unobserved-retained-intent`) was never parsed as its own queue item. It begins right after the lazily.md block without a `- ` bullet, so it was read as part of that block, and `queue claim` and `queue brief` both rejected it. I dispatched it by hand to a subagent on branch `sdkretained`, using your text verbatim plus the queue preset. The subagent will only read sdk.md and its logs. The likely defect: the hook waits on a write holder it never observed, while the cycle is already committed. Per `#percellconverge`, that wait must prove a holder or name a recovery that terminates. The subagent will also check whether the quadratic-merge stall fixed in `sessionactorspin` started it.

I filed both parsing failures as `multilinequeuehead`. Since the sdk.md text isn't a head, I'll close it by hand once its fix lands. `stopreopenflake` is with a subagent too.

### Re: auto-clear default, GH #242 — opus-5-5 · 2026-10-12T10:10-04:00

> **Chat prompt (#chatprompt):** auto-clear should on by default and disabled with a queue element attribute. disable-auto-clear perhaps?

Agreed, and `#bgclearpolicy` is with a subagent on branch `bgclearpolicy` to build it:
- **On by default for an active queue.** After an item's closeout is proven, at the idle point before the next item is dispatched, the supervisor resets the session and then sends the next item to the fresh session.
  - Claude Code gets `/clear` typed into the live process, so background agents and shells survive. OpenCode gets `/new`.
  - Codex gets a fresh restart, because its `/clear` exits the process.
  - It never clears mid-turn, over an unsent draft, or with your steering still unanswered.
- **`<!-- agent:queue disable-auto-clear ... -->`** turns it off, and preflight reports the setting.
- **The instructions get corrected to match.** The agent-doc skill, the session-size guidance and the Codex stop-hook continuation will describe the shipped behavior. When a clear is due, the Codex hook hands off to the supervisor instead of continuing in the same pane. The guidance will also say that only compaction shrinks an over-long exchange.

The subagent will first find out why `45ff25f994` turned clearing off in June, and confirm the new design doesn't bring that failure back.

**GH #242** (the JetBrains `TypingTracker` retry storm, 32,277 refusals in 92 minutes) is with a subagent on branch `gh242`. The retry will fire on the event that can actually let it succeed, instead of on an unlimited 75 ms timer. Repeated identical refusals will be logged once with a count, without a Lazily actor round trip for each one. It will be closed after merge.

Also in flight:
- IDE navigation P4;
- the sdk.md retained-write wedge;
- `stopreopenflake`;
- `multilinequeuehead`.

P3 is done and waiting for a plugin version bump at integration.

### Re: bgclearpolicy direction — opus-5-5 · 2026-10-12T11:05-04:00

> **Queue prompt:** do [#bgclearpolicy]: Do you have enough direction for this? The hourglass is still visible in the queue.

> **Operator note:** Do you have enough direction for this? The hourglass is still visible in the queue.

Yes, there's enough direction. Your two answers, "Re-enable clearing" and "auto-clear should on by default and disabled with a queue element attribute", went to the subagent working on branch `bgclearpolicy`. The hourglass was there only because the item text still began with `[operator-verify] … Decide:`, which flags it as waiting for you. This cycle rewrites the item as the decided spec:
- on by default;
- a bare `disable-auto-clear` attribute on the queue element turns it off;
- Claude Code gets `/clear`, OpenCode gets `/new`, and Codex gets a fresh restart, all at the idle boundary between items.

The item no longer waits for you.

**Deferred:** the work itself is still in progress on that branch. It closes once it is merged and installed.

v0.35.493 is in `make release` now, on the batch merged to `main` (`36da03f4f5`), plus `inlinecodemarker` and IDE navigation P4.

### Re: batches 6–8 landed, v0.35.493 tagged — opus-5-5 · 2026-10-12T11:40-04:00

`main` is now at `d6da68546a` and installed. Batches 6, 7 and 8 each passed `make check` and `make tmux-ci`; batch 8 ran 10345 tests. v0.35.493 is tagged (`6e03ce43ae`) and installed locally. Its GitHub Release workflow is still running and PyPI doesn't have it yet, so **release + publish** stays open until both show 0.35.493.

> **Queue prompt:** do [#bgclearpolicy]: Do you have enough direction for this? The hourglass is still visible in the queue.

> **Operator note:** Do you have enough direction for this? The hourglass is still visible in the queue.

Yes. It is built and merged (`60eeb0b8f5`), and it ships in the next release.
- Auto-clear between queue items is on by default. `disable-auto-clear` on the `agent:queue` element turns it off.
- After each item's closeout, at the idle point and before the next head is dispatched:
  - Claude Code gets `/clear` typed into the same process, so background agents and shells survive.
  - OpenCode gets `/new`.
  - Codex gets a fresh restart.
- It never clears mid-turn, over an unsent draft, while your prompts are unanswered, or while a subagent claim is in flight.

June's `45ff25f994` turned clearing off because `/clear` was typed into a busy Codex on every tick, and a restarted supervisor replayed a leftover `/clear` draft. The new design prevents both:
- Codex is restarted instead of typed into.
- There is at most one clear per finished item, recorded before it acts.
- Each clear is tagged with its source.

While building it, the subagent also fixed a supervisor stall: a new queue head first seen inside the 2-second settle window was never dispatched.

> **Queue prompt:** do [#ridernavp3]

Done (`38febbe454`, in 0.35.493). Each IDE now reads its own focus state, attaches its terminal to its lane session, and publishes layouts (including Sync Tmux Layout) into its own lane. JetBrains ignores focus updates from another lane. The plugins are JetBrains 0.2.549 and 262 0.2.550.

> **Queue prompt:** do [#ridernavp4]

Done (`e4a048b8a7`, in 0.35.493). Claim for Tmux Pane or a manual Sync Tmux Layout in the other IDE moves a shared document's pane there. Passive syncs never move it, and only sessions agent-doc owns are touched. The old lane republishes without that document. A SimWorld search checks that no pane is ever duplicated or lost.

> **Queue prompt:** do [#ridernavp5]

Done in part (`1d3361c6db`). This part covers switching `editor_navigation` either way:
- **Turning lanes on** moves nothing.
- **Turning them off, then running `agent-doc gc`,** moves lane panes back into the shared session's `stash` window.
- **`gc` also closes empty lane sessions** that agent-doc created. It never touches unmarked or foreign sessions, and `--dry-run` previews the plan.

A two-IDE end-to-end test covers this. The rest is filed as five new items: `ridernavp6route`, `ridernavp6republish`, `ridernavp6status`, `ridernavp6gc` and `ridernavp6switchback`.

> **Queue prompt:** do [#stopreopenflake]

Done (`c58a75081e`). The test read the process-wide `CODEX_THREAD_ID` variable that sibling tests set, and it failed 8 of 8 parallel runs on main. Tests now set the Codex session per thread. The fix exposed a real bug: in an active Codex session, a proven `agent:done` archive move counted as drift. That is fixed too.

> **Queue prompt:** do [#multilinequeuehead]

Done (`7dd46ee898`). The strike had put its "auto-struck" note on the closing `~~~` line, which left the fence unclosed and swallowed the queue's closing marker. That is why the earlier closeout failed. A multi-line item now counts as its own queue item when it follows a `---` separator or another multi-line item.

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/242

Fix then close: fixed (`3feec687db`), shipped in 0.35.493, and GH #242 is closed with a root-cause comment.
- **Cause:** IntelliJ moves the document stamp without firing an edit event (undo, save), so a refused burst could never match again and kept retrying every 75 ms.
- **Fix:** retries now happen only on a new edit event, and identical refusals are logged once with a count.

The earlier parse refusal on this document was not caused by line 690. The `inlinecodemarker` investigation found that the document parses with both main and 0.35.492. The most likely cause was a half-edited queue in your editor buffer at that moment. Parse errors now name the marker's line.

### Re: v0.35.493 published — opus-5-5 · 2026-10-12T12:05-04:00

> **Queue prompt:** release + publish

v0.35.493 is published:
- **GitHub Release:** the release workflow succeeded. It published Linux (gnu, musl, aarch64), Windows and macOS archives, the JetBrains plugins 0.2.551 and 262 0.2.552, and VS Code 0.2.81.
- **PyPI:** the workflow succeeded, and `pypi.org/pypi/agent-doc/0.35.493/json` serves `agent_doc-0.35.493-py3-none-any.whl`. PyPI's cached simple index still lists 0.35.492 as the newest, which is a CDN delay only.
- **Locally:** installed.

What's in it:
- independent IDE navigation lanes P1–P4 (opt-in `editor_navigation = "per_installation"`);
- harness-keyed Codex resume ids;
- the linear CRDT range delete;
- the Stop hook naming its real holder;
- the terse Codex continuation;
- multi-line queue heads;
- the JetBrains typing-retry fix (GH #242);
- line-numbered parse errors;
- a faster `make check` with workspace-wide clippy;
- codex-stop test isolation.

Auto-clear between queue items (`bgclearpolicy`) and lane migration/cleanup (P5) landed on `main` after the tag. Both are installed locally and ship in the next release.

`ridernavp6status` is done (`5355e01c79`): each lane now publishes its own pane-layout status, and shared mode is unchanged. It merges with `ridernavp6republish`, which is now being built on top of it. `ridernavp6route` is in progress in parallel, and `ridernavp6gc` and `ridernavp6switchback` follow.

### Re: infra.md Clear wedge — opus-5-5 · 2026-10-12T15:15-04:00

> **Queue prompt:** do [#infraclearsession]

**Deferred:** a subagent is working on it on branch `infraclearsession`.

The upcoming release probably does not fix this. The binary installed when you pressed Clear (15:06Z) already included the 0.35.492 Codex Ctrl+C fix (`codexclearwedge`), yet haiven-dev's ops.log shows:
- The first press (15:06:06Z) was deferred with `evidence=turn_active_lease`, so the interrupted turn's lease was still held.
- Every later press (:14, :21, :27) was re-deferred with `evidence=dispatch_turn_pending rearmed=true`.

That fix only settles a dispatch that had not reached preflight when the turn was interrupted. This turn had been running for over 13 minutes, so an interrupt mid-turn after preflight is likely a separate, uncovered path. The supervisor was also recycling around 15:00–15:04 after the install. The subagent will:
- reconstruct the sequence from ops.log and `state.db`, read-only;
- find the record that keeps `dispatch_turn_pending` true;
- settle it on the interrupt or turn-end event, not with a longer timeout;
- add a regression test that replays it.

infra.md and its pane are not touched.

### Re: lane route landed; P5 test flake — opus-5-5 · 2026-10-12T12:45-04:00

**`ridernavp6route` is done** (`9f42c2a214`), waiting to merge:
- **Route, start and claim** accept `--client-id`. In `per_installation` mode a new pane goes into the requesting IDE's lane session, entering through its `stash` window past the lane's empty placeholder shell.
- **JetBrains** sends the id with Run Agent Doc and with Claim, New Pane and Force Claim. The plugin and binary must ship together.
- **Shared mode** and VS Code are unchanged.

**A flaky test found:** P5's end-to-end test (`two_ide_navigation_lanes_migrate_and_clean_up_end_to_end`, on `main` since `d6da68546a`) sometimes keeps an empty lane as "busy" instead of closing it. It failed once in `make tmux-ci` and 1 of 3 runs alone. A guess, not yet checked: the lane's fresh shell is still running its startup when `gc` reads it. It is filed as `lanegcflake` and goes to a subagent now. The fix will remove the cause, not rerun the test until it passes.

Also filed: `ridernavp6routecontended`. When the document's pane is in another IDE's lane, Run Agent Doc drops the document from the requesting IDE's layout, and nobody has defined or tested what happens next.

The infra.md Clear fix (`infraclearsession`) is in `make check` / `make tmux-ci` now.

### Re: infra.md Clear fix landed — opus-5-5 · 2026-10-12T13:15-04:00

> **Queue prompt:** do [#infraclearsession]

Done (`6d8f498bb3`). It is on `main` and installed: `make check` passed 10,348 tests, and `make tmux-ci` passed.

**Your question:** this was not fixed by the upcoming release, because 0.35.492's `#codexclearwedge` missed this path. What happened:
- 15:06:03Z: you interrupted Codex 3 seconds into the turn, before its first tool call, so preflight never marked the dispatch as running.
- A few seconds later, a different agent-doc process read the turn lease. That read ended the interrupted lease but settled nothing.
- Every Clear after that saw a dispatch still pending and re-queued itself.
- The stale dispatch then also caused your next Run Agent Doc to mark the wrong dispatch as running.

**The fix:** the dispatch is now settled at the moment the lease is ended, in the same database transaction. It no longer matters whether the CLI, a supervisor, or a supervisor mid-recycle ended it. The normal turn-end hook uses the same rule. No timer was added or lengthened.

**Regression test:** a SimWorld replay of the infra.md sequence fails without the fix.

Your queued Clear on infra.md (epoch 79) should have gone through when that turn ended.

Also filed: `steeringeditmisalign`. When you delete struck queue lines, steering reports untouched items as "edited", with "previous" text taken from a removed item. It pairs queue lines by position instead of by item, which is the noise you've been seeing.

### Re: steering misreport fixed; another flaky test — opus-5-5 · 2026-10-12T13:55-04:00

`steeringeditmisalign` is fixed (`b4544973ef`, `make check` passed) and will merge with the next batch.

**The cause.** Steering paired the old and new queue lines by text similarity alone. Once the struck lines were deleted, a removed `do [#ridernavp4]` paired with the untouched `do [#ridernavp6route]`, so an unchanged claimed item was reported as edited. The bogus edit also woke the coordinator.

**The fix.** Lines now pair by item identity first: the tracked id, or for an untracked line its text with markers stripped. Similarity is only a fallback between untracked lines. A tracked id never pairs with a different item.

**The constant "operator is still typing N more item(s)" message was the same bug.** Each new line re-paired the items and restarted their settle timers, so nothing ever settled while the document kept changing. Three of the four new regression tests fail on the old code.

**Another flaky test, filed as `laneplaneflake`.** The P3 controller test `plugin_publications_project_into_the_publishing_clients_navigation_lane` failed under parallel load and passed 3 of 3 alone. A subagent is taking it, to fix the cause rather than rerun it until it passes.

**Batch 10 (`lanegcflake` + `ridernavp6republish`)** is in `make check` / `make tmux-ci`. `ridernavp6route` is being merged with it by its own subagent to resolve conflicts.

### Re: batches 10–11 landed — opus-5-5 · 2026-10-12T14:40-04:00

Batches 10 and 11 are on `main` (`4f168dad62`) and installed. Each passed `make check` and `make tmux-ci`; batch 11 ran 10,362 tests.

> **Queue prompt:** do [#lanegcflake]

Done (`021ffcf29c`). `gc` read a lane's own shell, still running its startup files, as a running program. Panes agent-doc creates in a lane are now marked. A marked pane counts as idle until a client has ever attached to that lane, so anything you start there still keeps the lane. The test went from 12 failures in 15 runs to 45 passes in 45, including runs under heavy CPU load.

> **Queue prompt:** do [#laneplaneflake]

Done (`9fe499436f`). This was a real controller bug. A lane frame was marked as "seen" for the shared lane only after that lane's worker had published it, so shared-lane routes could wait out their full 500ms catch-up budget, or longer if the worker kept retrying. Frames are now marked seen when they are dispatched. Before the fix the test failed 103 of 320 runs; after it, 0 of 480.

> **Queue prompt:** do [#ridernavp6status]

Done (`5355e01c79`). Each lane publishes its own pane-layout status on its own channel. A client that subscribes with its id gets only its own lane's status. Shared mode is unchanged.

> **Queue prompt:** do [#ridernavp6republish]

Done (`e68649aec2`). The `sync_tmux_layout` command goes to the requesting client's lane. A recycle re-applies every lane's retained layout, each in its own lane, and never moves a pane across lanes.

> **Queue prompt:** do [#ridernavp6switchback]

Done (`78a7be20ee`). Checking this found a real bug: after switching back to `shared`, a shared sync could follow a stale lane, rearrange it, and push shared panes into it. Now every shared sync targets the shared session and adopts the layout's lane panes first, keeping the same panes so nothing is duplicated or lost.

> **Queue prompt:** do [#steeringeditmisalign]

Done (`b4544973ef`). Steering now pairs old and new queue lines by item identity first. Untouched items are no longer reported as edited, and the stuck "operator is still typing N more item(s)" message stops.

**Up next:**
- `ridernavp6route` is merged with all of this (`8ec61eaf7b`). One unrelated `make tmux-ci` test failed: `queued_clear_is_dropped_when_a_newer_prompt_starts_first`, which passed when run alone. It is filed as `queuedclearflake` and goes to a subagent now.
- `ridernavp6gc` (controller-side lane cleanup) merges on top of route next.
<!-- agent:boundary:75cf33a3 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built
<!-- agent:queue subagents preset="#spec-test-build-install-commit-push" priority go -->
- do [#queuedclearflake]
- ~~do [#laneplaneflake]~~
- ~~do [#infraclearsession]~~
- 🚧 do [#ridernavp6route]
- ~~do [#ridernavp6republish]~~
- ~~do [#ridernavp6status]~~
- 🚧 do [#ridernavp6gc]
- ~~do [#ridernavp6switchback]~~
- ~~do [#lanegcflake]~~
- do [#ridernavp6routecontended]
- ~~do [#steeringeditmisalign]~~
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#queuedclearflake] [agent-doc] tests/clear_queued_live_turn_tmux.rs queued_clear_is_dropped_when_a_newer_prompt_starts_first failed its ui_outcome=queued assertion inside make tmux-ci (2026-10-12, ridernavp6route merge run) and passed 2 of 2 alone. Find the timing dependency and fix it with an explicit readiness signal; never rerun to green or lengthen a timeout.
- [ ] [#ridernavp6routecontended] [agent-doc] IDE navigation lanes follow-up (docs/plan-ide-independent-nav.md): Run Agent Doc counts as a passive claim, so for a document whose pane lives in another IDE's lane the route drops it from the requesting IDE's layout; what the route's layout wait does in that case is untested. Define the behavior (wait, refuse with owned_by_other_lane, or hand over) and pin it with a test.
- [ ] 🚧 [#ridernavp6route] [agent-doc] IDE navigation lanes follow-up (docs/plan-ide-independent-nav.md): route, start and agent-doc claim --client-id create panes in the requesting IDE's lane in per_installation mode; today a new pane reaches an installation lane only through that IDE's explicit claim.
- [ ] 🚧 [#ridernavp6gc] [agent-doc] IDE navigation lanes follow-up (docs/plan-ide-independent-nav.md): a controller-side trigger for empty-lane cleanup and per_installation-to-shared migration that knows which IDE installations are live; today only agent-doc gc runs it, using an attached terminal as the only liveness signal.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#frontendlivedispatch] [operator-verify] [agent-doc] Live confirmation that Run Agent Doc on haiven-dev frontend.md dispatches (fixes 0.35.483: runfrontenddispatch 3d9fbad86, dispatchreadyselfheal 81b9e1910). Precondition met 2026-10-11: pane %166's supervisor re-registered on the current binary (pid 3851566, healthy/ready) and install_fanout recycles now settle. Remaining: the operator clicks Run Agent Doc on frontend.md; pass = route_dispatch_only_recycle_inflight_ready_after_start outcome=settled or supervisor_recycle_graph_settled reason=dispatch_ready_after_start in haiven-dev ops.log, with no refusal=recycle_ttl_elapsed_supervisor_alive.
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
