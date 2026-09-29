---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 4a326d6d-35d4-40d1-a46d-4014d1c3996d
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260929-004013.md`*

Compacted content:
- Prior summary/context: prior compacted content: Archived 32 response topic(s): complete #confirmfixlive; do #opcapturedormant; monsterrodhol...
Fix api.md issue
```
• Failed (exit 1) agent-doc write --commit /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/api.md
  └ Error: [finalize] retained document-write delivery from a prior cycle remains unsettled for /home/brian/work/btaki…
    + Show details

• The response-aware path correctly refused to create a second response while the retained delivery is settling. I’m
  waiting briefly for the scheduled reconciliation, then running only the required session check.

• Failed (exit 1) agent-doc session-check /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/api.md
  └ [perf] session_check.operations file=/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/api.md total_ms=2726…
    Error: [session-check] INTERRUPTED: canonical editor authority and disk projection diverge for /home/brian/work/bt…
    + Show details

• The prescribed check still sees the authoritative editor buffer ahead of disk, so agent-doc forbids another check or
  write. The controller owns recovery now; I’m leaving the retained response untouched and observing its state
  transition instead of forcing a competing closeout.

• Ran agent-doc session status /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/api.md
  └ controller_last_command: none
    status_warning: owned_pane_ready_busy_conflict actor=ready supervisor_actor=ready controller_lease=ready live_pane…
    recovery_hint: supervisor will treat a stable stale ready/busy probe as recoverable after a bounded re-probe; if t…
    + Show details

• Ran agent-doc reliable-sync-status
  └   d9272bd99fc83eb3c4050a3a0b3c3e0d84d979354d03d0f157187d0fe82f9b50
      f073f1718a7ebc9a902fb32f4f0c79fc7153e878b07d782a96693f1931a9b334
      f3b3b8d2aeee52fd97a96dd820fcdb1d5da996d0005143787f9b540f857bc131
    + Show details

• No new queue work is available; the retained continuation response remains under automatic agent-doc controller
  reconciliation.

• Hook stopped
  └ agent-doc Stop hook already continued once for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/api.md, but
the cycle is still open. [session-check] INTERRUPTED: binary-owned response delivery `1790641671607888913-1-
40bd928acd47a1b05b99e31eac227d86b0035df5bb529a5a3b2dc933647e054d` is retained for `/home/brian/work/btakita/agent-loop/
src/haiven-dev/tasks/api.md` (reason=editor_delivery_worker_stale, source=serialized_atomic_write,
target_hash=40bd928acd47a1b05b99e31eac227d86b0035df5bb529a5a3b2dc933647e054d); the same capture will resume
automatically after editor/controller delivery converges. NOTE: zero live editor replicas are registered for this
document, so there is currently nothing for the delivery to converge WITH — it will stay retained rather than drain on
its own. Reopen the document in the editor so its replica re-registers; the retained capture then drains automatically.
The capture is durable in CRDT/Lazily state, so it is not at risk while you do that. Do not issue another closeout
payload and do not force disk; retry only `agent-doc session-check /home/brian/work/btakita/agent-loop/src/haiven-dev/
tasks/api.md`.

  Worked for 3m 55s · 8:31 PM
