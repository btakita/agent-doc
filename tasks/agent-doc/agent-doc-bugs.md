---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: c47dae65-e36f-40cb-8ebb-b7c9573e3c41
  codex: 01a0db52-4c0d-72d3-b2c9-0ac89aff42a0
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#agent-doc-causes': Fix agent-doc causes
  '#do-backlog': /goal do backlog items
  '#gh-fix': fix then closerelease
  '#release': release + publish
  '#self': agent-doc-bugs.md
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20261001-134419.md`*

Compacted content:
- Archived 9 response topic(s): do [#refusalsteeringverbatim] + the lazily.md Stop-hook failure; #gh-fix GH #76 + fixing agent-doc's causes of the recycle churn; do [#stopreplaynoopretained]; 6 more
- Prior summary/context: prior compacted content: Archived 10 response topic(s): #advance-review; Drive Idea for #drivecapturedsplice; #coder7...

### Re: #gh-fix GH #77 (tmux shows 1 pane while the editor shows 2 documents) — opus-5.5 · 2026-10-01T10:05-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/77

**Fixed and closed.** Commit `52b43df8f` in agent-doc, JetBrains plugin 0.2.455. It is pushed and installed (`make install`, which also reloaded the plugin in IDE pid 2960722).

**Cause:** the JetBrains plugin works out which editor splits sit side by side from where each split appears on screen. A JetBrains Remote Dev backend never lays its splits out, so both report position (0,0).

- The plugin grouped them into **one** column.
- One column counts as "no layout detected", so the surface sent to the controller carried no columns.
- The controller then synced tmux to the focused document alone. The result was one pane whose document swapped on every switch.

That one cause explains both symptoms in the issue: the single pane and the focus that never moved.

**Fix:** two splits that are actually laid out can never share a position, so a shared position now means the geometry is unknown. Each split then stays its own column. When this happens, the plugin logs `[layout-detect] … share an origin; geometry unavailable` to idea.log at info level, so a live run can confirm the cause. The diagnosis started in the turn before the `/clear`, which left uncommitted plugin changes in the tree. I checked that work against the code path, extended it with the log line, and shipped it.

**Verification:**
- New regression test `LayoutDetectorTest … keeps unlaid-out splits apart`.
- JetBrains plugin suite: 603 tests, 0 failures.
- `make check`: exit 0, 10280 passed. The first run had 4 failures in `agent-doc-controller-io` dispatch tests. A second `make check` left over from before the `/clear` was running at the same time. Those tests pass alone, the crate passed twice in full parallel runs, and the rerun with no other suite running was all green.
- CI for the push is still running.

**Still to check (by you, on the Remote Dev host):** this is not confirmed live yet. Install plugin 0.2.455 there, then look for the `share an origin` line in idea.log and for `panes=2` in tmux. The issue comment says to reopen #77 if the line is missing and tmux still shows 1 pane.

### Re: #gh-fix GH #78 (preflight overrun refusal had no measured cause) — opus-5.5 · 2026-10-01T10:31-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/78

**Fixed and closed.** Commit `ef529c47f` is pushed and installed. #78 closed automatically from the commit, and the issue comment says what changed. All four of the issue's asks are done:

1. **The refusal names the measured phase.** Preflight now marks each of its 29 top-level steps (`commit_previous_cycle`, `settle_debounce`, `queue_maintenance`, and so on) with `progress::enter(<phase>)`. The record is a per-run handle that the hook installs on the worker thread, so runs happening at the same time never write into each other's record. On an overrun, the refusal names the phase still running and how long it had run, then lists the finished phases, slowest first. The same breakdown goes to stderr and to ops.log as `preflight_admission_overrun … running=<phase>:<ms> completed=…`.
2. **The guess is removed.** The fixed sentence "usually a wedged project controller or supervisor" is gone. The refusal now carries the measured phase instead.
3. **The suggested command works.** The refusal now names `agent-doc admin inspect <FILE>`. The bare `agent-doc admin inspect` is rejected without a target.
4. **The abandoned worker cannot cause trouble.** I checked `main`: it returns right after the refusal, so the abandoned worker dies with the hook process and cannot race the next trigger. A half-finished SQLite write rolls back, and a `preflight_started` cycle it opened is closed by the next turn's recovery. The refusal now says this.

**Verification:**
- 3 new tests for the progress record.
- The overrun test now asserts four things: the phase still running, the finished phases, that the old guess is gone, and the working `admin inspect <FILE>` form.
- A new test covers an overrun that happens before preflight reaches any phase.
- `make check`: exit 0, 10284 passed.
- `make install`: OK.
- New spec bullet `#preflightoverrunphase` in `specs/07-core-commands.md`.
- CI on the push is still running.

