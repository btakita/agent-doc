# Plan — JB `Run Agent Doc` no-submit during post-`lib-install` supervisor recycle (#jbdisprecycle)

**Class:** dispatch-restack / no-submit (same family as `#rdypoll`, `#rdypoll-deploy`, `#jbclrdispdup`/`#t3mn`).
**New trigger isolated 2026-06-21:** reproduces on freshly-installed **0.34.35** (`libagent_doc-0.34.35.so`) specifically in the window where `lib-install` auto-recycle drives the supervisor through a burst of `execve` hot-reloads.

## Operator evidence (supervisor-stderr.log)

```
[lib-install] ... -> /home/brian/.cargo/bin/libagent_doc-0.34.35.so
[lib-install] auto-recycle: 1 controller(s) marked to recycle at next idle boundary, 3 skipped
[agent-doc] supervisor auto-install succeeded; recycling onto the freshly-installed binary
[agent-doc] supervisor hot-reloading onto freshly-installed agent-doc binary (next_queue_item); preserving the live agent child via execve
[agent-doc] supervisor hot-reloading onto freshly-installed agent-doc binary (idle); preserving the live agent child via execve   (x2)
Error: project controller command `start_session` failed: controller failed to start session actor for tasks/software/tsift.md
[agent-doc] idle-queue watch: reconciled stale busy actor to ready from idle pane evidence (no pane kill)   (xN)
[agent-doc] idle_queue_watch: clear cooldown active for tasks/agent-doc/agent-doc-bugs2.md, skipping passive queue dispatch
```

Symptom: `Run Agent Doc` on `equityfundingsource.md` typed the trigger into the pane but **did not submit** (no Enter).

## Root cause (from code read 2026-06-21)

The dispatch path (`route/dispatch.rs`, `route/dispatch_only.rs`) gates on a **harness dispatch-ready prompt** (#rdypoll §D: "no harness dispatch-ready prompt yet", `dispatch_start_proof`, late-resubmit retries) but has **no awareness of supervisor recycle state** — `grep` for `recycle_yield` / `is_recycling` / `mid-recycle` in those two files returns nothing. So when a dispatch lands while `idle_watch` is mid-`execve` hot-reload storm:

1. The pane's session actor is being torn down / restarted (proximate proof: `start_session failed` for the sibling `tsift.md` in the same recycle batch — `project_controller/rpc.rs`).
2. The dispatch-ready gate can read a transiently-"ready" pane (`reconciled stale busy actor to ready from idle pane evidence`) and inject text, but the in-flight route / submit is interrupted by the `execve`, so the Enter never lands.
3. `clear cooldown active … skipping passive queue dispatch` shows the cooldown machinery suppressing the supervisor's own retry, leaving the half-typed dispatch stranded.

Net: dispatch-ready proof and recycle settling are **not sequenced** — a dispatch may type-without-submit across an `execve` boundary.

## Fix rungs (each independently shippable; all need live-pane repro to verify)

- **R1 — recycle-aware dispatch gate. ✅ SHIPPED (agent-doc `ad55b993`, 2026-06-21).** New `recycle_inflight.rs`: project-root-scoped, short-TTL (15s) "supervisor mid-recycle" marker (project-scoped because one controller hosts many docs — the live repro's recycle was for `agent-doc-bugs2.md`/`tsift.md` yet broke an `equityfundingsource.md` dispatch). `idle_watch.rs` marks it before auto-install and before each `supervisor_perform_reexec` (auto-recycle + operator restart); the fresh supervisor clears it on watch-loop start (TTL is the backstop). `route/dispatch_only.rs` does a bounded 10s recycle-settle wait BEFORE any pane input — defers while mid-recycle, proceeds once settled, fails closed (never types) if it never settles so the caller retries instead of stacking an unsubmitted trigger. Unit tests cover the freshness predicate, mark/read/clear roundtrip, and the cross-document project-scoped repro; 4 new `reason=` tokens audited through the FlowCore budget gate. `make check` green (4857 tests), clippy clean. **NOT installed/released** (STOP-cutting-releases) — running supervisor still on the old binary until the operator installs at a recycle boundary.
- **R2 — `start_session` retry during recycle. ✅ SHIPPED (2026-06-21).** `start/run.rs` now wraps the `crate::project_controller::start_session` call in a recycle-aware retry: on failure it checks the project-scoped `recycle_inflight` marker (`recycle_inflight_pending`); if the supervisor is mid-`execve` recycle it logs `start_session_recycle_retry`, waits the bounded settle window (`wait_for_recycle_settle`, ≤10s), re-runs `ensure_controller_running`, and retries (≤2 retries) instead of surfacing the terminal `start_session failed`. A failure that is NOT a recycle race, or an unsettling recycle, still surfaces the original error (terminal, unchanged). New pure predicate `start_session_retryable_during_recycle(recycle_pending, attempts_used, max_attempts)` in `recycle_inflight.rs` makes the retry policy unit-testable without a live controller.
- **R3 — submit-once proof spans recycle. ✅ SHIPPED (2026-06-21).** `route/dispatch.rs send_command_unchecked` now adds a recycle-settle gate immediately after the trigger is injected (`dispatch_inject attempt=1`) and the first acceptance poll comes back non-Accepted: if the supervisor is mid-recycle (`recycle_interrupted_resubmit_should_wait`), it waits for settle (`wait_for_recycle_settle`), logs `route_dispatch_submit_recycle_settle ... action=submit_once_after_settle`, and re-polls acceptance ONCE — so the existing budgeted bare-Enter/re-type loops run against the settled supervisor and land the submit exactly once, never burning the budget on input the recycle silently drops (the #rdypoll attempt=2+ restack symptom). Pure predicate `recycle_interrupted_resubmit_should_wait(trigger_already_injected, recycle_pending)` unit-tested.
- **R4 — SimWorld coverage. ✅ SHIPPED (2026-06-21).** New `route_sim_dispatch_defers_during_recycle_then_injects_once_after_settle` models the recycle-inflight marker as a `RouteModel.recycle_inflight` flag with `MarkSupervisorRecycleInflight` / `SettleSupervisorRecycle` / `DispatchDuringSupervisorRecycle` SimCommands. Asserts: 7 dispatches during the recycle window all fail closed (`dispatch_into_recycling_pane`, `dispatch_injects == 0`, no stacked triggers); after settle the same dispatch injects exactly once (`dispatch_inject attempt=1`, `route_dispatch_proofs == 1`, never `attempt=2`). `make check` green (clippy -D warnings + full suite), 4 new `recycle_inflight` unit tests + the SimWorld scenario pass.

## Acceptance

Live-pane repro: run `make install` to trigger lib-install auto-recycle, immediately fire JB `Run Agent Doc` on a second doc during the hot-reload burst; the dispatch must land exactly once **with** submit (Enter), `ops.log` shows a single `dispatch_start_proof`, and no `start_session failed` is left terminal. SimWorld assertion green in `make tmux-ci`.

## Constraints

- Operator-verify: needs a live editor/pane; cannot be fully agent-verified.
- Queue is **paused** (`#rt83/#qflood`) and the **STOP-cutting-agent-doc-releases** gate is active — do **not** ship + install mid-session against the live supervisor. Drive at a recycle boundary in a focused session.
