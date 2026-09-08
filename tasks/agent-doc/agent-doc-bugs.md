---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: afcc61d9-14a3-4322-99e1-7734891e9959
  codex: 01a0754d-5499-7160-ac2b-09ad05ea0ece
agent: codex
claude_model: fable
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->

Fenced closeout queue parsing fixed as 0.35.328; release tag held pending PyPI quota decision.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260906-045135.md`*

Compacted content:
- Archived 8 response topic(s): fenced closeout reports becoming queue work; response copied into `agent:queue`; malformed backlog residue; 5 more
- Prior summary/context: prior compacted content: Archived 12 response topic(s): / 0 FAIL, the pinned-publication test with HAIVEN_CONTRACTS_R...

### Re: monsterrodholder.md stability — gpt-5

> **Queue prompt:** Please look into monsterrodholder.md issues over the past few days. Are they all fixed? Addh any more work to fix monsterrodholder.md session crashing into the backlog + queue.

No—not every residual risk is fixed, but the navigation event and the five-second auto-sync lag have a concrete explanation, and the specific defect is fixed in the installed agent-doc 0.35.334.

`monsterrodholders.md` is an unusually demanding session: its long-lived log is about 155 MB, its state database is about 40 MB, it crosses the agent-loop and boost-client repository roots, it runs under queue pause/focused-cycle states, and it has been active through frequent agent-doc dogfood reloads. Those conditions repeatedly exercise pane ownership, controller reconciliation, and session recovery boundaries; the Markdown content itself is not the cause.

For the reported editor navigation, the previously registered pane `%65` was already absent. The supervisor therefore provisioned replacement pane `%88`; that transition was recorded as `session_start`, which made it look like the agent had crashed and restarted. Before commit `7365e3ceb` (`fix: quiesce paused layout provisioning`), a paused queue incorrectly rejected this structural provisioning and retried every five seconds, causing the lag. The fix separates dispatch admission from layout provisioning: pausing work still blocks dispatch, but it no longer blocks creation of the idle pane needed by editor sync.

Field evidence after the installed fix is good: the replacement layout converged, a later navigation focused existing pane `%88` in 13 ms, no new session was created, and `agent-doc session-check` reports the current monsterrodholders cycle healthy. No additional code edit was needed in this cycle. CI for the current agent-doc head is still running.

I am not declaring the session fully hardened. The logs show 1,356 old accepted-only dispatch receipts repaired in one reconciliation, occasional `Too many open files` errors, and historical restart/timeout churn. I added two active backlog-and-queue items:

1. `#mrhstatepressure` — compact terminal receipt/state history, rotate logs, preserve audit cursors, and stress-test against replay growth and file-descriptor exhaustion.
2. `#mrhsyncslowpath` — finish the remaining cross-root latency work already specified in `src/agent-doc/tasks/agent-doc/plan-sync-slow-path-latency.md`, including bounded no-retry lookup, reactive proven bindings, redundant-round-trip removal, and sub-second/no-spurious-session verification.

### Re: Response

Yes. Use two separate mechanisms:

- Automatic log rotation: rotate by size, around 16–32 MiB; compress old segments; retain roughly 30 days or a fixed storage budget.
- Regular state cleanup: run during idle periods or after successful closeout when thresholds are exceeded. Compact only terminal receipts/events below an acknowledged high-water mark.

Important safeguards:

- Never rotate or compact during an open cycle, retained response, or editor reconciliation.
- Preserve audit IDs, cursors, and recent recovery checkpoints.
- Use atomic rotation with a project-level lock.
- Keep cleanup off the navigation/sync hot path.
- Track open file descriptors separately—rotation alone will not fix an FD leak.
- Stress-test thousands of retries and reloads while asserting bounded disk usage, replay time, and descriptor count.

That is the right scope for the queued `#mrhstatepressure` task.

### Re: #mrhsyncslowpath — gpt-5

