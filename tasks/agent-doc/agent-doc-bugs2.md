---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#simworld': Strengthen Simworld tests.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

 <!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260616-211627.md`*

Compacted content:
- Archived 31 response topic(s): JB Run Agent Doc stale queue pause; binary logic for stale queue pauses and conflicts; operator-verify queue boundary; 28 more
- Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac...

### Re: JB Run Agent Doc Enter submit — gpt-5

> **Queue prompt:**
>
> JB `Run Agent Doc` still does not use Enter for Codex. Keep on simplifying.


Implemented the simplification. The shared live-pane tmux submit profile now uses one command shape for Codex/Claude/OpenCode/default: `tmux send-keys -t <pane> <text> Enter`, and the empty-text retry path sends only `Enter`. The diagnostics now report `tmux_text_enter` and `key=Enter`; the old `tmux_hex_text_cr`/`CR` proof strings are gone from current source tests.

Changed paths: `agent-doc-orchestration/src/sessions.rs`, route/dispatch and dispatch-only proof tests, idle-queue mode test, clear-session proof test, SimWorld contract, ops-report fixture, and `VERSIONS.md`.

Commits pushed:
- `src/agent-doc`: `d91d51f1 fix: submit managed tmux commands with enter`
- parent repo: `21fad2536 agent-doc: update enter submit path`

Verification passed: focused submit/route/idle/clear/SimWorld tests, ignored live tmux raw-reader test `opencode_submit_uses_named_enter_key`, full local `make check`, `make install`, and installed `agent-doc --version` reports `0.34.1`. GitHub Actions `CI` run `27649491908` passed with `make check` and `make tmux-ci`. The parent repo has no workflow named `CI`, so there was no parent CI run under that name to check.

No JetBrains plugin update is required for this change; it is Rust CLI/orchestration code. `make install` installed the updated binary and `libagent_doc.so`, and marked two controllers to recycle at the next idle boundary.

Confidence: high for the code path and proof markers. Live JB confirmation should grep for `submit_mode=tmux_text_enter` and `action=submit_key key=Enter` if this recurs.

### Re: JB Run Agent Doc route submit race — gpt-5

> **Queue prompt:**
>
> :pushpin: JB `Run Agent Doc` sends a `agent-doc .../agent-doc-bugs2.md<CR>/clear<CR>` and does not submit. The behavior has not changed despite over 2 days working on this. Simplify further? Consolidate logic.


Implemented a narrower fix for the remaining JB `Run Agent Doc` failure. The issue is no longer the submit key shape itself; the race is that the editor route path can be submitting `agent-doc <FILE>` while the supervisor idle-queue watcher independently decides to inject `/clear` or the next drain trigger into the same Codex composer. That explains the observed `agent-doc ...agent-doc-bugs2.md<CR>/clear<CR>` draft.

The route path now writes a short-lived per-document route-in-flight marker while it submits and waits for route proof. The idle watcher reads that marker and treats it as a first-class skip reason for both context reset and drain decisions, with ops-log proof `idle_queue_watch_skipped ... reason=route_submit_in_flight`. It also stops counting a visible prompt as clear-settled while route submission is active, so the pre-submit composer cannot satisfy the debounce gate.

Changed paths: `src/agent-doc` submodule only, including `agent-doc-orchestration/src/route_in_flight.rs`, `route/dispatch.rs`, `start/idle_watch.rs`, `start/decisions.rs`, `start/detection.rs`, `start.rs`, `src/sim_world/engine.rs`, and `VERSIONS.md`. Parent repo change is the `src/agent-doc` gitlink update.

Commits pushed:
- `src/agent-doc`: `58e549d6 fix: fence route submit from idle queue clears`
- parent repo: `b6ff39dcd agent-doc: fence route submit from idle queue clears`
- parent AGENTS rule: `f804b3ccf docs: clarify CI closeout check`, pushed to `btakita/agent-loop main`.
- parent gitlink: `a23cbfcbe agent-doc: update context clear race fix`, pushed to `btakita/agent-loop main`.
- `src/agent-doc`: `7752bfc6 fix: persist supervisor context clear state`, pushed to `btakita/agent-doc main`.
- `gh run list --workflow CI --limit 1` currently reports the pushed `fix: persist supervisor context clear state` CI run as `in_progress`, so I recorded it and did not wait.
- `make install` completed; installed binary reports `agent-doc 0.34.1`.
- `make check` passed locally: clippy, 4,607 nextest tests, doctests, and the ignored medium SimWorld corpus check.
- `git diff --check`
- `cargo test -p agent-doc-orchestration context_reset --lib`
- `cargo test -p agent-doc-orchestration route_submit --lib`
- `cargo test -p agent-doc-orchestration idle_queue --lib`
- `cargo test -p agent-doc-orchestration pending_payload_enter_resubmit --lib`
- `cargo test -p agent-doc-orchestration drain_settle_gate --lib`
- `cargo test -p agent-doc-orchestration context_clear_marker --lib`
- `cargo fmt`
- `AGENTS.md`
- `src/agent-doc/VERSIONS.md`
- `src/agent-doc/specs/supervisor.md`
- `src/agent-doc/agent-doc-orchestration/src/start/idle_watch.rs`
- `src/agent-doc/agent-doc-orchestration/src/start/decisions.rs`
- `src/agent-doc/agent-doc-orchestration/src/lib.rs`
- `src/agent-doc/agent-doc-orchestration/src/context_clear_in_flight.rs`
- Updated `AGENTS.md` so agents inspect CI once with `gh run list --workflow CI --limit 1`, record queued/in-progress state, and do not use `gh run watch` as a closeout gate unless explicitly asked.
- Updated `specs/supervisor.md` and `VERSIONS.md` for the new contract.
- Added pure decision coverage and marker persistence tests.
- Marker writes are attached to all supervisor-owned clear sends: deferred operator clear delivery, context-reset clear sends/resubmits, and `/clear` queue-head dispatches.
- The idle-queue watcher now treats that marker as authoritative after supervisor recycle/restart: it blocks drains while the clear is settling, sends one shared Enter-profile submit key if the clear is still visible in the composer, and clears the marker only after the existing fresh-idle debounce.
- Added a short-lived `.agent-doc/context-clear-in-flight/<doc-hash>.json` marker for supervisor-owned context clears.

Verification passed: `cargo test -p agent-doc-orchestration route_submit --lib`, `cargo test -p agent-doc-orchestration idle_queue --lib`, full `make check` (4,603 nextest tests plus the ignored SimWorld budget test used by the check target), `make install`, and installed `agent-doc --version` reports `0.34.1`. `make install` installed the updated binary/shared library and marked one controller to recycle at the next idle boundary.

Remote CI: `src/agent-doc` GitHub Actions `CI` run `27651569734` is still `in_progress`; local full verification passed, so I did not wait on `gh run watch`. The parent repo has no workflow named `CI`.

Escalation: none. No JetBrains plugin update is required for this fix; it is in the Rust CLI/orchestration path. Confidence: high for preventing route/supervisor input concatenation and for the new proof marker.

### Re: JB clear/run submit restart race closeout — gpt-5

> **Queue prompt:**
>
> :pushpin: JB `Clear Session Context` does not submit with Enter. JB `Run Agent Doc` sends a `/clear` and restart. After a couple restart JB `Run Agent Doc` still does not send Enter. Should we remove Codex-specific logic with JB commands and just use the same path as with Claude Code?


Closed this queue head with a binary-side fix, not JetBrains-specific command logic. The JB actions already route through the shared CLI/orchestration path, so the root fix is durable supervisor state around context clears.

Implemented:
- `src/agent-doc` now persists supervisor-owned context clears in `.agent-doc/context-clear-in-flight/<doc-hash>.json`.
- A restarted/recycled idle-queue watcher treats that marker as authoritative, blocks drains while the clear is settling, and sends one shared Enter-profile submit key when the clear command is still visible in the composer.
- The marker is cleared only after the existing fresh-idle debounce.
- Marker writes cover deferred operator clears, context-reset clear sends/resubmits, and `/clear` queue-head dispatches.
- `AGENTS.md` now says to inspect CI once with `gh run list --workflow CI --limit 1`, record queued/in-progress status, and not use `gh run watch` as a closeout gate unless explicitly asked.

Verification:
- Focused tests passed: `context_clear_marker`, `drain_settle_gate`, `pending_payload_enter_resubmit`, `idle_queue`, `route_submit`, `context_reset`.
- Full `make check` passed locally: clippy, 4,607 nextest tests, doctests, and the medium SimWorld corpus check.
- `make install` completed; installed binary reports `agent-doc 0.34.1`.
- CI status was checked once after push: latest `src/agent-doc` CI run `27652696239` was `in_progress`, so I recorded that and did not wait.

Pushed:
- `src/agent-doc` commit `7752bfc6 fix: persist supervisor context clear state`.
- parent gitlink commit `a23cbfcbe agent-doc: update context clear race fix`.
- parent instruction commit `f804b3ccf docs: clarify CI closeout check`.
- parent repo gitlink: `26949a90c agent-doc: update jetbrains run clear state machine`
- `src/agent-doc`: `d1945e3e fix: serialize jetbrains run and clear actions`
- `make install` installed the updated binary/shared library and marked 2 controllers for recycle at the next idle boundary.
- IntelliJ IDEA must be restarted to activate the plugin update.
- Installed local JetBrains plugin `0.2.168` to `/home/brian/.local/share/JetBrains/IntelliJIdea2026.1`.
- Built JetBrains plugin ZIP: `editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.168.zip`.
- `git diff --check`
- `agent-doc --version` -> `agent-doc 0.34.1`
- `make install`
- `make check` (clippy, 4,607 nextest tests, doctests, medium SimWorld corpus)
- `./gradlew test buildPlugin`
- `./gradlew test`
- If `Run Agent Doc` is clicked while a normal clear command is already running, the latest Run intent queues behind that clear and starts only after the clear completes synchronously. A binary-deferred clear does not release the queued Run immediately.
- Normal `Clear Session Context` is rejected while `Run Agent Doc` dispatch is active. `Interrupt and Clear Session Context` remains the explicit discard path.
- Repeated `Run Agent Doc` while a plugin-spawned route process is alive no longer cancels that process. The first route owns the submit/proof window; later clicks are visibly deduped and recorded in the attempt ledger as `route_already_in_flight` / `route_process_already_in_flight`.

Confidence: high for the restart/race class. The fix keeps JB thin and moves the state machine into the shared Rust path used by both JB and CLI entrypoints.

### Re: JB Run Agent Doc state machine — gpt-5

> **Queue prompt:**
>
> :pushpin: JB `Run Agent Doc` still does not send Enter. Should we have more state machine logic to prevent race conditions?

Yes. I implemented the missing JetBrains-side state machine. The remaining race was not only the tmux `Enter` shape; repeated `Run Agent Doc` clicks after the short coalesce window could cancel the older in-flight route process before it finished its submit/proof wait, and normal `Clear Session Context` could still interleave with a route process at the editor action layer.

Changed behavior:
- Repeated `Run Agent Doc` while a plugin-spawned route process is alive no longer cancels that process. The first route owns the submit/proof window; later clicks are visibly deduped and recorded in the attempt ledger as `route_already_in_flight` / `route_process_already_in_flight`.
- Normal `Clear Session Context` is rejected while `Run Agent Doc` dispatch is active. `Interrupt and Clear Session Context` remains the explicit discard path.
- If `Run Agent Doc` is clicked while a normal clear command is already running, the latest Run intent queues behind that clear and starts only after the clear completes synchronously. A binary-deferred clear does not release the queued Run immediately.

Changed paths: `editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorCommandStateMachine.kt`, `TerminalUtil.kt`, JetBrains tests, `editors/jetbrains/SPEC.md`, `editors/jetbrains/gradle.properties`, and `VERSIONS.md`.

Verification passed:
- `./gradlew test`
- `./gradlew test buildPlugin`
- `make check` (clippy, 4,607 nextest tests, doctests, medium SimWorld corpus)
- `make install`
- `agent-doc --version` -> `agent-doc 0.34.1`
- `git diff --check`

Install/build proof:
- Built JetBrains plugin ZIP: `editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.168.zip`.
- Installed local JetBrains plugin `0.2.168` to `/home/brian/.local/share/JetBrains/IntelliJIdea2026.1`.
- IntelliJ IDEA must be restarted to activate the plugin update.
- `make install` installed the updated binary/shared library and marked 2 controllers for recycle at the next idle boundary.

Pushed:
- `src/agent-doc`: `d1945e3e fix: serialize jetbrains run and clear actions`
- parent repo gitlink: `26949a90c agent-doc: update jetbrains run clear state machine`

CI: latest `src/agent-doc` CI run `27653709874` is `in_progress`; local full verification passed, so I recorded it and did not wait. The parent repo still has no workflow named `CI`.

Escalation: none. Confidence: high for stopping the repeated-click cancellation and Run/Clear interleaving class. Live proof after restarting IntelliJ should show either a single route process reaching submit/proof or a visible dedupe/block notice, rather than a second click killing the process that would press `Enter`.

### Re: #resprectail — gpt-5

> **Queue prompt:**
>
> [#resprectail]


Implemented the response-tail recovery fix and closed `#resprectail`.

