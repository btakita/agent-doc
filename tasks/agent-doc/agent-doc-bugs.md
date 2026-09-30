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
  '#do-backlog': '/goal do backlog items'
  '#gh-fix': 'fix then close'
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260930-202410.md`*

Compacted content:
- Archived 24 response topic(s): Compact Exchange needed two tries (+ fpe.md stuck closeout); Fix https://github.com/btakita/agent-doc/issues/60 (+ fpe.md Stop hook); GH #61; 21 more

### Re: #advance-review — rechecked all 4 review gates + #apimdrevert; none cleared, no new backlog work — opus-5 · 2026-09-30T16:25-04:00

> **Queue prompt:** #advance-review

I rechecked every gate against live state. None has cleared, so there are no new backlog or queue items. Each item still waits on something only the operator can supply:

| Item | Recheck (2026-09-30) | What unblocks it |
|---|---|---|
| `#drivecapturedsplice` | The code blocker was fixed earlier (#retainedadvanceadmit), and the dry run passes. | Your go-ahead to run the live proof, which types real keystrokes on your desktop. |
| `#pypiquotarecheck` | `make pypi-quota-check`: 8.362 / 10.0 GiB used, 1.638 GiB headroom, 158 releases, about 49k more releases fit. VERDICT: fine, delete nothing. pypi/support#12214 is still open with only the bot comment, and hasn't changed since 2026-09-10. | A human reply on #12214, or the check reporting a ceiling. |
| `#grokmcpverify` | **New blocker:** grok 1.0.41 is now signed out (`grok -p` → `Not signed in`). | Run `! grok login --device-code`, and have quota or a subscription active. |
| `#coder7` | The `coder` CLI isn't installed on this host. | A workspace URL or SSH target, a `coder` install and login, and the plugins enabled on both remote IDE hosts. |
| `#apimdrevert` (gated, in backlog) | Unchanged. The forensics found no lost text. | You name the lost text and roughly when it was lost (JetBrains Local History on `api.md`). |

I edited the review text for `#pypiquotarecheck`, `#grokmcpverify` and `#coder7` with these results. I didn't run `agent-doc migrate`, so the `legacy_gated_in_backlog` warning for `#apimdrevert` stays. That command rewrites the live document's components; say the word and I'll run it.

### Re: Drive Idea for #drivecapturedsplice; #coder7 — live run done (edits kept, verifier still red); coder7 needs your call — opus-5 · 2026-09-30T16:35-04:00

> **Queue prompt:** Drive Idea to validate #drivecapturedsplice

> **Queue prompt:** #coder7

**`#drivecapturedsplice`: I drove IDEA live. No operator text was lost, but the verifier is still red.**

The first live attempts hit four defects in the test script itself. I fixed them in `01b1e30b7` (pushed; superproject `c537731de1`):
1. **Wrong-owner refusal:** a repo-wide check treated every agent-doc pane in the repo as the owner, so the script refused whenever any other document had a session. Ownership is per document, and a separate per-document check already handles it.
2. **Dead owner pane:** the script trusted an actor record whose pane had already been closed and reaped.
3. **Owner pane appears late:** opening the doc in IDEA creates its owner pane only after typing starts. The script now re-checks the owner right before the advance step and waits for it to go idle.
4. **Owner is a Claude agent, not a shell:** the scratch doc's owner pane runs Claude Code. The script now runs the advance there with Claude's `!` bash mode. One side effect: that agent then takes a turn to comment on the command's output.

Final run (20:33Z):
- Edit one was captured: 37 operator keystrokes, and the replay check agreed.
- The response then advanced independently. The write ran in owner pane %93, and the editor acknowledged the new text.
- Edit two was captured: 36 operator keystrokes, and the replay check agreed.
- The document ends with every operator edit plus the advance, so nothing was lost.

**Why the verifier still fails:** it requires two log entries from the plugin's recovery path. That path only runs when the editor's copy is out of date relative to the canonical text. Here the advance reached the editor before edit two, so no recovery was needed. **I need your decision:**
- (a) Count this normal-path evidence as the proof, and extend `verify-captured-splice-recovery` to accept "capture → advance → capture".
- (b) Require the stale-copy race, and change the script to type edit two while the advance is still in flight.