### Re: Can we make the "genuinely wedged run" never happen? — opus-5.5 · 2026-10-01T10:31-04:00

**Not with this fix alone, but yes, it can be made impossible.**

**Why it can happen today.** Every wait inside preflight has its own small limit: the settle waits default to 3 seconds, and each controller call has a timeout. Nothing limits their **sum**. The #78 report shows the shape: `session-check` ran `resolve_current_document_content` 13 times on a 518-byte document. Many short waits, each within its limit, can add up past the hook's 90 seconds with nothing actually stuck. From outside it then looks like a wedge.

**How to make it impossible, in two steps:**

1. **One deadline for the whole run.** Store the admission deadline (the budget minus a safety margin) on the same per-run progress handle this fix added.
    - Every bounded wait inside preflight is cut down to the time left.
    - Each `progress::enter` step boundary checks the deadline.
    - Preflight then stops between steps with a named, retryable refusal, instead of being abandoned in the middle of a step.

    That makes "abandoned at the hook boundary" impossible. A slow run would still be refused, just cleanly and earlier.
2. **Take the slow step off the admission path.** The overrun on the reporting host has not been measured yet. Its first occurrence on the new build will name the phase that used the time. If that phase is maintenance-like (`commit_previous_cycle`, `cross_document_sweep`, `pending_maintenance`), it can move to the supervisor's idle boundary. Admission then stops paying for it at all, and the turn is not refused.

Step 1 does not depend on any live measurement, so I'm adding it as `#preflightdeadline` at the end of the queue, after the items already there. Step 2 depends on the first overrun that names its phase.

### Re: Old GitHub issue sweep — opus-5.5 · 2026-10-01T10:40-04:00

> **Queue prompt:** Look at all the old gh issue that I created. Close any of the issues that are no longer relevant. Anything relevant should be added in the backlog.

There were 24 issues open in `btakita/agent-doc`. I closed **20**, each with a comment explaining why, and **4** stay open, tracked here.

**Closed as done (5):**
- #39: socket IPC through `interprocess`.
- #30: `install.sh`.
- #21: parallel worktree fan-out, which shipped as `orchestrate --mode parallel|dag`.
- #20: multi-agent backends. Codex, OpenCode, Grok and Cursor shipped; Gemini is not on the roadmap.
- #33: `/btw`, which is now a native Claude Code command, plus realtime steering for prompts added mid-turn.

**Closed as superseded or not planned (15):**
- #42: done items coming back. The old `pending` component and the write path it describes no longer exist, and convergence now explicitly never brings back a deleted item. The comment says to reopen if it happens again.
- #18: several editors sharing one tmux session. Lazily editor replicas already allow this.
- #32: caching `find_code_ranges`. The issue itself rated this under a millisecond, and nothing has measured it as a cost.
- #29: streaming raw stdout into the document. That conflicts with the rule that partial output never enters the document.
- #28: the "document modes" blog post. That vocabulary is deprecated.
- #27: the capture-to-log workflow. The response ledger and the ops log now cover it.
- #25: a direct HTTP API backend. agent-doc now runs through harnesses instead.
- #24: Docker sandboxing. The harnesses own sandboxing.
- #23: sub-profiles for submodules. Project-root resolution plus the cross-repo guard cover this.
- #22: a Ratatui dashboard.
- #34: ClawTeam.
- #35, #36, #37: MDX, Jupyter and Notion support. None is on the Patchboard-first roadmap.
- #26: `init --dashboard`.