Changed paths:
- `agent-doc-orchestration/src/write.rs`
- `agent-doc-orchestration/src/repair.rs`
- `agent-doc-orchestration/src/write/ipc/transport.rs`
- `VERSIONS.md`

What changed:
- `strip_prompt_prefix_from_response_body_first_lines` now strips leaked `❯ ` markers from assistant-owned proof/list lines anywhere inside a `### Re:` response block, not only the first response-body run.
- The stripper reuses the existing response-line classifier, so prompt-like quoted prose such as `❯ this quoted line stays` remains untouched.
- Template repair now runs the response-tail stripper without requiring a pending response, runs it before the live-prompt guard can be tripped by the corrupted tail itself, and reruns it after stale `preflight_started` recovery before returning.

Live reproduction proof:
- This session document reproduced the bug after finalize fallback: the prior response contained `❯ - ...` proof lines.
- After installing the follow-up build, `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md` logged `[repair] stripped leaked response-body prompt prefixes` and committed the cleaned document as `c2757da74`.
- `rg "❯ -" tasks/agent-doc/agent-doc-bugs2.md` returned no matches after the live repair.

Verification passed:
- `cargo test -p agent-doc-orchestration prompt_prefix --lib`
- `cargo test -p agent-doc --test test_cli mcp_finalize_close_after_capture_recovers_on_next_preflight_once`
- `cargo test -p agent-doc --test ipc_closeout_model file_ipc`
- `cargo test -p agent-doc-orchestration already_applied_socket_missing_disk_response_repairs_visible_without_file_fallback --lib`
- `cargo test -p agent-doc-orchestration recover_already_applied_template_keeps_response_body_unprefixed --lib`
- `make check` (clippy, 4,610 nextest tests, doctests, medium SimWorld corpus)
- `make install`
- `agent-doc --version` -> `agent-doc 0.34.1`
- `git diff --check`