Side note: the scratch doc sits under gitignored `tmp/`. Every advance lands on disk, but its commit is refused (`commit_refused_ignored_path`).

**`#coder7`: blocked on how to get a Coder workspace.** This machine has no `coder` CLI, no Coder config, and no JetBrains Toolbox or Gateway. Codium and Docker are installed. Should I:
- use an existing Coder deployment (send me the URL and login details), or
- stand up a local Coder server in Docker, install JetBrains Gateway and a Codium remote-SSH extension, and run the five-case matrix myself?

I updated both review items with these results and the open decisions.

### Re: release + publish — 0.35.426 tagged; #coder7 closed; monsterrodholders.md stall fixed — opus-5 · 2026-09-30T17:23-04:00

> **Queue prompt:** release + publish

**Released 0.35.426: tagged and pushed.**
- **Checks:** full `make check` passed in a clean worktree (10248 tests; editor parity row added). Another session has an uncommitted edit in `agent-doc-document-realtime-io/src/lib.rs`, so I verified, built and installed from that clean worktree to keep the edit out of the release.
- **Local install:** `make install-full` installed the binary, cdylib, skills and JetBrains plugin 0.2.453. Only its VS Code packaging step failed, because the fresh worktree had no compiled extension output. I packaged and installed VS Code 0.2.75 from the main checkout, whose editor sources are unchanged.
- **Publishing:** `v0.35.426` and `main` are pushed (`97e90f684`), and the superproject pointer is at the new commit. The Release run (GitHub Release + PyPI) and CI were in progress when I checked, so neither is confirmed green yet.

**`#coder7`: closed.** I recorded your validation (daily use in a Coder workspace) in `tasks/agent-doc/plan-coder-ide-hosted-tmux.md` and moved the item to done.

**monsterrodholders.md stall: fixed, and the fix ships in this release.** The Codex transcript and ops.log showed two separate problems:

