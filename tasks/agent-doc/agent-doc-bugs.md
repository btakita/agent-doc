---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 3d56a2a6-95ce-42c4-b71c-66c102e54fb4
  codex: 01a0b180-e3a6-77b1-a146-0756d3071cd9
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#actionable-review': Add actionable review items into backlog + queue
  '#agent-doc-causes': Fix agent-doc causes
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: go
---

## Status

<!-- agent:status patch=replace -->

GitHub #52 is closed after release-asset and PyPI verification. GitHub #53 is fixed and pushed on main at c92d5d629; atomic gate/type closeout composition is fixed at a10a684b3. Two full make check runs and both make install runs passed. The enforced weekly release gate defers tagging/publishing until 2026-09-22T16:41:52Z; CI runs 35250089483 and 35250915588 are in progress. Existing operator-verification gates remain open.
<!-- /agent:status --> 

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260917-164943.md`*

Compacted content:
- Archived 15 response topic(s): #fixcauseslazily; rename-safe panes and route-layout supersession; #fixbackloglog; 12 more
- Prior summary/context: prior compacted content: Archived 33 response topic(s): #timestampresponseheader; #idledispatchstack; preflight stall...

### Re: GitHub issues #52 and #53 — gpt-5 · 2026-09-17T13:08-04:00

> **Queue prompt:**
>
> do [#fixhttpsgithub]


Issue #52 is fully resolved: release assets carry the native library, PyPI now matches the latest published version at 0.35.398, and the issue is closed.

Issue #53 is fixed on `main` in `c92d5d629`. The tmux server-identity query now uses a printable space while its parser accepts arbitrary whitespace, preserving legacy tab-delimited state. JetBrains install/update now verifies the extracted plugin JAR and reports its plugin version, matching `plugin list`. Focused regressions passed, the full `make check` suite passed with zero failures, `make install` completed successfully, and the public-repo private-name scan is clean.

The first closeout attempt exposed a separate atomic tracked-work defect: `--backlog-gate` moved the item into review before `--backlog-set-gate-type` looked only in backlog. That owner lookup now follows the open item into `agent:review`, with an exact same-transaction regression test, in `a10a684b3`. The full suite and local install passed again. Both commits are pushed; CI runs `35250089483` and `35250915588` are in progress.

The architecture remains narrow. `tmux_server.rs` owns identity formatting/parsing; `plugin.rs` owns post-extraction verification/reporting; `backlog_cmd.rs` owns tracked-item component lookup. Tests cover printable and legacy identity forms, installed-JAR version reporting, and the gate-then-type transition. These are actorless one-shot boundaries with no long-lived derived-state relationship, so no reactive graph or imperative-state extraction applies.

Release and publication cannot complete today because the enforced weekly cadence gate is closed. Version 0.35.398 was published at 2026-09-15T16:41:52Z; the next complete release is eligible at 2026-09-22T16:41:52Z. I did not bypass that invariant. No additional backlog follow-up is needed because `#fixhttpsgithub` itself now carries the exact typed release gate and the remaining publish/verify/close steps.

### Re: macOS-only release cadence and editor-authoritative saves — Codex · 2026-09-17 14:57 EDT

> **Queue prompt:** Only OSX should be on the cadance. Linux builds can happen on demand.

Implemented and shipped the cadence correction: ordinary tags and the automated Linux/Windows assets are now on demand; only operator-built macOS assets retain the seven-day cadence. Release `v0.35.399` was tagged after the full gate passed. Its Release workflow is still running, so `#github5253release` remains correctly gated until the draft release is published and all expected assets can be verified.

Also fixed the `fpe.md` editor/disk divergence class at its ownership boundary. The live editor is canonical and disk is only a persistence receipt. A generation-fenced native save may surface the host editor File Cache Conflict; agent-doc retains the authoritative response while the user resolves it, then settles from the later exact native-save event without another patch, finalize, commit, or session-check attempt. It never reloads or merges the third-party disk value into the editor.

Plugin parity is enforced across JetBrains, VS Code, and Zed: JetBrains observes the post-conflict VFS write, VS Code observes `onDidSaveTextDocument`, and Zed accepts `didSave` only when editor shadow, replica, and disk bytes agree. A shared conformance test pins all three behaviors.

Verification passed: full `make check`, 179 VS Code tests, targeted JetBrains compilation/tests, Rust Zed and cross-editor receipt regressions, private-name hygiene, and `make install`. The installed JetBrains and VS Code packages were rebuilt, and controllers/supervisors were scheduled for safe recycling. Core/plugin fix: `fef46e9fe`.

### Re: Response

> **Queue prompt:** File Cache Conflict occurred, I in sdk.md or fpe.md. Fix this class of error.

Fixed and pushed as `fd89a1d8e`.

The root cause was a same-version deployment gap: newly installed plugin code and an already-running stale plugin both reported the same package version, so the controller treated the stale endpoint as current and repeatedly requested a native save.

The fix makes the editor authoritative and fences every disk-writing effect:

- `persist_current` is sent only to a live JetBrains or VS Code registration whose reported plugin generation exactly matches the controller's baked package generation.
- Unversioned replica identities can still receive CRDT notifications, but can no longer authorize a disk-writing effect.
- Zed remains on its editor-native `didSave` path until it has a native endpoint.
- A third-party disk edit is not merged or overwritten by the controller. The editor owns the File Cache Conflict and, once the user resolves it in the editor, the editor's native save overwrites disk and publishes the exact receipt.
- Generation mismatch is a retryable, explicit diagnostic instead of another save/commit/session-check/finalize loop.
- Package generations were bumped together: JetBrains `0.2.387`, VS Code `0.2.71`, Zed `0.1.1`.
- The normal `make check` now rejects editor behavior changed after the last package-generation bump across JetBrains, VS Code, and Zed, including already-committed same-version drift.

Also fixed the aggressive tmux focus shift: the fast automatic tab-focus path had bypassed the desktop-activity guard. Automatic focus-only and layout focus now both refuse to select the right-hand tmux pane while the operator is in a terminal or another application. Explicit focus and manual layout actions are unchanged.

Verification passed: 9,484 Rust tests, JetBrains and VS Code plugin suites, cross-editor native parity, bootstrap checks, Lean/TLA gates, private-name scan, install, and the latest prior GitHub Actions run. The commit is pushed to `main`.

Restart/reload the running IDE host once to activate the bumped plugin package. Until then, the new controller fails closed on the old reported generation instead of sending another native-save request.

### Re: Editor-authoritative closeout and busy-turn rerouting