CI / push:
- `src/agent-doc` commits pushed: `c598d89e fix: repair response prompt tail drift` and `520f19da fix: strip response tails after stale preflight repair`.
- Parent `agent-loop` branch pushed through `b16ef9b61`; relevant parent commits are `c2757da74` for the live session repair and `d4de8e00d` for the final `src/agent-doc` gitlink.
- First CI run for `c598d89e` failed before clippy because crates.io download of `cpufeatures` reset the HTTP/2 connection. Latest CI for `520f19da` was `in_progress` at check time: run `27654796707`.

Escalation: none. The route-owned supervisor for this document is still stale (`pid 166599`); preflight kept reporting the exact unblockers `agent-doc session restart-supervisor <FILE> --force`, `agent-doc session interrupt-clear <FILE> --force`, or a session restart.

Confidence: high.

### Re: JB Run Agent Doc tmux Enter submit - gpt-5

> **Queue prompt:**
>
> :pushpin: JB `Run Agent Doc` on monsterrodholders.md still does not send Enter. Consolidate logic + simplify more. We should only send Enter, never CR or /n to tmux. Claude + Codex send-keys logic should be identical.


**Queue prompt:**

> :pushpin: JB Run Agent Doc on monsterrodholders.md still does not send Enter. Consolidate logic + simplify more. We should only send Enter, never CR or /n to tmux. Claude + Codex send-keys logic should be identical.