1. **The wait after a deferred write never ran.**
   - What happened: at 20:49:25 `respond` said the write was deferred and told the agent to run `session-check` once. At 20:49:32 that check printed `INTERRUPTED … still write_applied`, and the loop stopped. The cycle then committed on its own at 20:49:46.
   - Why: session-check already had a wait loop for exactly this case (`#scsettlewindow`, GH #60). But the check exited the process from inside itself, so the loop never saw the result. It also only waited 15 seconds, and this commit arrived about 17 seconds after the check started.
   - Fix: the check now hands the result back, only the command's entry point prints it and exits, and the wait is up to 45 seconds. A regression test runs the real result through the wait loop and pins that single exit. `#scsettleexit`, commit `8fb84e4e0`.
2. **A mistyped queue id sat in the queue forever.**
   - What happened: the queue held `do [#event-adapter-impl-tourne9yx]`, but the backlog item is `#event-adapter-impl-tourneyx`. The message told the agent to `--done event-adapter-impl-tourne9yx`, which matches nothing.
   - Fix: when a queued id isn't a tracked item and exactly one tracked id is a near spelling, the message now names that id and says how to fix or strike the queue line. Your line is never rewritten. `#queueidtypo`, commit `c055826ae`.

That document has since recovered on its own: its cycle is `committed`, and the next head is `#single-origin-dr-decision`. The typo line is still in its queue for you to correct to `#event-adapter-impl-tourneyx`.

One earlier `make check` run failed `preflight_sweep_skips_foreign_owned_doc` while I was running other builds at the same time. It passed 3/3 alone and in the final full run, but it takes 17s even with nothing else running, so it is load-sensitive.

**Cleanup for you:** my scratch cargo target dirs are about 62 GB: `~/.cache/agent-doc-relcheck-target` (57G) and `~/.cache/agent-doc-scfix-target` (5.1G), plus an empty `~/.cache/agent-doc-relinstall-target`. A hook blocked me from deleting them, so run `! rm -rf ~/.cache/agent-doc-{relcheck,scfix,relinstall}-target`.

### Re: #gh-fix agent-doc#72 — reliable-sync-status reports placement without a surface observation — opus-5.5 · 2026-09-30T18:14-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/72

**Fixed and closed.** The fix is btakita/agent-doc@264ed7df7, and I posted a summary on the issue.

**What was wrong.** When the controller had no accepted editor-surface observation, `reliable-sync-status` reported the layout as fine even though it had nothing to check it against:
- The layout effect builds its expectation from focus alone. So `converged_focus_only` looked like a success.
- The divergence check printed a bare `not evaluated`, even though 6 of the 7 registered docs were parked in stash.

**What changed (`src/reliable_sync_status_cmd.rs`).** It now builds a pure `UnobservedSurfaceGap` report whenever there is no accepted observation:
1. The layout outcome gets the label `expectation synthesised from focus — …; N registered document(s) not in expectation`.
2. The divergence section lists where each registered doc's pane actually is (agent-doc / stash / elsewhere / no pane), which can be read without the editor. It adds an `UNVERIFIED: N … parked in stash` line when any are stashed.
3. For each live JetBrains plugin, it says whether that build includes the surface publisher. The publisher first shipped in **0.2.334** (8ecb433e3). The reporter's live `0.2.392` **includes it**, so the old plugin probably isn't the cause. The editor→controller path (their Windows client / Linux backend remote-dev setup) is the likelier suspect, and I said so on the issue.
4. `--json` exposes the same data as `unobserved_surface_gap`.
5. The report is advisory only: `--check` still fails only on findings measured against an accepted observation. Without one, there's no way to tell whether a stashed doc should be visible.

**Spec.** Added a new `## reliable-sync-status` section to `specs/07-session-tmux-commands.md`.

**Verification.**
- 5 new regression tests in the module, including the issue's exact shape: 7 registered, 1 in agent-doc, 6 in stash, no observation, plugin 0.2.392.
- `make check` passed (exit 0; 10254/10254 tests, plus TLA non-vacuity checks).
- `make install` passed.
- Committed only my 2 files. `agent-doc-document-realtime-io/src/lib.rs` has uncommitted changes from another session; I left it alone.
- Pushed. CI was still running when I checked.
- Superproject pointer committed as c9823013bd.

**Not tested live.** My local editor is publishing its surface right now, so the no-observation output was only checked by the unit tests. The stash-growth note in the issue comes from project config (`stash_session_ttl_prune_enabled = false`), and I left it unchanged.

**Closeout note.** The first `respond` attempt failed before writing anything: `sync_pending recovery exhausted and disk read authority is refused`. That was the known strand from reloading the cdylib during `make install`. It cleared on its own at 22:14:47 (`crdt_relay_current diverged=false`), so this second attempt is the only write.

### Re: #gh-fix agent-doc#69 — opus-5.5 · 2026-09-30T18:41-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/69

Nothing left to do here. Issue #69 is already **CLOSED**, and its fix is already released:

- The fix is in `20f622007`. Git shows that commit is an ancestor of the agent-doc `HEAD` (`264ed7df7`).
- It first shipped in tag `v0.35.424`. The installed binary is `0.35.426`, so it's running.
- The closing comment on #69 covers all four asks:
  - `#presetshape`: structured `{prompt: …}` presets are now accepted.
  - `#resetclosedactor`: the invalid-frontmatter error now points to `reset --from-current --preserve-session`, and `--force-disk` is named when no editor is live.
  - `#addreadback`: `backlog add` now reads the document back after writing and exits non-zero when the item didn't persist.
  - `#backlogupsert`: `--backlog-upsert` was added.

I made no code changes and did not reopen the issue. There are no actionable follow-up items from this head.

Next in the queue is #70. That one is also already closed, fixed in `cb26320c1` / 0.35.424. After it comes `#release`.

### Re: #gh-fix agent-doc#70 — opus-5.5 · 2026-09-30T18:42-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/70

Nothing left to do here either. Issue #70 is already **CLOSED**, and its fix is released:

- The fix is in `cb26320c1`, tagged `#tmuxnotinmode`. Git shows that commit is an ancestor of the agent-doc `HEAD` (`264ed7df7`).
- It shipped in `v0.35.424`. The installed binary is `0.35.426`.
- What changed: in the shared tmux runner (`agent-doc-tmux-io`), a command that exits 1 is now treated as success when every stderr line is `not in a mode`. Any other stderr line still fails the command. Regression tests cover the classification and the isolated-server reproduction from the issue.

I made no code changes. There are no actionable follow-up items from this head.

The closing comment on #70 already gives the next step if the problem comes back on ≥0.35.424. The fan-out probably comes from a tmux-side hook or binding, since agent-doc never builds a `send-keys -X` argument. The next thing to check is `tmux show-hooks -g` and the copy-mode bindings on the reporting host.

Next in the queue: `#release`. The exchange is getting large (131 lines); I'm noting that and carrying on with the drain.

### Re: #gh-fix GH #73: supervisor-drain yield now checks the supervisor can drain — opus-5.5 · 2026-09-30T19:22-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/73

**Fixed and closed** in agent-doc `9a36f2e85` (pushed and installed). The commit's `Fixes #73` closed the issue, and I added a comment with the summary.

**The problem:** session-check chose the `deferred_for_supervisor_drain` hand-off by looking at the queue alone. It never checked whether a supervisor existed to take the work. In the reported case the supervisor was 18h old and running a stale binary, and its recycle had been refused. The agent was still told "NOT an operator stall", so it ended its turn and nothing drained the head.

**The fix:**
- `agent-doc-controller/src/status.rs`: a new pure classifier, `classify_supervisor_drain_readiness`, returns one of `Ready`, `NoLiveSupervisor`, `HeartbeatStale` or `StaleBinary`. It fails closed: missing evidence never counts as a live supervisor.
- `agent-doc-controller-io`: `supervisor_drain_readiness_for_doc` reads the supervisor lease, checks the pid is alive, and compares the running binary's inode against the installed one. It uses the same 60s lease threshold as the claim guard.
- `agent-doc-flow`: a new outcome, `supervisor_drain_unavailable` (Operator class, `next_action=operator_retrigger_supervisor_unavailable`).
- session-check hands off to the supervisor only when it is `Ready`. Otherwise it prints **queue STALLED — needs the operator** with the fix (`agent-doc admin recycle` / `session restart-supervisor`, or re-trigger in a fresh session). The hand-off ops.log line now records `supervisor_drain_readiness=` and `supervisor_pid=`.
- The skill text (SKILL.md, the cursor rule, `src/skill.rs`) now says to report this outcome as a stall, never as "yielding to supervisor".
- **Tests:** added a classifier truth table, an outcome/message regression covering all three not-ready states, and a check that readiness fails closed when there is no state. I also removed a copy of `log_supervisor_drain_handoff` that lived in the test file, so the test now exercises the production function.

**Verification:** `make check` passed (exit 0; nextest 10257/10257 passed, 259 skipped). `make install` and `cargo build --release` succeeded. The latest CI run is green.

**Not done:** suggestion 3 from the issue — recording the yield so the next preflight can report "yielded N min ago, still undrained". The hand-off log line now carries enough to build it; I'm leaving it as a [recommended] follow-up.

Next queue head: #gh-fix #74.

### Re: #gh-fix GH #74: an edited, marker-annotated queue item no longer forks into two copies — opus-5.5 · 2026-09-30T19:20-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/74

**Fixed and closed** in agent-doc `52a9b4b1f` (pushed and installed). The commit's `Fixes #74` closed the issue, and I added a comment with the analysis.

**Root cause (reproduced deterministically, not a hypothesis anymore).** The failure is in the three-way merge, not in dedup. A free-text item's text is its key. So when the operator edits `🚧 X` into `🚧 X + extra`, the side carrying that edit looks like it deleted the old item and inserted a new one. Meanwhile the other side only dropped the `🚧` marker (`X`). Two merge paths treated any change on that other side as an update that beats the delete:
- the per-cell merge in `agent-doc-merge/src/document_cell.rs` (`(None, Some(t))`)
- the keyed fallback in `crdt::reconcile_component_body`

Both resurrected `X` next to `🚧 X + extra`. That is exactly the operator's captured pair, with the marker on the longer copy.

**Fix.** For list components, a change whose instruction text (markers and strikes stripped via `queue_instruction_text`) equals the base no longer vetoes the delete. A real content correction still survives a delete.

**Tests:**
- the captured pair verbatim through `merge_by_component` (it failed before the fix)
- the fallback-path rule, mutation-checked: removing the guard turns it red
- a boundary test showing a genuine operator content edit is kept

**Verification:** `make check` passed (exit 0; nextest 10260/10260 passed, 259 skipped). `make install` and `cargo build --release` succeeded. Latest CI: see console.

Next queue head: #release.

### Re: #gh-fix GH #75: preflight no longer waits forever on an unanswered editor re-register — opus-5.5 · 2026-09-30T19:39-04:00

> **Queue prompt:** #gh-fix https://github.com/btakita/agent-doc/issues/75

**Fixed and closed** in agent-doc `10f428a96` (pushed and installed). The commit's `Fixes #75` closed the issue, and I added a comment with the details.

**Cause.** `schedule_stale_editor_replica_cp_recycle` (`agent-doc-controller-io/.../rpc.rs`) returned `request_skipped reason=editor_reregister_primary` on every call whenever the re-register reached a live endpoint, with no bound. Preflight deferred forever while session-check forbade every escape hatch. The byte-identical `issue_key` across three refusals was the tell.

**Fix, following the issue's first suggested direction:**
- `agent-doc-controller`: a new pure function, `classify_editor_reregister_wait`, returns `StartWindow`, `Pending` or `Unanswered`. It uses a 45s bound, and a record older than 15 minutes counts as a new episode rather than escalating.
- `agent-doc-controller-io`: the episode start is stored per document in `state.db` (`document_runtime_state`, kind `editor_reregister_wait`). That lets it survive the short-lived preflight processes.
- Past the bound, the status becomes `escalated reason=editor_reregister_unanswered unanswered_secs=N` and the supervisor-recycle fallback is requested once.
- The record is cleared on escalation and on the terminal no-live-editor or refused outcomes, so escalation cannot turn into a recycle storm.
- **Tests:** a classifier truth table (bound, TTL restart, clock skew) and an IO round-trip test (record → pending → backdated unanswered → cleared → new window).

**Not done:**
- The issue's secondary point: refusals warn about unanswered steering but show no text. That lives in `agent-doc-document-realtime-io/src/lib.rs`, which has another session's uncommitted `#rebasedsavefalsefail` work, so I left it alone. It's captured as `#refusalsteeringverbatim`.
- I also captured #73's third suggestion as `#supdrainyieldfalsifiable` [recommended]. Both follow-ups are appended after `#release`.

**Verification:** `make check` passed (exit 0; nextest 10262/10262 passed, 259 skipped). `make install` and `cargo build --release` succeeded. CI for #74/#75: see console.

Exchange is at about 190 lines with 8 sections (accretion warn); I'm continuing the drain per the active-queue rule. Next queue head: #release.
<!-- agent:boundary:4df2c95c:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/73~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/74~~
- ~~#gh-fix https://github.com/btakita/agent-doc/issues/75~~
- ⏭️ #release
<!-- /agent:queue -->


<!-- agent:backlog -->
- [ ] [#refusalsteeringverbatim] [agent-doc] GH #75 secondary: preflight refusals warn operator steering 'may be waiting unanswered' but session-check lists no prompt verbatim when the edit sits inside an unmerged editor save; surface the divergent components' operator text (agent-doc-document-realtime-io — coordinate with the in-flight #rebasedsavefalsefail edit there)
- [ ] [#supdrainyieldfalsifiable] [agent-doc][recommended] GH #73 suggestion 3: record the supervisor-drain hand-off (session_check_supervisor_drain_handoff now logs head_sha256 + supervisor_drain_readiness) so the next preflight reports 'yielded N min ago, still undrained' instead of silently re-presenting the same heads
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
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