> **Queue prompt:**
>
> Fix agent-doc causes to `- 🚧 do [#sbxdeploymentkick]` remaining in fpe.md after the turn completed. Run Agent Doc seems wedged: `dispatch-only codex reopen refused to inject into pane %17 for /home/brian/work/btakita/agent-loop/tasks/software/tsift.md because the latest run is still booting and never reached a dispatch-ready prompt (timed_out); wait for the pane to become ready and reroute again ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=wait_for_dispatch_ready_prompt`. There are also 3 tmux panes with 2 editor panes. I ran Sync Tmux Panes and the pane layout corrected itself. I ran agent-doc directly from the session.

Fixed and pushed in two agent-doc commits:

- `fd89a1d8e` fixes the editor/disk divergence class. The fpe history and ops log show that closeout had already removed `🚧 do [#sbxdeploymentkick]` from the editor-authoritative projection, but native save was sent to a dead/stale JetBrains endpoint and disk stayed behind. Native disk-writing effects are now generation-fenced to a live JetBrains or VS Code registration running the exact expected plugin generation. Versionless CRDT identity can no longer authorize `persist_current`, Zed remains on its didSave path, and the controller never falls back to overwriting the editor from disk. A third-party disk edit remains the IDE/user's File Cache Conflict to resolve.
- `910008f08` fixes the false booting wait. Pane `%17` had accepted the first routed run and its authoritative actor was already busy, but a duplicate editor reroute let a historical startup probe win and waited about 119 seconds before returning `timed_out`. A healthy same-pane authoritative actor in `busy` state with an active-turn pane blocker now settles that stale startup probe immediately, then uses the existing queue-behind-active-turn path instead of reporting that the pane is still booting.
- The earlier patch also fixes automatic tmux focus moving to the right-side pane: automatic focus now respects desktop-editor activity, while explicit/manual focus remains available. The three-pane/two-editor geometry itself had already converged when Sync Tmux Panes was run, so no destructive layout action was applied to the live fpe or tsift sessions.
- Plugin parity is enforced across JetBrains, VS Code, and Zed by one shared expected-version authority and a normal `make check` gate. Installed versions are JetBrains `0.2.387`, VS Code/Codium `0.2.71`, and Zed `0.1.1`.

Verification passed: the focused controller/route suites, the full `make check` repository gate, private-name denylist, plugin parity, and installation. The latest GitHub Actions run for the generation-fence commit is green. The new binary/library is installed, and 10 supervisors plus 3 controllers were scheduled to recycle at idle boundaries.

Restart the running JetBrains IDE once to activate plugin `0.2.387`. Until that restart, the generation fence intentionally refuses native save requests from the stale `0.2.386` process rather than risking another editor/disk divergence. Neither `fpe.md` nor `tsift.md` was mutated during this diagnosis.

### Re: Response

> **Queue prompt:**
>
> sdk.md auto-queue stalled. #agent-doc-causes


Verified. IntelliJ is running plugin `0.2.387` under new PID `1545434`, with generation fencing and editor-authority capabilities registered. All seven open documents—including `agent-doc-bugs.md` and `tsift.md`—are live and projection-complete.

No further restart is needed.

### Re: sdk.md queue stalled #agent-doc-causes — gpt-5.6-sol · 2026-09-17T16:51:43-04:00

> **Queue prompt:** sdk.md queue stalled #agent-doc-causes

Implemented and deployed the queue-stall fix across the shared runtime.

- Root cause: the supervisor dedup latch remembered only the last prompt text. If another head was handled through an attended or manual path, the latch stayed armed; when the older text returned, the supervisor silently classified it as already dispatched. The durable projection compounded this because completed node identities were absorbing and selected/deferred event IDs could collide across queue generations.
- Fix: dispatch dedup now re-arms on any intervening head; a live editor worklist reopens a previously completed node; selected/deferred state events are fenced by the queue-component generation. Exchange-only edits do not manufacture a new queue generation.
- Editor authority: the previously landed native-save changes keep the IDE buffer authoritative and overwrite disk through a generation-fenced editor save. External file changes remain an editor File Cache Conflict for the user to resolve.
- Parity: the behavior is in the shared Rust queue engine. The parity installer rebuilt and installed the CLI/library, JetBrains plugin 0.2.387, VS Code extension 0.2.71, and refreshed the Claude Code, Codex, Cursor, Grok, and OpenCode skills.
- Tmux focus: the stashed-document focus repair is already landed in commit 3a73c09f0, preventing the router from repeatedly pulling focus to the right-side pane.
- Verification: focused queue/runtime tests passed; the complete make check suite passed; make install passed. Source commit ed6085856 is pushed to origin/main. GitHub CI and Book workflows were queued for that exact SHA at closeout.
- Live SDK check: the SDK pane is actively working on #sdkauthunreal. The stale document projection still showed #sdkauthts, matching the completed-node reuse defect fixed here; the installed supervisor reports fresh generation 27.

### Re: Notes edits should not start turns — gpt-5 @ 2026-09-17T18:54:00-04:00

> I added content into agent:notes, but agent-doc ran a turn on the notes anyways. It should be informational but not cause a turn to occur.

Implemented and shipped in `aa061cb76`.

- **Invariant and owner:** `agent:notes` body changes are context-only and cannot create a runnable diff. The pure `agent-doc-element` turn-role policy owns that decision; `agent-doc-diff` consumes it before preflight dispatch.
- **Transitions covered:** notes-only changes produce no diff/no turn; notes plus an exchange prompt preserve the prompt and run; malformed and unknown components fail open and remain turn triggers.
- **Reactive topology:** editor/current document → comment and informational-component normalization → unified diff → existing preflight/dispatch decision. No new long-lived state, polling, or imperative effect path was added.
- **Surfaces changed:** element descriptors and registry, pure diff and diff-I/O regression coverage, functional specs, and the required tsift navigation refresh. Existing unrelated JetBrains/session-actor work was left untouched.
- **Verification:** focused descriptor/registry/diff/diff-I/O tests passed; full `make check` passed; the public-repo private-name denylist returned no matches; `make install` completed successfully.
- **Delivery:** pushed `aa061cb76` to `origin/main`. GitHub Actions CI run `35284240619` was queued at inspection time.

No blockers remain.

### Re: notes/queue completion cleanup — gpt-5 · 2026-09-17T21:02:53-04:00

> **Queue prompt:**
> agent-doc did not not clear the queue item in this documnet when complete. agent-doc did not strike or clean `- Look at /home/brian/work/btakita/agent-loop/src/haiven-dev/tmp/HarmblockSDK_LINUX_M1L1_hAIven_Alpha + https://github.com/haiven-dev/haiven-api/pull/519. Can this be integrated into the fpe?` in fpe.md when complete. Please fix this class of errors.

