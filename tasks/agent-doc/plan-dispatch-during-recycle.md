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
- **R2 — `start_session` retry during recycle.** `project_controller/rpc.rs` should treat a `start_session` failure that races a hot-reload as retryable (re-issue after the `execve` settles) rather than surfacing a terminal `Error:`. A failed actor start should not silently leave a pane half-dispatched.
- **R3 — submit-once proof spans recycle.** Extend the `dispatch_inject attempt=N` marker so an injected-but-unsubmitted dispatch interrupted by a recycle is detected and re-submitted exactly once after settle (not re-typed N times — that is the #rdypoll restack symptom).
- **R4 — SimWorld coverage.** Add a deterministic SimWorld scenario: `lib-install` auto-recycle storm + concurrent `Run Agent Doc` dispatch ⇒ assert the dispatch defers until settle and submits exactly once, with zero stacked re-types.

## Acceptance

Live-pane repro: run `make install` to trigger lib-install auto-recycle, immediately fire JB `Run Agent Doc` on a second doc during the hot-reload burst; the dispatch must land exactly once **with** submit (Enter), `ops.log` shows a single `dispatch_start_proof`, and no `start_session failed` is left terminal. SimWorld assertion green in `make tmux-ci`.

## Constraints

- Operator-verify: needs a live editor/pane; cannot be fully agent-verified.
- Queue is **paused** (`#rt83/#qflood`) and the **STOP-cutting-agent-doc-releases** gate is active — do **not** ship + install mid-session against the live supervisor. Drive at a recycle boundary in a focused session.
