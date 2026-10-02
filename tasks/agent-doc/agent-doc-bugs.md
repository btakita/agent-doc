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

### Re: GH #80 — restart-free JetBrains upgrade fails linking `UnloadPluginOptions` — opus-5.5 · 2026-10-01T13:16-04:00

> **Queue prompt:** do https://github.com/btakita/agent-doc/issues/80

**Fixed and installed.** agent-doc `743c97f83` + `c8da0ef1d` (plugin 0.2.457), pushed to `main`. I commented the root cause and verification on the issue; it stays open until someone confirms the fix on IU-262.

**Root cause.** The error `com/intellij/ide/plugins/DynamicPlugins$UnloadPluginOptions` was a `NoClassDefFoundError`. `JetBrainsPluginUpgradeAction.run()` re-wrapped it as `IllegalStateException(message)`, which dropped the class name. The class did **not** move in 262. I pulled the published `ide-impl` 262.9437.185 module and checked: the nested class has the same no-arg constructor and `withDisable` / `withUpdate`. What changed is which classloader can see it. The attach bootstrap loads the action child-first, with the IDE's system classloader as parent. On 262 that loader resolves `DynamicPlugins` but not its nested class, so the compile-time `new` failed before any unload ran.

**Fix:**
- `updateUnloadOptions` resolves the nested class through the loader that defined `DynamicPlugins`, then the outgoing plugin's classloader. It builds the update-mode options reflectively (`disable=false`, `update=true`). If neither loader can see the class, it fails closed with a message naming the build and the loaders tried.
- A failure in agent-doc's upgrader and a refusal by the IDE are now reported differently (issue point 2):
  - An upgrader failure keeps its exception class and is prefixed `agent-doc upgrader could not call the platform`.
  - A real `unloadPlugin` refusal is prefixed `plugin cannot unload dynamically`.
  - `plugin.rs` says "the IDE refused" only for a real refusal. Otherwise it says the upgrade "failed before the IDE could accept or refuse it".
- Regression tests:
  - `JetBrainsPluginUpgradeOptionsLoaderTest` loads the owner class in an isolated loader. I mutation-checked it: switching resolution back to the system loader makes it fail.
  - A new Rust test covers the refused-vs-failed wording.
  - The source-shape assertion in `PluginLifecycleListenerTest` now matches the reflective form.

**Verification:**
- `make check` passes: exit 0, 10289/10289 Rust tests, 606/606 JetBrains tests.
- `make install` upgraded the live IntelliJ 2026.1 through the new path with no restart. 15 documents were still re-registering their replicas on the editor's own retry schedule.
- I did not reproduce on IU-262 itself.
- The latest CI run was still **pending** at closeout.

**Follow-up.** Issue point 3 is filed as `jbstageonfail` [recommended]: stage the package instead of replacing jars under the live JVM when the hot-swap fails. With the dynamic path working, this only matters when the hot-swap fails.

### Re: GH #81 — layout still 1 pane after #77; focus lands in `1:stash` — opus-5.5 · 2026-10-01T13:58-04:00

> **Queue prompt:** do https://github.com/btakita/agent-doc/issues/81

**Fixed and installed: agent-doc `ca1ac2267` (plugin 0.2.458).** The issue stays open until I get evidence from IU-262. As you suggested, I posted a comment on #81 asking the reporter for exactly what I need.

**Probable mechanism.** It fits every symptom in the report, but I could not confirm it on IU-262. When the plugin detects no split layout, the route, the sync command, and the sync columns all fell back to one **joined** column of the visible documents: `--col 1109.md,agent-doc.md`. The controller keeps only the **first** agent document per column (`first_agent_doc_in_layout_column`). IntelliJ's `selectedFiles` lists the focused window's selection first. That produces every symptom in the report:
- `expected=[<focused doc>]`, flipping on each switch;
- `columns=1`;
- one pane whose occupant swaps;
- the sibling's pane left in `1:stash`, where focus then lands invisibly.

The surface path had the same gap. A surface with no columns produced an empty `Sync` layout, which the controller refuses (`desired pane layout is empty`).

