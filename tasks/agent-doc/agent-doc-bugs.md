---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: 3d56a2a6-95ce-42c4-b71c-66c102e54fb4
  codex: 01a0b046-cac2-7f32-a18a-c61373d0f342
agent: codex
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
queue: stop
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
<!-- agent:boundary:93b90753:agent-doc-bugs -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
<!-- /agent:queue -->


<!-- agent:backlog priority -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/release] [#github5253release] Publish the next weekly agent-doc batch no earlier than 2026-09-22T16:41:52Z; verify GitHub assets and PyPI, then close issue #53. Issue #52 is already closed; fixes are on main in c92d5d629 and a10a684b3.
- [/] [#release035376verify] [release] Verify GitHub Release run 34737823202 completes green and v0.35.376 publishes all six platform assets; all matrix jobs were still externally queued at closeout.
- [/] [#cargovodtrim] [operator-verify] [P2][security] Trim the credential exposure from the livestream VOD once the operator supplies the VOD URL or platform/start-time locator. Known exposed interval: 2026-09-12T02:57:27.170Z through approximately 02:58:14Z.
- [/] [#fpecompactheal] [operator-verify] Confirm fpe.md's corrupted `<dynamic_context_ref>` block is gone after one compact on the FIXED binary (installed 0.35.367, ~/.cargo/bin/agent-doc at 23:01:39). Must be run FROM THE PANE THAT OWNS src/haiven-dev/tasks/fpe.md — that is a live session (b69e5aeb, `queue: go`) and this cycle deliberately did not hand-align its disk. Expected after `agent-doc compact src/haiven-dev/tasks/fpe.md`: exactly ONE block, with the two self-referential handles (absolute `.../src/haiven-dev/tasks/fpe.md` and relative `tasks/fpe.md`) gone, leaving at most the four instruction-file handles; and the `Prior summary/context:` line still summarising the prior compact list rather than quoting XML.
- [/] [#fixrunlazily] Run Agent Doc on lazily.md fails with `command-plane editor_route request failed via .agent-doc/controller.sock: Project Controller did not respond within 60000ms; the controller may be wedged`. NARROWED 2026-09-11, two hypotheses measured and DISPROVED — do not re-walk them. (1) The failure is the ACCEPT leg, not the work leg: the prefix `command-plane editor_route request failed` plus 60000ms is SOCKET_REQUEST_TIMEOUT_MS in sendAcceptedCommandSubmitToSocket; awaitCommandSubmitTerminal uses waitForReadySeconds*1000+grace instead. So the controller failed to ACKNOWLEDGE a command submit for 60s, and handle_editor_command_submit_async_rpc only enqueues. (2) NOT serialized accepts: rpc.rs:11185 listener.accept() spawns a thread per client. (3) NOT a slow pre-serve window: the socket binds at ListenerOptions::create_sync() (rpc.rs:11134) and the accept loop starts at 11184, so synchronous work between them leaves clients connected and hanging — the ss -lxp backlog signature — but the candidate, restore_reliable_sync_liveness, measures 0.08s for all 59,671 reliable_sync_liveness_journal rows / 6.5MB / 60,582 ops on this project, and its recovery effects are already spawned async. Live evidence at the failure window: ffi_ensure_controller_running_failed Connection refused (os error 111) x7 at 14:00:58-59, controller adopted 14:01:44, and a separate client still timing out at 14:02:23 (timed out after 5.0s waiting for project controller response) — unresponsive at least 40s after adoption. NEXT LEAD: what holds a lock across handle_request_locked (rpc.rs:12526) at runtime.bootstrap_snapshot() (12560) during a controller first seconds. Repro lever: kill the controller, then click Run Agent Doc while a replacement is launching.
- [/] [#pypiquotafix] [operator-auth] [P1] AUTHORIZED cleanup: after 0.35.383 thin wheel is live, sign into https://pypi.org/manage/project/agent-doc/releases/ in Chromium tab 0; then permanently delete the 34 oldest existing releases, 0.35.0 through 0.35.112 (all pre-2026-08-02), freeing about 1.75 GiB. Pinned PyPI installs of deleted versions will stop working. Browser is currently logged out; support request pypi/support#12214 remains open.
- [/] [#tmuxfocussyncverify] [operator-verify] Restart IntelliJ to activate installed JetBrains plugin 0.2.379, then keep agent-doc-bugs.md and the cross-root infra.md visible, click each embedded tmux pane, and verify editor selection follows without stealing focus from an actively focused editor component. Automated lifecycle and transition regressions plus make check tmux-ci passed; source v0.35.362 is published. Plan: tasks/agent-doc/plan-tmux-auto-sync-reactive.md.
- [/] [#grokmcpverify] [operator-verify] Verify an authenticated Grok Build MCP admit/read/plan/finalize cycle and same-turn auto-queue continuation once the free usage quota renews or the planned subscription is active. Grok CLI authorization, native JSON/backend commit fixtures, pane detection and quit behavior passed; the live MCP probe stopped at the usage limit before committed closeout. Installed v0.35.359; Heavy is not required. Plan: tasks/agent-doc/plan-grok-build-support.md.
- [/] [#activateinstalledjetbrai] [operator-verify] Installed JetBrains plugin 0.2.379 is staged while running IDEA PID 1035669 remains on 0.2.378; one restart activates both the reverse tmux-focus fix and the current package generation. Remaining splice proof: captured local edits recover across an independently advanced canonical response in a disposable live editor document. The original retained capture committed as 1b35dcb028, and all automated regressions passed. Plan: tasks/agent-doc/plan-captured-editor-splice-recovery.md.
- [/] [#harnessswitchverify] [operator-verify] Current prerequisites verified: IntelliJ PID 1035669 runs plugin 0.2.378; CLI/native 0.35.361 is installed. Remaining proof: in a disposable live IntelliJ session, make an unsaved codex -> claude -> codex frontmatter switch while observing supervisor receipts; prove one spawn per authoritative switch, deferred restart for non-authoritative evidence, and no refusal/respawn storm. Unblocks: provide the disposable document path and perform the real unsaved editor edit; persisted CLI writes cannot substitute for this proof. The unrelated Stop-hook gate-selection defect is fixed in v0.35.361; plan: tasks/agent-doc/plan-stop-gated-queue-head.md.
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