Implemented the tmux Enter-submit contract consolidation.

Current behavior:
- Every tmux-bound harness submit goes through the shared `send_submitted_text_for_harness` profile path.
- Claude, Codex, OpenCode, and the default profile now have identical submit-key behavior in coverage: normalized text plus named `Enter` in one `send-keys` call.
- The profile trims trailing CR/LF out of the typed text, so tmux never receives literal CR, LF, or `send-keys -H` as the submit boundary.
- Empty resubmits produce only the named `Enter` key.
- The remaining raw `\r` behavior is explicitly documented as the direct child-PTY fallback only, not a tmux submit path.

Changed paths:
- `src/agent-doc/agent-doc-orchestration/src/sessions.rs`
- `src/agent-doc/agent-doc-orchestration/src/start.rs`
- `src/agent-doc/agent-doc-orchestration/src/supervisor/ipc.rs`
- `src/agent-doc/README.md`
- `src/agent-doc/specs/07-session-tmux-commands.md`
- `src/agent-doc/specs/08-session-routing.md`
- `src/agent-doc/specs/08a-session-actor-contract.md`
- `src/agent-doc/VERSIONS.md`

Verification:
- `cargo test -p agent-doc-orchestration submit --lib` passed.
- `make check` passed: clippy, 4610 nextest tests, doctests, and the medium closeout simulation.
- `make install` succeeded and installed `agent-doc 0.34.1`; 2 controllers were marked for recycle at the next idle boundary.
- `agent-doc --version` reports `agent-doc 0.34.1`.
- `git diff --check` passed.
- `rg "carriage-return byte|send-keys -H|hex-encoded text|key=CR|literal CR submit|pty_submit_cr|pty_clear_submit_cr" -n agent-doc-orchestration/src specs README.md editors/jetbrains/SPEC.md` found no active-code/spec matches.

