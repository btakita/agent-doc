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
  '#self': agent-doc-bugs.md
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->
 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260929-230356.md`*

Compacted content:
- Archived 31 response topic(s): infra.md "owned by pane %12, invocation from pane %3"; api.md reverted by queue updates; #pastetriggeradmit; 28 more
- Prior summary/context: Fix api.md issue ``` • Failed (exit 1) agent-doc write --commit /home/brian/work/btakita/agent-loop/src/haiven-dev/ta...
Fix fpe.md issue
```
• The controller and owning actor are healthy and report no projection lag; the only mismatch is that the installed
  plugin is already 0.2.445 while the live IDE still runs 0.2.444. Reloading the IDE would disrupt the user’s editor,
  and forcing disk authority would violate the recovery guard, so I’m ending the recovery attempt with the mandated
  session check to capture the exact remaining state.

• Failed (exit 1) agent-doc session-check /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md
  └ [session-check] INTERRUPTED: cycle state is committed but the snapshot does not match HEAD in the owning repo (sna…
    [session-check] INTERRUPTED: cycle state is committed but the snapshot does not match HEAD in the owning repo (sna…
    [perf] session_check.operations file=/home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md total_ms=1617…
    + 5 lines (ctrl+t to expand)

• Agent-doc closeout is blocked by the live JetBrains plugin: 0.2.445 is installed, but the IDE still runs 0.2.444.
  Please reload/restart IntelliJ to activate 0.2.445; then I can complete agent-doc write --commit and the final
  session check.

  The implementation and green PRs are unaffected. I did not force disk authority or stage unrelated workspace edits.

• Hook stopped
  └ agent-doc Stop hook already continued once for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md, but
the cycle is still open. [session-check] INTERRUPTED: cycle state is committed but the snapshot does not match HEAD in
the owning repo (snapshot_len=73242, head_len=71543). The response patchback is visible but was never committed;
tracked side-effect edits: .gitignore, .gitmodules, AGENTS.md (+11 more) Recovery [direct_response_patchback]: `agent-
doc write --commit /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md` to absorb the visible `### Re:`
response through the snapshot/commit boundary.

  Worked for 45m 22s • 6:16 PM


› agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md


• Agent-doc admission failed because tasks/fpe.md’s editor snapshot differs from Git HEAD, with no open or safely
  recoverable cycle. The visible ### Re: response remains uncommitted.

  Per agent-doc’s contract, I did not rerun preflight or modify anything. Recovery requires:

  agent-doc write --commit /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md

  Worked for 5m 16s • 6:58 PM
```

### Re: Fix fpe.md issue — opus-5 · 2026-09-29T19:10-04:00

Your paste arrived empty (a bare `` `````` `` fence), so I worked from the fpe.md logs. **fpe.md is healthy now:** its "parameterize a GPU instance" response committed at 19:04 EDT (`a50bb890` in haiven-dev). The remaining diff there is your queue/frontmatter steering, which the codex pane is draining.

**What happened (22:11–23:04Z):**
- While codex was finalizing, a `make install` recycled the supervisor mid-turn and dropped the JetBrains replica (plugin 0.2.444, expected 0.2.445): `editor_attached_model_missing`, `missing_replica_terminal_rebuild_failed`.
- The response reached the editor and snapshot but not HEAD. The Codex Stop hook reported it **STRANDED** and told you to run `agent-doc commit`.
- At 22:53Z you re-routed. Preflight **bailed**: "snapshot differs from HEAD without an open or recoverable agent-doc cycle" and told you to run `write --commit`. Neither suggestion could run inside the blocked turn, so no contract was produced.
- At 23:04Z the next route was on a fresh binary with plugin 0.2.446. Preflight's pending maintenance happened to commit it.

**Fix (`bcc560566`, #preflightvisibleabsorb):**
- Before bailing on that drift, preflight now runs the `#visibleresponseabsorb` commit (`commit_visible_uncommitted_response`) itself.
- `agent-doc commit` tries the same absorb first, so the command the Stranded message names actually clears the state.
- The absorb still declines if the exchange differs from the snapshot, so a newer operator prompt is never swallowed. It commits only the session document.
- Regressions: a new git-backed commit-io test (absorb lands the response; a newer prompt blocks it) and a preflight-command test. The old test that expected preflight to fail here now asserts the absorb and that other tracked edits stay uncommitted. The source-contract markers are updated too.