Fixed and cleaned up.

The FPE text was stored in `agent:notes`, not `agent:queue`. Notes are persistent operator-owned context, so queue completion never had authority to strike or clear that line. The earlier defect was that note prose could nevertheless be interpreted as a runnable prompt. The current diff layer already neutralizes informational component bodies; this change locks down the mixed case where a real queue prompt and a notes edit arrive together, ensuring only the queue prompt is emitted as completion work.

I added the regression `notes_change_with_queue_prompt_reports_only_the_queue_prompt`, documented the authorization boundary in the diff specification, and updated every generated harness runbook: notes remain context, but are not answered, struck, cleared, or otherwise mutated unless an explicit exchange/queue prompt asks to edit notes. That prevents future work from being “completed” out of notes while preserving legitimate persistent context.

The historical FPE note was explicitly removed through agent-doc's authority-safe patch path and committed/pushed on `docs/fpe-service-topology-temporal-plan` as `8fda4aca5`. The product hardening is committed/pushed on agent-doc `main` as `f833a7b9b`.

Verification: focused regression passed; full `make check` passed; `audit-docs` passed; the matching local binary and editor packages were installed; the live controller was recycled. The newly pushed GitHub Actions CI run is queued, and the preceding `main` CI run completed successfully.

### Re: fpe.md closeout/admission recovery — gpt-5 · 2026-09-17T23:37:00-04:00

> **Queue prompt:**
> fpe.md is having issues completing and starting new turns. Fix agent-doc causes.

Fixed and shipped in `16070edb4`.

- Root cause: a JetBrains replica could repeatedly pull the same retained update without projecting or acknowledging it. Each duplicate pull erased the relay's independent stalled-wait evidence, so the closeout delivery barrier could remain active indefinitely and admission reported `delivery_pending`/`open_cycle`.
- Fix: duplicate pulls no longer reset expired-wait evidence; bounded nonconvergence release has a one-shot recovery latch; relay I/O now re-registers the editor replica for every bounded nonconvergence release, not only the old exact redelivery-count case. Genuine progress and fresh obligations reset the latch.
- Regression coverage proves repeated pulls cannot erase stalled-wait evidence, the barrier releases after bounded waits, and recovery signaling is one-shot. The focused regression, both affected crate suites, `cargo fmt --check`, and full `make check` passed.
- Delivery: `16070edb4` is pushed to `origin/main`; the matching CLI/library, JetBrains plugin 0.2.387, and VS Code package were installed. GitHub Pages/book workflows passed; CI run `35303586580` was still in progress with no reported failure at final inspection.
- Live FPE proof: retained cycle `cycle-1789700747711` converged with the editor, reached `Committed`, and generation 16 immediately admitted and began the next cycle (`cycle-1789702637186`). The subsequent session check reports that new cycle as `preflight_started`, which is expected active-turn state and proves new turns start again.

This session document is heavily accreted and should be compacted after its active queue is drained.

### Re: fpe.md retained-closeout recovery — gpt-5 · 2026-09-18 00:29 EDT
> **Queue prompt:**
> fpe.md is still wedged. fix agent-doc causes. Also build in a recovery mechanism. authoritative actor generation 16 for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md owns pane %5 but route could not drain the active closeout before dispatch: closeout recovery blocked [open_cycle]: missing proof: open cycle must finish, be replayed, or be explicitly queued behind; recommended: resume durable checkpoint cycle=cycle-1789703140795 phase=write_applied target="- What is the full url in sbx + stg for /admin/chat-safety/status?" capture_id=cycle-1789703140795; the canonical write is already applied; retained commit recovery owns the terminal boundary; do not resubmit finalize or write for /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/fpe.md

Fixed the agent-doc causes and verified recovery against the live wedged cycle without resubmitting its write/finalize or patching `fpe.md`.

Two independent recovery gaps were involved:

1. A silent registered editor could exhaust the bounded delivery wait and leave the barrier, but re-registration was only requested from a later replica pull. A replica that never pulled again therefore had no path back to projection. Commit `f9c12352c` (`fix: recover silent replica delivery`) makes the bounded-wait release claim the one-shot recovery latch and send `EditorReplicaReregister` itself; a later pull cannot duplicate the signal.
2. A newer durable persistence epoch could arrive while an older native-save request was in flight. The table retained the newer epoch, but after the old attempt released the single-flight token the reactive output was unchanged, so no second Effect edge occurred. Commit `5af59b5c1` (`fix: hand off superseding retained persistence`) makes completion atomically claim and enqueue the retained successor. It deliberately does not reclaim the same failed epoch, preventing retry spin.

Regression coverage includes silent-replica release without a pull, preservation of stalled-wait evidence, superseding persistence inheriting the released token, and same-epoch failure remaining quiescent. The relay/realtime suites passed 379 tests, the complete controller-IO suite passed, `cargo fmt --check` passed, and full `make check` passed with zero failures. Both commits are pushed to `origin/main`; `make install` installed agent-doc 0.35.399 and recycled the controller/supervisor onto the installed inode. GitHub Book is green; CI run 35306690901 was still executing its `make check` stage with no failures when this response was captured.

Live proof: the retained FPE cycle recovered under controller ownership, reached `crdt_commit_barrier ... ready=true ... disk_projection_ready=true`, and logged `flow_event ... stage=commit outcome=completed` at 04:23:26Z. The supervisor then dispatched the next queue item; `fpe.md` is now busy in new cycle `cycle-1789705413812`. No operator save, force-disk path, manual document edit, finalize replay, or second diagnostic session-check was used.

There are no actionable follow-up items from this completed fix.

This session document is accreted; compact it in a later dedicated cycle rather than stalling the current queue continuation.

### Re: release + publish — gpt-5 · 2026-09-18T11:33:15-04:00

> **Queue prompt:**
> release + publish

Released agent-doc v0.35.400.

- Changed paths: projected 0.35.400 across all 144 Cargo manifests and internal constraints, Cargo.lock, pyproject.toml, the source and five installed skill mirrors, VERSIONS.md, and editors/release-parity.tsv. Release commit: `77c2c2b8b chore: release v0.35.400`.
- Commands: `make release-version VERSION=0.35.400`, `make check`, `npm ci` for the clean clone's locked VS Code dependencies, and `make release`.
- Publication: pushed `main` and tag `v0.35.400`. The tag-triggered Release, PyPI, and CI workflows are in progress; Book and Pages are green. The GitHub Release object is not created yet because its workflow is still running, which is recorded rather than awaited per the repository release contract.
- Verification: the complete gate passed, including 9,498 Rust tests, 179 VS Code tests, JetBrains tests, cross-editor native parity, release projection/docs/editor parity audits, Python bootstrap tests, Lean, and TLA. The installed executable reports `agent-doc 0.35.400`; the release install also installed the matching native library, JetBrains 0.2.387, and VS Code 0.2.71.
- Realtime steering: the fresh Monster Rodholders run recovered and committed the missed queue strike. Commit `cf05b94c` changed `- 🚧 do [#netcupinstancesavailable]` to the struck form, and `5bef6ce5` struck the following free-text head. No further Run Agent Doc is needed for that line. This recovery exercised the reused-queue-head rearm fix included in 0.35.400.
- Confidence: high. Escalation: none.