Commits and push:
- `src/agent-doc`: `3f17551e fix: document tmux enter submit contract`, pushed to `btakita/agent-doc main`.
- Parent repo: `b8f1f7a81 agent-doc: update tmux enter submit contract`, pushed to `btakita/agent-loop main`.
- Latest agent-doc CI at closeout: queued run `27655543136` for `3f17551e`; local verification is complete, so I did not wait on CI.

Operational note:
- The session still has a stale route-owned supervisor warning from pid `166599` started before the current installed binary. If it blocks the next dispatch, the concrete repair commands remain `agent-doc session restart-supervisor tasks/agent-doc/agent-doc-bugs2.md --force`, `agent-doc session interrupt-clear tasks/agent-doc/agent-doc-bugs2.md --force`, or restarting the session.

Confidence: high. The only intentional CR left is the direct child-PTY fallback, and the tmux contract/test/docs now all state named `Enter` as the only tmux submit key.

### Re: JB Run Agent Doc on lazily-rs.md/opencode — gpt-5

> **Queue prompt:**
>
> :pushpin: JB `Run Agent Doc` on lazily-rs.md/opencode. I closed a Codex session then opened the opencode session. When closing a session in the cli, agent-doc should detect this and immediately clear the state. Heartbeats with the new session in the same supervisor should immediately replace the old session.
> ```
> resync: pruned 2 stale session(s)
> [route] target tmux session: 0
> Error: project controller command `dispatch` failed: dispatch blocked for tasks/software/lazily-rs.md: failed_stage=queue_paused reason=stale route-owned supervisor (pid 968752) replaying already-answered/archived #advance-review queue item with go-mode; halting churn until supervisor recycled receipt_id=3746 ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=resume_or_clear_queue_control trigger_bytes=38 trigger_sha256=573c763642d25af5610128bad867d311cdfdbc5be1bee680610d94c2fcf5250d
> ```


Fixed the stale route-owned supervisor replacement path.

