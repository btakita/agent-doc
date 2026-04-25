# Plan: Orchestrate Preflight Cycle Conflict

**Pending:** `#opcc` in `tasks/agent-doc/agent-doc-bugs.md`
**Status:** Draft

## Problem

When a user writes an orchestration directive (e.g., "Synchronous orchestra:") inside a session document, the skill workflow runs `agent-doc preflight` in step 0 — opening a cycle — before it knows the user's intent is orchestration. When the skill then routes to `agent-doc orchestrate`, the orchestrator runs its *own* internal preflight, which fails because a cycle is already open.

**Observed behavior:** `agent-doc orchestrate` exits with:
```
previous cycle `cycle-NNNN` is still `preflight_started` with no recoverable response
```

**Workaround:** The agent executes all tasks manually within the existing cycle. This produces correct results but loses the orchestrator's per-task commit boundaries.

## Root Cause

The skill and the orchestrator both assume they own the full lifecycle. The skill opens a cycle via `preflight` before it can determine intent, and the orchestrator refuses to start because a cycle is already open.

## Goal

`agent-doc orchestrate` should work correctly when invoked from within a skill cycle that has already run preflight.

## Options

### Option A: Preflight idempotency

Make `agent-doc preflight` idempotent — a second call within the same cycle reuses the existing preflight state instead of failing. The orchestrator's internal preflight would detect the open cycle and inherit it.

- **Pro:** Fixes the problem at the root. No changes to skill workflow ordering.
- **Con:** Preflight currently assumes it owns cycle creation; making it reentrant requires careful state management. The orchestrator still needs to create *new* cycles for tasks 2–N.

### Option B: Orchestrator accepts existing cycle

Add a `--reuse-cycle` flag (or auto-detect) to `agent-doc orchestrate` so it inherits the already-open preflight for task 1, then creates fresh cycles for subsequent tasks.

- **Pro:** Minimal change — only the orchestrator needs updating. Skill workflow stays unchanged.
- **Con:** Orchestrator must handle two entry modes (fresh vs inherited). Task 1 gets different treatment than tasks 2–N.

### Option C: Detect orchestration intent before preflight

The skill scans the document for orchestration keywords *before* running preflight, then hands off to the orchestrator without opening a cycle first.

- **Pro:** Clean separation — orchestrator always owns the full lifecycle.
- **Con:** Requires pre-scanning without the diff (no preflight output to analyze). Fragile keyword matching without the structured diff context that preflight provides.

### Option D: Skill closes its cycle before orchestrating

After preflight detects orchestration intent (from the diff), the skill explicitly closes/abandons the current cycle, then hands off to the orchestrator which starts fresh.

- **Pro:** Clean lifecycle ownership. Skill uses preflight output to detect intent, then steps aside.
- **Con:** Needs a `cancel-cycle` or `abandon-cycle` command. Must not leave orphan state.

## Recommended Approach

**Option B** for MVP — it's the smallest change with the clearest semantics. The orchestrator already manages per-task cycles for tasks 2–N; inheriting an existing cycle for task 1 is a natural extension. Auto-detection (checking if a cycle is already open) is better than a flag since the skill shouldn't need to know about the internal cycle state.

**Option A** as a longer-term hardening measure — preflight idempotency is independently valuable for retry scenarios and hook-driven recovery.

## Implementation Steps

- [ ] 1. In `src/agent-doc/src/orchestrate.rs`, detect an already-open `preflight_started` cycle before running the first task's preflight
- [ ] 2. When detected, reuse the existing cycle state (diff, baseline_file, claims) for task 1 instead of failing
- [ ] 3. Tasks 2–N continue to create fresh cycles as they do today
- [ ] 4. Add test: orchestrate with a pre-existing open cycle completes task 1 using that cycle
- [ ] 5. Add test: orchestrate without a pre-existing cycle works as before (regression)
- [ ] 6. `cargo test && cargo build --release && cargo install --path .`
- [ ] 7. Verify end-to-end: write orchestration directive in a session doc, invoke `/agent-doc`, confirm orchestrator runs all tasks with per-task commits

## Files to Change

- `src/agent-doc/src/orchestrate.rs` — cycle detection + reuse logic for task 1
- `src/agent-doc/src/preflight.rs` — possibly expose cycle state query for orchestrator
- `src/agent-doc/tests/` — orchestrate test coverage for inherited cycle