Completed and shipped the cross-root sync slow-path fix as agent-doc 0.35.335.

The ownership invariant remains fail-closed: the project controller owns actor and structural pane evidence, while sync may only consult an already-running owner controller through a read-only adapter. Structural file→pane receipts are now a retained reactive Source joined with desired layout and live actors in the same pure Computed. The former worker-side imperative append is gone; receipt evidence is constrained to the current structural shape, actor evidence wins on overlap, and focus-only generations can safely reuse an unchanged assignment.

The interactive fallback now makes at most one owner-controller request with a caller-owned 250 ms deadline, with no retry and no lazy controller launch. `Found`, `Missing`, and `DeadlineExceeded` are typed outcomes; missing or timed-out evidence falls through the existing ownership checks. The post-content path cannot issue a second cross-root lookup.

Transition coverage now proves:

- unchanged structure plus focus-only change reuses the structural receipt;
- changed layout structure or changed actor bindings rejects reuse;
- receipt publication invalidates and re-derives the retained binding projection;
- a non-responsive owner controller returns `DeadlineExceeded` after one bounded request;
- exact-visible post-content resolution performs zero repeat actor lookups;
- live cross-root exact-visible sync reuses the owner pane without creating a replacement session.

Window-resolution telemetry is split into target-session, agent-doc-window, and layout-observation subphases while retaining the aggregate meter. The latest measured safe-passive sample was 19 ms total (7 ms + 2 ms + 8 ms), below the 250 ms budget.

Verification passed: `make check`, formatting and diff checks, focused controller/sync regressions, the ignored live-tmux cross-root regression, release-version audit, and the public-repo private-name denylist scan. The CLI, native library, Codex/Claude/OpenCode/Cursor skills, JetBrains package, and VS Code package were installed at 0.35.335; running supervisors/controllers were marked for normal idle-boundary recycle. No data migration is required.

Committed and pushed to `main` as `767eddd8b` (`fix(sync): bound cross-root ownership lookup`). The new CI run `34017106312` was queued at closeout; the preceding main CI run `34012077142` completed successfully.

### Re: #mrhstatepressure — gpt-5