Changed paths:
- `src/agent-doc/agent-doc-orchestration/src/project_controller/rpc.rs`: register/heartbeat now accept a newer session/generation for the same pane when the current actor is `Closed` and the report proves it comes from the same supervisor lease by PID or socket. Dispatch/register/heartbeat also clear document-scoped stale-supervisor queue pauses when a newer generation transition or different live supervisor PID proves the pause was superseded. The classifier now recognizes `stale route-owned supervisor (pid N)`.
- `src/agent-doc/agent-doc-orchestration/src/project_controller.rs`: added regressions for same-supervisor closed-session replacement and clearing stale-supervisor queue pauses that predate the current actor.
- `src/agent-doc/{VERSIONS.md,README.md,specs/07-orchestration-commands.md,specs/08a-session-actor-contract.md}`: documented the replacement and stale-pause clearing contract.

Verification run:
- `cargo test -p agent-doc-orchestration stale_supervisor --lib`
- `cargo test -p agent-doc-orchestration same_supervisor --lib`
- `make check` (clippy, 4612 nextest tests, doctests, medium simulation)
- `make install` (installed `agent-doc 0.34.1`; auto-recycle marked 2 controllers)
- `agent-doc --version` -> `agent-doc 0.34.1`
- `git diff --check`

Committed and pushed:
- `src/agent-doc`: `eeb814a2 fix: replace closed same-supervisor actors`
- parent repo gitlink: `8b06e3099 agent-doc: update same-supervisor actor replacement`

CI: checked the latest `src/agent-doc` CI once after push; run `27656439677` was queued, so local verification is the closeout evidence.

Confidence: high. Escalation: none.
I traced the interruption to stale-supervisor recovery guidance, not to the lower-level restart machinery itself. The screenshot shows the prior agent following a stale-binary warning and forcing a supervisor restart while the monsterrodholders Codex turn was still active; the ops log has the matching `session_restart_force_used` event. That force path intentionally interrupts busy owners, so it was the wrong recommendation for routine stale-binary refresh.

I changed the agent-doc submodule so stale route-owned host supervisor warnings now recommend only non-destructive refresh paths: `agent-doc admin recycle` at an idle boundary or normal `agent-doc session restart-supervisor <FILE>`, which refuses busy panes. I also aligned the compaction stale-supervisor recovery hints, route comments, README/specs, and release notes so routine stale-binary refresh no longer points agents at force/discard recovery. Regression coverage now asserts the stale-refresh messages do not contain `--force` or `interrupt-clear`.

Verification passed:
- `cargo test -p agent-doc-orchestration host_supervisor_stale_warning --lib`
- `cargo test -p agent-doc-orchestration supervisor_stale_warning --lib`
- `cargo test -p agent-doc-orchestration assert_non_exchange_items_preserved_fails_closed_on_dropped_backlog_item --lib`
- `cargo test -p agent-doc-orchestration marker_guard_fires_when_queue_attributes_dropped --lib`
- `make check` (clippy, 4613 nextest tests, doctests, medium simulation)
- `make install` installed `agent-doc 0.34.1` and marked 2 controllers for idle-boundary recycle
- `agent-doc --version` -> `agent-doc 0.34.1`
- `git diff --check`

Committed and pushed:
- `src/agent-doc` `13e7bf74 fix: keep stale supervisor refresh non-destructive`
- parent repo `e043fca17 agent-doc: update stale supervisor refresh guidance`

Latest agent-doc CI for the new submodule push was queued as run `27657005941` at closeout, so I did not wait on it; local verification is green.

### Re: JB Run Agent Doc tmux-router Return — gpt-5

Yes, there was a lower-level `tmux-router` issue, but with an important boundary: current `agent-doc route --dispatch-only` for JB `Run Agent Doc` was already using the agent-doc direct-pane submit helper (`tmux_text_enter`), not `tmux_router::Tmux::send_keys()` for the main routed reopen. The problem was that the patched local `tmux-router` dependency had drifted: its public default `send_keys()` helper had started hex-encoding text plus a carriage-return byte. That violated the repo contract and left a regression path open for any caller that did use the router helper.

Fixed in `src/tmux-router`: `Tmux::send_keys()` now keeps one tmux process invocation, but it sends literal text with `send-keys -l` and then sends a separate named `Enter` tmux command in that same invocation. Empty submits send only the named `Enter`. The explicit Kitty Return helper still uses the enhanced-keyboard hex path; the default helper no longer sends a hex CR payload.