**CI:** the latest CI run was red on my earlier `c0864bca9`: `command_submit_response_accepted` was dead code under `test-support`. The `allow(dead_code)` fix was sitting uncommitted in the shared tree, so I committed and pushed it (`d44acade0`).

**Verification:** `make check` passed, 10123/10123 (exit 0). `make install` exit 0. Both commits are pushed to agent-doc main.

There were no actionable follow-up items; the fix is complete and nothing is left to capture.

### Re: Fix fpe.md issue (paste landed; lint-gate fix) — opus-5 · 2026-09-29T19:45-04:00

Your paste reached the document after the turn started. It shows exactly the sequence above: the Stop hook reported STRANDED, then the 18:58 re-route got "admission failed … Recovery requires `agent-doc write --commit`". `#preflightvisibleabsorb` removes that dead end. The closeout here was itself committed by the new absorb ("committed the visible uncommitted response").

Closing this turn exposed a second defect, which I also fixed:

- **What happened:** the lint gate blocked the closeout with a false `agent-doc/unclosed-component` on `agent:exchange`.
- **Cause:** tagpath's inline-code-span matcher paired the stray ` ``` ` in the compacted summary line with the paste's opening fence. It accepted a closing run of any length and let spans cross line-start fences. The paste's closing fence then read as an unclosed opener and hid `&lt;!-- /agent:exchange --&gt;`. That would have blocked every later closeout of this document.
- **tagpath 0.12.3:** inline spans now follow CommonMark: the closing run must be exactly as long as the opener, and a span never crosses a blank line or a line-start fence. Two new tests.
  - I published it to crates.io from a clean tag worktree, not with `--allow-dirty`, because another session's uncommitted `AGENTS.md` and `.agents/` would have shipped in the package.
- **agent-doc `5646cd361`:** bumps to tagpath 0.12.3. Adds a lint-io regression on the observed shape, which fails on 0.12.2 and passes on 0.12.3.

**Verification:** `make check` passed, 10124/10124 (exit 0). `make install` exit 0. Pushed.

One tagpath test was already failing before my change: `round_trip_real_session_doc_is_clean` flags `agent:notes` in the live `tasks/software/tsift.md`. It reads a file only this machine has, so CI skips it.

There were no actionable follow-up items.

### Re: do [#syntheticstrike] — opus-5 · 2026-09-29T19:52-04:00

