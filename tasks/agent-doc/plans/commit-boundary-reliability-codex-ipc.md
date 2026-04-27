# Plan — commit-boundary reliability for Codex/IPC sessions

## Goal

Fix the `agent-doc` failure family where a response patchback is either:

1. never started on a fresh Codex launch because the initial route trigger is missed, or
2. written to the document and snapshot, but left uncommitted because the cycle dies before `commit_*`.

## Scope

- `src/agent-doc/src/start.rs`
- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/repair.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/src/codex_hook.rs`
- related runbooks/spec/instruction surfaces

## Phase 1 — fresh-start trigger watchdog

### Objective

Fail closed when a fresh Codex session starts but never actually enters the `preflight -> write -> commit` loop.

### Changes

- Audit the fresh-start path where `start.rs` disables `auto_trigger` and `route.rs` becomes solely responsible for first-trigger injection.
- Add a bounded watchdog after `codex_start mode=fresh`:
  - if no trigger dispatch or `preflight_diff_start` follows,
  - emit a specific failure signal instead of silently idling.
- Add a deterministic fallback:
  - retry trigger injection once readiness stabilizes, or
  - persist an explicit recoverable “route never triggered” state that later commands can surface.

### Verification

- Fresh Codex start with delayed/noisy readiness prompt still triggers successfully.
- Fresh Codex start with no accepted readiness prompt fails closed with a specific reason.
- Ops log distinguishes this case from ordinary idle sessions.

## Phase 2 — resume commit after successful write

### Objective

Auto-finish the commit boundary when the response write and snapshot save succeeded but commit did not.

### Changes

- Treat `write_origin` + `ipc_write_consumed` / `snapshot_saved_file_ipc` without later `commit_*` as a first-class interrupted state.
- Extend `preflight` / `repair` to prefer **resume-commit** when:
  - captured response exists,
  - baseline still matches,
  - working file already contains the intended assistant patchback.
- Tighten `codex_hook.rs` Stop-hook recovery so it can finish just the commit boundary when the write already landed.
- Refuse resume-commit if the file drifted after the interrupted write.

### Verification

- IPC write succeeds, commit never starts, next preflight finishes commit automatically.
- Commit starts but does not finish, next repair resumes correctly.
- Resume-commit is blocked on post-write drift and reports why.

## Phase 3 — observability and user contract

### Objective

Make the two failure modes easy to distinguish in logs, `session-check`, and harness instructions.

### Changes

- Add explicit ops-log markers:
  - `fresh_route_trigger_missing`
  - `write_completed_commit_missing`
  - `resume_commit_attempt`
  - `resume_commit_success`
  - `resume_commit_blocked_drift`
- Update `session-check` messages so they separate:
  - “no patchback cycle ever started”
  - “patchback landed but commit boundary was interrupted”
- Update:
  - `SKILL.md`
  - `runbooks/commit.md`
  - harness invocation docs/specs
  so the documented recovery path matches the binary behavior.

## Test plan

- Add focused unit/integration coverage around:
  - fresh-start routing watchdog
  - interrupted write-before-commit recovery
  - interrupted commit-after-staging recovery
  - drift-blocked resume-commit
- Run the full `src/agent-doc` verification suite after implementation.

## Success criteria

- Fresh Codex launches cannot silently stall before first trigger.
- A visible assistant patchback cannot remain uncommitted without `preflight` / `repair` recognizing and recovering it or failing closed with a precise reason.
- `session-check` and ops logs identify which failure class occurred.