```

### Re: infra.md "owned by pane %12, invocation from pane %3" — and recovery from that state — opus-5-5 · 2026-09-28T20:00-04:00

**Cause: Codex's shared app-server daemon.** The refusal was printed *in* pane `%12`, infra.md's own owner. Codex 0.158 attaches every TUI to one shared `codex app-server` daemon, and the daemon spawns hooks and tool commands, so they inherit its environment. That daemon (pid 171804) had been started by the Codex TUI in `%3`, so it carried `TMUX_PANE=%3`. The `%12` TUI was connected to it over `/tmp/codex-daemon-1000/…`. Pane execution authority proves the invoking pane from `TMUX_PANE`, so the real owner looked foreign. Walking the process tree can't recover the true pane: the daemon's ancestry leads back to `%3` as well. fpe.md only ever worked because it really does live in `%3`.

**Fix 1 — new launches (`f12d13c1c`):** the supervisor adds `--no-daemon` to every Codex launch before `fresh_base_args` is taken, so fresh, exact-resume and restart launches all carry it. `codex resume` accepts the flag, and the resume translator passes it through. A test covers every launch shape, and a mutation check confirmed it fails with the call removed.

**infra.md is fixed now.** I restarted its supervisor onto the fixed binary. That kept the old Codex child, so I then replaced the agent. `%12` now runs `codex resume 01a0e8b5-… -c sandbox_mode="danger-full-access" --no-daemon` with its hook as its own child, and `agent-doc tasks/infra.md` ran instead of being refused.

**Fix 2 — recovering sessions already in this state (`cb783a93b`):** Codex TUIs launched earlier (`%3` and `%13` right now) are still on the daemon. On a live-owner mismatch, pane authority now checks two things before refusing:
- the invoking process descends from a Codex app-server daemon
- the owner pane runs a Codex TUI without `--no-daemon`

When both hold, it requests the owner's `agent:continue` replacement and logs `pane_authority_shared_codex_daemon`. That is the Restart Agent intent: the live supervisor defers it to the turn boundary and completes it as an exact resume, so the conversation survives and the relaunch carries `--no-daemon`. The refusal then says the reported pane isn't evidence and to re-run the trigger once the pane is back at its prompt. If the request can't be made, it names `agent-doc session restart-agent <FILE>`. On the live desktop, detection flags `%3`'s TUI (pid 171289), not `%12`'s, and reports no daemon ancestry for a process outside it.

**Verification:** `make check` → exit 0, nextest 10032 passed, 0 failed. Installed, and the release `.bin` was rebuilt. **Caveat:** the `src/agent-doc` tree still holds another session's uncommitted edits (33 files, including `src/main.rs` and `src/session_actor_cmd.rs`). Those builds include them; I committed none of their files.

### Re: api.md reverted by queue updates — opus-5.5 · 2026-09-29T01:06-04:00

> **Queue prompt:** api.md reverted by queue updates. Looks like a previous revision rollback applied to the entire document. My changes should be preserved. Perhaps we need to do a roll-forward merge? Fix the current agent-doc state of api.md. It seems there are 2 sessions associated with api.md. Fix the runtime + agent-doc causes.

I could not find any of your text that was lost, so I did not do a roll-forward merge. There was nothing to merge onto. I also have not fixed any runtime cause yet, because I have not been able to reproduce a lost edit. Here is what I checked.

**Current state of `src/haiven-dev/tasks/api.md` (consistent, not wedged):** disk, the controller's CRDT text, and the JetBrains buffer all match: 34186 bytes, hash `c9273a0b…`, `delivery_converged=true`. That is HEAD `1b432940` (the ADR answer, 00:34) plus one uncommitted change from Codex's closeout at 00:49:49: `queue: go → stop`, and the answered ADR queue item is struck through. That change was applied as a CRDT delta (`update_bytes=2038`) against the text all three sides held at the time, so it did not overwrite anything that was only in the editor.

**The only edit on the editor side since the last commit** is a pair of changes one second apart, both classified as local document changes (`local-splice-batch`) in `idea.1.log`:
- 00:40:02: 3 splices, 34346 → 34137 chars (`bdba4d4e…`, roughly the queue-stop/strike shape).
- 00:40:03: 3 splices, 34137 → 34346 chars, restoring the exact previous hash `25509a1e…`.

Nothing else in `idea.1.log` was logged around it (no file reload, no agent-doc projection). An edit followed one second later by its exact inverse looks like undo/redo or a flip made inside the plugin, not a controller rollback. The 00:46 recycle re-registered the editor three times. The plugin logged "retained the exact published shadow … publishing the buffer", but the buffer hash was the shadow hash `25509a…`, so it held no unpublished text.

**A false lead I ruled out:** controller `canonical_len` counts UTF-16 characters, while `crdt_current_text len` counts bytes. The 34346 vs 34398 "divergence" is the same text measured two ways.

**"2 sessions":** the registry has one owner, pane `%19` (Codex). The frontmatter `resume:` block keeps both the `claude` and `codex` harness session ids from the claude → codex switch. Pane `%20` (the haiven-frontend Codex session) only logs `foreign_owned_sweep_skip` for api.md, meaning it sees another pane owns the file and does nothing. So there is no second writer.

**What I need from you:** which text disappeared, and roughly when did you see it go? If it was typed in the editor, JetBrains Local History (right-click api.md → Local History → Show History) should still have it. With the lost text or a timestamp I can locate the path that dropped it. I filed that follow-up as `#apimdrevert`.

### Re: #pastetriggeradmit — opus-5.5 · 2026-09-29T01:23-04:00