The semantic match to the older issue-46 completion was advisory only; this cycle performed a new release. Session accretion is elevated, but the active queue drained without a compaction stall.

### Re: complete the FPE caveat item — gpt-5 · 2026-09-18T11:36:03-04:00

> **Queue prompt:**
> In fpe.md, the backlog item was not marked as complete `- [ ] [#caveatmeanapply] What does the "caveat" mean and when will it apply? When there is a timeout?` after it was completed.

Corrected the missing lifecycle transition.

- Completion evidence: the compacted FPE archive contains both the substantive answer and an explicit closeout stating that the caveat investigation was completed after tracing the production API/FPE paths and live task definition.
- Changed paths: `src/haiven-dev/tasks/fpe.md` and `src/haiven-dev/tasks/fpe.done.md`.
- Command: `agent-doc write --commit src/haiven-dev/tasks/fpe.md --origin skill --backlog-only --done caveatmeanapply`.
- Result: the item was atomically completed and reaped into `tasks/fpe.done.md`; commit `122c210 agent-doc(fpe): 2026-09-18 11:35:35`.
- Verification: FPE `session-check` reports the cycle committed with `no_drainable_work`; the active file no longer contains the item, the done archive does, and both paths have no working-tree drift.
- Confidence: high. Escalation: none.

Exchange accretion remains elevated, but the active queue was drained without pausing for compaction.

### Re: JetBrains focus effect fencing and FPE recovery — gpt-5 · 2026-09-18 16:39 EDT