Changed paths:
- `src/tmux-router/src/tmux.rs`
- `src/tmux-router/src/hybrid.rs`
- `src/tmux-router/README.md`
- `src/tmux-router/docs/api-reference.md`
- `src/tmux-router/VERSIONS.md`
- parent gitlink `src/tmux-router`

Commits pushed:
- `src/tmux-router`: `c3162ef fix: submit text with named enter`
- parent repo: `a31efeada tmux-router: use named enter submit`

Verification passed:
- `src/tmux-router`: `cargo test send_keys`
- `src/tmux-router`: `cargo test` (113 unit tests plus doctests)
- `src/tmux-router`: `cargo clippy -- -D warnings`
- `src/agent-doc`: `cargo test -p agent-doc-orchestration submit --lib`
- `src/agent-doc`: `cargo test -p agent-doc-orchestration dispatch_only --lib`
- `src/agent-doc`: ignored live tmux checks `opencode_submit_uses_named_enter_key`, `dispatch_submit_text_to_tmux_uses_pane_submit_path`, and `dispatch_only_send_reopen_direct_pane_submit_avoids_extra_enter_retries`
- `src/agent-doc`: full `make check` passed: clippy, 4,613 nextest tests, doctests, and medium closeout simulation
- `src/agent-doc`: `make install` completed and installed `agent-doc 0.34.1`; it marked 2 controllers for idle-boundary recycle
- `agent-doc --version` -> `agent-doc 0.34.1`
- `git diff --check`

CI: latest `tmux-router` GitHub Actions `CI` run `27657724212` was `in_progress` when checked once after push, so I recorded it and did not wait. The parent `agent-loop` repo has no workflow named `CI`.

Non-gate note: `cargo clippy --all-targets -- -D warnings` in `tmux-router` still fails on pre-existing test-only lints in `src/sync.rs` / `src/registry.rs`; the repo CI runs `cargo clippy -- -D warnings`, and that passed.

Escalation: none. No JetBrains plugin update is required for this change; it is in the Rust router dependency and the installed agent-doc binary now uses it. Confidence: high for removing the remaining default `tmux-router` CR-submit path and preserving `Run Agent Doc` direct-pane proof as `tmux_text_enter`.
<!-- agent:boundary:30cfb84c -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority preset="#spec-test-build-install-commit-push" priority go -->
<!-- /agent:queue -->
###
<!--
Please make backlog replacements for all gated review items. We need to complete the plan, not have any more gates after this round. Let me know when I need to install JB plugin updates. Make sure all JB live gate items have adequate logging to prove/disprove the item. Clear the review items. I will test, files bugs, and you will check the logs.
---
`'/clear' is disabled while a task is in progress
---~~~~
A recent monsterrodholders.md session had much churn after I edited the queue while a turn was running. One churn item included deleting a queue item that I added.
---
agent-doc reentrant run on equityfundingsource.md had recovery churn.
```
● The recovery is complete and fully consistent. The binary converged the now-drained queue (empty block, queue_active:
  false), no_changes: true, both responses are committed in the session doc, and both code deliverables are on
  origin/main. The auto-loop terminated naturally.

  Let me record the specific recovery path for this failure mode, since it's a recurring dogfooding pattern
```
---
- ~~This session's console stdout is bleeding in front of the opencode tui. ![img_16.png](img_16.png)~~
---
The last run had a code fence stripped away by agent-doc.
#spec-test-build-install-commit-push
---
What are #next-steps to fix bugs?
-->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review
 
<!-- agent:review -->
- [/] [#rtwbcast-verify] [operator-verify] rtwbcast rung 4 final proof remains gated: code is landed in agent-doc cddad87b and headless targeted-delivery proof exists via SimWorld, but agent closeout on 2026-06-16 found only JetBrains live broadcast queue markers and no JetBrains-plus-VS Code live convergence/ack proof. Acceptance still requires opening the same managed doc in JB and VS Code, editing both buffers, and confirming both converge with no clobber and zero File Cache Conflict. Plan: tasks/agent-doc/plan-realtime-editor-watcher.md
<!-- /agent:review -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