Fixed in agent-doc `27a168060` (pushed to `main`, installed).

**Cause:** there were two hook paths, and both treated a paste as a trigger.
- **Admission:** the parser only looks at the first line of the prompt. A pasted transcript that begins with `agent-doc tasks/fpe.md` matched. The path was then resolved against the hook's own cwd (agent-loop) and failed, which produced a loud `cycle contract UNAVAILABLE`.
- **Codex session tracking** (`resolve_agent_doc_path`): it scans every line for an invocation. It would bind, or fail on, a path that does not exist in this project.

**Why not "first line only":** requiring the whole prompt to be the trigger would break a supported shape, `agent-doc FILE` followed by prompt-body lines (`harness_prompt::prompt_body_from_text`). That shape always names a document that resolves.

**Fix:** a multi-line prompt counts as an invocation only when its document resolves.
- A new predicate, `harness_prompt::is_multi_line_prompt`, counts non-blank lines.
- **Admission** returns `NotATrigger` for an unresolved multi-line prompt and logs one line to stderr.
- **Tracking** returns `None` for the same case, so the thread keeps the document it already had admitted. It no longer fails, and no longer rebinds to a path from another project root.
- A single-line trigger with a bad path still fails loudly, as `#hooktriggerunresolved` requires.

**Verification:**
- **New tests:** a unit test for the predicate, and a Codex hook integration test that pastes a transcript naming a missing `tasks/fpe.md`. The integration test asserts there is no refusal marker, no canonicalize failure, and no tracking error.
- **Mutation check:** with the predicate forced to `false`, the integration test fails with the exact refusal from this bug. With the fix restored, it passes.
- **Full suite:** `make check` exited 0, with 10037 tests passed and 0 failed. I ran it in a scratch worktree holding HEAD plus only these four files, because the shared tree currently fails clippy on another session's uncommitted `src/session_actor_cmd.rs` (an 8-argument `poll_context_clear_submit_acceptance`). That session still needs to fix it.
- **Install:** `make install` exited 0.
- **CI:** the newest run, `36525812441`, has no result yet (status only showed a clock icon).

### Re: #drivecapturedsplice — opus-5.5 · 2026-09-29T01:36-04:00