> **Queue prompt:**
>
> do [#fixchromiumbridge]


Implemented and shipped the JetBrains focus-effect race fix, while preserving the newer lifecycle-locked generation fence already on `main`.

The effect now has a typed request/response boundary. Its request carries the admitted focus generation and target document; its response is classified under the listener lifecycle lock as applied, structural repair, retained, superseded, or transport unavailable. Only a response whose generation is still owned by the admitted surface observation may install the reverse-focus lease or enqueue forced layout repair. A newer admitted observation invalidates and interrupts the in-flight effect, and rejected lower-authority observations cannot cancel valid focus work.

The passive focus socket request has a dedicated 1-second deadline. Cancellation plus this deadline keeps a stale or wedged controller response from blocking newer selection work; stale, timed-out, and failed responses are inert. The controller/Rust surface protocol is unchanged.

Changed paths:
- `editors/jetbrains/SPEC.md`
- `editors/jetbrains/gradle.properties` (plugin `0.2.388`)
- `editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CpRouteClient.kt`
- `editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt`
- `editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/EditorTabSyncListenerTest.kt`

Proof:
- Focused JetBrains tests passed: `CpSocketDeadlineTest` and `EditorTabSyncListenerTest`.
- `make bump-plugin` built `agent-doc-jetbrains-0.2.388.zip`.
- Full `make check` passed with zero failures.
- `make install` succeeded for CLI/skill `0.35.400`, JetBrains `0.2.388`, and the VS Code extension.
- Commit `5bfe9bd3b` (`fix: bound JetBrains focus effect responses`) was pushed to `origin/main`.
- GitHub Actions CI run 35392633692 is in progress for that exact commit.
- Confidence: high. Escalation: none.

Unblocked `src/haiven-dev/tasks/fpe.md` first. Controller status proved pane `%5` still owned an active empty `preflight_started` cycle; `agent-doc session cancel-turn` explicitly abandoned cycle `cycle-1789763038157`, preserved context, and restarted generation 16 on its queued work. No response, document write, or commit was resubmitted. Queued `#duplicatepreflightunblock` at the agent-doc queue tail to make this recovery bounded and automatic.

Accretion: retained 199 exchange lines and 12 response sections without compaction.

### Re: harness switch to Claude — opus-5 · 2026-09-20T00:21-04:00

> **Queue prompt:** `agent: codex` → `agent: claude`

Claude owns this session now. Nothing else in the document changed, so this cycle is a handoff plus verification of the two things the last Codex turn left open.

**CI is green.** Run `35392633692` completed `success` for `5bfe9bd3b` (`fix: bound JetBrains focus effect responses`) — the run the prior turn reported as "in progress". The two runs behind it (`35389414278`, `35386729040`) are green as well.

**`#release035376verify` is satisfied — closed.** `v0.35.376` is published (not a draft, `2026-09-13T05:05:15Z`) and carries all six platform assets: `aarch64-apple-darwin`, `x86_64-apple-darwin`, `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`, `x86_64-pc-windows-msvc`. The gate was an external queue that has since drained, so this needed no operator proof. Review is now 10 items.

**Not touching `src/agent-doc` this cycle.** Preflight's `stale_install` warning is accurate — the installed `0.35.400` predates `6eb81f3bd` (`fix: defer compaction while operator typing is active`) — but another session holds uncommitted work in that repo right now:

- `M editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TurnStateBannerRefresher.kt`
- `M src/session_actor_cmd.rs`
- `?? editors/jetbrains/.../InputRequiredNotifications.kt` + its test

Running `make install` against that tree would build and publish someone else's in-flight edits. This cycle does not own agent-doc development, so the owning cycle installs and the supervisor recycles.

**Queue is stopped and empty.** The backlog head is `#duplicatepreflightunblock` (bounded cancellation / stale-cycle recovery for a duplicate invocation blocked behind a `preflight_started` cycle) — ready to drain on `do [#duplicatepreflightunblock]` or a `queue: go`.

Exchange is at 232 lines across 13 response sections, just past the warn threshold. Say the word and I'll run `agent-doc compact tasks/agent-doc/agent-doc-bugs.md --commit`.

### Re: retained delivery projection is a deferral, not a half-apply — opus-5 · 2026-09-20T00:43-04:00

> **Queue prompt:**
>
> do [#retainedprojexit1]


Fixed `#retainedprojexit1`. Commit `b7043757b`, pushed to `origin/main`.

**Root cause.** The closeout's tracked-work mutation phase (`run_command_inner_within_pass`, `agent-doc-write-runtime-io/src/lib.rs`) wrapped *every* failure that reached it in one `with_context` message: "the document is half-applied", plus `agent-doc commit <FILE>` and a `write --commit --backlog-only` rerun as the remedies. Nothing classified the failure first. On `cycle-1789878037902` the failure was a retained refusal stamped `retained=delivery_projection_pending` — the mutation envelope had reached the editor authority and its keyed worker converged seconds later — so the message asserted a half-apply that never happened and named two remedies that would have double-applied the tracked-work half that had already landed.

**Fix.** Classify before rendering, from facts that can be proven rather than from "the mutation phase returned `Err`":

- **Retained delivery projection → deferral.** Recognised by the stamped `RETAINED_DELIVERY_PROJECTION_PENDING_TOKEN` (not prose — that needle is what let this class stay unhandled in three crates already: `#retainconv`, `#retaineddeferisnotafailure`). The command logs `tracked_work_mutations_deferred_retained_delivery_projection`, prints the deferral, and returns **exit 0 without committing**, naming only `agent-doc session-check <FILE>`. It does not commit, because the projection has not reached disk yet and committing there would publish the partial state the old message falsely claimed.
- **Half-applied framing is now gated on proof.** It fires only when this cycle's own recorded mutations are still missing from the document per `recorded_tracked_work_is_unlanded` — the same predicate `commit`, `session-check`, and the captured-closeout resume decide provenance from. Unprovable (no cycle state, document unresolvable) stays on that fail-closed branch.
- **Provably-landed mutations get a fourth verdict.** `ProjectionFailedAfterLanding` says the failure is in the projection tail after both halves landed and carries no resubmit remedy, because resubmitting a landed mutation double-applies it.

Classification and wording live once, in `agent_doc_turn::write_ownership`: `classify_tracked_work_mutation_failure` + `tracked_work_mutation_failure_message`, alongside the existing `RetainedWriteOwnership` owner.

**Coverage.** Five new tests in `write_ownership.rs`:

- the retained token classifies as a deferral even when the landing witness says "unlanded";
- a deferral message names `session-check` and no resubmit remedy;
- half-applied requires proof, and `None` (unprovable) stays fail-closed;
- only the proven half-apply renders `--backlog-only` (parameterised across all four verdicts);
- a structural guard that the closeout mutation phase actually calls the classifier, the message owner, and the landing witness, and no longer re-authors the half-applied wording. Two of these tests caught the first draft reproducing the resubmit commands inside its own do-NOT list — the wording was rewritten so the flags are not copy-pasteable from a deferral.

Spec bullet added to `specs/07-closeout-commands.md` under the `write` invariants.

**Verification.** `make check`: **9508 tests run, 9508 passed, 259 skipped**. `audit-docs` fails, and it is not this change — `.grok/skills/agent-doc/SKILL.md` and `.codex/skills/agent-doc/SKILL.md` in the superproject carry an `agent-doc-version` stamp bump (`0.35.398` → `0.35.400`) from the concurrent session's uncommitted work; my three files are `agent-doc-turn/src/write_ownership.rs`, `agent-doc-write-runtime-io/src/lib.rs`, and `specs/07-closeout-commands.md`. CI run `35489614892` is **in progress** on `b7043757b`; the parent `5bfe9bd3b` was green.

**On the push.** `origin/main` had moved 5 commits ahead and local `main` held a duplicate of one of them (`6eb81f3bd` ≡ origin's `2e1219b95`). Rebased with `--autostash` — none of the incoming commits touch the concurrent session's uncommitted files (`src/session_actor_cmd.rs`, `TurnStateBannerRefresher.kt`, two untracked JB files), and the autostash reapplied cleanly with their work intact.

**Not installed this cycle, deliberately.** `make install` builds from the shared working tree, which currently holds another session's uncommitted `src/session_actor_cmd.rs` and JetBrains plugin edits — installing would ship their in-flight work as the live binary. Preflight's own `stale_install` advisory says the same thing: the owning development/release cycle installs and the supervisor performs the safe recycle. Separately, `make install`'s `lib-install` step reloads the cdylib, which strands this attached document's Lazily replica mid-session and needs an editor tab reopen to clear. Queued as `#retainedprojexit1install` at the backlog head so the next cycle with a clean tree picks it up.

### Re: the empty-preflight reclaim had exactly one proof, and only an operator could carry it — opus-5 · 2026-09-20T01:03-04:00

> **Queue prompt:**
>
> do [#duplicatepreflightunblock]


Fixed `#duplicatepreflightunblock`. Commit `ea57c13b6`, pushed to `origin/main`.

**Root cause — a structurally dead branch.** Reclaiming an empty `preflight_started` cycle is gated on a proof, and correctly so: that shape is *also* the normal state while a model generates its first response, so abandoning it aborts a live turn (the `#suprecyclespin-falseabandon` regression). But there was exactly **one** proof — `run_cancelled` — and it is reachable from exactly one place, `agent-doc session cancel-turn`, which a human has to type at the right moment.

Every other caller passed "unproven" and got `Protected`. That includes the route closeout drain: `route_cancel_empty_preflight` bound to `cancel_preflight_cycle`, the *unproven* entry point, so `closeout_drain.rs`'s `if cycle.is_empty_preflight() && … && (effects.cancel_empty_preflight)(file)?` could **never** be true. The branch reads like automatic recovery and did nothing. A duplicate `agent-doc <FILE>` invocation therefore fell through to repair → `session-check` → a 30s projection await → `Blocked`, and every retry repeated it — indefinitely. Exactly the `fpe.md` wedge on pane `%5` (`cycle-1789763038157`).

**Fix — a second proof, not a weaker gate.** `agent_doc_turn::repair::EmptyPreflightCancelAuthority` now owns the authority vocabulary: `Unproven`, `RunCancelled`, `OwnerReleased`. The drain reclaims on the `RecoverAfterOwnerRelease` branch, where a controller projection has proven the owning actor released the cycle — after a bounded 30s await, with no operator in the loop.

**The first cut of that was wrong, and the existing suite caught it.** `plain_run_trigger_cannot_overtake_a_fresh_open_preflight` went red: the drain's projection await *also* returns `OwnerReleased` when there is no controller projection to release anything, which is indistinguishable from a brand-new cycle whose model has not started answering. Reclaiming on release alone abandoned a live turn. So `OwnerReleased` now additionally requires the cycle to be stalled past `STALLED_CYCLE_RESOLVE_SECS` — the deadline sized in `#suprecyclespin-falseabandon` for precisely this hazard, because a slow first response ticks nothing while it generates and so must not look stalled. `RunCancelled` needs no deadline: the caller stopped the run itself.

The authority widens **who** may reclaim, never **what**. A cycle past `preflight_started` or owning a response capture is still protected, and an unproven caller still gets `Protected` with `reason=run_cancel_not_proven`. A release refused for want of the deadline logs `reason=owner_released_cycle_not_stalled`, so a wedge is distinguishable from a refusal in the ops log.

**Coverage.** 3 unit tests (each authority's cancel authorization; only `OwnerReleased` requires the stall; proof tokens are distinct and stable for ops-log grep), 3 new integration tests in `agent-doc-repair-command-io` (a stalled empty preflight is reclaimed; a **fresh** one is protected; a capture survives a released owner), and a structural guard that the drain **calls** the owner-release reclaim *on the owner-release branch* and that the runtime effect binds to the owner-release entry point — a declared-but-uncalled effect is the exact shape that made the original branch invisible.

I attacked the guards rather than reasoning about them. Three mutations, each reverted after: moving the reclaim off the owner-release branch (structural guard red), rebinding the effect to the unproven entry point — the original defect (structural guard red), and neutering `OwnerReleased` in `authorizes_cancel` (unit + integration red). While restoring one of them I reverted my own in-progress fix with `git checkout --` and had to reapply it; [[feedback_mutation_script_restore_must_not_use_git_checkout]] says exactly that and I did it anyway.

**Verification.** `make check`: **9516 tests run, 9516 passed, 259 skipped** — including the route test that caught my wrong first cut. `audit-docs` still fails on the same pre-existing `.grok/skills/agent-doc/SKILL.md` version-stamp drift from the concurrent session's uncommitted work; my six files are `agent-doc-turn/src/repair.rs`, `agent-doc-repair-io/src/lib.rs`, `agent-doc-route-io/src/{closeout_drain,runtime_effects}.rs`, `agent-doc-repair-command-io/tests/repair.rs`, and `specs/07-core-commands.md`. CI `35490686334` is **queued** on `ea57c13b6`; the prior run `35489614892` on `b7043757b` is still in progress.

Install remains deferred for the same concrete reason as last cycle — the shared tree still holds another session's uncommitted `src/session_actor_cmd.rs` and JetBrains plugin edits — and is already tracked as `#retainedprojexit1install`, which now covers both fixes. Exchange is at 289 lines / 15 response sections; the queue is active, so I am not stopping to compact.

### Re: the review list was ten gated items, and four of the gates were not real — opus-5 · 2026-09-20T01:10-04:00

> **Queue prompt:**
>
> do [#kqht]


`#kqht` is `#actionable-review`: audit `agent:review` against the complete-over-gate rule. I checked each blocking condition against the live system instead of trusting its prose, and four of the ten did not survive that check. Review is now **7**, under the < 10 target.

**Resolved — the gate was already satisfied.**

`#fpecompactheal` asked an operator to run one compact from the pane owning `fpe.md` and confirm the corrupted `<dynamic_context_ref>` block was gone. It already is: `fpe.md` is 107 lines with **zero** `dynamic_context_ref` occurrences and no self-referential handles, and line 33's `Prior summary/context:` line summarises the prior compact list in prose (`Archived 58 response topic(s)...`) rather than quoting XML — the second half of the stated expectation, verbatim. The item predicted "exactly ONE block" and there are none, which I am flagging rather than smoothing over; what it was guarding against — a duplicated, self-referential block — is definitively absent. No operator eyeball adds anything to a file read.

**Ungated to actionable — real work, not an operator gate.**

`#fixrunlazily` carries no `[operator-verify]` tag and never should have sat in review. It is a deeply narrowed bug with two hypotheses already measured and disproved and a named NEXT LEAD (what holds a lock across `handle_request_locked` at `runtime.bootstrap_snapshot()` during a controller's first seconds). Nothing about it is blocked on a human; it is unstarted implementation work, which belongs in `agent:backlog`.

`#github5253release` was gated behind "no earlier than 2026-09-22T16:41:52Z" — exactly seven days after v0.35.398. **That gate is stale and provably unhonoured:** v0.35.399 (09-17) and v0.35.400 (09-18) both shipped inside it, because the policy changed — only Darwin asset uploads are weekly now, while tags and Linux/Windows assets publish on demand. I verified the substance rather than the date: `c92d5d629` and `a10a684b3` are both ancestors of the released `v0.35.400` tag, `tmux_server.rs` now queries with a printable space and parses via `split_whitespace()`, v0.35.400 carries all four automated platform archives plus `SHA256SUMS`, and PyPI's latest is 0.35.400. So the "verify GitHub assets and PyPI" half is **done**. What remains is the outward-facing step — replying on issue #53 and closing it — which I deliberately left for its own cycle rather than closing an external reporter's issue as a side effect of a hygiene pass.

**Kept gated, premise corrected — three items were chasing a dead process.**

`#tmuxfocussyncverify`, `#activateinstalledjetbrai`, and `#harnessswitchverify` all instructed the operator around "IntelliJ PID 1035669 on plugin 0.2.378, staged 0.2.379". PID 1035669 no longer exists; the live IDEA is **PID 1645748**, started 2026-09-18 16:22:20, and the plugin on disk is **0.2.388**, written 16:39:05 — seventeen minutes *after* IDEA started, so the staged-needs-restart shape is still genuinely true, just nine releases further along. `#harnessswitchverify` also cited CLI 0.35.361 against an installed 0.35.400. The proofs themselves still need a human in a live editor, so all three stay in review with refreshed premises.

While measuring this I had to correct my own instrument: `ps -eo cmd | grep idea` returned zero matches and would have had me report IntelliJ as not running. It truncates the command column; `pgrep -f "idea-IU"` returns 25. The PID/plugin facts above come from `/proc/<pid>/cmdline` and `stat`, not from that grep.

**Left untouched — genuinely blocked, premise accurate.** `#cargovodtrim` (needs the VOD locator), `#pypiquotafix` (needs a browser sign-in), `#grokmcpverify` (needs quota or the subscription), `#coder7` (needs a reachable workspace plus auth). Each names exactly what unblocks it.

Before running the mutation envelope I validated its shape on a throwaway document — `--review-remove` + `--backlog-add`, `--backlog-ungate` on both plain `[/]` and typed `[/release]` gates, and `--review-edit` — because a wrong flag here fails the closeout, and two of those behaviours were not documented in the help text. The first scratch attempt died on the AF_UNIX `sun_path` limit under the scratchpad path; retried under `/tmp/ungt`.

No repo changes this cycle, so no `make check` run: the deliverable is the tracked-work state itself. Exchange is at 316 lines / 16 response sections; the queue is active, so I am not stopping to compact.

### Re: #resumereplaynotidempotent, and Run Agent Doc not submitting on contracts.md — opus-5 · 2026-09-20T12:17-04:00

Both landed in `275ab868e` (pushed; CI queued at the time of writing).

**`#resumereplaynotidempotent` — fixed as scoped.** The drop now happens per-entry in `apply_unlanded_captured_mutation_plan`, against the already-resolved `current_content`, before `has_pending_mutation()`. The four guarded `recorded_tracked_work_is_unlanded` call sites are untouched, so the cycle-wide predicate keeps its existing meaning.

Both sharp edges you flagged are in the code and in the tests:

- **resolve/remove** witness is *absent from the `agent:review` component entirely* — `review_component_contains_id`, which parses the component and sees every state and gate type. Not `collect_gated_review_ids`: it `strip_prefix`es `- [/]` only, so a typed `- [/release]` gate is absent from that set and using it would have classified an unlanded typed-gate replay as already-applied and dropped it silently.
- **ungate** witness is *still gated anywhere* — `id_is_gated_in_tracked_work` checks review **and** backlog, because `backlog_cmd::ungate` resolves against review first and falls back to the pending component. "Absent from review" would have read a backlog-gated item as already ungated.

Both witnesses answer `true` when the document cannot be parsed or the id normalizes to nothing: keeping a replay is recoverable, dropping one is silent tracked-work loss.

Coverage runs both directions per shape (already-applied → filtered, not-yet-applied → still replayed), plus a mixed plan where only the applied half is dropped. Five mutations, all caught: no filtering at all; each witness swapped for the wrong one; the review witness blinded to typed gates; gated-anywhere narrowed to review only. A dropped entry is logged as `captured_finalize_resume_tracked_work_already_applied ... dropped=review-resolve:#id,...`.

I did **not** extend this to `--review-edit`. It is the same error-on-missing family, but its "did it land" question is a text comparison rather than a presence check, so it is a different witness and was outside what you scoped. Worth a look if it ever shows up in a latch.

**Run Agent Doc on contracts.md — reproduced and root-caused, different bug.** Not `#fixrunlazily` (that is the 60s controller wedge). Pane `%19` still held `❯ /agent-doc …/contracts.md` unsubmitted when I captured it, and ops.log has the whole story: `route_pass_through_submit_draft … outcome=cleared enters_sent=0`. The repair loop that exists to press Enter on a stranded draft decided the composer was clear.

It was wrong because `route_trigger_visible_in_current_draft` scanned its 16-line window **oldest-first and returned on the first match**. The 11:59 consumed `agent-doc …/contracts.md` sat 12 non-empty lines above the 16:05 draft of the same trigger, so the predicate answered for the *consumed* copy — and a prompt line follows that one (the stranded draft is itself a prompt line), so it reported "not visible". The composer holding the operator's trigger was then logged as `cleared`.

The predicate only ever asks about the **current** draft, so it now scans newest-first. `#autotriggerscrollbackecho` is unchanged: a newest occurrence that really is consumed still has a later prompt line and still answers false.

One thing worth knowing, because it is why this survived: the test module's `claude_prompt_line` helper claimed to mirror `HarnessConfig::claude().is_prompt_line` but omitted its prefix-plus-whitespace clause — so a drafted `❯ <text>` line was a prompt line in the product and not in the mirror, and no test over this predicate could express the real shape. My first regression test passed under the oldest-first mutation because of it. The mirror is now faithful, and the test reddens on the mutation.

**State.** `make check`: 9527 tests pass; clippy and fmt clean; `audit-docs` needed `agent-doc skill install --harness grok` (the Grok surface was stranded at 0.35.399 — pre-existing, unrelated to this work) and is now green. `make install` ran, so the binary and cdylib are current; the supervisor takes the fresh build at its next idle boundary. Exchange is at 347 lines across 17 response sections — noted, not stopping the drain.

The live-pane proof that "Run Agent Doc" now actually submits is the one thing I cannot do myself, so it is in `agent:review`.
<!-- agent:boundary:93b90753:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 do [#resumereplaynotidempotent]
- do [#github5253release]
- do [#fixrunlazily]
- do [#retainedprojexit1install]
<!-- /agent:queue -->


<!-- agent:backlog priority -->
- [ ] 🚧 [#resumereplaynotidempotent] The captured-closeout resume replays tracked-work mutations NON-IDEMPOTENTLY and latches the cycle at write_applied forever. Observed live 2026-09-20 on cycle-1789880782658: the closeout's --review-resolve/--review-remove/--backlog-ungate envelope APPLIED (ops.log showed `review-resolve: archived 1 entry for #fpecompactheal to agent:done` and `review-remove: removed 1 entry`; the document showed review at 7, both converted items in agent:backlog, and #fpecompactheal archived to agent-doc-bugs.done.md), the document write was then retained by the delivery projection, and the capture resume refused with `captured_finalize_resume_needs_operator ... action=retain_without_mutation retry_on=controller_document_state_edge reason_head="...refusing to commit a half-applied cycle: review item not found: #fpecompac..."`. That retry can NEVER succeed -- the id will never return to agent:review -- so every state edge re-fails. Only a manual `agent-doc commit` cleared it. ROOT CAUSE (narrowed, do not re-walk): apply_unlanded_captured_mutation_plan gates the replay on recorded_tracked_work_is_unlanded, whose only witnesses are --done ids, explicit add ids, and `requested_mutations && !mutations_applied`. review-resolve/review-remove/ungate carry NO id witness, and mutations_applied is set only after the pending-write TRANSACTION succeeds -- here the mutation content reached the document while the transaction was retained, so the flag under-reports and the predicate answers "unlanded" for mutations that provably landed. FIX SITE: filter the already-applied ids out of `options` in apply_unlanded_captured_mutation_plan (agent-doc-repair-command-io) BEFORE its has_pending_mutation() check, witnessed against the already-resolved current_content -- this is local to one function and does not touch the 4 guarded recorded_tracked_work_is_unlanded call sites. TWO SHARP EDGES FOUND WHILE SCOPING, both in the silent-tracked-work-loss direction, so get them right before landing anything: (1) agent_doc_element_review::collect_gated_review_ids matches ONLY `- [/]` via strip_prefix, so a TYPED gate such as `- [/release]` is absent from that set -- using it as the witness would classify an unlanded typed-gate item as 'already applied' and silently drop its replay. Use 'absent from the agent:review component entirely' as the resolve/remove witness instead. (2) --backlog-ungate can target an item gated in agent:backlog, not only agent:review, so 'absent from review' is the WRONG witness for it; ungate needs 'still gated anywhere' (a `- [/` line bearing the id in either component). Regression coverage must run BOTH directions per shape: already-applied -> filtered out, and not-yet-applied -> still replayed. Same family as #retainedprojexit1 (fixed in b7043757b): the closeout reporting an already-applied state as half-applied.
- [ ] [#github5253release] Reply to and close agent-doc issue #53. VERIFIED 2026-09-20: both fixes (c92d5d629, a10a684b3) are ancestors of the released v0.35.400 tag; tmux_server.rs now queries `#{pid} #{start_time}` with a printable space and parses via split_whitespace(); v0.35.400 carries all four automated platform archives plus SHA256SUMS; PyPI latest is 0.35.400. Issue #52 was already closed. The prior `no earlier than 2026-09-22T16:41:52Z` weekly gate is STALE: 0.35.399 and 0.35.400 both shipped inside it, and under the current policy only Darwin asset uploads are weekly while tags and Linux/Windows assets publish on demand. Remaining work is the outward-facing step only: post a fixed-in-0.35.400 reply on the issue and close it.
- [ ] [#fixrunlazily] Run Agent Doc on lazily.md fails with `command-plane editor_route request failed via .agent-doc/controller.sock: Project Controller did not respond within 60000ms; the controller may be wedged`. NARROWED 2026-09-11, two hypotheses measured and DISPROVED — do not re-walk them. (1) The failure is the ACCEPT leg, not the work leg: the prefix `command-plane editor_route request failed` plus 60000ms is SOCKET_REQUEST_TIMEOUT_MS in sendAcceptedCommandSubmitToSocket; awaitCommandSubmitTerminal uses waitForReadySeconds*1000+grace instead. So the controller failed to ACKNOWLEDGE a command submit for 60s, and handle_editor_command_submit_async_rpc only enqueues. (2) NOT serialized accepts: rpc.rs:11185 listener.accept() spawns a thread per client. (3) NOT a slow pre-serve window: the socket binds at ListenerOptions::create_sync() (rpc.rs:11134) and the accept loop starts at 11184, so synchronous work between them leaves clients connected and hanging — the ss -lxp backlog signature — but the candidate, restore_reliable_sync_liveness, measures 0.08s for all 59,671 reliable_sync_liveness_journal rows / 6.5MB / 60,582 ops on this project, and its recovery effects are already spawned async. Live evidence at the failure window: ffi_ensure_controller_running_failed Connection refused (os error 111) x7 at 14:00:58-59, controller adopted 14:01:44, and a separate client still timing out at 14:02:23 (timed out after 5.0s waiting for project controller response) — unresponsive at least 40s after adoption. NEXT LEAD: what holds a lock across handle_request_locked (rpc.rs:12526) at runtime.bootstrap_snapshot() (12560) during a controller first seconds. Repro lever: kill the controller, then click Run Agent Doc while a replacement is launching.
- [ ] [#retainedprojexit1install] Install + recycle the agent-doc fixes landed 2026-09-20 (`b7043757b` retained-projection deferral, `ea57c13b6` empty-preflight reclaim authority): `make install` in src/agent-doc from a tree with no concurrent-session uncommitted source, then `agent-doc admin recycle`. Deferred because the shared working tree held another session's uncommitted `src/session_actor_cmd.rs` + JetBrains plugin edits (installing would ship their in-flight work) and because `lib-install` strands this attached document's Lazily replica mid-session.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#cargovodtrim] [operator-verify] [P2][security] Trim the credential exposure from the livestream VOD once the operator supplies the VOD URL or platform/start-time locator. Known exposed interval: 2026-09-12T02:57:27.170Z through approximately 02:58:14Z.
- [/] [#pypiquotafix] [operator-auth] [P1] AUTHORIZED cleanup: after 0.35.383 thin wheel is live, sign into https://pypi.org/manage/project/agent-doc/releases/ in Chromium tab 0; then permanently delete the 34 oldest existing releases, 0.35.0 through 0.35.112 (all pre-2026-08-02), freeing about 1.75 GiB. Pinned PyPI installs of deleted versions will stop working. Browser is currently logged out; support request pypi/support#12214 remains open.
- [/] [#tmuxfocussyncverify] [operator-verify] Restart IntelliJ to activate the staged JetBrains plugin, then keep agent-doc-bugs.md and the cross-root infra.md visible, click each embedded tmux pane, and verify editor selection follows without stealing focus from an actively focused editor component. PREMISE REFRESHED 2026-09-20: the plugin on disk is now 0.2.388 (written 2026-09-18 16:39:05) and the live IDEA is PID 1645748, started 2026-09-18 16:22:20 — 17 minutes BEFORE that write, so the staged build is still not activated. The previously recorded 0.2.379 / PID 1035669 pair is dead (that PID no longer exists). Automated lifecycle and transition regressions plus make check tmux-ci passed. Plan: tasks/agent-doc/plan-tmux-auto-sync-reactive.md.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#activateinstalledjetbrai] [operator-verify] One IntelliJ restart activates the staged JetBrains plugin. PREMISE REFRESHED 2026-09-20: staged build is 0.2.388 (2026-09-18 16:39:05) against live IDEA PID 1645748 started 2026-09-18 16:22:20; the recorded 0.2.379-staged-over-0.2.378 / PID 1035669 pair is dead. Remaining splice proof is unchanged: captured local edits recover across an independently advanced canonical response in a disposable live editor document. The original retained capture committed as 1b35dcb028 and all automated regressions passed. Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#harnessswitchverify] [operator-verify] Remaining proof: in a disposable live IntelliJ session, make an unsaved codex -> claude -> codex frontmatter switch while observing supervisor receipts; prove one spawn per authoritative switch, deferred restart for non-authoritative evidence, and no refusal/respawn storm. PREMISE REFRESHED 2026-09-20: the recorded prerequisites (IntelliJ PID 1035669 on plugin 0.2.378, CLI/native 0.35.361) are all stale — that PID is gone, the live IDEA is PID 1645748, the staged plugin is 0.2.388 and the installed CLI is 0.35.400. Unblocks: provide the disposable document path and perform the real unsaved editor edit; persisted CLI writes cannot substitute for this proof. Plan: tasks/agent-doc/plan-stop-gated-queue-head.md.
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#ha3hpolicy] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml receive.auto_create_components = ["pending"] (or = false); flip notify/write default from auto-create to consult-recipient-config. Hard-blocked until multi-user workspace support exists; issue #40 (open) explicitly says not to ship before that prerequisite. Moved out of agent:review because it is an unstarted future feature, not code-complete work awaiting validation. Tracked: https://github.com/btakita/agent-doc/issues/40
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