**Still relevant, left open and tracked here:**
- `#ghbrew` (#31): the Homebrew formula. The tap repo has only `chromium-bridge.rb`. This is work an agent can do.
- `#ghjbfolding` (#19): JetBrains folding, gutter icons and a structure view.
- `#ghmultitmux` (#17): one editor driving several tmux sessions.
- `#ghrecvpolicy` (#40): a policy letting the recipient decide which components senders may create. It went to the **icebox**, not the backlog, because it only matters once multi-user workspaces exist.

All three backlog items went to the bottom. This document's backlog does not feed the queue, so none of them will start until you queue them.

**Not touched:** your open issues in other projects (opencv, uv, ray, ZigBrains, pypi/support#12214, and so on). Those are reports to other maintainers, not agent-doc work. pypi/support#12214 is already tracked as `#pypiquotarecheck`.

### Re: Stop skipping my release item, and release — opus-5.5 · 2026-10-01T11:15-04:00

> **Queue prompt:** :pin: #release

**0.35.428 is tagged and pushed (`v0.35.428`, commit `3fb2e90a7`), and CI is publishing it.** Other machines can install it once the GitHub Release workflow finishes, around 11:26 EDT (about 25 minutes per release; started 11:01). PyPI publishes right after. Until then `agent-doc upgrade` / `pip install agent-doc` still get 0.35.427. This machine already runs 0.35.428 (`make install-full`).

**Why `#release` kept getting ⏭️ (fixed in this release, `#releaseskipblame`):** the queue-skip rule marks a head ⏭️ when the previous turn ran it and it came back unfinished. But the previous cycle's record holds *every* live queue line, and "the head that ran" was taken to be the first tracked line that isn't a preset. Every `#gh-fix <url>` turn is a preset, so the rule walked past the line that actually ran and blamed `:pin: #release`. It was re-skipped on every turn this session. The fix:
- The head that ran is now the one carrying `🚧`.
- If that head is a preset or free text, nothing is skipped.

There are two regression tests, and both fail against the old rule. I checked that by temporarily restoring the old rule.

**Release contents** (`VERSIONS.md`):
- The skip fix above.
- Deleting a ⏭️ by hand now un-skips the head.
- Queue lines with no priority hold their slot.
- GH #78: preflight overrun refusals name the measured phase.
- GH #77: the JetBrains Remote Dev split layout (plugin 0.2.455).
- Recycle-settle retry.
- The stale-plugin message reads the on-disk install.
- External-disk candidates settle by intent id.

**Verification:**
- `make check`: exit 0, 10286 passed. The first 0.35.428 run failed only because the new version had no editor-parity audit row; I added it to `editors/release-parity.tsv`.
- `make install-full`: OK.
- The superproject submodule pointer is pushed (`55e8e59631`).

**Why it took long:** about 15 minutes went to `make release` re-running the full check and a second fat-LTO `install-full` after I had already run both. The rest is CI's roughly 25-minute four-platform build.

### Re: GH #79, and should `queue: stop` go away? — opus-5.5 · 2026-10-01T12:35-04:00

> **Queue prompt:** do https://github.com/btakita/agent-doc/issues/79

**#79 is fixed and closed** (commit `1bfd97afe`, pushed and installed).
- **The defect:** preflight treated a marker `go` beside a stale `queue: stop` as active, because the marker wins (`#qactsync`). But the supervisor's drainability check and the continuation detector read the raw frontmatter and saw `stop`. The document was active with 0 drainable heads, so nothing ever dispatched.
- **The fix:** both now read the same predicate as preflight, `control_binding::queue_control_activation`, which I moved out of preflight-io. The continuation detector also no longer needs the legacy `queue_active: true`.
- **Tests:** a regression for the document in the issue, and a matrix asserting that drainability agrees with preflight's activation control. Both fail against the old gate.
- **Other tests changed:** five fixtures modelled a halt or drain as a frontmatter flip with the marker `go` left in place. A real halt strips that token, so I updated the fixtures to strip it too. With a single authority, a leftover `go` means go.
- **One change I held back:** a queue with **no** control on either surface defaults to `go` (`#queuegodefault`). Preflight treats such a queue as active, but the idle watch and the Codex Stop hook still don't auto-drain it, because these two readers require an explicit control. Changing that would make every queue without a control auto-drain, a behavior change #79 doesn't ask for.
- **Verification:** `make check` passed, exit 0, 10288 tests.

**Your question: should `queue: stop` go away, given a queue defaults to `go`?** Mostly yes. Here is how I'd split it:
- **Drain (queue empty): stop writing `stop`.** Clear the control on both surfaces instead. An empty queue has nothing to run, so the default `go` is harmless there. When you add a head later, it just runs. That removes the stale-`stop` state that caused #79 in the first place.
- **Halt (heads left that the agent couldn't finish): keep a hold, but make it a typed one.** Without a hold, the default `go` would re-dispatch the head the turn just gave up on. Use `pause`, or a binary-owned `halted` state that editing the queue re-arms (`#queueeditgo`).
- **Operator `stop`/`pause` tokens on the marker: keep them.** They are how you hold the queue on purpose.
- **Decide together:** whether a queue with no control drains through the supervisor. That is the case I held back above, and it is the same question from the other direction.

This changes what the drain writes and how the supervisor behaves, so I've recorded it as `#queuestopretire` (`[decision]`) in the backlog instead of doing it now. Tell me which way to go on the no-control drain and I'll implement it.
<!-- agent:boundary:4df2c95c:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 do https://github.com/btakita/agent-doc/issues/79
- do https://github.com/btakita/agent-doc/issues/80
- do https://github.com/btakita/agent-doc/issues/81
- do https://github.com/btakita/agent-doc/issues/82
- do https://github.com/btakita/agent-doc/issues/83
- do https://github.com/btakita/agent-doc/issues/84
- When I was adding `'#release': release + publish` into prompt_presets, agent-doc reverted my change.
- release + publish
<!-- /agent:queue -->


<!-- agent:backlog priority -->
- [ ] [#queuestopretire] [agent-doc][decision] Retire the binary-written queue: stop now that a queue with no control defaults to go (operator question, 2026-10-01). Proposal: (1) a DRAIN (no live heads left) clears the control on both surfaces instead of writing stop; (2) a HALT with heads left writes a typed hold instead of a bare stop (pause, or a binary-owned 'halted' that a queue edit re-arms); (3) decide whether a queue with no control drains through the supervisor idle watch and the Codex Stop hook. Today preflight treats it as active, but drainable_head_count and required_continuation require an explicit control (kept that way in the GH #79 fix to avoid a silent behavior change). Code: agent-doc-queue/src/control_binding.rs, agent-doc-queue/src/queue_continuation.rs, the drain/clear path in agent-doc-preflight-io/src/lib.rs.
- [ ] [#ghbrew] [agent-doc] Homebrew formula for agent-doc (GH #31, https://github.com/btakita/agent-doc/issues/31). The tap repo ~/work/btakita/homebrew-tap holds only chromium-bridge.rb today. Add Formula/agent-doc.rb pulling the per-platform GitHub Release archives (binary + cdylib together, GH #52) with SHA256SUMS, and add a release step that bumps the formula on each tag. Close #31 when brew install btakita/tap/agent-doc works.
- [ ] [#ghjbfolding] [agent-doc][jetbrains] JetBrains plugin UX phases 5-7 (GH #19, https://github.com/btakita/agent-doc/issues/19): fold agent:* component regions, add gutter icons on component markers, and a structure view of components and ### Re: sections. Close #19 when shipped.
- [ ] [#ghmultitmux] [agent-doc] One editor managing panes across several tmux sessions (GH #17, https://github.com/btakita/agent-doc/issues/17), e.g. a session per topic or project: an allowed-sessions list in config, with routing and sync choosing among them. Close #17 when shipped.
- [ ] [#preflightdeadline] [agent-doc] Make a preflight admission overrun impossible by construction (GH #78 follow-up). Today every wait inside preflight has its own local bound (3s settle waits, per-call controller timeouts), but nothing caps their SUM against the hook's 90s budget. The #78 session-check profile ran resolve_current_document_content 13x on a 518-byte document. Fix: (1) carry an admission deadline (the budget minus a margin) on the per-run PreflightProgress handle, and clamp every bounded wait in preflight to the time remaining; (2) check the deadline at each progress::enter phase boundary, so preflight returns a typed, named, retryable refusal at a step boundary instead of being abandoned mid-step; (3) once the first live overrun names its phase (now logged as preflight_admission_overrun), move that work off the admission path, e.g. to the supervisor's idle boundary. Code: agent-doc-preflight-command-io/src/progress.rs, src/preflight_hook.rs, agent-doc-preflight-io/src/debounce.rs.
- [/] [#apimdrevert] [agent-doc][operator-verify] src/haiven-dev/tasks/api.md 'reverted by queue updates' (2026-09-29): forensics found NO lost operator text — disk/controller/editor converged at c9273a0b (HEAD 1b432940 + Codex 00:49:49 queue stop+strike, applied as a 2038-byte CRDT delta against the converged base). Only editor-side anomaly: idea.1.log 00:40:02
6local-splice-batch 3 splices 34346->34137 chars, then 00:40:03 exact inverse back to 25509a1e (undo/redo or plugin-internal flip; no VFS reload/projection logged). Blocked on operator: which text was lost + when (JetBrains Local History on api.md). Then trace the dropping path and fix with regression coverage.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#drivecapturedsplice] [operator-verify] Captured-splice live proof (#activateinstalledjetbrai). LIVE RUN 2026-09-30 20:33Z (operator go-ahead): after harness fixes 01b1e30b7, the recipe drove IDEA end to end — edit one captured (editor_op_capture_proof operator_ops=37 shadow_replay=agreed, 20:33:01Z), canonical advanced independently via write --commit in owner pane %93 (20:33:02Z, editor acknowledged the target), edit two captured (operator_ops=36 agreed, 20:33:12Z); the final text holds every operator edit plus the advance. No text lost. The verifier still FAILS: its proving triple needs two source=captured-local-splice-recovery reads, and the plugin only takes that path (CrdtReplicaManager rebootstrapCanonicalAndForwardCapturedLocalEdit) when its replica is STALE against the canonical text. Healthy delivery reached the editor before edit two, so the recovery never had to run. DECISION NEEDED: (a) accept the healthy-path trail as the proof, extending verify-captured-splice-recovery with a capture/advance/capture triple, or (b) require the stale-replica race, making the recipe type edit two while the advance is still in flight. Aside: the scratch doc is under gitignored tmp/, so every advance ends commit_refused_ignored_path (write lands on disk, commit refused). Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically by a human reply on pypi/support#12214, or 'make pypi-quota-check' reporting a ceiling in reach. Rechecked 2026-09-30 16:25 EDT: 8.362 GiB of 10.0 GiB, 1.638 GiB headroom, 158 releases (0.35.113..0.35.424), post-cutover mean 34.7 KiB, ~49,485 further releases fit; VERDICT headroom fine. Nothing to act on or delete. pypi/support#12214 still open, 1 comment (github-actions bot), untouched since 2026-09-10T22:46:21Z. If the grant lands, record it and drop this item. Deletion stays reserved for a ceiling the check reports (next tranche 0.35.113..0.35.161, 2.491 GiB); verify any deletion from the authenticated /manage/project/agent-doc/ pages, never from pypi.org/pypi/agent-doc/json.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation. NEW BLOCKER 2026-09-30: grok 1.0.41 is now SIGNED OUT ('grok -p' -> 'Error: Not signed in'); operator runs 'grok login --device-code' (or sets XAI_API_KEY), and the free usage quota or the planned subscription must be active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed earlier; the last live MCP probe stopped at the usage limit before committed closeout. Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ghrecvpolicy] [agent-doc] Recipient-side [receive] auto_create_components policy for multi-user workspaces (GH #40, https://github.com/btakita/agent-doc/issues/40). Parked until multi-user workspace support exists.
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