> **Queue prompt:**
>
> do [#drivecapturedsplice]


I gated this without running the live proof. The run types real keystrokes into your desktop (XTEST into the live IntelliJ), and the plan requires your OK for that first. You aren't here to give it, so I didn't send any input to the IDE.

**What the blocker is:** "retained document-write delivery from a prior cycle remains unsettled" comes from `guard_historical_retained_write_before_new_capture` → `retained_write_blocks_new_cycle`. The settlement table in `agent-doc-state-backbone/src/retained_write.rs` (`settlement_input`) treats any difference between the controller's text and disk as `AuthorityDiskDiverged`, and that verdict is `Unsettled`. A captured operator edit that is still unsaved in the editor is exactly that difference, so every new cycle is refused until the editor saves. The one existing exception, `CommittedStateOnlyAuthorityMaterialized`, is deliberately limited to a committed free-text queue strike ("Only the typed state-only queue strike may use this").

**Proposed decision:** yes, an independent advance should be admitted while a captured operator edit is retained, provided two things hold:
- the retained intent's cycle has already committed (`closeout_committed`);
- its payload or delta is already present in the controller's text.

The CRDT authority is the source of truth and the disk is a durability projection. A new cycle reads the authority, and the commit barrier already refuses to commit until disk catches up, so admission doesn't need to wait on disk too. Concretely, this generalizes the strike-only arm to response- and delta-carrying intents.

**Why I didn't make that change now:** it reverses a deliberate restriction. The earlier live run's logs don't show whether the refused intent had `closeout_committed=true`. They only show `native_editor_save_request_deferred` (a rejected IPC receipt) and `retained_write_disk_projection_ignored reason=authority_hash_mismatch` at 04:46Z. Without that fact, I can't tell whether the relaxation would admit the recipe or just widen the gate.

**To unblock:** say OK to the keystroke run (`scripts/xdotool-live-verify.sh captured-splice`, with no stale-binary supervisors). Its `retained_write_blocks_new_cycle ... intent_id=… cause=…` line plus the intent's facts decide the fix. If `closeout_committed` is true, I'll land the generalized arm with transition-table coverage.

### Re: #fpestopreplay — opus-5.5 · 2026-09-29T01:54-04:00

Fixed in agent-doc `1ad82610d` (pushed and installed).

**Step 1, reproducing:** today's `captured_finalize_resume` does not land bare prose. A Stop capture has no mutation plan, so it resumes through `repair_preserving_live_authority`, which wraps prose as `### Re: recovered response`. The actual cause was one step earlier.

**Step 2, the decision (should a Stop payload reopen a cycle behind a committed predecessor?):** not when that committed cycle already settled this turn's prompt.

Here is the incident path:
- The recurring head reappeared as a fresh prompt diff behind the committed `#advance-review` answer, so session-check reported the committed-prompt-diff interruption.
- The Stop hook's interruption branch then called `attempt_stop_closeout`.
- That called `reopen_terminal_cycle_before_stop_capture`, which captured Codex's closing chat restatement as the answer to the new diff.

The settled-debt check (`committed_cycle_settled_prompt_debt`) existed, but only the prompt-writeback path consulted it. The interruption branch never did.

**The fix:** in the interruption branch, when all three hold, the hook no longer reopens or captures:
- the interruption is a committed prompt diff;
- the cycle is committed;
- that cycle settled this turn's prompt debt.

It logs `codex_stop_post_commit_replay_skipped` and returns the existing "answer the fresh diff in-pane" block. A genuine follow-up, where the prompt was observed after the cycle was already terminal, still reopens and closes as before. `stop_reopens_and_closes_prompt_debt_observed_after_cycle_committed` and `recursive_stop_reopens_and_closes_committed_cycle_fresh_prompt` both still pass.

**Verification:**
- **New test:** `stop_does_not_replay_a_restatement_over_the_turns_committed_cycle`. It binds the prompt while the turn's cycle is open, commits, adds a fresh `❯` prompt, and asserts the session-check interruption. It then Stops with a restatement and checks that the result is a block, the restatement is not in the document, the cycle id is unchanged and still `committed`, the skip is logged, and there is no reopen.
- **Mutation check:** with the guard disabled, the hook reopens and writes the restatement (`Continue`), which is the incident.
- **Full suite:** `make check` exited 0, 10038 tests passed. I ran it in an isolated worktree, because the shared tree has another session's clippy failure.
- **Install:** `make install` exited 0.
- **CI:** run `36527892614` for this commit was still in progress.

**Cleanup notes:**
- My first isolated build put a 46 GB target dir on the `/tmp` tmpfs and filled it to 100% for a few minutes. It is deleted.
- The second build left `tmp/fpewt-target` (46 GB, gitignored) that the safety hook would not let me `rm -rf`. Please delete it.
- The exchange is getting long (8 cycles in 30 minutes). I'm not compacting it without an opt-in.

### Re: #nameelseclaude — opus-5.5 · 2026-09-29T02:08-04:00

> **Queue prompt:** do [#nameelseclaude]: This happened after transitioning from `agent: claude` to `agent: codex`

Done in agent-doc `5799eb417` (pushed and installed). `agent-doc session status <FILE>` now names what holds the document's Claude Code conversation.

**What it reports:**
- **Live and stale holders:** every live Claude Code CLI record in the project, and every record whose pid is dead (marked `STALE(dead pid)`), from `~/.claude/sessions/<pid>.json`. Each line names the holder, pid, session id, entrypoint, kind, status and tmux pane.
- **The document's own conversation (`resume.claude`):** after `agent: claude` → `agent: codex` the frontmatter keeps the old Claude id, and that id is what a Claude client refuses to reopen. A record holding exactly that id is reported even if its cwd is elsewhere, and is marked `holds_this_conversation`.
- **A positive "not held" finding:** when no live CLI holds the conversation, the report says so, and says the holder is a desktop, web or browser native-host client outside the terminal.
- **Not gated on the declared harness:** api.md is `agent: codex`, so any harness gate would hide this report.

**Live, on `src/haiven-dev/tasks/api.md`:** `conversation_holders.this_conversation: claude=baaf5655-b09c-46eb-b83d-ed3a27e99499 is not held by any live Claude Code CLI session`. The "open in another app" lock you saw came from a client outside the terminal.

**Origin:** I finished an abandoned uncommitted draft for this item: an untracked `src/conversation_holders.rs` plus a hook in `session status`. I added the per-conversation finding, the flag, and three tests (11 total). I also marked the now test-only `report_from_holders` as `#[cfg(test)]` for clippy.

**Verification:**
- **Full suite:** `make check` exited 0 with 10049 tests passed. I ran it on the exact commit tree in an isolated worktree.
- **Commit:** I built it through a private index. `src/session_actor_cmd.rs` still holds another session's uncommitted `/clear` settle-timeout change. That change is what fails clippy in the shared tree (8 args on `poll_context_clear_submit_acceptance`), and I left it out and untouched.
- **Install:** `make install` exited 0.
- **CI:** the newest run, `36529492455`, has no result yet (status only showed a clock icon).

### Re: #publishjetbrainssibling — opus-5.5 · 2026-09-29T02:12-04:00

Closed with no code change. The unblock question is "does either IDE host actually require a signed package?", and the answer is no:

- **JetBrains:** IntelliJ IDEA 2026.1 (pid 3417877) is running `agent-doc-jetbrains-0.2.439.jar`, installed unsigned from disk into `~/.local/share/JetBrains/IntelliJIdea2026.1/agent-doc-jetbrains/`. `idea.log`/`idea.1.log` contain no signature warnings or refusals for it. IntelliJ enforces plugin signatures only for JetBrains Marketplace publishing, not for disk or GitHub-release installs, and that is the only way agent-doc ships.
- **VS Code/codium:** `make install` installs the unsigned `.vsix` through `codium --install-extension` ("Extension installed via `codium`" in tonight's install logs).

So a `-signed` sibling has no consumer. Adding `signPlugin` to the release `plugins` job without the three secrets would only fail the release. The client half (7649ccafd, `find_asset` accepting `<prefix>*-signed.<ext>`) stays as harmless forward compatibility. Signing becomes a real requirement only if agent-doc is ever published to JetBrains Marketplace. That would be a new item with its own secrets, not this one.

Queue state: only `#focuscrossrootverify` remains, and it is `[operator-verify]` (a live IDE tab switch into `src/haiven-dev/tasks/api.md`). The loop stops here.

### Re: #retaineddeferwedge — opus-5-5 · 2026-09-29T02:32-04:00

Fixed in agent-doc `8da5f5015` (pushed and installed).

**What went wrong:**
- **Why nothing committed.** When the tracked-work mutations were deferred on the retained delivery projection, the write command returned `Ok(())` right away (`agent-doc-write-runtime-io/src/lib.rs`). That skipped queue consumption and the commit.
  - The message promised "the same intent commits itself once delivery converges". Nothing did that. The write command is the only thing that owns this commit.
- **Why the three recovery verdicts disagreed.** The cycle had been abandoned by `cancel_preflight` at 05:17, and the recovery classifier treated an abandoned cycle as `Clean` without checking anything. So `repair` said clean. With no recovery command, the session-check hint fell back to "document-only drift, run `agent-doc commit`". Then `commit` refused, because it compares the current document to HEAD and found a response HEAD doesn't have.
- **Which content the snapshot recorded.** The 22261-byte snapshot was saved when the response cell was written (`response_cell_projection_materialized` → `document_baseline_checkpoint`, 05:23:46). That was after the response was added but before the tracked-work mutations. The mutations (reaping done items) then shrank the document to 21647 bytes. Because the snapshot already contained the response, a check that compares the document against the snapshot could not see it either.

**Fix:**
- **Write path:** after a deferral, the command now waits up to 30s for delivery to converge. Once this cycle's recorded mutations show up in the document, it carries on through the normal steps (queue consumption, then commit). If the wait ends without that proof, the closeout fails instead of exiting 0 uncommitted, and the error names `write --commit` without the tracked-work flags. The decision logic is a pure function: `agent_doc_turn::write_ownership::resolve_deferred_tracked_work`.
- **Classifier:** it now also checks whether the current document holds a response that HEAD doesn't. This is the same check `commit` refuses on, applied to both abandoned and committed cycles, so session-check, repair and commit all name `write --commit`.

**Verification:**
- New tests:
  - unit tests for the deferral decision and the classifier
  - a file-level regression test that reproduces the incident (abandoned cycle, response hidden from the snapshot)
- I broke the fix on purpose to check the file-level test: with the new abandoned-cycle branch disabled, the test fails.
- Full `make check`:
  - It fails in the shared tree on clippy `too_many_arguments` in `src/session_actor_cmd.rs`. Another session is editing that file and hasn't committed it; I didn't touch it.
  - So I ran it in a clean worktree at HEAD with only my 5 files applied: exit 0; nextest reported 10052 run, 10052 passed, 0 failed; clippy, sim, lean and tla all passed.
- CI run for `8da5f5015` is still in progress.

**Not covered:** the in-process wait itself (the RPC loop) has no SimWorld test; only its decision function is tested.

Warning from preflight: 1 gated item is still in `agent:backlog` (`legacy_gated_in_backlog`). Running `agent-doc migrate` moves it to `agent:review`.
<!-- agent:boundary:fe1c0161:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#retaineddeferwedge]~~
- do [#focuscrossrootverify]
<!-- /agent:queue -->


<!-- agent:backlog -->
- [/] [#apimdrevert] [agent-doc][operator-verify] src/haiven-dev/tasks/api.md 'reverted by queue updates' (2026-09-29): forensics found NO lost operator text — disk/controller/editor converged at c9273a0b (HEAD 1b432940 + Codex 00:49:49 queue stop+strike, applied as a 2038-byte CRDT delta against the converged base). Only editor-side anomaly: idea.1.log 00:40:02 local-splice-batch 3 splices 34346->34137 chars, then 00:40:03 exact inverse back to 25509a1e (undo/redo or plugin-internal flip; no VFS reload/projection logged). Blocked on operator: which text was lost + when (JetBrains Local History on api.md). Then trace the dropping path and fix with regression coverage.
- [ ] [#focuscrossrootverify] [operator-verify] [agent-doc] Confirm the cross-root editor focus delegation live (`#focuscrossroot`, 730d74600). Agent-side is complete and verified: 6 tests, make check green, installed and pushed. The remaining step needs a human at a real IDE because only the editor surface reproduces it — the CLI `agent-doc focus` already resolves the document's own project root and never took the broken path. UNBLOCK: after the controllers pick up 730d74600 at their next idle boundary, switch an IDE tab (rooted at the agent-loop superproject) to a submodule document such as src/haiven-dev/tasks/api.md and expect its pane to take focus, with `controller_focus_delegated document=... owner_root=.../src/haiven-dev focused=true` in agent-loop's .agent-doc/logs/ops.log and no further `reason=missing_actor_record` for that document.2
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#drivecapturedsplice] [agent-doc] Run the captured-splice live proof to completion (#activateinstalledjetbrai). HARNESS FIXED 2026-09-28 (095d2af54): the old 'xdotool keystrokes always arrived' premise was false — `type --window` goes through XSendEvent and JetBrains drops send_event=True keys, so nothing ever reached the editor. The harness now types via XTEST under an absolute-path focus guard, keeps scratch docs in tmp/live-repro/ (under .agent-doc/ they became a nested project with their own controller and log), waits only on fresh doc-scoped receipts, pipes a real response into the advance, runs it in the scratch doc's owner pane, and seeds a lint-valid boundary. A live run then produced the first fresh doc-scoped `editor_op_capture_proof operator_ops=N shadow_replay=agreed` any scratch doc has carried. REMAINING BLOCKER: the canonical advance is refused with 'retained document-write delivery from a prior cycle remains unsettled' while the captured edit is still unsaved in the editor (native_editor_save_request_deferred; authority/disk diverge on exchange). Decide whether an independent advance must be admitted while a captured operator edit is retained — that admission is what this recipe exists to prove — and fix whichever side is wrong. Re-run from a desktop with no stale-binary supervisors and a CLI build matching the IDE cdylib (the runs saw crdt_replica_build_mismatch_recovery_requested x4). Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically now: a human reply on pypi/support#12214, or `make pypi-quota-check` exiting non-zero. As of 2026-09-27 that check exits 0 — 8.362 GiB of 10.0 GiB, 1.638 GiB headroom, 151 releases (0.35.113..0.35.417), room for ~49,662 further post-cutover releases — so there is nothing to act on and nothing to delete. pypi/support#12214 is still open with one comment (the acknowledgement bot) and untouched since 2026-09-10T22:46:21Z. If the grant lands, record it and drop this item. Deletion stays reserved for a ceiling the check actually reports (next tranche would be 0.35.113..0.35.161, 2.491 GiB); verify any deletion from the authenticated /manage/project/agent-doc/ pages, never from pypi.org/pypi/agent-doc/json.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