**Fix:**
- **Fallback columns:** with no detected layout, each visible document is its own column. This applies to the plugin's `undetectedLayoutColumns` (route args, sync command, sync columns) and to the controller's `EditorSurface::sync_columns`. Visible documents are the selected tabs of distinct editor windows, so each one is a split. The surface signature already read an undetected layout this way.
- **Window bounds:** if a window's bounds can't be read, the layout degrades to window order (the #77 path) instead of detection failing.
- **Discriminator log:** the issue's suggestion, at info level and logged only when it changes: `[layout-detect] observed windows=N snapshots=[(x,y) file…] columns=K […]`.

**Verification:**
- `make check` passes: exit 0, 10290 Rust tests, 608/608 JetBrains tests.
- New regression tests: `SyncLayoutActionTest`, mutation-checked (restoring the joined column fails it); `LayoutDetectorTest` for the discriminator line; and an editor-surface `Sync` test.
- `make install` dynamically upgraded the live IntelliJ 2026.1 with no restart, which is a second live confirmation of the #80 fix.

**What remains.** On IU-262, two outcomes are possible. If the backend exposes **one** `EditorWindow` to plugins (`windows=1`), no plugin-side fix can see the client's split, and that is a deeper Remote Dev problem. The comment asks the reporter for the `observed windows=` line, the post-fix `pane_layout_projection` lines, and any `layout detection failed` line, which tells the two apart. The exchange is now about 200 lines; the queue keeps draining.

### Re: GH #82 — route refuses a document through its own pane, which runs a bare harness — opus-5.5 · 2026-10-01T13:56-04:00

> **Queue prompt:** do https://github.com/btakita/agent-doc/issues/82

**Fixed, installed, and closed.** The fix is agent-doc `9f79342f3`, pushed to `main`. I posted a comment explaining it and closed #82.

**Cause.** `pane_runs_other_document_owner` returns true both for a pane owned by a **different document** and for an **unmanaged bare harness** (`#bare-foreign-session-guard`). The dispatch-only route guard treated both as "it now runs another document". As a result, `1109.md` was refused through its own registry pane even though that pane held only `-zsh` + `claude`.

**Fix:**
- `pane_occupant_for_document` (sync-io) returns `Free`, `OtherDocument(doc)`, or `ForeignHarness`. The boolean predicate keeps its old meaning for other callers.
- **Another document:** the route still refuses. The message now names the owning document and the registry binding.
- **Bare harness:** the route releases this document's stale registry binding and any actor binding to that pane. It does this before associated-pane discovery reads them, then routes to a **new pane**. The operator's session is never dispatched into or reaped. The release is logged as `route_foreign_harness_pane_released`.
- **Not adopted: issue suggestion 2** (the registry outranks the pane). A managed pane always runs `agent-doc start <doc>`, so a pane with only a bare harness has no agent-doc session. Letting the registry win would type the prompt into the operator's own Claude session. I explained this on the issue.

**Verification:**
- `make check` passes: exit 0, 10292/10292.
- The new live-tmux classifier test passes, as do the route message tests.
- I had to update one architecture assertion in `tests/test_cli.rs`. The boolean `process_tree_owns_other_document` no longer has a call site in `sync.rs`; it is still required in `process-owner-io`.
- I installed with `make install`.
- The ignored tmux suite for sync-io and route-io fails 49 tests on a clean HEAD worktree too, so those failures predate this change. My change adds no new failures: the 4 tests that failed only on my run pass in isolation, twice.
- CI for `ca1ac2267` was still running at closeout.

The exchange is now about 230 lines; the queue keeps draining.

### Re: GH #83 — tmux renders at ~50% width with two differently-sized clients — opus-5.5 · 2026-10-01T14:08-04:00

> **Queue prompt:** do https://github.com/btakita/agent-doc/issues/83

**Done: agent-doc `c1a7667f0`, installed and pushed.** I explained the change on #83 and closed the issue.

**Diagnostic and warning** (issue suggestions 1 and 3):
- `agent-doc-tmux` has a new `TmuxClientGeometry` type. It records the clients, the effective `window-size`, and the window's size, and flags a mismatch when clients differ by more than 10 cells.
- `tmux-io` has a new `client_geometry` probe. It reads `list-clients`, `show-options -wAv window-size`, and the window's size.
- After each `pane_layout_projection` line, the controller logs `tmux_client_geometry … clients=N […] window_size=… window=WxH mismatch=…`. It logs whenever the geometry changes, and on every pass while the clients disagree. When they disagree, the line adds a warning and a remedy.
- I checked the probe's commands against a live tmux 3.7c.

**Not pinning `window-size`** (issue suggestion 2). agent-doc already removes per-window overrides on purpose, and `largest` would just crop the smaller client instead. The warning names `window-size largest` as a choice for the operator to make.

**The issue's open question is now answered.** agent-doc attaches a client only when the session has none:
- the IDE terminal returns `NOOP_EXTERNAL_ATTACHED` when a client is already attached;
- `agent-doc terminal` checks `is_session_attached` first.

So the second client was not agent-doc's.

**Verification:**
- `make check` passes: exit 0, 10295/10295.
- New unit tests cover the model (using the issue's exact rig numbers) and the probe.

The exchange is now about 250 lines; the queue keeps draining.

### Re: agent-doc#84 — superseded-but-attached editor refusal names a remedy — opus-5.5 · 2026-10-01T14:35-04:00

> **Queue prompt:** do https://github.com/btakita/agent-doc/issues/84

Fixed in agent-doc `21ad58344`, pushed. #84 is closed.

**Root cause:** if an editor is still attached but its replica can't be recovered, `resolve_editor_unavailable_disk_read_fallback` refuses to fall back to disk. It reported only the mechanism (`missing_replica recovery exhausted and disk read authority is refused`). It gave no pid, no jar and no action, so finalize and session-check dead-ended.

**Fix:**
- **One shared diagnosis:** the superseded-editor check (restart-required marker, `recorded_restart_verdict`, ladder remedy, `probe_superseded_editors`) moved into the leaf crate `agent-doc-fs::plugin_jar`. Preflight's `plugin_bytes_superseded` warning and the write path now use the same code. The warning text is unchanged.
- **Superseded editor:** the refusal now names the editor kind, pid and replaced jar. It derives the remedy from the #67/#76 ladder:
  - by default, re-run the install once (restart-free);
  - if an install already recorded that *this* process refused the dynamic update, restart the editor.
- **No superseded bytes:** the refusal names the holding editor and says to run `agent-doc admin reload-lib`, then retry.
- **Disk stays refused for a superseded editor.** Its JVM still owns a buffer that may hold unsaved operator text, so adopting the disk copy could drop it. The retry predicate still matches, because the existing message prefix is kept.
- **Not done here:** making the installer stage the update instead of unlinking live jars after a dynamic failure (the issue's suggestion 2) belongs to #80, which is still open.

**Verification:**
- 6 new regression tests: 3 in `fs::plugin_jar`, 3 in realtime-io.
- `make check` exit 0, 10301/10301 passed.
- `make install` done. CI run 36906135836 is still pending.

### Re: #jbstageonfail + fpe.md Codex Stop-hook overrun — opus-5.5 · 2026-10-01T14:39-04:00

> **Queue prompt:** do [#jbstageonfail]

**#jbstageonfail: done in `5c6df493f`, JetBrains plugin 0.2.459.**

- **What changed:** if the restart-free upgrade fails, the in-IDE upgrader now stages the package with the IDE's own `PluginInstaller.installAfterRestart` and returns `staged:<version>:<reason>`.
  - It finds the method by parameter types, because the descriptor argument has moved between first and last position across IDE builds.
  - It copies the package into the IDE's plugin temp directory first, because the launcher deletes its own copy on exit.
- **Installer:** a `staged:` result leaves the plugin tree alone, so the live jars are no longer deleted from under the running IDE. It still writes the restart-required marker that preflight reads. A process that does finish the hot-swap still takes precedence.
- **Tests:** 3 Java tests and 2 Rust tests.
- **Not tested live:** this install's hot-swap succeeded, so the staging path hasn't run against a real IDE yet.
- I posted on #80 that point 3 landed; the original upgrade failure in #80 is still open.

**fpe.md Stop hook over its 45s budget: fixed in `07bb0e667`.**

- **Cause:** it was my own `make install` from the #84 cycle, not anything in fpe.md.
  1. `binary-install` swapped the binary, and haiven-dev's controller restarted onto the new build (generation 2034 → 2035).
  2. `lib-install` then sent a plain `recycle` to all 9 controllers, including 2035, which was already on the new build.
  3. That pointless handoff sat in `Preparing` for its full 30s successor wait (18:21:19 → 18:21:49 UTC) and refused every `crdt_replica` call meanwhile.
  4. The Stop hook's `active_session_prompt_or_queue_head` waited 28.7s through that window, so the hook went over 45s.
- **Nothing was lost.** At 18:21:50 the hook reopened the cycle and saved your capture (`codex_stop_capture_saved`); the supervisor retained it from there. Typing directly into the Codex session was fine.
- **Fix:** install fan-outs now tag their recycle as `install_fanout`. A controller whose binary exactly matches the installed one declines it and logs `controller_recycle_skipped`. Unknown binary identity still recycles, and an explicit `admin recycle` always does.
- **Verified live:** this cycle's install reported "1 controller marked, 8 skipped", and haiven-dev logged `controller_recycle_skipped pid=818850 generation=2037 reason=install_fanout skip=binary_already_current`, with no handoff aborted.

**Verification:** `make check` exit 0, 10304/10304 passed; JetBrains Java tests 3/3; `make install` done.

### Re: #queuestopretire option 1 — a drain clears the queue control — opus-5.5 · 2026-10-01T14:59-04:00

> **Queue prompt:** [#queuestopretire]: do option 1

**Done: agent-doc `965257125`, pushed and installed.** The superproject pointer is `bbdae60efb`.

**What changed.** When the queue drains (no live heads left), preflight now removes the control from both surfaces instead of writing `queue: stop`. The frontmatter `queue:` line is removed, and the marker token was already stripped. The empty queue is idle. When you add a head later, it runs in the default `go` and writes `queue: go`. A drain can no longer leave the stale `stop` behind GH #79.

**What still writes `stop`:**
- **A halt with heads left** (a `stop` marker, or an `item_modified` halt). Without the hold, the default `go` would re-dispatch the head the turn gave up on. Typed holds are option 2, which I left for later.
- **An operator `pause`.** A drain keeps it.
- **A drain that held backlog items back.** I found this case while implementing, and my earlier answer missed it. A queue with no control resolves to `go`, and the next cycle mirrors the whole `queue`-attr backlog into it. So clearing the control after a `start` drain would have turned it into a continuous backlog loop. When backlog ids were held out of the queue this cycle, the drain keeps `queue: stop`.

**Tests:**
- New: a drain clears the control, a second pass is idle and writes nothing, and an added head re-arms to `go`.
- New: a `stop`-marker halt with heads left keeps `stop`.
- New: `clear_queue_control` removes both spellings but keeps `pause`.
- Extended: the held-backlog drain test now asserts `stop` is kept.
- Updated: three tests that asserted the old drain-writes-`stop` behavior.
- Spec: `specs/07` now describes the drain rule.

**Verification:** `make check`: exit 0, 10307 passed. `make install`: OK. CI for `965257125` was still running at closeout.

**Still open:** options 2 and 3 are now `#queueholdtyped`. Option 3 matters more now: every drained queue has no control, so a head you add later runs when you invoke the document, but the idle watch does not dispatch it on its own yet.

### Re: #followqueuestopretireopt — a queue with no control drains unattended — opus-5.5 · 2026-10-01T15:32-04:00

> **Queue prompt:** [#followqueuestopretireopt]: Yes, the queue should continue draining. No stalling.

**Done: agent-doc `026fb0e46`, pushed and installed.** The superproject pointer is `f2e3ee7938`.

**What changed (option 3).** A queue with no control on either surface now drains unattended, the same as an explicit `go`. Two readers used to skip it, and both now accept the default `go`:
- the supervisor idle watch's drainability count, and
- the continuation detector that the Codex Stop hook reads.

After `#queuestopretire`, a drained queue has no control. So a head you add to a finished queue now gets dispatched without you invoking the document. To hold a queue on purpose, write `pause` or `stop` on either surface. Those still hold.

**Option 2 needed no code.** A halt that leaves heads still writes `queue: stop`, but that already acts as the typed "halted" state I proposed. Editing the queue re-arms a `stop` to `go` (`#queueeditgo`). Only your `pause` survives a queue edit. So a halt doesn't stall: it waits for your next queue edit, then drains.

**Tests:**
- Flipped `control_less_queue_is_not_auto_drained` to `..._is_auto_drained`. It also checks that `pause`, `stop`, a marker `pause`, and the legacy `queue_active: false` all still hold the queue.
- New Stop-hook test: a control-less queue with a ready head blocks the turn and continues it.
- Two Stop-hook tests about the direct-chat writeback guard used a control-less queue to mean "a head the continuation doesn't drive". That shape now drains, so their fixture is now `queue: start` (supervisor-scoped), which keeps what they were testing.
- Updated `specs/07`.

**Verification:** `make check`: exit 0, 10308 passed. `make install`: OK. CI for `026fb0e46` was still running at closeout.

### Re: agent-doc reverted my `'#release'` preset edit, and the fpe.md queue handed back to you — opus-5.5 · 2026-10-01T15:52-04:00

> **Queue prompt:** When I was adding `'#release': release + publish` into prompt_presets, agent-doc reverted my change.

**Fixed: agent-doc `9195e04f7`, pushed and installed.** The superproject pointer is `532730b2fd`.

**What happened (ops.log, 16:15:26–27Z):**
- You pasted a copy of the `'#gh-fix': 'fix then close'` line, presumably to rename it. For a moment, `prompt_presets` held the same key twice.
- One second later, queue maintenance wrote the `queue:` control. Frontmatter writes keep the bytes of every key they don't change, but that relies on the frontmatter parsing as YAML. The repeated key made it fail to parse, so the write fell back to rebuilding the whole frontmatter from the parsed struct.
- That dropped your pasted line and removed the quotes from every preset. The log shows it: the editor text was 19094 bytes with your paste, and the write produced 19061.
- Your next keystrokes then landed in the original line. That's why `'#gh-fix'` now reads `fix then closerelease`. **Please fix that value by hand.** I didn't edit your frontmatter on the live document.

**The fix:** when the frontmatter can't be parsed as one YAML value, the write still splits it into top-level keys. Each key's value from before and after the write is compared using the struct parse, which tolerates the repeated key. A key the write didn't change keeps your bytes verbatim, even if it doesn't parse. Full rebuilding now only happens for a repeated **top-level** key. Every agent-doc frontmatter writer goes through this path, so queue control, pipeline state, and session/resume ids are all covered.

**Test:** the frontmatter from your edit (the repeated `'#gh-fix'` line) passes through `merge_queue_state`, `merge_queue_control`, and `clear_queue_control`. It must keep the repeated line and the quotes byte-for-byte. Before the fix it reproduced the bug exactly.

**Your mid-turn report: the fpe.md queue was handed back to you.** Fixed in agent-doc `c04b1ffd9` (superproject `f9ae08a320`).
- **Cause:** the next fpe head was `[focused-cycle]`, and the in-session loop always leaves those to the supervisor. That supervisor's heartbeat was stale, so session-check printed `queue_continuation_required=false` and "queue STALLED — … re-trigger in a fresh session". Codex followed it.
- **Now:** when the supervisor can't drain such a head, the in-session loop drains it:
  - Preflight sets `queue_continuation_required` and prints a fallback guidance line.
  - Session-check prints `queue_continuation_required=true` with the next head, and records the continuation as pending. If a loop still stops there, the next preflight reports `queue_stall_detected`.
  - `supervisor_drain_unavailable` is now a recoverable outcome. Its next action is `drain_in_session_supervisor_unavailable`; it was `operator_retrigger_supervisor_unavailable`.
  - The skill text says to drain it and never to ask for a re-trigger.
  - If a recycle-yield is pending, that still wins, because the recycle is what makes the supervisor ready again.
- **Tests:**
  - The readiness test now requires an in-session message, with no STALLED and no re-trigger.
  - A new unit test covers the fallback head.
- **Not fixed yet:** I filed `#supstaleopencycle` for why that supervisor was stuck. It logged `supervisor_recycle_deferred_cycle_open stale=true harness_turn_live=false` about once a second for 15 minutes.

**Verification:** `make check` after each fix: exit 0, 10309 passed. `make install`: OK both times.

The exchange is now 360+ lines. The queue is still active, so I'm continuing to drain it.

### Re: do [#supstaleopencycle] — opus-5.5 · 2026-10-01T16:05-04:00

> **Queue prompt:** do [#supstaleopencycle]

**Fixed: agent-doc `c7564f69d`, pushed and installed.** The superproject pointer is `e9983bc4d3`.

**Root cause: the fpe.md supervisor was not stuck. Its log line was wrong.**
- The supervisor dispatched `agent-doc fpe.md` into Codex pane %51 at 18:51:08Z. Preflight opened `cycle-1790880699535` at 18:51:39Z.
- The Codex rollout shows that turn was **live the whole time**. It worked on `#removelegacymany` until 19:25:39Z: tests, benchmarks, live conformance stacks, and PR #167. A long work turn stays in `preflight_started` until it captures a response, so the cycle was open for a real reason.
- The supervisor reads the turn marker only on 1 tick in 30, when it tries the reclaim. **Each of those ticks logged `harness_turn_live=true`**, so the live-turn veto (`#reclaimliveturn`) correctly blocked the reclaim. The other 29 ticks never read the marker, but they still printed `harness_turn_live=false`. Those were the 1135 lines the backlog item quoted.
- The recycle then happened as designed: at 19:25:52Z, the first boundary after the turn ended, `supervisor_binary_stale_self_recycled`. The interlock didn't need to change.

**The fix (telemetry only):**
- A tick that didn't read the marker now prints `harness_turn_live=unprobed`, not `false`.
- The receipt is logged only on ticks that read the marker. The first tick of each deferral is always one of them, so a deferral is never silent. That's about 30× fewer lines.
- The receipt now has a `deferrals=N` count, so a long deferral is still easy to see.

**Test:** `cycle_open_deferral_receipt_never_reports_an_unprobed_turn_as_idle` checks the three labels and the logging gate. It also checks that the first tick of an episode is probed, and that the receipt is wired through both helpers.

**Verification:** `make check`: exit 0, 10311 passed. `make install`: OK. CI for `c7564f69d` was still running at closeout.

The queue is `queue: start`, so the supervisor drains the next head (`#gh-fix` #85).

### Re: #gh-fix GH #85 — the Stop hook's continuation led with a known no-op — opus-5.5 · 2026-10-01T16:22-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/85

**Fixed and closed: agent-doc `728c98fc6`, pushed and installed.** The superproject pointer is `707f8c3f62`. I commented on #85 with the details.

**The problem:** whenever another queue item was ready, the Claude Code Stop hook told the agent to "Invoke the `loop` skill now". Once `/loop` is loaded, that Skill call does nothing. The instruction that actually works, scheduling a wake-up that submits `/loop agent-doc <FILE>`, appeared only later, inside the explanation of why the first step fails. The message also opened like a report, and Claude Code shows any blocking Stop reason as `Stop hook error:`.

**The fix:**
- The hook already reads the end of the transcript. It now also checks whether `/loop` is loaded: either you typed `/loop`, or the agent made a `loop` Skill call.
- **Once `/loop` is loaded,** the message leads with "Re-enter now: schedule a wake-up that SUBMITS `/loop agent-doc <FILE>`" (`ScheduleWakeup`). It also says not to invoke the Skill again, and explains why.
- **On the first iteration,** it still leads with the Skill call, which works then and needs no wake-up delay. The scheduled re-entry is named as the fallback.
- Both versions now open with "Queue continuation, not an error:".
- The bans on shell-running the trigger and on shelling `agent-doc preflight` are unchanged.

**Tests:**
- `loaded_loop_continuation_leads_with_the_scheduled_reentry`: once `/loop` is loaded, the wake-up comes before any mention of the Skill.
- `transcript_detects_an_already_loaded_loop_skill`: a typed `/loop` counts in both string and block form, and so does a `loop` Skill call. Another skill, or quoted text from the assistant, does not count.
- The existing content test now covers both versions.

**Verification:** `make check`: exit 0, 10313 passed. `make install`: OK.

**About the `#gh-fix` preset:** its value still reads `fix then closerelease`. That's leftover from the earlier frontmatter rebuild, and you can fix it by hand. I read it as "fix then close". The release is its own queue item.

The exchange is now 410+ lines. The queue is still active.
> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/86

### Re: GH #86 — welded backlog response + route remedy — opus-5-5 · 2026-10-01T16:45-04:00

Fixed and released **agent-doc 0.35.429** (`95feade0b`, tag `v0.35.429`). GitHub closed GH #86 through the commit.

**1. Response welded into a backlog row.** Commit was right to refuse this document. The bug was that nothing could repair it, so the open cycle never closed.
- New `repair_response_welded_inside_backlog_item` (agent-doc-template). It moves the welded turn into `agent:exchange` at the boundary without changing any bytes, or drops it when `exchange` already holds the same turn.
- The row goes back on its own line. If an exact twin of the row follows the weld, the welded copy is dropped instead.
- It refuses ambiguous shapes: a second scaffold, a prefix that is not a row, or a weld inside code or a comment.
- `agent-doc repair` and preflight run it before the live-steering guard. That is safe because it moves only binary-authored bytes. Compact runs it before its integrity gate.
- The commit refusal now names `agent-doc repair <FILE>` whenever this repair applies. That is the escape hatch the issue's item 2 asked for.

**2. Route guidance (items 3–4).**
- A blocked decision now leads with the caller's blocker reason, but only when that reason names a runnable `agent-doc` command. This keeps the existing rule that low-level capture text stays out of route output.
- `blocked_closeout_recovery_command` no longer falls back to prose, so a sentence is never reported as `recovery_command=`.
- Open-cycle text no longer says "keep the new route queued". A route that fails closed now says it was NOT queued.

**Not fixed:** how the weld happened in the first place. Every target the controller writes already passes this same guard before relay, so the weld most likely came from an editor-side CRDT interleave. Finding it needs the reporter's document or ops.log, which live on a remote workspace. For now the weld is repairable instead of terminal.

**Verification:** `make check` passed (10321 tests, exit 0). New tests: 5 for the template repair and 3 for closeout guidance. `make install-full` → `agent-doc 0.35.429`. Pushed main, the tag, and the superproject bump (`36a46c8374`). CI, Release, and PyPI runs are queued and were not awaited.

The next queue head, `release + publish`, is covered by this release.

### Re: complete GH #80, plus the prose `### Re:` split repair — opus-5 · 2026-10-01T17:14-04:00

> **Queue prompt:** complete https://github.com/btakita/agent-doc/issues/80

**GH #80: closed.** Both fixes shipped in **v0.35.429**, which is published with JetBrains plugin 0.2.459:
- 743c97f83: the restart-free upgrade builds `UnloadPluginOptions` through the `DynamicPlugins` loader.
- 5c6df493f: a failed hot-swap stages the package for the next IDE start instead of unlinking the live jars.

The closing comment asks the reporter to run `agent-doc plugin update jetbrains` on IU-262 and to reopen the issue if it still fails. I have not reproduced the bug on IU-262.

**The corrupted `### Re:` heading: source fixed and a repair added** (`7155446bd`, superproject `71e288bd69`). The source was my own 0.35.429 GH #86 repair. It treated *any* `### Re:` inside a backlog row as a welded response. So it moved the tail of `#ghjbfolding` ("…components and ### Re: sections. Close #19 when shipped.") into the exchange as an empty heading. After that, every turn was refused.
- The welded-response repair now also requires the `> **Queue prompt:**` quote that a real weld always carries. A row that only mentions `### Re:` is left alone.
- New `repair_prose_response_heading_split_from_tracked_row` undoes the split, using committed HEAD as evidence. It applies only when exactly one empty heading sits at the end of the exchange and rejoining it to exactly one current row rebuilds a HEAD row. Text typed after the boundary is kept, and every other shape stays fail-closed. `agent-doc repair` runs it on every preflight, before the step that misread the heading as a hand-pasted response.
- Checked byte-for-byte against the real corrupted document: the repair restores HEAD exactly. It does nothing on the half-fixed state left after the heading was deleted. This cycle restores `#ghjbfolding`'s tail with "response sections" in place of the literal `### Re:`, because live sessions on the old binary would misread it until they recycle.
- Also added the missing 0.35.429 row to `editors/release-parity.tsv`. Without it, `make check` was already failing at HEAD.

**Verification:** `make check` passed (exit 0, 10325/10325 tests) and `make install` passed. Both pushes are pushed. Both CI runs were still in progress at closeout and were not awaited.

The exchange is now 461 lines; this queue does not auto-compact.

### Re: GH #86 remaining issue, GH #81 unblock, and the preflight wedge — opus-5.5 · 2026-10-01T17:59-04:00

> **Queue prompt:** Look at https://github.Look at https://github.com/btakita/agent-doc/issues/86 response. Fix the remaining issues.com/btakita/agent-doc/issues/86 response. Fix the remaining issues. complete https://github.com/btakita/agent-doc/issues/81

This queue line came out spliced, so I read it as two requests: fix what remains in GH #86, and complete GH #81.

**GH #86: the remaining issue is fixed** (`579f15cd7`). The reporter's one open point was that the write that produced the weld still accepted it; only commit refused it later. The response-cell write computes its own canonical target, and it was the one target applied without the semantic guard every other canonical target passes. `apply_response_cell_on_hub` now checks its own target before applying it:
- If the write introduced a weld, the same repair `agent-doc repair` uses moves the response back into `exchange` first.
- If that repair can't prove a safe placement, the write is refused and the document is left unchanged.
- A weld that already existed before the write still goes to the commit-side remedy.

The regression test fails when the guard is disabled. I posted a follow-up comment on #86, which was already closed.

**GH #81: unblocked, still waiting on the reporter.** Their last comment said they couldn't test because `ca1ac2267` wasn't in a tagged release. It is now in **v0.35.429**, which ships JetBrains plugin 0.2.459. I commented to tell them, and repeated that `windows=N` from `[layout-detect]` is the measurement that decides it. The issue stays open until their IU-262 measurement comes back.

**Earlier in this session I also fixed the cause of the preflight refusals** (`e531a5c4f`):
- **Root cause:** after the dynamic plugin reload, the IDE rejected a notify. The controller dropped its editor copy from delivery, but the plugin kept pulling and got `updates=0` forever. The copy never received the GH #80 response, the save that settles the write never came, and preflight refused every turn. The same thing happened to `fpe.md`.
- **Fix:** a pull from a dropped editor copy now gets the `missing_replica` refusal. The plugin re-registers on that refusal and receives everything it missed.
- **Second fix:** `session-check` no longer relays committed (HEAD) response text as "operator steering".
- **Still unproven:** why the IDE rejected the controller's messages. The plugin rejects without logging which check failed. That diagnosis is new backlog item `jbrejectlog`.

**Verification:** `make check` passed (exit 0, 10328/10328 tests) and `make install` passed. Both commits are pushed. CI was still in progress at closeout and was not awaited.

The exchange is now 481 lines; the queue does not auto-compact.

### Re: do [#jbrejectlog] — opus-5.5 · 2026-10-01T18:12-04:00

> **Queue prompt:**
>
> do [#jbrejectlog]


**Done.** The JetBrains plugin now logs why it rejects a `deliver_crdt_remote` event. Commit `348160fb0`, plugin 0.2.460, installed and pushed.

**What was silent before.** The controller only gets a bare `rejected` receipt. On four paths the plugin returned that receipt without writing anything to `idea.log`:

- `file` missing.
- `editor_id` missing, or different from this editor's id.
- The recovery re-register could not find the replica manager, the file, or its document. When the manager was missing there was not even a logger to write with.
- A socket message had no `type`.

**Now each rejection writes one line:**

- `[socket] deliver_crdt_remote rejected cause=missing_file|missing_editor_id|editor_id_mismatch file=… reason=… editor_id=… this_editor_id=…`
- `[socket] deliver_crdt_remote rejected cause=reregister_not_attached`
- `[crdt-replica] recovery re-register rejected … cause=project_disposed|no_replica_manager|file_not_found|no_document`

**Likely cause of the 2026-10-01 rejection (not yet proven).** `EditorIdentity.id` is a random UUID created inside a Kotlin `object`. A dynamic plugin reload loads that class again in a new classloader, so the id changes. A controller still addressing the old id would then be rejected with no log line. That matches what happened after the reload. The new `editor_id_mismatch` line prints both ids, so the next time this happens the log will confirm or rule it out. Installing this build was itself a dynamic upgrade, and a few seconds afterwards `idea.log` held no rejection lines.

**Verification:**
- New tests cover both reason functions, and the existing source check now also requires that no silent `editor_id` early return remains.
- `make check` passed (10328 tests, 0 failed).
- `make install` hot-swapped the plugin into the running IDE.
- CI run `36933407786` is still pending.

The exchange is now 506 lines; I'm continuing the queue rather than stopping to compact.

### Re: #gh-fix GH #87 + the sdk.md controller timeout — opus-5.5 · 2026-10-01T18:39-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/87

**GH #87: fixed and closed** (`b5239609c`). The `stale_plugin` warning now checks whether the expected build is already staged for the next IDE start. It does that before it says "a restart alone cannot reach parity". It reads two sources:

- IntelliJ's own pending-install queue: `unzip:` lines in `<system>/plugins/action.script` that name `agent-doc-jetbrains-<expected>.zip`.
- agent-doc's restart-required marker. A staged install now records `staged_version=<v>` there.

When either source shows the expected version, the warning says the build is already staged and that the fix is to restart the IDE, not to install again. The other messages are unchanged. New tests cover the parser, the marker round-trip, and both message entry points.

**sdk.md "timed out after 5.0s waiting for project controller response":**

- **What you saw.** At 18:21:18 local, the IDE's "Run Agent Doc" route on sdk.md failed. Its `route_document_prep` call to the controller timed out.
- **The stall window.** From 22:20:43 to 22:21:44Z, every state-event call from the idle watch, session-check, captured finalize and route timed out at 5 s. Calls to the CRDT relay kept succeeding.
- **What was happening at the same time:**
  - A retained editor save stayed unsettled for those 70 s. The IDE refused `persist_current` four times with `visible_length_mismatch` while you typed, then with `document_lane_timeout`.
  - sdk.md was re-registered with a 974 KB full bootstrap for a 12 KB document.
  - The stall ended at 22:21:44, the moment that save settled. The second route attempt then dispatched and committed.
- **Not yet proven.** Each state-event call waits on one controller lock (`state_event_ingress`). Nothing logged how long that lock was held, so the logs can't show what held it. I didn't want to ship a guessed fix. Two commits instead:
  - `80d82d4ad`: every holder of that lock now logs `state_event_ingress_slow site=… fact=… wait_ms=… hold_ms=…` to ops.log once a wait or hold reaches 1 s. The next stall will name its holder. That follow-up is backlog item `#sdkingressstall`, which also covers haiven-dev's 305 MB `reliable_sync_liveness_journal`.
  - `f66059470` fixes the cause that the new `#jbrejectlog` logging confirmed today: `cause=editor_id_mismatch` for IDE pid 2960722. A dynamic plugin reload rebuilt `EditorIdentity` and gave the same IDE a new id, while the controller kept using the old one. That is the IDE involved here, and its re-register churn overlapped the stall. The id and the `:refresh-N` counter are now kept for the life of the IDE process. Plugin 0.2.461.

**Verification:**
- `make check` passed (10,333 tests) and the new Kotlin tests pass.
- `make install` hot-swapped plugin 0.2.461 into the IDE.
- Pushed; CI run `36936340958` is queued.
- This upgrade was the last one that changes the editor id, because the old build never saved it.

The exchange is over 500 lines; I'm still draining the queue rather than stopping to compact it.
### Re: #gh-fix GH #88 + follow-ups on the stale plugin callback — opus-5.5 · 2026-10-01T19:04-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/88

**GH #88: fixed and closed** (`fb65d4557`, plugin 0.2.462). A JetBrains Remote Dev backend exposes **no** editor windows, because its editor UI lives in the thin client. Every automatic path enumerated `FileEditorManagerEx.windows`:

- **Layout:** with `windows=0` the plugin published `columns=0`, so tmux stayed at one pane. Manual Sync Tmux Panes worked because it reads `FileEditorManager.selectedFiles`, which still lists one file per visible split. The automatic capture and `LayoutDetector` now use that same fallback when the IDE has no editor windows, giving one column per selected session document.
- **Focus:** with no editor windows, `currentWindow` is always null, so every selection used to fall through to "background split" and the pane never followed. With no editor windows, a selection now owns focus.
- **Observability:** each selection now logs its focus decision at INFO. The `[layout-detect] observed` line also carries `source=` and `obs=N` and re-logs every 50th unchanged observation, so "never ran" and "stuck" now look different.
- **Not verified on Remote Dev:** I tested on this desktop IDE only. If it still fails there, the new INFO lines show where. I asked the reporter to reopen the issue with them.

**Fix landed after the last response was written:** `0e491da01`. A dynamic plugin reload asked the native library to start its message listener while the old one was still running. That call was a no-op, so the retired plugin's code kept answering the IDE's socket. That is what rejected every delivery with `editor_id_mismatch` and held the previous closeout until the native reload. Now the newest plugin takes over the running listener. A regression test fails without the fix.

**Verified by this cycle's 0.2.462 reload**, the first one with both fixes active:
- Mismatches stopped within one second (19:00:59 to 19:01:00), where before they went on for minutes.
- The plugin now logs `[layout-sync] selection … authority=ActiveEditorSplit windows=2` on selection.
- The few rejections left were addressed to ids from 0.2.455, hours old. The controller never pruned those registrations. I queued that as `#staleeditorregprune`, behind the release.

**Verification:** 618 Kotlin tests pass; `make check` passed (10,335 tests); installed and hot-swapped with no restart needed; pushed.

The exchange is now 569 lines; I'm continuing the queue rather than stopping to compact.

### Re: #gh-fix GH #89, the stale 🚧, and the Stop-hook block — opus-5.5 · 2026-10-01T20:18-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/89

**GH #89: fixed and closed** (`d00b412c9`). After a rename, the route refused the one pane the registry binds the renamed document to. The pane's supervisor argv still named the old path, and argv is fixed when the process starts. The occupant check now treats that argv document as a stale spawn-time name when two things hold: the registry binds the routed document to the pane, and no entry still binds the argv document to it. The pane then reads as free, logged as `pane_occupant_stale_argv_identity`. A pane that is still bound to another document keeps the cross-document guard. The fix sits in `pane_occupant_for_document`, so route, the controller's orphan drain and sync's layout reuse all get it. Unit tests cover the rename case, a pane still bound to the argv document, and a pane with no binding.

**Two fixes from the earlier turns.** Neither of those turns reached this document, so they are recorded here.

- **Stale `- 🚧 release + publish`** (`b90676808`). Queue maintenance put `🚧` on "release + publish" while it was the only live head. You then queued two heads above it. A baseline rebuilt from HEAD lost the marker, so preflight saw the binary's own `🚧` as newly added and treated it as you choosing that head. Selection stayed stuck on it. Cycle state now records the heads the binary marked itself (`projected_in_progress_queue_heads`), and a marker on one of those is never read as your choice. Moving the marker yourself is still honored. This cycle selected #89 correctly. The visible `🚧` should move on the next queue pass; if it doesn't, deleting it by hand is safe.
- **Stop-hook block** (`81034a801`). Two causes:
  - The stale-lock repair closed my abandoned cycle with no response, and your next trigger was refused. The Stop hook still blocked the final answer and ordered a `/loop` that could only hit the same refusal. A closed run with no captured response now lets the final answer through (`#stopnoresponserun`), so you see the refusal.
  - The hook's message said "Invoke the `loop` skill now". A Skill call never runs `UserPromptSubmit`, so it admits nothing on any iteration; I confirmed that on this run. The hook message, the `SKILL.md` auto-loop section and the harness runbook now give the re-entry that works: `ScheduleWakeup` submitting `/loop agent-doc <FILE>` (`#loopskillnoadmit`).
- **My mistake behind the first block:** I ended a turn while `make check` was still running in the background, which left its cycle open. I now wait for builds inside the turn.

**Verification:** `make check` passed after each fix (10341 tests on the last run). `make install` is done and everything is pushed. CI for these pushes is still running. Release stays with the queued `release + publish` head. Next head: `do [#staleeditorregprune]`.

The exchange is 590+ lines; I'm continuing the queue rather than stopping to compact.

### Re: do [#staleeditorregprune], #midinstallreloadstall, and why writes stopped reaching the file — opus-5.5 · 2026-10-01T20:57-04:00

> **Queue prompt:**
>
> do [#staleeditorregprune]
>
> do [#midinstallreloadstall]


**`#staleeditorregprune` — done** (`b6bbb617a`). When the IDE registers a document under a new editor id, the relay now also removes that IDE process's old-id registrations on every other document. Before, only the document being registered was cleaned up, so closed documents (`api.md`, `contracts.md`, `agent-loop.md`) kept dead ids, and every plugin reload was rejected against them as `editor_id_mismatch`. Each document is cleaned under its own lock, after the registering document's lock is released, so two registrations can't deadlock. Other editor processes are untouched. Tests cover the end-to-end prune and show other processes and the current document are spared. One caveat: under plain `cargo test`, which runs tests as threads of one process, the prune crosses into other tests that use the same pid. nextest (`make check`) and serial runs pass all 110 tests in the crate.

**`#midinstallreloadstall` — done** (`42e54fd97`). This was why nothing reached the file for about 50 minutes. A listener from an old library build (`c32820c7`) stayed alive inside the IDE and kept the editor's connection point. Each later reload saw a live listener and stepped aside (`ipc_listener_bind_refused_live_peer`, four times). Meanwhile the old listener turned away every newer build, including the reload message that could have replaced it, so only an IDE restart recovered. Now:
- A listener reporting a different build on the IDE's own connection point is replaced by the current one. A same-build duplicate is still refused.
- The build-mismatch error no longer swaps "listener" and "client" when the sending side reports it. That swap is why I first blamed the controller, and why the retry throttle was keyed on the wrong build.

After installing, the IDE reloaded cleanly, with no refusals or mismatches.

**Also fixed earlier, not yet recorded here** (`ac33d5b73`, `#retainedsteeringecho`): while my response was still being delivered, `session-check` reported its own paragraphs as your "operator steering". It now ignores text the binary itself wrote: the committed document plus the response still being delivered.

**Verification:** `make check` passed (10346 tests). Everything is installed and pushed.

**Next:** as you asked, the remaining heads run in parallel, each in its own git worktree with its own build: `#sdkingressstall` and the "active while Idea is Analyzing project" item. I'll merge them, run one full check and install, then do `release + publish` last, because it ships what they produce.

### Re: do [#sdkingressstall], the IDE-indexing menu block, and the Stop-hook "error" — opus-5.5 · 2026-10-01T21:31-04:00

> **Queue prompt:**
>
> do [#sdkingressstall]


> **Queue prompt:** agent-doc should be active while Idea is "Analyzing project...". Currently, agent-doc is blocked from showing the menu.

I ran these two in parallel, each in its own git worktree, then merged them onto `main`, ran one full check (`make check`, 10351 tests passed), installed and pushed.

**`#sdkingressstall` — done** (`0cb2d9655`). The new lock-timing log fired 16 times in haiven-dev's ops.log, every time while the controller was writing a state event. The hold times cluster on SQLite's own retry intervals (about 1.0, 1.1, 1.4 and 1.5 s). So the controller was holding its state-event lock while it waited for another connection's write lock on `state.db`. That other connection was the cleanup pass that trims old state events, which every new process runs on its first open of `state.db`. Each pass scanned for rows inside a single DELETE statement, so it held the write lock for the whole scan: up to 0.46 s on haiven-dev, even with nothing to delete. The slow lines cluster at install fan-outs, when several supervisors restart at once.
- **Fix:** the three cleanup passes now find their rows with a plain read and take the write lock only to delete them.
- **The 305 MB `reliable_sync_liveness_journal`:** it was append-only. Every editor heartbeat added a row, so it held 497,353 rows for 1,037 distinct facts. The controller now compacts it in the background, after startup and then every 2,048 appends. A row is dropped only once every fact in it has a newer replacement. On a copy of haiven-dev's database: 497,353 rows became 2,664, a cold reload went from 3.5 s to 19 ms, and the rebuilt state is identical.
- **Still unproven:** what held the lock in the original 22:20Z stall, because that window predates the new logging.
- **Tests:** each new test fails with its fix reverted.

**The IDE-indexing menu block — fixed** (`4372a2a4c`, plugin 0.2.465). Every agent-doc action was a plain IntelliJ action, and IntelliJ disables those while it indexes. That covered the Alt+Space popup, the Tools, editor and project-view menus, and every shortcut. The slash autocomplete and the turn banner were affected the same way. None of them use IntelliJ's indexes, so all 22 actions, the two popup groups, the autocomplete and the banner now stay available while the IDE indexes. A new test fails if any agent-doc action or group can't run during indexing. I haven't watched it in a live IDE yet, so an `[operator-verify]` check is worth doing during the next "Analyzing project...".

**The Stop-hook "error" — fixed** (`e669d79a6`). Claude Code labels every Stop-hook block "Stop hook error", whatever its text says. The hook now lets the turn end when a live, current supervisor will continue the queue and no earlier handoff to it is still waiting. If not, it still blocks and asks for the scheduled `/loop` re-entry. That case is now uncommon, so the queue no longer prints an error after every item.

**Next:** `release + publish`.

The exchange is 630+ lines; I'm continuing the queue rather than stopping to compact.

### Re: release + publish — opus-5.5 · 2026-10-01T22:08-04:00

> **Queue prompt:** release + publish

**v0.35.430 released.** The tag is pushed, and the GitHub Release and PyPI workflows are running; I'm not waiting on them. `make release` ran the full check (10351 tests passed), tagged, pushed, and ran the one-time `install-full`. The first attempt stopped before tagging because the editor-parity gate needs one `editors/release-parity.tsv` row per release. I added the 0.35.430 row and amended the unpushed release commit. `VERSIONS.md` covers everything since 0.35.429. Highlights: the superseded IPC listener takeover, the cross-document stale editor-id prune, state.db retention scans no longer holding the write lock plus liveness-journal compaction, the IDE-indexing menu fix (plugin 0.2.465), the Stop-hook and queue fixes, and GH #89.

**haiven-dev `sdk.md` (your mid-turn report).** It has recovered: the held write settled and a new cycle is running in its Codex pane. The cause is a new defect, and it isn't fixed yet:
- After a library reload re-registered the editor, the editor published 99 bytes of your unpublished text. The controller then moved to the agent write's target, which did not contain that insert.
- Your editor kept the text and correctly refused to save the target over it (14 refusals), so every cycle was refused until a controller restart reseeded from the editor.
- Your text was never lost.

I filed it as `#retainedtargetdropsedit` and it's being fixed now in a parallel worktree. I'll merge, check and install it when it reports back.

### Re: #gh-fix GH #90 and the re-queued release — opus-5.5 · 2026-10-01T22:25-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/90

**Deferred to a running subagent, tracked as `#gh90markercodespan`.** GH #90 is being fixed in its own worktree (branch `fix/gh90-marker-in-code-span`). It covers all five asks:
1. Marker detection that treats a marker inside inline backticks or a fenced block as prose, in both the replay guard and the structural target check.
2. Validating the response at write time, before it's captured.
3. Filtering editor registrations on whether their process is still alive before the plugin-version gate, so a dead pid is never named as the blocker.
4. Showing the real structural reason in the retry loop's report.
5. A recovery command for a captured response that can never land.

When the branch reports back I'll merge it, run one full check, install, and close the issue. I stopped a redundant #89 audit I had started from the earlier chat message, since #90 was the issue meant.

**Running in parallel**, each in its own worktree:
- `#retainedtargetdropsedit`
- the `lazily.md` queue revert
- the codex `/clear` report for `fpe.md`
- GH #90

The head `release + publish` stays queued until they land: it ships 0.35.431 with all of them.

**Also landed on `main` since 0.35.430** (`e2a36608d`): a background-task notification no longer hides a `/loop` re-entry the session already scheduled. That had been making notification turns re-block with a "Stop hook error".

The exchange is 670 lines; I'm continuing the queue rather than stopping to compact.

### Re: lazily.md queue revert (in progress) — opus-5.5 · 2026-10-01T22:32-04:00

> **Queue prompt:** Editing lazily.md `agent:queue` then Run Agent Doc: the queue reverted to the previous state. Then the edits came back as I was rewriting the queue items.

**Deferred to a running subagent, tracked as `#lazilyqueuerevert`.** It is being fixed in its own worktree (branch `fix/queue-revert-on-run`). The agent is reconstructing from the logs which writer projected the older queue over your edits when Run Agent Doc fired, and why the edits came back. It will then fix that writer with a regression test. It is on its final `make check` run.

The codex `/clear` head is tracked the same way, as `#codexclearnotproven` (branch `fix/codex-clear-pane-render`). `#retainedtargetdropsedit` and `#gh90markercodespan` are also in progress. I'll merge all four, run one full check, install, then release 0.35.431.
<!-- agent:boundary:1956bca7:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~do https://github.com/btakita/agent-doc/issues/79~~
- ~~do https://github.com/btakita/agent-doc/issues/80~~
- ~~do https://github.com/btakita/agent-doc/issues/81~~
- ~~do https://github.com/btakita/agent-doc/issues/82~~
- ~~do https://github.com/btakita/agent-doc/issues/83~~
- ~~do https://github.com/btakita/agent-doc/issues/84~~
- ~~do [#jbstageonfail]~~
- ~~[#queuestopretire]: do option 1~~
- ~~[#followqueuestopretireopt]: Yes, the queue should continue draining. No stalling.~~
- ~~When I was adding `'#release': release + publish` into prompt_presets, agent-doc reverted my change.~~
- ~~do [#supstaleopencycle]~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/85~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/86~~
- ~~Look at https://github.Look at https://github.com/btakita/agent-doc/issues/86 response. Fix the remaining issues.com/btakita/agent-doc/issues/86 response. Fix the remaining issues. complete https://github.com/btakita/agent-doc/issues/81~~
- ~~do [#jbrejectlog]~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/87~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/88~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/89~~
- ~~do [#staleeditorregprune]~~
- ~~do [#midinstallreloadstall]~~
- ~~do [#sdkingressstall]~~
- ~~release + publish~~
- ~~agent-doc should be active while Idea is "Analyzing project...". Currently, agent-doc is blocked from showing the menu.~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/90~~
- 🚧 Editing lazily.md `agent:queue` then Run Agent Doc: the queue reverted to the previous state. Then the edits came back as I was rewriting the queue items.
- Diagnose + fix agent-doc command failed (exit 1): Error: session_clear codex command `/clear` for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md was not proven submitted in pane %51 after supervisor_ipc_acceptance (result=pane_not_rendered, command_visible=false); treating Clear Session Context as not submitted. ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=wait_for_pane_render unblocker=wait_for_pane_render. The codex pane stayed blank while awaiting its repaint. No duplicate clear was sent; wait for the prompt to render and check its context
<!-- /agent:queue --> 


<!-- agent:backlog priority -->
- [ ] [#lazilyqueuerevert] lazily.md: editing agent:queue then Run Agent Doc reverted the queue to its previous state, then the edits reappeared while the operator was rewriting them. Being fixed in worktree branch fix/queue-revert-on-run; merge, check, install, then release.
- [ ] [#codexclearnotproven] haiven-dev fpe.md: Clear Session Context reported the codex /clear as not proven submitted in pane %51 (pane_not_rendered, command_visible=false) while the pane repainted blank. Being fixed in worktree branch fix/codex-clear-pane-render; merge, check, install, then release.
- [ ] [#gh90markercodespan] GH 90 (btakita/agent-doc#90): a response quoting a component marker inside inline backticks is captured, then refused forever (replay_guard substring test + structural_corruption_reason), while the retry loop blames a dead editor pid plugin_generation_mismatch. Being fixed in worktree branch fix/gh90-marker-in-code-span (all five asks); merge, check, install, close the issue, then release.
- [ ] [#retainedtargetdropsedit] haiven-dev sdk.md, 2026-10-02 01:49:58-01:55:36Z: after a native reload re-registered the editor, the editor published 99 bytes of unpublished operator text (buffer 9ff424), but the controller canonical moved to a retained agent-write target (468b48) that lacks that insert. The editor correctly refused 14 exact-target persists (visible_length_mismatch, visible 8849a8 = target + 99 bytes), its deferred replay failed post-register-replay-replica-raced every 30s, and every cycle was refused (disk_projection_pending) until a controller restart reseeded from the editor. Keep the concurrent operator edit when a retained write replays, and self-heal without a restart. Being fixed in worktree branch fix/retained-target-drops-operator-edit.
- [ ] [#ghbrew] [agent-doc] Homebrew formula for agent-doc (GH #31, https://github.com/btakita/agent-doc/issues/31). The tap repo ~/work/btakita/homebrew-tap holds only chromium-bridge.rb today. Add Formula/agent-doc.rb pulling the per-platform GitHub Release archives (binary + cdylib together, GH #52) with SHA256SUMS, and add a release step that bumps the formula on each tag. Close #31 when brew install btakita/tap/agent-doc works.
- [ ] [#ghjbfolding] [agent-doc][jetbrains] JetBrains plugin UX phases 5-7 (GH #19, https://github.com/btakita/agent-doc/issues/19): fold agent:* component regions, add gutter icons on component markers, and a structure view of components and
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
