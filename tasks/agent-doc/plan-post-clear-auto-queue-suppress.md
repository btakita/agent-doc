# Plan — suppress auto-queue triggering after `session clear`

**Related:** `#clear-auto-queue-stall` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

JB "Clear Session Context" successfully runs `agent-doc session clear`, which sends `/new` (OpenCode) or `/clear` (Claude/Codex) to reset the harness conversation. After the clear completes and the pane returns to idle:

1. The `<!-- agent:queue auto -->` component with `queue_active: true` persists in the session document
2. The idle-queue-watch thread (`start.rs:1476-1565`) detects the active queue head and injects `agent-doc <FILE>`
3. The auto-trigger thread (`start.rs:1344-1463`) may also fire on supervisor restart
4. The cleared session has no agent-doc context — the harness has no memory of the skill/workflow
5. This stalls: agent-doc runs preflight, but the harness cannot execute the response cycle correctly

Additionally, `prompt_requests_clear()` (`codex_hook.rs:819-821`) only matches exact `/clear`, not `/new` — so the route-level fresh-restart path (`route.rs:978-997`) does not apply to OpenCode sessions.

## Root Causes

| # | Root Cause | Location | Impact |
|---|-----------|----------|--------|
| 1 | No post-clear auto-queue suppression | `start.rs` idle-queue-watch, auto-trigger | Auto-queue fires immediately after clear |
| 2 | `prompt_requests_clear()` ignores `/new` | `codex_hook.rs:819-821` | OpenCode clear not detected by route |
| 3 | `session clear` does not mark queue as paused | `session_actor_cmd.rs:472-544` | Queue state unmodified across clear |

## Fix

### Step 1 — `prompt_requests_clear()` recognizes `/new`

**File:** `agent-doc-orchestration/src/codex_hook.rs:819-821`

Change `prompt_requests_clear()` to also match `"/new"` (OpenCode's clear equivalent):

```rust
pub fn prompt_requests_clear(prompt: &str) -> bool {
    let trimmed = prompt.trim();
    trimmed == "/clear" || trimmed == "/new"
}
```

This makes `tracked_harness_clear_requires_fresh_restart()` (`route.rs:978-997`) work for OpenCode sessions, forcing a fresh restart after `/new`.

### Step 2 — Post-clear auto-queue cooldown marker

**File:** `agent-doc-orchestration/src/queue_continuation.rs`

Add a durable marker file at `.agent-doc/queue-continuations/<doc-hash>.clear-cooldown`:

```rust
pub fn write_clear_cooldown(doc_path: &Path) -> Result<()> {
    let marker = cooldown_marker_path(doc_path);
    fs::write(&marker, chrono::Utc::now().to_rfc3339())
}

pub fn is_in_clear_cooldown(doc_path: &Path) -> bool {
    let marker = cooldown_marker_path(doc_path);
    if !marker.exists() { return false; }
    let ts = fs::read_to_string(&marker).ok()
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok());
    // Cooldown expires after 60 seconds or on first explicit Run Agent Doc
    ts.map_or(false, |t| Utc::now() - t.naive_utc() < Duration::seconds(60))
}

pub fn clear_cooldown_on_explicit_invoke(doc_path: &Path) {
    let _ = fs::remove_file(cooldown_marker_path(doc_path));
}
```

### Step 3 — `session clear` writes cooldown marker

**File:** `src/session_actor_cmd.rs:472-544`

After sending the clear command and recording the prompt (lines 527-533), write the cooldown marker:

```rust
queue_continuation::write_clear_cooldown(doc_path)?;
```

### Step 4 — Idle-queue-watch respects cooldown

**File:** `agent-doc-orchestration/src/start.rs:331-371`

In `idle_queue_drain_decision()`, add a cooldown check:

```rust
if queue_continuation::is_in_clear_cooldown(doc_path) {
    return IdleQueueDrainDecision::SkipClearCooldown;
}
```

Add `SkipClearCooldown` variant to `IdleQueueDrainDecision`.

### Step 5 — Auto-trigger thread respects cooldown

**File:** `agent-doc-orchestration/src/start.rs:1344-1463`

In `spawn_auto_trigger_thread()`, check cooldown before injecting:

```rust
if queue_continuation::is_in_clear_cooldown(doc_path) {
    return AutoTriggerOutcome::SkippedClearCooldown;
}
```

Add `SkippedClearCooldown` variant to `AutoTriggerOutcome`.

### Step 6 — Explicit invoke clears cooldown

**File:** `agent-doc-orchestration/src/route.rs`

At the start of the main `route()` function (or wherever `Run Agent Doc` enters), clear the cooldown:

```rust
queue_continuation::clear_cooldown_on_explicit_invoke(doc_path);
```

This ensures the next explicit operator action re-enables auto-queue.

### Step 7 — Preflight diagnostic for cooldown state

**File:** `agent-doc-orchestration/src/preflight.rs`

Add `clear_cooldown_active: bool` to `PreflightOutput` so the skill/agent can see the cooldown state and surface a diagnostic.

## Verification

1. `cargo test` — unit tests for `prompt_requests_clear` with `/new`, cooldown marker read/write/expiry
2. SimWorld regression: clear session → verify idle-queue-watch does not fire within cooldown window
3. SimWorld regression: explicit `Run Agent Doc` after clear → verify cooldown is cleared and auto-queue resumes
4. `make check` passes
5. Live JB repro: Clear Session Context → verify no stall → Run Agent Doc → verify auto-queue resumes

## Scope

- No changes to the session document format
- No changes to the `agent:queue` component schema
- Durable marker is a single small file, cleaned up by existing GC
- Backwards compatible: if marker file is missing, behavior is unchanged