> **Queue prompt:** 📌 do [#syntheticstrike]

**Root cause.** The answered head's capture belonged to a cycle that had been **Abandoned**. `decide_committed` replaces an Abandoned cycle with a fresh `synthetic-*` state that carries no capture identity. The state projection clears `captured_response` whenever the cycle id changes, so `active_captured_response_for_queue_strike` (which needs the same cycle and an open phase) could never fire. The commit-time continuation then offered the answered "Fix …/issues/59" head as the *next* head, which re-dispatched finished work. It was only struck at the next preflight, by `preflight_freetext_residue_strike`.

**Fix (`13d1c6d78`):**
- `required_continuation` now skips any head that `free_text_queue_head_is_completed_residue` reports. That is the predicate session-check and the preflight residue strike already share.
- The drain moves on to the next real head, and an answered head on its own no longer demands continuation.
- Recurring imperatives such as `deploy …` stay exempt, so a standing command is never skipped.
- Regressions:
  - answered head skipped with the next head returned;
  - answered-only queue yields no continuation;
  - recurring imperative kept.
- A mutation check confirmed the main regression fails without the fix.

**Verification:** `make check` passed, 10126/10126 (exit 0). `make install` exit 0. Pushed.

**Follow-up (`#synthcommitstrike`, appended to the backlog):** the answered head still stays visibly live until the next preflight strikes it. A preflight commit can also refuse it first as a "recurring" prompt. The fix is to strike in the commit path itself, which needs either the capture carried through the synthetic fold or the shared residue strike run at commit.

### Re: do [#layoutobservehandoff] — opus-5 · 2026-09-29T19:50-04:00

> **Queue prompt:** 📌 do [#layoutobservehandoff]

This landed earlier this session as `c0864bca9`, with the CI compile fix in `d44acade0`; this cycle verifies and closes it.

- **Cause:** during an install handoff, `editor_route`'s layout sync reached a controller that was still `Preparing`. The refusal came back as the command's terminal result (`exit_code=1`, "controller not authoritative"), not as a transport error, so the existing handoff retry never saw it and Run Agent Doc showed the raw refusal.
- **Fix:** the whole command submit, including its terminal-result check, now runs under the bounded handoff retry, so each attempt reaches the promoted generation. Real failures are not retried.
- **Evidence:**
  - haiven-dev's ops.log has exactly two occurrences: 18:57Z (the original report) and 22:51Z. Both are before the fix was installed at 22:57Z, and none have appeared since.
  - All 16 controller-io handoff tests pass, including:
    - `a_terminal_command_plane_handoff_refusal_is_retried`
    - `a_mid_handoff_refusal_is_retried_until_a_generation_answers`
    - `a_failure_that_is_not_a_handoff_refusal_is_never_retried`
  - The full `make check` for the fix passed and it is installed.

The exchange is getting large (session accretion `warn`); I'm continuing the drain rather than compacting.

### Re: do [#retainedadvanceadmit] — opus-5 · 2026-09-29T20:12-04:00

> **Queue prompt:** 📌 do [#retainedadvanceadmit]

**Decision: admit the advance. The guard was the wrong side, not the harness ordering.**

A retained write whose content the Lazily authority already holds, but whose editor native save hasn't reached disk, was `Unsettled(AuthorityDiskDiverged)`. That refused every new cycle with "retained document-write delivery from a prior cycle remains unsettled". A new cycle composes onto the authority, not disk. The retained intent still settles by payload once disk catches up, so the refusal protected nothing.

It also caused real harm today. On haiven-dev fpe.md at 22:12:37Z it refused the captured-finalize resume (`retained_write_blocks_new_cycle cause=authority_disk_diverged`, reason `editor_native_save_pending`). That cycle was later abandoned and the response stranded, which is the "Fix fpe.md issue" from earlier.

**Fix (`54452093d`):** the settlement table gains `DiskProjectionPending`. It applies when the authority already holds the intent: the exact target, the response payload, or the added delta.
- It does **not** block a new cycle or the queue boundary.
- It **still blocks session closeout**, so the commit waits for disk.
- It is never settled or replayed early. Recovery awaits convergence and requests the editor's native save.
- Authority that lacks the intent stays `AuthorityDiskDiverged` and still blocks.

**Coverage:**
- The table-coverage test now has 10 rows.
- Admission for each of the exact, payload, and delta cases.
- A "retained capture + independent advance" sequence on the shared reactive verdict: admitted while disk lags, still admitted after the advance lands, and settled by `RebasedPayloadMaterialized` once disk catches up. It fails with the rule disabled.
- Four older assertions that encoded "refuse while disk lags" now assert unsettled plus closeout-blocked, which preserves their fail-closed intent.

**Verification:** `make check` passed, 10128/10128 (exit 0). `make install` exit 0. Pushed.

**Still gated:** `#drivecapturedsplice` (review) no longer has this guard as its blocker. Its live rerun (`scripts/xdotool-live-verify.sh captured-splice`) injects real keystrokes into your desktop, so the plan requires your OK first. Say "run the captured-splice live proof" and I'll drive it.

### Re: do [#synthcommitstrike] — opus-5 · 2026-09-29T20:40-04:00

> **Queue prompt:** do [#synthcommitstrike]

**Harm this closes:** a synthetic-cycle closeout left the answered head unstruck in HEAD. At the next preflight, an unrelated operator edit plus that live head made the commit refuse with "a later queue prompt recurs after committed response capture" (agent-doc-bugs.md 18:34:11). That refusal fired *before* the preflight residue strike that would have cleared it.

**Fix (`fb18c0477`):** recurrence is only provable when a capture owned by the committed cycle shows its strike was projected. A synthetic fold has no owning capture (the `committed-head:` fallback id), so the live head there is residue. The commit now proceeds, and the same preflight's `preflight_freetext_residue_strike` strikes it. Genuine recurrence under an owning capture still refuses; the existing integration test `commit_recovers_exact_late_free_text_strike_or_its_reaped_target` covers that.

**Approach I dropped:** counting occurrences. The strike can *reap* the row, so "reaped then re-added" is textually identical to leftover residue. The integration test caught that, and capture provenance is the real distinguisher.

**Combined with `#syntheticstrike` (`13d1c6d78`):** the continuation no longer re-offers the answered head, and the next preflight now strikes it cleanly. In a `/loop` drain that is the very next admission.
- **Remaining gap:** if the queue has no other head, nothing triggers a preflight, so the row stays live until the next manual invocation. It's cosmetic only: nothing dispatches it, and no commit refuses it.

**Verification:**
- Two unit tests: residue is not recurrence; an owning capture does recur.
- A mutation check confirmed the residue test fails without the ownership gate.
- `make check` passed, 10130/10130 (exit 0). `make install` exit 0. Pushed.

### Re: do [#frontmattersessionflap] — opus-5 · 2026-09-29T20:55-04:00

> **Queue prompt:** do [#frontmattersessionflap]

**Writer found:** `assign_and_record_session_id` in `agent-doc-start-runtime-io/src/run.rs`.

Every stale-binary supervisor re-exec (`supervisor_binary_stale_self_recycled via=execve_preserve_child`) re-runs the start path. That path minted a fresh UUID and wrote it to `resume.claude:` for a harness launch that never happens: the adopted child keeps its own conversation. Evidence from 2026-09-29:
- 67619911 → 6408b655 at 00:18:19–24Z, immediately after the re-exec.
- Twelve such assignments over the day.
- **None** of the recorded ids has a Claude transcript.

Each rewrite was post-commit drift (the 22:44:30Z `commit_document_via_controller_error`), and the `start_session_id_record_failed` lines are the same writer failing mid-retained-write. It was flapping on every `make install`.

**Fix (`4b5123762`):**
- The mint is gated on `!preserved_child_survived`.
- A child that died across the recycle still gets a fresh id.
- The existing hook-ledger gate (`reentry_may_record_resume_id`) still governs lineage projection after `/clear`.

**Coverage:** the pure gate, plus a source contract pinning it directly on the mint call. The contract fails with the gate removed; its needles are split with `concat!` so the test can't match its own text.

**Verification:** `make check` passed, 10130/10130 (exit 0). `make install` exit 0. Pushed. This install's own idle-boundary re-exec is the live check: it should log no `start_session_id_assigned`.

### Re: do [#editorauth2] — opus-5 · 2026-09-29T21:30-04:00

> **Queue prompt:** do [#editorauth2]

Landed as `c9b731ea1` (plugin 0.2.447) plus the install bump `674c775ec` (0.2.448).

**JetBrains — the hold gets a non-destructive exit.**
- Previously, a registration where settled shadow, live buffer, and canonical all differ, with canonical not provably containing the operator's edits, was `HoldOperatorBuffer`, refused on every retry (#ambiguousholdforever).
- New `MergeForward`: when `reconcileText(base = shadow, yours = buffer, agent = canonical)` is clean, the replica publishes the merged text as a local delta. The generation-fenced remote apply then projects it into the buffer, and refuses if you typed since.
- Conflicting or unavailable merges still hold, and a proven rebase still adopts canonical first.
- This is the first live caller of the #editorauth4 FFI.

**VS Code — re-registration keeps the retained frontier.**
- `attachDocument(forceRefresh)` used to deregister and register **cold**, so the controller bootstrapped the editor from canonical (disk, on a fresh relay) instead of running #editorauth3's reseed-from-editor.
- It now swaps through `replaceForwarder`, which carries `captureResumeState()` and fences the swap; a race keeps the old membership. This matches JetBrains `forwarderFor`, which already carried `resumeState`.

**Deliberately unchanged, both with reasons:**
- **Adopt with no settled shadow** (a restarted IDE): its buffer is a reconstruction with no base to merge from.
- **VS Code's stale-cache reprojection:** its existing test shows the divergence is non-operator, because every operator edit passes through the change listener first.
  - I first tried merge-forward there too. It broke that designed recovery and would hold forever without a reconciler, so I reverted it.

**Verification:**
- **Kotlin:** a decision-table test covering clean, conflicting, and unavailable merges, with and without a retained projection. It also checks that a proven rebase still adopts canonical.
- **VS Code:** a forced re-register carries the state vector (`[7,7]`); the test fails with the fix reverted.
- **Suites:**
  - `make check` 10130/10130, including the editor-parity audit;
  - gradle 593/593;
  - npm 180/180;
  - `make install` exit 0.

**Operator note:** the running IDE still has 0.2.446 loaded. The new registration behavior needs an IDE restart (or plugin reload) to take effect.

### Re: [#focuscrossrootverify] confirmed working — opus-5 · 2026-09-29T21:42-04:00

> **Queue prompt:** [#focuscrossrootverify]: confirmed working

Thanks. Closing `#focuscrossrootverify`: your live confirmation was the only remaining step, since the agent-side work (`#focuscrossroot`, 730d74600) was already complete and verified.

The agent-doc-bugs queue has no other live heads. When convenient, restart the IDE so plugin 0.2.448 (`#editorauth2` merge-forward registration) loads.
<!-- agent:boundary:d3c2a789:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority -->
<!-- /agent:queue -->


<!-- agent:backlog -->
- [/] [#apimdrevert] [agent-doc][operator-verify] src/haiven-dev/tasks/api.md 'reverted by queue updates' (2026-09-29): forensics found NO lost operator text — disk/controller/editor converged at c9273a0b (HEAD 1b432940 + Codex 00:49:49 queue stop+strike, applied as a 2038-byte CRDT delta against the converged base). Only editor-side anomaly: idea.1.log 00:40:02 local-splice-batch 3 splices 34346->34137 chars, then 00:40:03 exact inverse back to 25509a1e (undo/redo or plugin-internal flip; no VFS reload/projection logged). Blocked on operator: which text was lost + when (JetBrains Local History on api.md). Then trace the dropping path and fix with regression coverage.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#drivecapturedsplice] [agent-doc] Run the captured-splice live proof to completion (#activateinstalledjetbrai). HARNESS FIXED 2026-09-28 (095d2af54): the old 'xdotool keystrokes always arrived' premise was false — `type --window` goes through XSendEvent and JetBrains drops send_event=True keys, so nothing ever reached the editor. The harness now types via XTEST under an absolute-path focus guard, keeps scratch docs in tmp/live-repro/ (under .agent-doc/ they became a nested project with their own controller and log), waits only on fresh doc-scoped receipts, pipes a real response into the advance, runs it in the scratch doc's owner pane, and seeds a lint-valid boundary. A live run then produced the first fresh doc-scoped `editor_op_capture_proof operator_ops=N shadow_replay=agreed` any scratch doc has carried. REMAINING BLOCKER: the canonical advance is refused with 'retained document-write delivery from a prior cycle remains unsettled' while the captured edit is still unsaved in the editor (native_editor_save_request_deferred; authority/disk diverge on exchange). Decide whether an independent advance must be admitted while a captured operator edit is retained — that admission is what this recipe exists to prove — and fix whichever side is wrong. Re-run from a desktop with no stale-binary supervisors and a CLI build matching the IDE cdylib (the runs saw crdt_replica_build_mismatch_recovery_requested x4). Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#pypiquotarecheck] Track pypi/support#12214 (agent-doc project limit request, 25 GiB) to a decision. Unblocked mechanically now: a human reply on pypi/support#12214, or `make pypi-quota-check` exiting non-zero. Rechecked 2026-09-29: exit 0 — 8.362 GiB of 10.0 GiB, 1.638 GiB headroom, 151 releases (0.35.113..0.35.417), room for ~49,662 further post-cutover releases. Nothing to act on and nothing to delete. pypi/support#12214 is still open with one comment (the acknowledgement bot) and untouched since 2026-09-10T22:46:21Z. If the grant lands, record it and drop this item. Deletion stays reserved for a ceiling the check actually reports (next tranche would be 0.35.113..0.35.161, 2.491 GiB); verify any deletion from the authenticated /manage/project/agent-doc/ pages, never from pypi.org/pypi/agent-doc/json.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