> **Queue prompt:**
>
> do [#mrhstatepressure]


Completed and shipped the long-lived state-pressure fix as agent-doc 0.35.336.

Controller restart now reconciles accepted-only/start-unproven dispatch receipts once, persists one exact deduplicated `dispatch_receipt_reconcile` crash marker per receipt, and then deletes only those marker-proven terminal rows in 5,000-row batches. Failed and start-proven receipts remain. A 2,000-receipt regression proves the second reload has neither stale replay nor duplicate audit markers.

State-event retention keeps the existing live PID-scoped editor acknowledgement watermark. Eligible events strictly below the minimum acknowledged cursor are deleted in 5,000-row batches inside the same transaction; a missing live acknowledgement still pins deletion, no-live-peer state still deletes nothing, and the exact cursor row plus durable high-water mark remain. A 12,050-event regression proves the multi-batch boundary.

Long-lived `ops.log`, `cycles.jsonl`, and per-session logs now rotate at 32 MiB only after a durable cycle-closeout entry, with admission-time cleanup for legacy oversized session logs. Filesystem-owned rotation uses a project lock, retains one zstd-compressed segment plus the active tail, and exposes a rotation-aware reader used by restart metrics, session-boundary detection, and proof verification. Open cycles, retained-response persistence, editor reconciliation, navigation, and sync never trigger cleanup.

Supervisor log clones now share a synchronized append handle rather than cloning file descriptors. Writes take the rotation lock and reopen the active inode after rollover; separate resource-pressure telemetry reports the process descriptor count at admission and child launch/restart boundaries. A 2,000-clone stress regression proves constant log descriptors and complete readable history across rotation.

Verification passed: focused SQLite/controller/log/supervisor/start/preflight/session-accretion tests, formatting and diff checks, the public-repo private-name denylist, and the full `make check` matrix. The CLI, native library, bundled skills, JetBrains package, and VS Code package were installed; `agent-doc --version` reports 0.35.336. The architecture-first plan is recorded in `tasks/agent-doc/plan-long-lived-state-pressure.md` and the relevant control-plane, state-backbone, and supervisor specs are updated.

Committed and pushed to `main` as `eb1e36cfd` (`fix(runtime): bound long-lived state pressure`). CI runs `34018884314`, `34018884273`, and `34018884456` were in progress at closeout.

### Re: #fixcausesrun — gpt-5

> **Queue prompt:**
>
> do [#fixcausesrun]


> **Queue prompt:**
>
> Fix agent-doc causes of Run Agent Doc issue on dev.md: `editor route layout did not converge before dispatch: operator_owned_layout`

Fixed and released locally as agent-doc `0.35.338` in public subrepo commit `167c33255` (`fix(route): reopen operator-owned layout intents`).

The runtime wedge came from reusing an identical desired pane-layout generation across separate foreground Run commands. Once an operator changed the previously converged layout, that old generation correctly projected the terminal `OperatorOwned` state. Every later Run reused the same generation and receipt, so the retained effect could never become eligible again and dispatch failed immediately.

The invariant is now explicit: every foreground editor Run publishes a fresh layout-intent generation before dispatch, even when its canonical layout is identical to the retained desired value. `ControllerPaneLayoutGraph` owns that policy through typed `PaneLayoutPublication::{CoalesceIdentical, FreshIntent}` transitions. Background/state-frame publication still coalesces identical values; foreground routing uses `FreshIntent`, invalidates stale observation/receipt facts, derives `NeedsEffect`, performs the retained layout effect, observes convergence, and only then dispatches. Non-convergence still fails closed. The one foreground tmux survey remains the physical-state ingress because tmux provides no push stream; no polling loop was added.

Regression coverage recreates a previously converged generation that drifts to `OperatorOwned`, then proves that an identical foreground intent receives a higher generation and re-enters `NeedsEffect`. The existing end-to-end route test now repeats the same request and verifies the second dispatch uses a fresh generation. Routing and control-plane specs were updated with the invariant, owner, topology, and transition table.

Verification passed: both focused regressions, all pane-layout tests, non-test `cargo check`, formatting, version synchronization, private-name and diff checks, and full `make check` (9,233 tests passed; 255 skipped; audits plus Lean/TLA checks passed). `make install` installed `0.35.338`, marked 17 supervisors and 8 controllers for safe idle-boundary recycle, and installed the matching VS Code and JetBrains packages. Restart a running JetBrains IDE to activate its updated package.

### Re: Response

Restart confirmed. The updated JetBrains plugin is active; Book and Pages checks passed, while the full CI suite is still running without reported failures. You can retry **Run Agent Doc** on `dev.md`.

### Re: Response

Continuation cannot be admitted because `cont` did not trigger a new binary preflight contract. Please invoke:

`agent-doc tasks/agent-doc/agent-doc-bugs.md`

again after the editor replica has re-registered.

### Re: Response

Fixed and live-unwedged.

- Root cause: supervisor harness switches updated SQLite directly while the controller retained stale `codex` state, repeatedly reopening a completed handoff.
- Fix: harness writeback now atomically updates the controller’s actor source and durable record, fenced by session/pane/generation. See [rpc.rs](/home/brian/work/btakita/agent-loop/src/agent-doc/agent-doc-controller-io/src/project_controller/rpc.rs:1515) and [lib.rs](/home/brian/work/btakita/agent-loop/src/agent-doc/agent-doc-start-runtime-io/src/lib.rs:2138).
- Released and installed `agent-doc 0.35.339`.
- `dev.md` is now `ready`, generation 2, correctly bound to `claude-code`; controller and supervisor run the current binary.
- Both `priority` markers remain intact.
- `make check` and focused regressions passed.
- Commit `5445702d2` pushed to `main`; CI was in progress at last check.
- The public subrepo is clean.

I did not fabricate a response cycle in `agent-doc-bugs.md`; its earlier admission never opened a cycle.

### Re: background agent-doc process thrashing — gpt-5

> **Queue prompt:** There are still many background agent-doc processes thrashing.

Fixed and shipped in agent-doc 0.35.340.

The root cause was the quiescent supervisor's 60-second safety timer crossing the compact-revision boundary. Even when the controller reported the exact same document revision, every supervisor forced a full authoritative frontmatter read and then continued through the queue/pane projection. With roughly fifteen retained supervisors, that produced a fleet-wide thundering herd: the pre-fix supervisors averaged about 24% CPU in aggregate, individual one-second samples spiked to 8–13%, and the project controller averaged roughly 12–15%. The ops log independently showed one full `idle_watch_agent_change_frontmatter` read batch per idle document per minute.

The runtime now keeps the authority chain explicit: the controller's compact `CurrentRevision` is the source, revision/delivery change is the invalidation policy, and full Markdown/frontmatter plus downstream queue/pane work are effects. The bounded safety pass may re-observe the compact revision, but an unchanged identity cannot trigger either effect. Initial observation, a real revision transition, or a delivery edge still reconciles normally. Regression coverage proves both the frontmatter gate and the projection gate, including unavailable-to-current revision recovery.

Verification is green:

- `cargo test -p agent-doc-start-runtime-io` passed.
- Final `make check` passed the full workspace, documentation/projection audits, deterministic simulations, and all six TLA+ model checks.
- `cargo fmt --all --check` passed.
- `make install` installed the CLI, native library, skills, JetBrains plugin, and VS Code extension.
- A final 70-second live sample across the former timer boundary showed the updated `dev.md` supervisor averaging 0.67% CPU, peaking at 4%, with its full-document read counter unchanged at 362. Active older turns are intentionally not killed; supervisors still mapped to the prior deleted inode will self-exec 0.35.340 at their next safe boundary.

The earlier Run Agent Doc wedge on `dev.md` also remains cleared: `agent-doc session-check` reports its cycle committed, `queue_continuation_required=false`, and `no_drainable_work`.

Public commit `fabbbae67` (`fix(runtime): keep idle reconciliation revision-driven`) is pushed to `main`. The new GitHub CI run is in progress; the immediately preceding red run was an unrelated wall-clock-sensitive IPC timeout after 9,148 passing tests, while the same complete gate passed locally.
<!-- agent:boundary:d997bd6b -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority -->
<!-- /agent:queue -->

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#coder7] [operator-verify] Phase 7 — run the five-case live Coder workspace validation matrix after a reachable Coder workspace and authenticated access are available, with JetBrains Gateway remote backend, VS Code Remote-SSH, simultaneous dual-IDE attachment, workspace stop/start, and a no-tmux image variant. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md. Unblocks: provide the workspace URL or SSH target plus authentication and enable the current agent-doc plugin/extension in both remote IDE hosts.
- [/] [#harnessswitchverify] [operator-verify] #harnessswitchstorm remains unverified. The 2026-08-25 fpe.md attempt was invalidated by a stale live JetBrains plugin generation: ops.log showed controller_crdt_replica_refusal_storm (25 refusals), editor_absent/disk authority, agent_restart_respec_inert to Claude, then old_harness=claude -> new_harness=codex only after save. Unblocks after make install, restart the JetBrains IDE host, repeat the unsaved claude <-> codex switch, and prove exactly one spawn plus agent_restart_deferred reason=non_authoritative_document_view.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
- [/] [#restartintellijidea] Restart IntelliJ IDEA so installed agent-doc JetBrains 0.2.370 becomes live, then confirm native queue-prompt styling for Codex, Claude, and OpenCode, FFI loading through the resolved agent-doc executable, and pane synchronization remains stable.
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
