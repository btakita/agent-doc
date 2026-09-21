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

> **Queue prompt:**
>
> do [#resumereplaynotidempotent]


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

### Re: #github5253release, plus the monsterrodholders wedge and the stash-window focus — opus-5 · 2026-09-20T13:00-04:00

> **Queue prompt:**
>
> do [#github5253release]


Three things landed this cycle. Issue #53 is closed, and both of your mid-turn reports are fixed and shipped.

**#github5253release — closed, and the gate I was holding it on was wrong.** My 2026-09-17 comment said closing waited on a "complete release" eligible 2026-09-22. That described a policy agent-doc no longer has: only Darwin asset uploads are on the seven-day cadence, and v0.35.399 and v0.35.400 both shipped inside the window I called closed. I said so plainly in the reply rather than quietly closing.

Verified before posting: `c92d5d629` and `a10a684b3` are both ancestors of `v0.35.400`; `tmux_server.rs` queries `#{pid} #{start_time}` and parses with `split_whitespace()`; v0.35.400 carries four platform archives plus `SHA256SUMS`; PyPI latest is 0.35.400.

The reporter left two nits, and **one of them was not a nit** (`a17cb20aa`). `fetch_releases()` requested a bare `/releases` — GitHub's 30-per-page default, no pagination — so the plugin asset search could only see the 30 most recent releases. Measured live today: exactly one release carries `agent-doc-jetbrains-*.zip`, sitting at index 2 of that page. `plugin install` / `plugin update` were going to start failing against a release that is still published and still downloadable. Now `per_page=100`, walking newest-first to 5 pages, stopping at a match or a short page, with a failure message that says how many releases it actually scanned. The page source is a parameter, so all of that is tested without a network. The genuine nit — an unreachable `.filter(|field| !field.is_empty())` left behind by the `split('\t')` to `split_whitespace` switch — is gone too. Pagination only buys headroom, though, so the real fix is `#pluginzipeveryrelease`, now in the backlog.

**monsterrodholders.md — same defect family as the item I closed last cycle, a shape I had explicitly left out** (`f0c5dd9ce`). `cycle-1789921516976` sat at `write_applied` cycling needs_operator, state edge, declined, needs_operator. The refusal: `pending edit: no item with id [#ads-delivery-blackout]`. The closeout had gated that id out of `agent:backlog` into `agent:review` **and** carried a `--backlog-edit` for it; the move landed, the write was retained, and the replayed edit could never succeed because `--backlog-edit` searches only the backlog component.

Last cycle I wrote that I had not extended the fix to the edit family because its "did it land" question was a different witness. That was the right read of the shape and the wrong call on scope — it was already biting.

So the witness is now general. `TrackedWorkScope` names the component a witness asks about, because "is the id still somewhere in the document" is never the right question: the item was plainly visible in `agent:review` the whole time while being unreachable to the edit. `--backlog-edit` to backlog, `--icebox-edit` to icebox, `--review-edit` to review; the two earlier witnesses are now wrappers over the same predicate. Re-applying an edit whose target is still present is idempotent, so only "gone from its own component" drops it. What is still uncovered — gate, set-gate-type, set-verify, reorder — is filed as `#resumereplayauditrest` with each one's witness worked out, rather than guessed, because a wrong witness here silently drops tracked work.

The wedged cycle committed itself as `4ebd4361` once the fixed binary was installed.

**Stash-window focus — the guard existed and was being skipped entirely** (`51a6e8f0f`, plus `f4acd6d` in the tmux-router sibling). The pane-window-drift guard compares the layout's target window against where the pane actually lives, but it ran inside `if let Some(layout_window) = ...` — so when `pane_layout_target_window_id` answered `None`, the whole check was skipped and `select-pane` ran on whatever pane the record named. Your `monsterrodholders.md` case, and visible in ops.log as `focus_applied:...existence-lang.md:%8` and `...chromium-bridge.md:%24` while `tmux list-panes` puts both `%8` and `%24` in `0:1:stash`.

The stash window is agent-doc's own parking area, so "this pane is stashed" refuses on its own and needs no layout window to compare against. Both legs now read one `pane_window_identity` observation, so they cannot disagree across a concurrent `move-window`.

Worth flagging: a mutation that dropped the window name from the observation adapter **survived the entire suite** on the first sweep. It would have disabled the stash leg in production while every test over the pure guard stayed green. The adapter is now a pure seam with its own test, and that mutation reddens.

**State.** `make check` green at 9542 tests; tmux-router green at 146. Sixteen mutations across the three fixes all redden. `make install` ran after each. CI for all three commits is in progress — recorded, not waited on.

### Re: fpe.md stall in the queue — opus-5 · 2026-09-20T13:12-04:00

Fixed and verified against the live document (`75b2405fd`, installed).

The queue was not stuck — it was **lying about being empty**. `fpe.md` has a `priority go` queue whose live head is `- do [#fpeoptimizedprofileacceptance]`, and that id is `- [/] [#fpeoptimizedprofileacceptance] [operator-verify] …` in `agent:review`. session-check printed:

```
queue_continuation_required=false ui_outcome=no_drainable_work ui_outcome_class=ok next_action=no_agent_action
```

No `queue_deferred_heads=` field at all. `no_drainable_work` means *fully drained, nothing to do* — over a queue you could plainly see still had an item in it. Nothing told you the head was yours to clear, so it sat there.

The head is correctly non-drainable: its id is a gated review row. But it counted as neither drainable **nor** deferred, because `deferred_backlog_ids_split` and `collect_backlog_execution_contexts` scope themselves to `backlog | icebox | pending` — `review` is excluded. An `[operator-verify]` tag on a review row, which is the normal home for a gated externally-blocked item, was invisible to the deferral scan, so session-check's `deferred > 0 || noise > 0` branch fell through to the "nothing here" arm.

`deferred_head_count` now also counts a head whose id is a gated `agent:review` row carrying no operator verdict. Same document, same command, after the fix:

```
queue_continuation_required=false queue_deferred_heads=1 queue_stale_noise_lines=0 ui_outcome=deferred_for_operator_proof ui_outcome_class=operator next_action=operator_proof_required
[session-check] queue idle: 1 head(s) deferred (operator-verify), 0 predicate-proven noise line(s) — operator-gated heads need a human …
```

An operator verdict on the head still satisfies the deferral (`#opverifyanswered`) — that head is the operator action, not a wait for one.

Two scope calls, both pinned by tests rather than left to reading. The clause lives in the head **counter**, not in `deferred_backlog_ids`: that set also gates drain eligibility, so widening it would defer a genuinely drainable backlog item that happens to share an id with a review row. And `gated_review_ids` scans `agent:review` specifically, not "any gated row" — a `- [/]` item still in `agent:backlog` is code-complete work awaiting review, not an external gate on the queue. That second boundary was worth pinning: a mutation widening the scan past `review` initially survived the whole suite, because no fixture had a gated row outside review.

Gatedness comes from parsed items, not a line prefix, so a typed `- [/release]` gate counts exactly like `- [/]`. A `strip_prefix("- [/]")` witness would miss it and put the head straight back into the silent-stall shape.

`make check` green at 9549. Five mutations redden. Installed.

One thing this does **not** do: the head is still visibly un-struck. Preflight already auto-strikes gated-review heads, but preflight only runs when a cycle is dispatched, and nothing dispatches while the queue reports no work — so the line persists until the next real turn on `fpe.md`. That is now honestly reported rather than silent, which is the part that was costing you time, but if you want the head struck without a dispatch that is a separate change and I can file it.
> **Queue prompt:** This document's queue stalled. Fix agent-doc causes and fix this class of error.

### Re: queue stall causes, #resumereplayauditrest, monsterrodholders pause — opus-5 · 2026-09-20T13:52-04:00

Three agent-doc defects fixed, pushed as `8cb9aa16e`. `make check`: **9562 tests run, 9562 passed**, exit 0.

**This document's stall was `#qmultilinehead`.** Preflight this cycle reported `queue_active: true`, `queue_drainable_head_count: 5`, and `queue_continuation_required: false` — the `EXCHANGE_PROMPT_PREEMPTION_GUIDANCE` shape. The cause: you appended **two** queue lines in one edit, so the differ coalesced them into a single `PromptBearingChange` whose text was `- This document's queue stalled…\n- do [#resumereplayauditrest]`. `prompt_changes_preempt_queue` normalized that whole two-line blob and compared it against the one-line head set, which can never match, so it declared a preempting exchange prompt and dropped continuation. The `#qgoalstall` fix that exists precisely for "operator adds a directive to `agent:queue`" silently stopped applying the moment more than one line was added at once. The head match is now **per line**: a change is queue continuation only when every non-empty line is an active head; one line of prose alongside a head still preempts.

**`do [#resumereplayauditrest]` — audit finished.** The four uncovered error-on-missing shapes, each witnessed against what its command actually searches:

- `--backlog-gate` refuses only once the id is open **nowhere**. Its chain is take-from-backlog → accept an already-open review item as a NoOp → `gate_in_place`, and only the last step errors; re-gating a still-open item is a `validate_transition` NoOp. So a landed gate replays harmlessly and only a reaped id latches.
- `--backlog-set-gate-type` and `--backlog-set-verify` resolve through `find_open_tracked_work_component_in_content`, which fails with `id not found in backlog/icebox` on that same condition — so all three share one witness, `TrackedWorkScope::OpenAnywhere`.
- `--backlog-reorder` is one comma list, but `op_reorder` validates **every** id against the backlog component up front and bails on the first missing one, so a single reaped id kills the whole reorder forever. Its witness is per id, and **any state counts** (`TrackedWorkScope::BacklogAnyState`) — `op_reorder` reads the parsed item list, which still holds `[x]` items until they are reaped, so "open" would wrongly drop a done-but-unreaped id and silently change your requested order. Survivors keep reordering; the flag is dropped only when nothing survives.

I checked one tempting shortcut and discarded it: `queue_head_provenance.recorded_at` looks like a free "is this head older than X" witness, but its upsert is `ON CONFLICT DO UPDATE SET recorded_at = excluded.recorded_at`, so it records *last mirrored*, not first seen. Anything built on it would have been unsound.

Both directions are covered per shape, and both are mutation-proven: removing the new witness block fails all four drop-tests, and forcing the witnesses to answer "already applied" fails all four keep-tests.

**`monsterrodholders.md` — the pause was 18 days stale, and `admin queue pause` was corrupting paths.** Its queue was held by an `admin queue pause` from 2026-09-02 whose reason named `#6kda` (a Cloudflare token deletion) and a navbar design question — both long gone from the queue. The six live heads are different, so nothing auto-drained for 18 days and the pause reason no longer described anything real. Per your choice I kept the pause and rewrote its reason to name the actual current heads and why they are held (`#netcup-cutover-preflight-run` and `#waterways-edge-deploy-stale` are outward-facing production actions against the live store).

Doing that surfaced `#qpausepathdouble`: the first attempt with a repo-relative path reported `queue_paused accepted` but wrote a control row for `…/src/boost-client/src/boost-client/tasks/monsterrodholders.md`, because the controller resolves a relative `file` against the **project root** while you type it relative to your shell. Every reader canonicalizes, so the scope ids could never match and the real document's control state never changed — an accepted receipt for a document that does not exist. Admin document arguments are now anchored to the CWD before reaching the controller (`queue_control`, `reap`, `handoff`, `inspect`, `repair_projection`); an unresolvable path is passed through unchanged so the controller still owns that error. The bogus doubled-path row is resumed and inert.

**Not installed.** The shared working tree still holds another session's uncommitted `src/session_actor_cmd.rs` plus JetBrains plugin edits, so `make install` would ship their in-flight work — the same blocker already recorded in `#retainedprojexit1install`, now updated to cover this commit too. Latest CI before the push was green (`35525082336`); the push starts a new run, which I am not waiting on.

One note: this exchange is at 443 lines across 20 response sections, above the clear threshold — flagging it rather than stalling the queue for it.

### Re: #fixrunlazily — the controller was self-RPCing its own socket — opus-5 · 2026-09-20T14:12-04:00

> **Queue prompt:**
>
> do [#fixrunlazily]


Root-caused and fixed, pushed as `78c36ccd3`. `make check`: **9564 tests run, 9564 passed**, exit 0.

**The lead was right, the lock was not the mechanism.** `runtime.bootstrap_snapshot()` is a `clone()` under a mutex nobody holds across work, so that thread died. What is actually wrong is one scope mismatch: **`in_controller_request()` is thread-local, but "this process is the controller" is a process fact.**

`install_controller_request_thread_context` exists precisely for this, and its own doc comment describes the failure. `serve_client` calls it. Nothing else does. Every other thread the controller spawns starts with fresh thread-locals:

- the startup reliable-sync replica rebuild (`spawn_reliable_sync_recovery_effects`),
- the post-promotion rebuild (`schedule_editor_replica_rebuild_after_promotion`),
- the self-promote rebuild,
- the orphan-drain route worker,
- `spawn_editor_command_async_worker` — the async worker behind the very command that failed,
- the supervisor replacement worker.

Each of those calls `log_op`. `log_op` resolves a `turn=` id per line via `load_document_projection`, which branches on `controller_socket.exists() && !in_controller_request()`. With no thread-local marker the worker looks like an external client, so it **opens a 5-second-timeout socket request to the controller it is running inside** — once per logged line, each consuming an accept and a spawned thread in that same controller, during its first seconds.

That reconciles every measurement already in this item. The accept leg is where it fails because the accept leg is what the self-RPCs are saturating. Threads-per-client is true and is why it degrades rather than deadlocks. And the stray `timed out after 5.0s waiting for project controller response` at 14:02:23 is not a second unrelated symptom — 5.0s is exactly this read's timeout.

**Fix: make the identity process-scoped, not per-spawn.** Installing the thread context at each spawn site is the fix that already exists and keeps being forgotten, silently — you only learn about it as an unresponsive controller. So `serve` records its project root once in a process registry, and `load_document_projection` consults it alongside the thread-local. A new `thread::spawn` cannot reintroduce the gap.

It records the **root**, not a bare flag, because the suppression has to stay scoped: a controller serving project A is an ordinary client for project B and must still use B's socket. An unscoped flag would have it read cold state it has no claim to.

**Coverage, mutation-proven in both directions.** A stub actor on the controller socket answers with a projection that could only have come from an RPC: the external-client read must still see it (proving the socket is used), the in-controller read must not (proving the local path is taken). Reverting the fix fails the second assertion; making the suppression unscoped fails the first. The identity is also asserted across a real `thread::spawn`, and the `#ctrlbindbeforeserve` source-ordering guard now additionally requires the root be recorded before the bind — deleting that line reddens it.

`#ctrlbindbeforeserve` itself is holding: all 75 `controller_listener_serve_ready` rows in this project report `bind_to_accept_ms=0`, so hypothesis (3) in the item is closed structurally, not just measured.

Not installed, same blocker as before: the shared tree still holds another session's uncommitted `src/session_actor_cmd.rs` and JetBrains plugin edits. CI run `35527154818` was still in progress at closeout; not waiting on it.

### Re: #retainedprojexit1install — installed from a clean HEAD worktree — opus-5 · 2026-09-20T14:18-04:00

> **Queue prompt:**
>
> do [#retainedprojexit1install]


Installed and armed. Both halves of this item's recorded deferral turned out to be resolvable rather than blocking, but for different reasons.

**The concurrent-WIP half was real, and a clean worktree is the way around it.** `src/agent-doc` still holds another session's uncommitted `src/session_actor_cmd.rs` plus two new JetBrains plugin files, and `make install` builds from the working tree — so installing from there would have shipped their in-flight work. Instead I built from a detached worktree at `78c36ccd3`, placed at `src/agent-doc-cleanfix` so the sibling `../tmux-router` path dependency resolves (an isolated `/tmp` worktree fails that). Their WIP is untouched and still uncommitted.

**The `lib-install` half was stale.** The item deferred partly because a cdylib reload strands an attached document's Lazily replica — but that was fixed in 0.35.384 (`#reloadgateperprocess`) and plugin 0.2.384 (`#jbreloadreregconverge`), and the live plugin here is **0.2.388, running and not stale** (`live_plugin_generation_refresh ... running=0.2.388 expected=0.2.388 stale=false`, editor PID 306200). The install bore that out: `reload_library intent: delivered 1/1 editor endpoints across 9 projects (0 restart required, 0 deferred mid-cycle, 3 unavailable)`, then `crdt_replica_promote ... superseded_generations_pruned=1 generation_fenced=true` and a clean `replica_pull` for this document afterwards. No `missing_replica`.

Worth noting separately: that same evidence also stales the premise of two `agent:review` items (`#activateinstalledjetbrai`, `#tmuxfocussyncverify`), which both say the staged 0.2.388 plugin is *not* activated against a live IDEA started before it. The live IDE is now PID 306200 running 0.2.388. I have not touched those items — they need their own pass.

**What landed.** `binary-install` replaced `~/.cargo/bin/agent-doc` by atomic rename (live process inodes preserved); `lib-install` published `libagent_doc-0.35.400.so`. Proof the running binary is no longer behind: it contains `controller process root registry poisoned` and `set_controller_process_root` from `78c36ccd3`, and the `backlog-set-gate-type` drop descriptor from `8cb9aa16e` — and so does the cdylib.

**Recycle is armed, not forced.** `lib-install` reported `auto-recycle: 16 open supervisor(s) marked to recycle at next idle boundary, 0 skipped` and `9 controller(s) marked after supervisor handoff`. That is the product-owned safe path — `#supselfheal` blue/green `execve` at an idle boundary. I deliberately did not force `agent-doc admin recycle` mid-cycle against this document's own supervisor: a forced recycle against a busy supervisor is the shape that times out *and* exits 0, leaving a wedged `preparing` controller that only `kill -TERM` clears. The marking achieves the same end without that risk.

**One genuine failure, filed not swallowed.** `make install` exits 2 at `install-editor-plugins` because `vsce` is not on this machine (`sh: line 1: vsce: command not found`) — after the binary, cdylib, skills and the JetBrains plugin have all already installed successfully. So the install is complete but the command reports failure, which is exactly the kind of thing that gets read as "the install failed" next time. Filed as `#vsceoptionalinstall`.

### Re: #ctrlworkerliveproj — the live projection is now process-wide — opus-5 · 2026-09-20T14:27-04:00

> **Queue prompt:**
>
> do [#ctrlworkerliveproj]


Done, pushed as `c1088b2ce`. `make check`: **9565 tests run, 9565 passed**, exit 0.

**I had the fix wrong when I filed this item, and the code said so.** I wrote it as "thread the runtime into six spawn sites and call `install_controller_request_thread_context` in each". Tracing the actual read path first: `load_document_projection` never calls `local_document_projection` at all. So installing the thread context would have bought those workers nothing on the path that was costing them — it feeds `agent-doc-op-capture-io`, not the `log_op` turn resolution.

**What was actually wrong is an inversion of `#lazily-hot-path`.** That rule says the controller's live in-memory projection is the authority and `state.db` is the actorless bootstrap path. Inside the controller it ran the other way: after `#ctrlselfrpc` stopped the self-RPC, a controller-owned worker fell straight through to replaying the durable event ledger — and since `log_op` resolves a `turn=` id per line, one worker logging about one document replayed that document's entire ledger **once per line**. Request threads escaped it only because `handle_request_locked` opens a turn-attribution memo.

**Fix: publish the reader process-wide, not per thread.** The live reader was thread-local because it is a `Box<dyn Fn>`; capturing a `Weak<ControllerRuntime>` instead makes it `Send + Sync`, so the same closure can live in a process-wide slot. `local_document_projection` prefers the thread-local when present and falls back to the process one, and `load_document_projection` consults it — gated on the same `served_by_this_process` predicate `#ctrlselfrpc` introduced — before touching the ledger. No spawn site has to remember anything, which is precisely what let this class recur across a dozen `thread::spawn`s.

So the six sites named in the item need no change, and threading `runtime` through their signatures would have been churn. The `#ctrlbindbeforeserve` source guard that pins the `spawn_reliable_sync_recovery_effects(...)` call text also stays intact for the same reason.

**Coverage.** The reader is installed on the main thread and read from a spawned one, with no `state.db` on disk at all — so a non-`None` answer can only have come from the live projection. Dropping either half reddens it: removing the consult in `load_document_projection`, or making `local_document_projection` ignore the process slot. Both mutations verified.

One test-infrastructure note worth keeping: these two controller-identity tests mutate **process**-global registries, and `cargo test` runs a crate's tests as threads in a single process (nextest gives each its own). Run together, one test's teardown cleared the registry the other was mid-assertion on — green in isolation, red in the suite. They are now serialized behind a mutex that recovers from poison, so a failure reports as that one test failing rather than as every later test panicking on a poisoned lock.

CI run `35527154818` for `78c36ccd3` came back green; `c1088b2ce` starts a new run I am not waiting on.

### Re: #pluginzipeveryrelease — editor packages now ship with every tag — opus-5 · 2026-09-20T14:41-04:00

> **Queue prompt:**
>
> do [#pluginzipeveryrelease]


Done, pushed as `c3d41f96c`. `make check`: **9566 tests run, 9566 passed**, exit 0.

**The cause was exactly as filed.** `release.yml` built four platform archives plus `SHA256SUMS` on every tag and never touched the editor packages at all — they were published on a separate cadence, so the gap between plugin-bearing releases grew without bound. A `plugins` job now builds both on every tag, and `release` **hard-depends** on it: a tag that cannot produce editor packages fails loudly rather than quietly publishing the asset gap this item exists to close. With the packages always on the latest release, `#pluginassetpaging`'s 5-page bound becomes slack instead of load-bearing.

**Two details that would otherwise have shipped the wrong thing silently.**

- Both copies are pinned to the declared version rather than globbed. `build/distributions/` accumulates every version it has ever built plus `-signed` siblings — there are four zips in mine right now, including `0.2.387-signed.zip`. A glob is one cache restore away from attaching a stale or differently-signed artifact to a release and reporting success.
- The packages download into their own directory. `SHA256SUMS` is generated over `artifacts/` and is consumed by the PyPI bootstrap launcher and by `make release-macos-assets`; the packages also match `agent-doc-*`, so sharing that directory would have silently rewritten the platform-archive manifest. That is the kind of breakage that only shows up at the next `pip install`.

**Both build steps verified locally before making CI depend on them.** `gradlew buildPlugin` produces `agent-doc-jetbrains-0.2.388.zip`; `npm ci && npm run package` produces `agent-doc-0.2.71.vsix` — `@vscode/vsce` is already a devDependency, which also confirms `#vsceoptionalinstall` is purely a missing local `npm ci`, not a missing tool.

**One pre-existing guard had to be tightened rather than worked around.** `test_release_cadence_applies_only_to_macos_assets` asserted `!release.contains("macos-")` over the whole file, so merely *naming* `make release-macos-assets` in a comment about `SHA256SUMS` failed it. I scoped it to non-comment lines instead of renaming my comment, because the guard's intent is "do not schedule a paid runner" and prose cannot do that. Proven still sharp by adding an `aarch64-apple-darwin` / `macos-latest` matrix entry — it reddens.

The workflow itself cannot be exercised from a test, so the invariants are pinned the same way the issue-47 and issue-52 guards are: the dependency, both upload paths, the version-pinned copies, and the separate download directory. Dropping the `plugins` dependency reddens it.

That empties the queue. Both CI runs were still in progress at closeout; not waiting on them.

### Re: do [#vsceoptionalinstall] — opus-5 · 2026-09-20T14:52-04:00

> **Queue prompt:**
>
> do [#vsceoptionalinstall]


Fixed and pushed as `96d8719f4`.

**Root cause.** `npm run package` runs `vsce package`, and `vsce` resolves from `editors/vscode/node_modules/.bin`. A clean worktree that has never run `npm install` there has no such binary, so the script dies with `sh: line 1: vsce: command not found`. The `|| exit 1` propagated that out of `install-editor-plugins` (Makefile:259) and back through `@$(MAKE) install-editor-plugins` (Makefile:221), so `make install` exited 2 *after* the binary, cdylib, skills and the JetBrains plugin had all installed successfully.

**Fix.** The VS Code branch of `install-editor-plugins` is now gated on vsce being resolvable — either `editors/vscode/node_modules/.bin/vsce` or a global `vsce` on PATH:

```make
@if agent-doc plugin list 2>/dev/null | grep -q '^vscode'; then \
	if [ -x editors/vscode/node_modules/.bin/vsce ] || command -v vsce >/dev/null 2>&1; then \
		( cd editors/vscode && npm run package ) || exit 1; \
		agent-doc plugin install vscode --local; \
	else \
		echo "WARNING: vsce is not installed; VS Code package sync skipped. ..." >&2; \
	fi; \
else \
	echo "No existing VS Code agent-doc package; editor package sync skipped."; \
fi
```

The skip warning says the installed VS Code package stays on its previous generation and names the remedy (`npm install` in `editors/vscode`, or a global `@vscode/vsce`, then `make install-editor-plugins`) — a silent skip would just move the failure mode from a confusing exit code to a stale package nobody knew about.

A real `vsce package` failure still fails the install (`|| exit 1` is untouched), and the JetBrains branch still fails closed: there a failed Gradle build means a stale package would be installed, which is a different situation from the tool not being present at all.

**Regression coverage.** `test_missing_vsce_skips_vscode_packaging_instead_of_failing_the_install` in `tests/test_cli.rs` asserts the guard precedes `npm run package`, that the skip message names the remedy, and that nothing in the skip branch exits non-zero. Mutation-checked: reverting the Makefile to its pre-fix shape reddens it (`the VS Code branch must check for vsce before packaging`), and restoring turns it green.

**Verification.**
- `make check` — exit 0, nextest reports `9567 tests run: 9567 passed (1 slow), 259 skipped`; all stages ran (clippy, test, sim-medium, version-sync, audit-docs, editor-parity, python-bootstrap-test, lean, tla).
- Both shell branches exercised standalone: vsce absent → warning on stderr, status 0; vsce present → packaging path taken, status 0.
- `make install` — exit 0 end to end (this worktree has `node_modules`, so it took the packaging path and installed `agent-doc-0.2.71.vsix`). The binary is now on `c3d41f96c` plus this fix, clearing the `stale_install` advisory.
- CI run `35530446385` for this commit was still in progress at closeout; the two runs before it are also in progress, so nothing is red. Recording as pending rather than waiting.

❯ `make check` — exit 0, nextest reports `9567 tests run: 9567 passed (1 slow), 259 skipped`; all stages ran (clippy, test, sim-medium, version-sync, audit-docs, editor-parity, python-bootstrap-test, lean, tla).

### Re: do #actionable-review — opus-5 · 2026-09-20T15:22-04:00

> **Queue prompt:** do #actionable-review

Preset body: *Add actionable review items into backlog + queue.*

**All eight `agent:review` gates are still genuinely binding**, so none of them converts to an actionable item as written:

| item | gate | still blocking? |
|---|---|---|
| `#runsubmitfirstmatch` | human watches one Run Agent Doc dispatch | yes — and the correctness half is already covered by `route_trigger_visible_in_current_draft_answers_for_the_newest_occurrence` (agent-doc-controller/src/dispatch.rs:7037), so only the live UX confirmation remains |
| `#cargovodtrim` | operator supplies the VOD locator | yes |
| `#pypiquotafix` | browser auth on pypi.org | yes |
| `#tmuxfocussyncverify` | IntelliJ restart + pane clicks | yes |
| `#grokmcpverify` | Grok quota / subscription | yes |
| `#activateinstalledjetbrai` | IntelliJ restart | yes |
| `#harnessswitchverify` | live disposable IntelliJ session | yes |
| `#coder7` | reachable Coder workspace | yes |

What *is* actionable is the agent-doable work those items keep re-deriving by hand, plus two defects this cycle surfaced while expanding the preset. Three items added to `agent:backlog` + queue:

**`#pluginactivationprobe`** — the extraction from the review list. Three of the eight items carry a hand-recorded `(staged plugin version, live IDE PID, process start time)` triple as their premise, and each has been refreshed by hand at least twice. The 2026-09-20 refresh recorded IDEA PID 1645748; as of 15:20 no IntelliJ or java process exists at all, so that premise is dead again. `stale_plugin` (agent-doc-preflight-io/src/warnings.rs:380) only fires from live editor registrations, so it stays silent precisely when the triple rots. A registration-independent probe — installed artifact mtime vs live IDE process start time, surfaced in `session doctor` — makes the premise machine-refreshed.

**`#orchestratepresetexpand`** — SKILL.md §0b says to let `orchestrate` expand validated presets. It doesn't. `agent-doc orchestrate <FILE> --from-exchange` printed `mode: sequential`, parsed the *previous* response's four verification bullets as `tasks: 4`, then logged `closing preflight handoff cycle cycle-1789931555969 before task injection` — closing this turn's sealed cycle. The preset body was never read; I resolved `#actionable-review` from frontmatter `prompt_presets` instead. Nothing was injected into the document (queue and backlog verified unchanged), and a fresh cycle was admitted at 19:12:53Z.

**`#archiveindexwal`** — that orchestrate call sat 4 minutes in state `D`, its only non-socket fds on a 115.6 MB `archive-index.db` plus a live `-journal`, wchan `jbd2_log_wait_commit`, 7s CPU across 4 minutes wall. `archive_index.rs:564` sets only `PRAGMA foreign_keys = ON`; `state_store.rs:657` sets `journal_mode = WAL` with a 30s busy_timeout and documents why. The archive index is the outlier.

Exchange is at 608 lines / 26 sections — noting the size, not stalling the queue for it.

### Re: queue drain — opus-5 · 2026-09-20T16:25-04:00

> **Queue prompt:**
>
> Fix agent-doc causes or agent-doc-bugs.md + monsterrodholders.md turn being wedged on finish. Restarting Idea seemed to finish the turn, but this should never happen.

Three of the four queue items are landed, tested, installed and pushed (`agent-doc 22d8bd33b`). The wedge itself I have a hard root cause for and **no landed fix** — I tried one, it made the binary lie about editor acknowledgement, and I reverted it. Details below, because the constraint is the interesting part.

#### The wedge — root cause, measured, not inferred

The editor went **completely silent**: zero `crdt_replica_pull` for `monsterrodholders.md` between **19:41:00Z and 19:47:28Z**. Across that silence `delivery_converged=false` with `delivery_version` frozen at **60** from 19:41:46Z to 19:44:45Z. It cleared at exactly 19:44:32Z — when you restarted IDEA.

A silent replica can never advance `redeliveries_without_ack` (that only moves inside `pending_updates`, i.e. on a pull), so `#pullnoackdeadlock`'s budget of 50 is unreachable by construction. The intended escape is `#silentreplicabarrier`: `charge_barrier_wait_without_progress`, 12 expired convergence waits. **That release has never fired once** — zero `crdt_replica_barrier_released_without_progress` across 11.9 MB of `boost-client` ops history and the whole agent-loop log. The arithmetic says why: `await_delivery_convergence_for_file` charges **once per call and returns**, so 12 charges need 12 separate awaits; preflight makes one (`timeout_ms=3369`) and then defers (`preflight_visible_mutation_deferred_lazily_current state=delivery_pending`). The allowance is effectively infinite.

**Why the obvious fix is wrong.** I implemented it — park in `remaining/13` slices so one await spends its budget and actually reaches 12 charges. It worked (new test green, mutation-checked: reverting the `continue` fails it). It also broke `serialized_atomic_write_retained_ack_timeout_forbids_agent_recapture`, and it broke it *correctly*: releasing the silent member from the barrier flips `delivery_converged` to true, and the write path then reports "editor acknowledged the canonical target" for an editor that acknowledged nothing. Fail-closed still, no disk write — but a false statement. `delivery_converged` currently carries two meanings (consistent cut for the commit barrier; the editor has the bytes) and the release mechanism is only sound for the first. Reverted; suite back to 9579/9579.

So the fix needs those two meanings separated first. Filed at the top of the backlog as `#silentbarrierneverreleases` with the evidence and that constraint.

#### What did land

- **`#ghostrelaymember`** (not on the queue — found while chasing the above). `RelayHub` liveness is keyed by opaque CRDT client id, and the only production reap was *lazy*: `dead_editor_replica_ids` runs on the NEXT registration. `record_reliable_sync_editor_exit` updated the durable liveness plane and state-event retention and never touched the hub, so an editor that exits and does not come back holds `live_editors >= 1` forever. Now reaped eagerly on the OS exit fact. Mutation-checked: with the wiring disabled the controller test observes `live_editors` stuck at 1.
- **`#archiveindexwal`** — `archive-index.db` now opens `journal_mode=WAL` + `busy_timeout=30000` + `foreign_keys=ON` through one helper (all 6 open sites), and each reindex is one transaction instead of an fsync per row.
- **`#orchestratepresetexpand`** — preflight now emits `prompt_preset_expansions` (name + body) beside `prompt_presets_requested`; SKILL.md no longer routes expansion through `agent-doc orchestrate --from-exchange`, which never read `prompt_presets` frontmatter and abandoned the live `PreflightStarted` cycle.
- **`#pluginactivationprobe`** — new `src/plugin_activation.rs`: compares the installed jar's mtime against each live IDE's `/proc` start time, surfaced in `agent-doc session doctor`. Registration-independent, so it also answers "no IDE is running at all" (dead premise) rather than staying silent like `stale_plugin`.

#### Two corrections to my own work this turn

I twice reached a wrong conclusion from filtered tool output, and both would have shipped as confident claims:

1. `ps -eo pid,comm | grep -icE 'java|idea'` returned **0** while IDEA pid 1023909 was running. I concluded "no IDE exists" and built the whole ghost-editor theory on it. `live_editors=1` was correct the entire time.
2. A recursive `grep` for `charge_barrier_wait_without_progress` showed only test hits, so I concluded the escape hatch had no production caller. It is wired, in `agent-doc-crdt-relay-io/src/lib.rs:4148` — the grep missed it.

Everything above that survived is re-verified with `python3` reads of the raw log. The `#ghostrelaymember` fix stands on its own merits (mutation-checked), but it is **not** the cause of your wedge.

#### Your rolled-back queue edits

Same mechanism, same moment. The old IDE held the replica and stopped answering; your queue lines lived in that buffer without a converged delivery. When it died at 19:44:32Z the replacement registration bootstrapped from the canonical, which did not have them. The nine `do [#...]` lines are on disk now. This is downstream of `#silentbarrierneverreleases` — a barrier that releases on a bounded budget would have forced the question minutes earlier, while the old buffer was still alive.

Verification: `make check` green (9579 tests, clippy, sim-medium, version-sync, audit-docs, editor-parity, TLA+). `make install` done, supervisors recycled for both `agent-loop` and `boost-client`. Pushed `22d8bd33b`; CI run 35534811152 was still in progress at closeout, previous run green.

### Re: #silentbarrierneverreleases — opus-5 · 2026-09-20T16:44-04:00

> **Queue prompt:**
>
> do [#fixcausesbugs]
>
> do [#silentbarrierneverreleases]


Fixed and pushed (`6da64a97c`). The wedge you hit on both documents is closed, and the ordering of the two halves turned out to be the whole problem.

#### What was broken

A replica that registers and then goes **completely silent** — no pull, no ACK, no projection — held the delivery barrier forever. Both escape hatches were unreachable for it:

- `#pullnoackdeadlock` (budget 50) counts inside `pending_updates`, i.e. **on a pull**. A silent member sits at 0 forever.
- `#silentreplicabarrier` (budget 12) is charged by `await_delivery_convergence_for_file`, which charged **once per call and returned**. Twelve charges needed twelve separate awaits; preflight makes one 3.4s await and then defers.

The proof it was unreachable rather than merely slow: `crdt_replica_barrier_released_without_progress` had **never fired** — zero occurrences across 11.9 MB of `boost-client` ops history and the whole agent-loop log.

#### Why the fix needed two halves, in order

Making the charge reachable **alone** is wrong, and I confirmed that the hard way last cycle: slicing the await so it reaches 12 charges does release the barrier, and it immediately broke `serialized_atomic_write_retained_ack_timeout_forbids_agent_recapture` — correctly, because releasing a silent member flips `delivery_converged` to true and the write path then reported *"editor acknowledged the canonical target"* for an editor that acknowledged nothing.

The split I thought I had to build already existed. `RelayHub::visible_delivery_projected` is the strict receipt and its own doc comment says the availability release "must never authorize a native save or retained closeout". The defect was simply that `agent-doc-document-realtime-io`'s serialized-atomic-write path branched on `delivery_converged` instead. So:

1. **Firewall first** — that branch now gates on `visible_delivery_projected_for_file`. A strict tightening: the receipt implies convergence, so everything that already refused still refuses with the same message.
2. **Then make the charge reachable** — the await parks in `remaining/13` slices and charges each expired slice. Every slice is still a genuine expired convergence wait that saw no delivery-epoch movement; a replica that is merely slow moves the epoch and exits the loop, so only total silence accumulates charges.

Net effect on your wedge: a silent editor stops blocking the commit barrier within one bounded await instead of never, **and** closeout still refuses to claim an acknowledgement it does not have — it stays retained with "do not recapture, do not force disk", which is the honest state.

Both halves are mutation-checked. Neuter the `continue` and the silent replica holds the barrier again; neuter the receipt gate and the write path emits the false acknowledgement.

**Worth carrying forward:** anywhere else reading `delivery_converged` as proof the editor holds the bytes is this same bug. Availability answers "may unrelated work proceed"; `visible_delivery_projected` answers "did the editor actually take it".

#### Verification

`make check` green — **9580 tests** (up one, the new regression), clippy, sim-medium, version-sync, audit-docs, editor-parity, python-bootstrap, lean, TLA+. `make install` done and both supervisors recycled (`agent-loop`, `boost-client`). CI runs 35534811152 and 35536410465 were both still in progress at closeout; the last completed run was green. The built tree included another session's uncommitted JetBrains plugin edits (`TurnStateBannerRefresher.kt`, `InputRequiredNotifications*.kt`) and an in-flight `session_actor_cmd.rs` hunk — those were not committed; my `session_actor_cmd.rs` change went in last cycle as a hand-built blob so their work stayed untouched.

Exchange is 679 lines over 28 sections — noting it, not stopping for it.

### Re: #actionable-review — opus-5 · 2026-09-20T17:08-04:00

> **Queue prompt:** #actionable-review

All eight `agent:review` gates survive the audit again, but three of them were resting on identifiers that are now provably wrong, and the re-measurement turned up a better activation proof than the one those items were using. Five actionable items go to `agent:backlog` + queue.

**This cycle opened wedged.** Preflight refused admission: a binary-owned write retained on `content_hash=aaf0a5a9…` with `recovery=await_editor_replica_no_disk_write`. I confirmed pane ownership first (`actor: generation=121 pane=%0`, `TMUX_PANE=%0`) and then ran the prescribed `agent-doc commit`, which repaired a `committed_capture` drift into the snapshot and correctly refused to close over an unanswered typed-component edit. That refusal was right: the edit was this prompt.

**Review audit against the live system, not the prose.** None of the eight converts to actionable as written. `#cargovodtrim` still needs the VOD locator, `#pypiquotafix` still needs a PyPI login (the 0.35.383 thin-wheel precondition is satisfied, 0.35.400 is installed), `#grokmcpverify` still needs quota, `#coder7` still needs a reachable workspace, `#harnessswitchverify` still needs a real unsaved editor edit, `#runsubmitfirstmatch` still needs a dispatch watched live. Those are genuine external gates.

**But the JetBrains premises were wrong on every identifier.** Two items recorded live IDEA PID 1645748 with the jar written 2026-09-18 16:39:05; line 545 of this document separately asserts PID 306200 running 0.2.388. Neither PID exists. Live IDEA is **PID 1023909, started 2026-09-20 15:44:32**, and the staged jar was written **2026-09-20 16:42:24**.

The conclusion holds, and the proof is now much better than mtime arithmetic:

```
/proc/1023909/map_files → agent-doc-jetbrains-0.2.388.jar (deleted)
on disk                 → inode 76585058, mtime 16:42:24
```

The running IDE is executing a **deleted inode** while disk holds different bytes under the **same 0.2.388 version string**. Any version-string check reports "activated" and is wrong. That is `#pluginbyteidentity`, and it makes the activation half of both items machine-checkable instead of human-gated.

**A correction to my own working assumption, and to the memory I was reasoning from.** I found two `libagent_doc-*.so` mapped into the live JVM (16:07:57 and 16:42:13) which is the recorded `#installstrandsreplica` signature, and I expected today's `missing_replica` traffic to follow from the two mid-session installs. It does not. The counts run 04:16Z through 21:05Z across seven documents, including hours with no install, and every sampled occurrence is immediately followed by `crdt_commit_barrier ready=true visible_delivery_projected=true` and a full-length `crdt_current_text`, with 210 `crdt_replica_pull` events advancing generations normally. `missing_replica` here is **benign transient churn, 988 of it today**, logged identically to the terminal case. That indistinguishability is what cost me the detour, so it is filed as `#missingreplicanoise` rather than left as folklore.

The remaining two items are defects this cycle produced first-hand: the preflight refusal that told me both to wait and that waiting could never work (`#preflightrefusalcontradiction`, already classified by dogfood watch as `projection_failure`), and the `agent-doc commit` first run that printed nothing at all (`#commitsilentfirstrun`).

Exchange is at 720 lines across 29 response sections; noting it rather than stalling the queue.

### Re: do [#pluginbyteidentity] — opus-5 · 2026-09-20T17:25-04:00

> **Queue prompt:**
>
> do [#pluginbyteidentity]


Landed in `b82b701ba`, pushed to `origin/main`.

`stale_plugin_warnings` compares reported version strings, so it is structurally blind to the shape that actually happens: `make install` rewrites the jar at the same path under the same version number, and the running IDE keeps executing the replaced build while every version comparison reports it current. That is how two `agent:review` items sat gated on a premise derived from version and mtime arithmetic that was measurably wrong.

The new probe reads what the process actually mapped. `plugin_byte_identity_warnings` walks the live editor registrations, and for each JVM-hosted editor pid resolves `/proc/<pid>/map_files`, emitting `plugin_bytes_superseded` when the mapped jar is unlinked or its inode no longer matches the jar on disk.

**One thing the live system taught me that I had not designed for.** I originally reasoned about this as an inode comparison, with `(deleted)` as a convenience case. It is the other way round. After this cycle's own `make install`, the jar on disk came back as **inode 76585058, the same number the previous jar had** — the filesystem reused it. An inode-only comparison would have returned `Current` for a process demonstrably running unlinked bytes. The `(deleted)` suffix is checked first and short-circuits before any inode is consulted, and that ordering is now the load-bearing part rather than a nicety.

The probe fails open to `Unknown` on non-Linux, an unreadable `map_files`, or an unstattable side, so it can never send an operator restarting a healthy IDE.

**Verification.**

- `make check` exit 0: **9585 tests run, 9585 passed**, 259 skipped (nextest's own count, not a grep over output).
- Five new tests under `warnings::tests::`, confirmed executed by name rather than by a passing total.
- Mutation-checked, restoring from a scratch copy and touching after each restore: dropping the `(deleted)` parse reddens, ignoring an inode mismatch reddens, removing the version-trap sentence from the message reddens.
- **My first message assertion survived mutation.** `contains("version")` matched an unrelated occurrence of the word elsewhere in the same string, so it pinned nothing. Tightened to pin the actual sentence, then re-run to confirm it now reddens.
- Live proof through the real code path, not a fixture: `probe_mapped_plugin_jar(1023909, "agent-doc-jetbrains-")` returns `Deleted { path: ".../agent-doc-jetbrains-0.2.388.jar" }` against the running IDE.

**`make install` deferred correctly.** `lib-install` reported `1 deferred mid-cycle` and kept the previous native generation because this document has an open cycle. Worth recording, because it contradicts the assumption I carried into the previous cycle: the two `libagent_doc-*.so` mapped into this IDE today were not an ungated install stranding a replica. The `#installstrandsreplica` gate is working.

**Not done, and deliberately so.** The item also proposed bumping the plugin version on every build. I did not, because this probe makes activation detection independent of the version string, and auto-bumping would churn the version on no-op rebuilds. Filed as `#jbversionbumpperbuild` `[recommended]` at the back of the queue so the call is yours rather than mine.

CI run `35538568288` was still in progress at closeout; the prior run on `6da64a97c` was green. Recording it pending per the closeout rule rather than blocking on it. Exchange is at 747 lines across 30 sections.

### Re: do [#jbversionbumpperbuild] — opus-5 · 2026-09-20T17:50-04:00

> **Queue prompt:**
>
> do [#jbversionbumpperbuild]
>
> do [#missingreplicanoise]


Done in `f50298c69`, pushed. I filed this one `[recommended]` arguing against it; you queued `do`, so it is built. Two items close this cycle — `#missingreplicanoise` landed last turn in `f20d66aa6` but never got its closeout because that turn never obtained a cycle contract.

**First, live proof that the previous item works.** This turn's preflight opened with my own warning:

```
plugin_bytes_superseded: live jetbrains editor pid 1023909 is running
superseded plugin bytes ... mapped but its inode was unlinked
```

That is `#pluginbyteidentity` firing in production against the real IDE, not a fixture.

**The gap.** `install-editor-plugins` ran `gradlew buildPlugin` with no bump, so every `make install` reinstalled under the same version string. That is precisely how two distinct 0.2.388 jars came to exist.

**I got the predicate wrong first, and the measurement caught it.** My initial cut reused the existing fence logic — "sources changed since the version file was last committed" — because that predicate already existed and looked right. It is not: it stays true for as long as the tree is dirty, so it bumped on *every* invocation. Three bumps for zero distinct builds, observed directly rather than reasoned about. That is exactly the churn I had objected to when I filed the item, reintroduced by my own implementation.

The correct basis is a content digest. `pluginSourceDigest` is recorded beside `pluginVersion`, and the bump fires only when the source bytes differ from the packaged generation. Verified across five runs: bump, hold, hold, bump-on-change, and on revert a *new* version rather than reuse of the old one.

Tracked and untracked sources both count, because `gradlew` compiles whatever is on disk. An unstaged `.kt` edit is already in the artifact, and that is the specific gap the duplicate 0.2.388 pair came through — the pre-existing fence only ever looked at staged and committed files.

So the objection I raised when filing this is answered rather than overridden: the invariant holds, and an unchanged rebuild still adds no churn.

**Verification.** `make check` exit 0, 9586 tests run, 9586 passed, 259 skipped, with the new `--self-test` executing inside `plugin-version-check`. Mutation-checked: a digest that ignores file content, a bump that clobbers the file, and a `write_property` that always appends each redden. Live: `make install` bumped 0.2.388 → 0.2.389 and installed `agent-doc-jetbrains-0.2.389.jar`.

**`#missingreplicanoise`, closed here.** `f20d66aa6` adds `disposition=transient_probe|recovery_exhausted` plus `recovery_attempted`, extracted as `missing_replica_disposition` so it is testable. `make check` was green at 9586 and the live log showed **37 emissions, 37 `transient_probe`, 0 `recovery_exhausted`** — confirming the benign diagnosis instead of assuming it, and leaving the needle at zero so a real strand will stand out.

**One caveat worth stating.** The committed `pluginSourceDigest` covers the working tree at install time, which includes another session's in-flight plugin edits. A clean tree bumps once more on its next install and then holds. I committed only my three paths and left that session's files alone.

CI runs `35539851558`, `35539350263`, `35538568288` were all still in progress at closeout; recording them pending rather than blocking. Exchange is at 778 lines across 31 sections.

### Re: do [#preflightrefusalcontradiction] — opus-5 · 2026-09-20T18:32-04:00

> **Queue prompt:**
>
> do [#preflightrefusalcontradiction]
>
> do [#commitsilentfirstrun]


Two items close: `#preflightrefusalcontradiction` (`8733350bf`) and `#commitsilentfirstrun` (`dc23ccb5a`). A third commit, `14b3d1cdb`, fixes a CI failure the first one exposed.

**The fixes proved themselves on arrival.** This turn's preflight carried *both* plugin warnings:

```
stale_plugin:             live plugin 0.2.388 != 0.2.389 shipped
plugin_bytes_superseded:  pid 1023909 ... mapped but its inode was unlinked
```

Before this session neither could fire. `stale_plugin` was silent because both builds were called 0.2.388 — that is `#jbversionbumpperbuild` giving the new build a distinct version. `plugin_bytes_superseded` did not exist. And the refusal that opened this turn was coherent for the first time: one verdict, sentence boundary intact, no "the same intent resumes" fighting "STRANDED … waiting will not commit them".

`#commitsilentfirstrun` measured cleanly before and after. The same recovery command on the same document: **run 1 was 0 bytes / exit 0**, now it is **1611 bytes / exit 1** — the real diagnostic, immediately. The cause was one line discarding the outcome with `.map(|_| ())`, in the command the wedge diagnostics name as the remedy.

**CI caught something my local suite could not, and it was mine to fix.** Run `35540520917` failed on `8733350bf` while `make check` was green here. Not a false alarm: my local tree carries another session's uncommitted source, and my commit carried only my files.

The failure was `test_poll_lazily_event_timeout`, asserting `elapsed < 300ms` for a 100ms timeout. That runner took **1489s for a suite that takes ~170s here** — a 3x wall-clock bound cannot survive a ~9x slower machine. It is genuinely unrelated to my change, but "unrelated" and "flaky" are not waivers, and the bound was simply wrong.

Two things I did not do. I did not fix only the test that happened to fail: the same defect existed in three crates, and repairing one would have relocated the flake. And I did not simply widen the numbers — for two of the three, the property they claim to test is **already pinned by a machine-independent assertion on the line above** (`touched < 60`, and `StartupLockAcquire::Busy`), so the timing bound was redundant as well as fragile. Mutation-checked that the widened bound is still a guard: an 8s timeout where the test supplies 100ms still reddens.

**Verification.** `make check` exit 0, 9587 tests run, 9587 passed, 259 skipped. All fixes mutation-checked. Earlier CI: `#pluginbyteidentity`, `#missingreplicanoise`, `#jbversionbumpperbuild` all green; `#commitsilentfirstrun` still running.

**The item I owed, now filed.** `#reappersistloop` covers the closeout interruption I have been describing in prose for five consecutive cycles. It is structural, and it is worse than cosmetic: `session-check` calls it an INTERRUPT, `repair` calls the same document clean, and `commit` refuses while `repair` warns that running `commit` would commit unreviewed drift as metadata. Nothing is lost — a later preflight does land the reap — but a false interruption every cycle is, per the auto-loop skip list, a valid reason to stop draining. It is queued ahead of `#runsubmitautoproof` because it affects every closeout.

Exchange is at 816 lines across 32 sections, accretion now `block`; noting it rather than stalling the queue.

### Re: do [#runsubmitautoproof] — opus-5 · 2026-09-20T20:28-04:00

> **Queue prompt:**
>
> do [#runsubmitautoproof]


Done. `#runsubmitfirstmatch` now carries a typed verify predicate, so the gate no longer depends on a human watching a pane:

```
verify=ops_log:controller_editor_route_settled
disproof=ops_log:startup_miss
```

**No code was needed** — the `#optverify` machinery already existed, so this was a wiring task. What it required was picking two markers that mean what they claim, and three things needed checking rather than assuming.

**The marker had to be non-self-provable.** Predicates match plain substrings against `ops.log`, and `#gng8` strips double-quoted spans so embedded document prose cannot prove a gate from its own description. Since this response and the review item both *mention* the marker, that mattered: 36 occurrences, all structured `] controller_editor_route_settled ` emissions, **zero** as prose. The gate cannot prove itself.

**The obvious disproof was wrong.** `resubmit_required=true` looks like the failure signal and occurs twice, but post-fix it means the detector *caught* a stranded composer and recovered — the fix working, not failing. The original bug was a **false** `outcome=cleared`, which by construction leaves no honest log signature. So the disproof has to be a real miss: `startup_miss`, a routed dispatch that never became a turn, currently zero.

**The timestamp format could have made the whole thing inert.** `scan_ops_log` filters on epoch seconds while this log is ISO-8601; an epoch-only parser would skip every line and leave the predicate `Pending` forever while looking correctly configured. `parse_log_timestamp` accepts both.

Verified through the real function rather than a reimplementation of its rules: `scan_ops_log` returns `Provable { marker: "controller_editor_route_settled", at: 1789592470 }` against the live log. With `set_at` stamped now, it re-proves on the next Run Agent Doc dispatch.

**A correction to my own filed item.** It said to wire this via `--backlog-set-gate-type`. That flag sets a gate *type* and rejects the predicate outright (`invalid gate type: must be alphanumeric/dash/underscore`); the correct flag is `--backlog-set-verify`. I had misread two adjacent flag descriptions as one when filing. The first attempt was rejected before anything was written, so nothing partial landed.

**One thing I deliberately did not do.** Auto-*resolution* additionally needs `agent_doc_gate_autoverify: true`. There is no write flag or subcommand for frontmatter, so the only routes are hand-editing frontmatter or `.agent-doc/config.toml` — and the latter is project-wide, affecting every document and every concurrent session. Silently flipping human review gates across a shared project is your call, not mine. Without it the binary still reports the gate as `provable`, which is what removes the human-watcher requirement; enabling it removes the last manual step.

**On the `queue_stall_detected` warning.** It is accurate and it is about me. I have been re-entering the drain with scheduled wakeups, which the binary scores as neither a continuation nor a valid stop. The reason is a real defect, now filed as `#loopreentrynoop`: the Stop hook prescribes invoking the `loop` skill, but once `/loop` is loaded, re-invoking it returns "already loaded; instructions unchanged" and seals no contract, because a Skill call is not a submitted prompt. Four consecutive occurrences. With shelling `agent-doc <FILE>` forbidden by the hook and `agent-doc preflight` forbidden by `#preflightinbinary`, a scheduled re-submission was the only mechanism left that yields a sealed contract. Appended rather than prepended so it does not preempt `#reappersistloop`.

No code changed this cycle, so no suite run applies. Exchange is at `block` accretion; noting it rather than stalling the queue.

### Re: do [#loopreentrynoop] — opus-5 · 2026-09-20T20:56-04:00

> **Queue prompt:**
>
> do [#loopreentrynoop]
>
> do [#reappersistloop]


Two items close. `#loopreentrynoop` is fixed in `c0871a8c0`; `#reappersistloop`'s measured half landed last cycle in `80b49e61f`.

> **Queue prompt:** Selecting fpe.md focused on the tmux pane in the stash tmux window. Fix agent-doc causes.

**`#loopreentrynoop`.** The Stop-hook continuation instruction named exactly one mechanism — "invoke the `loop` skill" — and that mechanism is a **no-op for every iteration after the first**. Once `/loop` is loaded, the Skill tool answers "already loaded … instructions unchanged" and seals no contract, because a Skill call is not a submitted prompt and never fires the `UserPromptSubmit` hook that runs preflight. Six consecutive occurrences.

That left no legal move: the same message forbids shelling `agent-doc <FILE>`, and `#preflightinbinary` forbids shelling `agent-doc preflight`. I re-entered the only way available — a scheduled wake-up re-submitting the trigger — and `#qstallguard` then scored the drain as `no_valid_stop_with_continuation_required`. The hook and the stall detector were contradicting each other, with the agent in between.

The instruction now names the no-op answer so the agent can *recognise* it, names the fallback that actually re-enters with a sealed contract, and states that a scheduled re-entry is a continuation rather than a stall. Both forbidden shells stay forbidden. The regression pins those properties rather than the prose.

**A false green in my own mutation probe, worth recording.** Dropping `already loaded` first appeared *survivable*. It wasn't: my replacement hit the doc comment rather than the format string. Re-targeted at the format string it reddens. The check that mattered was looking at where the mutation actually landed, not at the exit status.

**`#reappersistloop`, and what it does not cover.** `80b49e61f` scopes the guard to items the current cycle did not reap — covering the post-`respond` session-check, where all six measured false interruptions occurred and which is the observation the auto-loop reads. It does **not** cover a session-check run mid-next-cycle, because `reaped_pending_ids` is per-cycle state and a new cycle has none by construction. I verified that live rather than assuming the fix was complete. The remainder is filed as `#reappersistcrosscycle` with the design choice stated, because closing it means either archiving inside the same closeout or adding a durable in-flight marker, and I would be guessing at which you want.

**Your fpe.md report is diagnosed, not yet fixed** — filed as `#fpeselectstashpane` at the top of the backlog. My first instinct was wrong: this is not stale binding. `session status` binds `fpe.md` to pane `%5` in window `@0`, which is the correct `0:0 agent-doc`. The enabling condition is duplication — your stash window holds 14 panes, **four of them `agent-doc` processes sharing `fpe.md`'s `src/haiven-dev` cwd** — so any resolver matching on cwd or command rather than the bound pane has four wrong answers to choose from.

I ruled out three plausible culprits by reading them: session selection already prefers `in_agent_doc_window`, passive sync no longer reselects panes at all, and both column resolvers work on file paths rather than panes. What remains is the column-to-pane mapping, most likely in `tmux-router`'s registry lookup. I stopped there rather than patch a path I had not traced.

I also did not run `session doctor --repair` to clear those stash panes. It acts on shared tmux and could kill panes belonging to your other live sessions — that is your call, not mine.

`make check` exit 0, 9592 tests run, 9592 passed, 259 skipped. CI `35549225894` and `35548340423` in progress; `#citimingflake` and `#commitsilentfirstrun` already green.

### Re: #fpeselectstashpane — opus-5 · 2026-09-21T05:15-04:00

> **Queue prompt:**
>
> do [#fpeselectstashpane]


Fixed and pushed as `4907a2230`. The diagnosis in the backlog item was right that this is not stale binding — but the wrong pane was never *resolved from* the records. Both records were correct and the selection effect simply did not use them.

**Two selection paths resolved a pane live and let that result outrank the binding.**

`decide_focus_pane_candidate` (controller; the editor tab-selection path, `handle_editor_surface_observe` → `handle_focus_document_pane_with_policy`) returned `proven_live_owner` **first**, before it looked at the actor row at all. A live, focusable actor at `%5` was replaced by the live-resolved pane with `focused_reason=focused_live_process_owner`. An existing unit test pinned exactly that — `focus_candidate_reconciles_stale_active_actor_to_new_live_owner` asserted `(actor %old focusable, registry %old, live %new) → %new`. The function cannot tell "stale" from "current"; it took the live owner whenever one existed.

The live owner comes from `proven_session_log_focus_owner`, which reads `status.latest_start_pane` — the pane named by the last `session_start` event. That is where the session was last **started**, not where it lives now. After a reroute, resume, or rebind the record still names the superseded pane, and that pane's stale owner process is alive and parked in `stash`. It passes both guards: `pane_owns_live_agent` is true and `process_tree_exactly_owns_document` is true, because it genuinely is an agent-doc process for that document — just not the current one.

`agent-doc-focus-io` had the same shape through `live_owner_override` and `decide_registered_focus_candidate`, which call `sync::find_live_owner_pane_quiet`. That is the **heuristic recovery** resolver, and `find_live_owner_pane_excluding_with_logging` says so in its own first comment: *"Full heuristic recovery remains available for explicit repair/resync paths. Normal route/start/sync ownership decisions must use `find_normal_path_owner_pane*` instead."* Focus is a normal navigation path and was using the heuristic one. Its legs are the session-log pane, a supervisor-pid process-tree scan across every pane on the server, and a whole-server cmdline scan — three independent ways to name a different pane, each taken over a binding that was still true.

That is where your "four wrong answers available" enters. It is not a cwd match. The stash currently holds eight `agent-doc` processes under `src/haiven-dev`, each started as `agent-doc start --route-owned --route-owned-reap-policy keep-alive --route-owned-start-purpose layout-provision --resume -- tasks/<doc>.md`. They are exactly the population a whole-server scan or a superseded start record draws from.

**The rule now:** a selection effect uses the bound pane; a live probe may only *invalidate* the binding, never replace it. The new proof asks one question about the one named bound pane — does its own process tree still run an agent-doc owner session for this document (`sync::pane_process_tree_owns_document`) — so its answer can never nominate some other pane. While it holds, the actor/registry pane is selected as-is and the resolvers are not consulted at all. Only a binding that has stopped being true falls through to the unchanged live-owner repair, and a `Blocked`/`Closed` actor projection is not revived by this precedence.

Changed: `decide_focus_pane_candidate` now takes a `FocusActorCandidate { pane_id, focusable, still_owns_document }`; `live_owner_override` short-circuits before the resolver runs; `decide_registered_focus_candidate` takes the same proof and no longer fails closed when the bound pane is itself the proof. `specs/07-session-tmux-commands.md` carries the clause, sitting with `#panewindowdrift` and the stash-surfacing rules.

**Coverage was attacked, not reasoned about.** Deleting the new precedence block makes `focus_candidate_keeps_the_bound_actor_pane_over_a_differing_session_log_owner` report `%28` / `focused_live_process_owner` where `%5` / `focused_agent_doc_actor` is required — I ran that mutation and restored from a scratch copy, not `git checkout`. `live_owner_override_declines_when_the_bound_pane_still_owns_the_document` panics its fake resolver if the heuristic path is entered at all, so the short-circuit is pinned by execution rather than by a return value.

`make check`: 9593 run, 9593 passed, 259 skipped. `cargo build --release` green, which matters because the supervisor auto-install builds release and `make check` does not. I confirmed all 12 focus tests execute under the same `--workspace --all-targets` nextest scope `make check` uses, rather than trusting the filtered run. CI run 35582189472 is queued as of this writing; the prior run on `main` is green.

No mid-session `make install`: this document is attached to a live JetBrains editor, and a cdylib reload there strands the attached replica and wedges closeout. The supervisor auto-install picks the commit up at its next idle boundary, and the release build is already proven green so it will not stall on a release-only break.

**One thing I did not close.** Your secondary note is a real defect, and I filed it rather than fixing it here because the fix is a reaper acting on shared tmux — the same reason you declined `session doctor --repair`. Those layout-provision panes are created with `--route-owned-reap-policy keep-alive`, so nothing ever reaps them: every editor tab visit to a document with no live actor leaves a permanent pane. It is `#stashpaneunbounded`, appended rather than prepended so it does not preempt the drain.

Two notes on the tree. The working copy carries another session's uncommitted `editors/jetbrains/**` and `src/session_actor_cmd.rs` changes; I committed only the five files of this fix, so `make check` was green over a tree that includes their work. And the exchange is at 913 lines across 35 response sections — noting it, not stopping for it, since the queue is active.

### Re: #stashpaneunbounded — opus-5 · 2026-09-21T05:43-04:00

Fixed and pushed as `0252240c8`. I also chased the tsift.md report you sent mid-turn; that one I filed rather than fixed, and the reason is specific — see the end.

**The defect, measured before touching anything.** `keep-alive` should bound a *commit edge* — "do not reap merely because a cycle committed." It was read as a lifetime. `route_owned_reap_decision` takes a liveness reason and, for `KeepAlive`, discards it unread:

```rust
RouteOwnedReapPolicy::KeepAlive => { reap: false, reason: "explicit_keep_alive" }
```

The haiven-dev ops log settles it: **45 `policy=keep_alive` decisions, 35 of them `reason=explicit_keep_alive`, zero reaps** — across api, contracts, docs, infra, payments-ledger, and sdk. One permanent pane per document ever selected in the editor. That is the 15-pane stash window, and it is the supply of wrong answers every live pane resolver draws from, which is how it fed `#fpeselectstashpane`.

**Why the reap is safe to do from inside the supervisor.** A layout-provision owner exists to hold a pane for an editor column and never dispatches — `route_owned_start_blocked_by_queue_control` fences `Dispatch` and admits `LayoutProvision` for exactly that reason. Once its pane is **stashed** it is holding no visible column, so it is doing its one job nowhere at all. It now ends itself and publishes `layout_provision_stashed_orphan`. Re-selecting the document provisions a new owner through the same missing-actor path that created this one.

This directly answers your constraint about `session doctor --repair`. Nothing here enumerates panes. `owned_pane_is_stashed` asks one question about the one pane this supervisor owns, and an unresolvable pane or window answers `false`, so the reaper acts only where it can positively prove a stash. No working directory, no command line, and structurally impossible to touch a pane another session owns.

Two more guards: a live-pane-busy reason and an open cycle each short-circuit first, so an active harness turn is never reaped. And a **non-empty backlog is not liveness for this leg** — it is the steady state of every task document, so counting it would restore the unbounded behaviour outright. A queued prompt, unanswered exchange tail, post-commit follow-up, uncommitted drift, or adapter failure all still keep the owner alive.

**The subtlety that would have made a smaller patch inert.** The existing loop only reaches a reap decision after `route_owned_cycle_committed_since_start`. The shape that actually accumulates — a pane provisioned for a column and then never used — never moves its document's cycle, so that predicate stays false and no decision is ever evaluated. Eight haiven-dev panes sat there indefinitely while the loop polled every 500ms. The orphan check therefore runs *before* the commit-edge machinery, on its own 15s throttle so it does not spend a tmux query per pane per half-second watching a fact that changes when you move a tab.

`auto` and `reap-after-commit` are provably untouched: one test asserts the purpose-aware wrapper is identical to the old function across both policies × both purposes × both stash states × both liveness shapes. A `keep-alive` **dispatch** owner is likewise untouched.

Also fixed a slip from my previous commit: `pane_process_tree_owns_document` had been inserted between `pane_in_stash_window`'s doc comment and its signature, silently stealing it. Moved below.

`make check`: 9599 run, 9599 passed, 259 skipped. `cargo build --release` green. One architecture guard in `tests/test_cli.rs` pinned the seam by substring and failed on the rename — I updated it to the new API and added the bare `route_owned_reap_decision(` spelling to the forbidden list, since that spelling is the one that answers `explicit_keep_alive` unconditionally. CI `35584864204` queued; the `#fpeselectstashpane` run before it is in progress.

### Re: the tsift.md fence — opus-5 · 2026-09-21T05:43-04:00

> **Queue prompt:**
>
> do [#stashpaneunbounded]


I found it, and it is not where the symptom points. tsift.md's queue component is clean (`- 🚧 do [#tsiftambientexplicit]`). The fenced block is **inside the backlog component**, above the only real item.

I traced the origin rather than guessing: `git log -S` gives exactly one commit, `46566f42ce` (2026-09-18 03:04:39), and the *same write* added your "Fix discord.md issue" prompt to the exchange and inserted the fence into a backlog component that was previously empty. It is a processed derivative, not a paste duplicate — the exchange copy carries the prompt marker and an indented continuation line; the backlog copy has the marker stripped and the indent trimmed. So some write path normalized a multi-line fenced prompt body and emitted it as a raw component body with no checkbox prefix.

Once there it is permanent. It parses as a `PendingSegment::Text` prelude, which `parse_items` preserves by design; `--backlog-edit` / `--done` / `--backlog-reorder` act only on items, `patch:backlog` full-replace is rejected, and `remove_items_with_trailing_spill` reaps only spill *trailing a removed item* — never a prelude. Your only recourse today is hand-editing a file the design forbids hand-editing.

I did not fix it in this cycle, and not because it is hard. The removal half needs a decision I should not make unilaterally: a component prelude can also be legitimate prose you wrote, and the standing rule is that operator-visible text is authoritative and must never be dropped, so a blanket prelude prune is the wrong instinct. The narrow rule I think is defensible — prune only text provably duplicated from the exchange prompt it was derived from — is a design choice worth your yes or no before I build it. Filed as `#componentresidue` at P1 with all of the above, appended so it does not preempt the drain. Say the word and I will take the narrow rule.

One aside found while filing it: a backlog item whose own text contains a component marker is rejected with "unclosed component" and nothing is applied. Fail-closed is right; the message just does not say that your *item text* was the cause.

Exchange is at 946 lines across 36 sections; noting it, not stopping.
<!-- agent:boundary:00048a2e:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- ~~do [#stashpaneunbounded]~~
- ~~do [#fpeselectstashpane]~~
- do [#reappersistcrosscycle]
- release + publish
<!-- /agent:queue -->


<!-- agent:backlog priority queue -->
- [ ] [#componentresidue] [P1] A managed component can hold unstructured text that NO agent-doc command can remove, and the operator hit it: tasks/software/tsift.md carries a fenced code block INSIDE its agent:backlog component, above its only real item, and reported it as a queue fence that was never cleared or struck. Origin proven by git archaeology, not guessed: commit 46566f42ce (agent-doc(tsift), 2026-09-18 03:04:39) is the single commit introducing that text, and the SAME write added the operator prompt to the exchange and inserted the fence into a backlog component that was previously EMPTY. It is a processed derivative, not a paste duplicate — the exchange copy starts with the prompt marker and has an indented continuation line, the backlog copy has the marker stripped and the indent trimmed. So a write path normalized a multi-line fenced queue/prompt body and emitted it as a raw component body with no checkbox item prefix. It then becomes a PendingSegment::Text prelude, which parse_items preserves forever: --backlog-edit/--done/--backlog-reorder act only on items, patch:backlog full-replace is rejected, and remove_items_with_trailing_spill only reaps spill TRAILING a removed item, never a prelude. The operator's only recourse is hand-editing a file the design forbids hand-editing. Two parts: (a) find and fix the producer so a multi-line or fenced body is never written as a component body — reject it or render it as a single item; (b) give the operator a way to remove existing residue. (b) needs a deliberate decision, not a guess: a component prelude can also be legitimate operator prose, and the standing rule is that operator-visible text is authoritative and must never be dropped, so a blanket prelude prune is wrong. A defensible narrow rule is to prune only text provably duplicated from the exchange prompt it was derived from. Related: a backlog item whose own text contains a component marker is rejected as an unclosed component (observed 2026-09-21 filing this item) — correct fail-closed behaviour, but worth a clearer message.
- [x] 🚧 [#stashpaneunbounded] [P2] Layout-provision panes accumulate in the stash without bound — 15 in window 0:1 right now, 8 of them agent-doc processes in src/haiven-dev. Each is created by handle_focus_document_pane_with_policy's missing-actor branch (controller_focus_resumed_missing_actor -> route_auto_start with ControllerRouteAutoStartPolicy::ProvisionOnly), which starts agent-doc start --route-owned --route-owned-reap-policy keep-alive --route-owned-start-purpose layout-provision. keep-alive means nothing ever reaps them, so every editor tab visit to a document with no live actor leaves a permanent pane. They are the supply of wrong answers every live pane resolver draws from (#fpeselectstashpane was one consequence), and they are why session doctor --repair is unsafe here — the operator declined to run it because it acts on shared tmux and could kill panes other live sessions own. Fix the lifecycle: a layout-provision pane that no editor surface still names, and whose document has a different live actor, should be reaped at an idle boundary. Any reaper must prove the pane is a layout-provision orphan from its own start record, never from cwd or command matching.
- [ ] [#reappersistcrosscycle] [P2] Finish the other half of #reappersistloop. 80b49e61f scoped the completed-reap guard to items the CURRENT cycle did not reap, which covers the session-check immediately after respond, where all six measured false interruptions occurred and which is the observation the auto-loop reads. It does NOT cover a session-check run mid-NEXT-cycle: reaped_pending_ids lives in per-cycle state, so a new cycle has none by construction, and the [x] item legitimately persists until that cycle closeout performs the archive move. Verified live: the guard still interrupted on a fresh cycle holding the previous cycle [x] runsubmitautoproof. Closing this needs a design choice the fix deliberately did not guess at: either perform the archive move inside the same closeout that marks [x], or add a durable reap-in-flight marker that outlives the cycle row. Pick one and make session-check, repair, and commit agree under it.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#runsubmitfirstmatch] [operator-verify] — confirm in a live pane that JetBrains "Run Agent Doc" now submits the trigger instead of stranding it in the composer. Fixed in 275ab868e: route_trigger_visible_in_current_draft scans newest-first, so repair_pass_through_stranded_draft no longer reports outcome=cleared over a composer that still holds the trigger. Reproduce the old shape by running Run Agent Doc twice on the same document without clearing, so a consumed copy of the identical trigger sits within 16 non-empty lines above the new draft. NEXT: a human watches one Run Agent Doc dispatch land as a started turn; then resolve. <!-- gate-verify verify="controller_editor_route_settled" disproof="startup_miss" set_at="1789950246" -->
- [/] [#cargovodtrim] [operator-verify] [P2][security] Trim the credential exposure from the livestream VOD once the operator supplies the VOD URL or platform/start-time locator. Known exposed interval: 2026-09-12T02:57:27.170Z through approximately 02:58:14Z.
- [/] [#pypiquotafix] [operator-auth] [P1] AUTHORIZED cleanup: after 0.35.383 thin wheel is live, sign into https://pypi.org/manage/project/agent-doc/releases/ in Chromium tab 0; then permanently delete the 34 oldest existing releases, 0.35.0 through 0.35.112 (all pre-2026-08-02), freeing about 1.75 GiB. Pinned PyPI installs of deleted versions will stop working. Browser is currently logged out; support request pypi/support#12214 remains open.
- [/] [#tmuxfocussyncverify] [operator-verify] Restart IntelliJ to activate the staged JetBrains plugin, then keep agent-doc-bugs.md and the cross-root infra.md visible, click each embedded tmux pane, and verify editor selection follows without stealing focus from an actively focused editor component. PREMISE RE-MEASURED 2026-09-20T17:05-04:00 and the prior record was wrong on every identifier: live IDEA is PID 1023909 started 2026-09-20 15:44:32, not PID 1645748; the staged jar was written 2026-09-20 16:42:24, not 2026-09-18 16:39:05. Hard activation proof, stronger than any mtime or version comparison: /proc/1023909/map_files resolves agent-doc-jetbrains-0.2.388.jar as (deleted), so the running IDE executes a superseded inode while disk holds inode 76585058 under the SAME 0.2.388 version string. Still not activated. Automate via #pluginbyteidentity. Plan: tasks/agent-doc/plan-tmux-auto-sync-reactive.md.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#activateinstalledjetbrai] [operator-verify] One IntelliJ restart activates the staged JetBrains plugin. PREMISE RE-MEASURED 2026-09-20T17:05-04:00: live IDEA is PID 1023909 started 2026-09-20 15:44:32; staged jar written 2026-09-20 16:42:24. The recorded PID 1645748 and the 2026-09-18 16:39:05 write are both wrong, and the PID 306200 asserted earlier in this document does not exist at all. Proof of non-activation is byte identity, not version: /proc/1023909/map_files shows agent-doc-jetbrains-0.2.388.jar (deleted) against on-disk inode 76585058, with the same 0.2.388 version string on both. Remaining splice proof unchanged: captured local edits recover across an independently advanced canonical response in a disposable live editor document. Original retained capture committed as 1b35dcb028 and all automated regressions passed. Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#harnessswitchverify] [operator-verify] Remaining proof: in a disposable live IntelliJ session, make an unsaved codex -> claude -> codex frontmatter switch while observing supervisor receipts; prove one spawn per authoritative switch, deferred restart for non-authoritative evidence, and no refusal or respawn storm. PREMISE RE-MEASURED 2026-09-20T17:05-04:00: live IDEA is PID 1023909 started 2026-09-20 15:44:32, so the previously recorded live PID 1645748 is gone as well as the older 1035669; the staged plugin jar 0.2.388 written 16:42:24 is not activated in that process; installed CLI is 0.35.400. Unblocks: provide the disposable document path and perform the real unsaved editor edit, since persisted CLI writes cannot substitute for this proof. Plan: tasks/agent-doc/plan-stop-gated-queue-head.md.
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
