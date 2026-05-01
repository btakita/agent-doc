# Plan: Codex resume can keep stale browser/CDP capabilities

## Problem

`agent-doc` can launch Codex with `codex_args: -s danger-full-access` and `codex_network_access: enabled`, yet a resumed Codex thread can still fail browser/CDP work that succeeds from a fresh outer Codex session on the same machine.

Observed repro shape on May 1, 2026:
- outer Codex session: `chromium-bridge check` succeeds against `127.0.0.1:9222`
- `agent-doc` task doc still has a persisted `resume:` id
- resumed Codex turn reports `chromium-bridge check` / `list` failure with `Operation not permitted (os error 1)` on `127.0.0.1:9222`

This makes `codex_network_access` look flaky when the real issue is stale capability inheritance on `codex exec resume <id>`.

## Goal

Make browser/CDP capability resets deterministic for Codex sessions so `agent-doc` does not keep retrying a stale resumed thread after policy or runtime capability changes.

## Proposed Fix

### 1. Detect stale resumed-session capability drift

Teach the Codex/route path to treat this pattern as a capability-reset signal:
- document or config requests `codex_network_access: enabled`
- local browser/CDP health is otherwise expected
- resumed Codex child reports local-socket permission failure such as `Operation not permitted (os error 1)` for `127.0.0.1:9222`

The recovery should not keep trusting `resume:` blindly once this signature appears.

### 2. Force a fresh Codex exec for the next retry

Add a recovery path that clears or bypasses the saved `resume:` thread and starts a fresh `codex exec` with the resolved `codex_args`, writable roots, and network policy instead of another `codex exec resume <id>`.

Candidate trigger points:
- routed retry after a failed browser/CDP check
- direct `run` retry after a failed browser/CDP check
- capability-reset flows such as `/clear`, where route already prefers a fresh restart

### 3. Improve diagnostics

When this recovery path triggers, emit a clear message that distinguishes:
- inherited network-disable mismatch
- browser not running on `127.0.0.1:9222`
- resumed-thread capability drift requiring a fresh Codex thread

The user-facing error should point at the actual remedy instead of implying that `codex_network_access` alone is unreliable.

### 4. Add regression coverage

Cover at least:
- resume path keeps using `codex exec resume <id>` under normal conditions
- capability-reset signal causes the next retry to use fresh `codex exec`
- `/clear` or equivalent capability reset does not fall back to stale resume inheritance
- diagnostics differentiate local browser-down from resumed-thread EPERM drift

## Exit Criteria

- A stale resumed Codex session no longer loops on browser/CDP `EPERM` against a healthy local `127.0.0.1:9222` endpoint.
- The next `agent-doc` retry starts a fresh Codex thread automatically.
- Logs and user-facing output explain whether the failure was network policy, browser availability, or stale resumed-session capability drift.
