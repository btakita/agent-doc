# Plan — Reentrant Finalize Pipeline

## Problem

When an agent-doc session crashes (tmux window crash, process kill, OOM) during the finalize step, the agent's response and/or the user's queue prompt can be lost. The pipeline has one uncovered crash window where the response exists only in process memory between stdin read and `save_pending()`. After restart, preflight may silently abandon the cycle, treating the lost response as an empty preflight.

## Root Cause

### Crash Window 1 — stdin-only gap (write.rs:6771→6793 inline, 6919→6984 template)

The response is fully read from stdin but not yet persisted to disk. Between these lines:

```
stdin.read_to_string(&mut response)   // line 6771 / 6919
  ↓ ~20 lines of validation/parsing   // lines 6775-6792 / 6924-6983
  ↓ CRASH HERE → response lost
repair::save_pending(file, &response)  // line 6793 / 6984
```

- No pending file exists
- No capture ledger exists
- Cycle state is still `PreflightStarted`
- `repair::run()` falls through to `repair_stale_preflight_started_cycle()` which may silently abandon the cycle
- The template path (`run_template`) has ~65 lines of processing (parsing, sanitization, normalization) between stdin read and save_pending — a much larger window than the inline path's ~20 lines

### Crash Window 2 — queue prompt consumed before crash (write.rs Phase 3)

If the response was persisted (save_pending succeeded) and the crash happens after atomic_write (Phase 2) but during or after queue consumption (Phase 3, lines 1488-1584):
- Document has the response on disk
- Queue prompt was removed from the document content
- But git commit (Phase 4) never ran
- Recovery: `recover_missing_commit_boundary()` handles this case correctly

This window is already covered. The primary gap is Window 1.

### Why preflight doesn't self-heal Window 1

`repair_stale_preflight_started_cycle()` (repair.rs:452) handles cycles stuck at `PreflightStarted`:

| Condition | Behavior | Response Recovery |
|-----------|----------|-------------------|
| No drift (hashes match) | Marks cycle `Committed` | **Silently lost** |
| Fresh cycle (<60s) + prompt drift | Fails closed | Blocked — operator must retry |
| Stale cycle (>60s) + prompt drift | Abandons cycle | Lost but prompt remains in queue |

The "no drift" case is the worst: the response was lost but the cycle is closed as if nothing happened. No prompt remains for re-dispatch because the queue item may have been consumed during the aborted Phase 2.

## Solution

### Phase 1 — Early response persistence

Move `save_pending()` to immediately after stdin read, before all validation, parsing, and processing.

**Changes in `run()` (write.rs:6763+):**
```rust
// Read response from stdin
let mut response = String::new();
std::io::stdin()
    .read_to_string(&mut response)
    .context("failed to read response from stdin")?;

// EARLY PERSIST: save to pending store before any processing
repair::save_pending(file, &response)?;

// Now do validation (empty check, imperative contract, etc.)
if response.trim().is_empty() {
    repair::clear_pending(file)?;
    if recover_empty_response_for_strict_closeout(file, &flags)? {
        return Ok(());
    }
    anyhow::bail!("empty response — nothing to write");
}
```

**Changes in `run_template()` (write.rs:6906+):**
```rust
// Read response from stdin
let mut response = String::new();
std::io::stdin()
    .read_to_string(&mut response)
    .context("failed to read response from stdin")?;

// EARLY PERSIST: save to pending store before any processing
repair::save_pending(file, &response)?;

// Now do validation
if response.trim().is_empty() {
    repair::clear_pending(file)?;
    // ...
}
```

**Empty response handling:** If validation later determines the response is empty or invalid, `clear_pending()` removes the spurious pending file before returning the error.

**Risk:** Minimal — `save_pending` writes to `.agent-doc/pending/<hash>.md` (temporary) and creates a capture ledger entry. If the response is later rejected by validation, both are cleaned up by `clear_pending`. The pending file is not visible to the user or the document.

### Phase 2 — Reentrant cycle resumption in preflight

Enhance `enforce_cycle_completion()` and `repair::run()` to detect and replay responses from any completed phase:

**Changes in `repair::run()` (repair.rs:1355+):**

When a cycle is in `ResponseCaptured` or `WriteApplied` state with a pending file or capture:
1. Load the pending/capture response
2. Verify the baseline still matches (no drift)
3. If match: replay the response through the normal write path (Phase 2 onward)
4. If drift: fail closed with diagnostic explaining the conflict

This already partially exists in the recovery cascade (steps 1-4), but the cascade is only triggered when the pending/capture files exist AND the cycle is in an appropriate state. The gap is that `repair_stale_preflight_started_cycle()` doesn't check for pending/capture before abandoning.

**Enhanced `repair_stale_preflight_started_cycle()`:**

```rust
// Before abandoning a PreflightStarted cycle, check for pending/capture
if let Some(pending) = load_pending(file)? {
    // Response was persisted but cycle didn't advance past PreflightStarted
    // This means save_pending succeeded but mark_response_captured didn't
    // Replay the response through the write path
    return replay_response_from_pending(file, &pending);
}
if let Some(capture) = capture::load_active(file)? {
    return replay_response_from_capture(file, &capture);
}
// Only then fall through to abandon logic
```

### Phase 3 — Queue prompt recovery

Store the consumed queue prompt text in the cycle state before removing it from the document.

**Changes in `cycle_state.rs`:**
- Add `queue_prompt: Option<String>` to the cycle state
- Record the queue prompt text when `mark_preflight_started()` is called (preflight already has the queue prompt at that point)

**Changes in `write.rs` Phase 3 (queue consumption, lines 1488-1584):**
- The queue prompt is already available from preflight
- If it wasn't recorded during mark_preflight_started, record it here before consuming

**Recovery:** If a crash loses the response AND the queue was consumed, `repair` can:
1. Check cycle state for the stored queue prompt
2. Re-inject the prompt into the queue for the next cycle
3. Log the recovery action

### Phase 4 — Reentrant `run_command()` orchestration

The main `write::run_command()` function (write.rs:1156-1631) should be made reentrant by checking cycle state at entry:

```rust
fn run_command(...) -> Result<()> {
    let cycle = cycle_state::load(file)?;

    match cycle.map(|c| c.phase) {
        None | Some(CyclePhase::Committed) | Some(CyclePhase::Abandoned) => {
            // Normal start from scratch
        }
        Some(CyclePhase::PreflightStarted) => {
            // Check for pending/capture before starting new cycle
            if has_pending_or_capture(file)? {
                // Resume from Phase 2 (response write)
                return resume_from_captured_response(file, ...);
            }
            // No response to recover — abandon stale cycle
        }
        Some(CyclePhase::ResponseCaptured) | Some(CyclePhase::WriteApplied) => {
            // Resume from the appropriate phase
            return resume_from_captured_response(file, ...);
        }
    }

    // Normal pipeline...
}
```

### Phase 5 — Frontmatter step tracker (turn_id + pipeline_step)

Add frontmatter fields that persist the current pipeline step without git commits, enabling any `agent-doc` invocation to continue from the exact step.

**New frontmatter fields:**

```yaml
agent_doc_pipeline:
  turn_id: "reentrant-finalize"     # derived from backlog ID or auto-generated
  run_id: "cycle-1780600334196"     # the current cycle_state ID
  step: "response_captured"         # current CyclePhase value
  queue_task_id: "#reentrant-finalize"  # optional: backlog ID for this run
```

**Behavior:**
- `step` updates at each phase transition (PreflightStarted → ResponseCaptured → WriteApplied → Committed) — written to frontmatter but NOT committed to git until the normal finalize commit
- `turn_id` is set from the queue item's backlog ID (if present) or auto-generated
- `run_id` maps to the cycle_state ID for correlation
- On crash recovery, any `agent-doc` invocation reads these fields and continues from the recorded step
- JB `Run Agent Doc` reads the step and routes to the appropriate recovery path

**Queue prompt → task conversion:** When a free-form queue prompt arrives:
1. If it matches an existing backlog ID (`do #id`), use that ID as `turn_id`
2. If no match, create a new backlog item with a generated ID and set that as `turn_id`
3. Store the task ID in the cycle state for cross-reference

**Changes in `cycle_state.rs`:**
- Add `turn_id`, `queue_task_id` fields to cycle state
- Phase transitions write to frontmatter in addition to the cycle state file

**Changes in `frontmatter.rs`:**
- Add `agent_doc_pipeline` struct with serde derive
- Read/write the pipeline section without touching other frontmatter fields
- The pipeline section is ephemeral — stripped before git commit or included as-is (commit happens at Committed phase anyway)

**Changes in `preflight.rs`:**
- On startup, read `agent_doc_pipeline.step` to detect where to resume
- If `step` is stale (cycle state says Committed but frontmatter says WriteApplied), trust cycle state
- If `step` is ahead (frontmatter says WriteApplied but no cycle state exists), trust cycle state absence

**Determinism requirement:** All step transitions are deterministic — given the same cycle state + pending/capture state, the same recovery path is taken regardless of which invocation triggers it.

## Files to Modify

| File | Changes |
|------|---------|
| `write.rs` | Move `save_pending` before validation in `run()` and `run_template()`; add reentrant resume logic in `run_command()` |
| `repair.rs` | Enhance `repair_stale_preflight_started_cycle()` to check pending/capture before abandoning; add `replay_response_from_pending/capture` helpers |
| `cycle_state.rs` | Add `queue_prompt`, `turn_id`, `queue_task_id` fields; phase transitions write frontmatter |
| `frontmatter.rs` | Add `agent_doc_pipeline` struct for step tracking without git commits |
| `preflight.rs` | Store queue prompt text in cycle state; read `agent_doc_pipeline.step` for resume detection |
| `capture.rs` | No changes needed — existing capture ledger already stores the response body |

## Test Plan

1. **Early persistence unit test:** Mock stdin, crash between read and validation, verify pending file exists with correct content
2. **Reentrant preflight test:** Create cycle in `PreflightStarted` state with pending file, run preflight, verify response is replayed and committed
3. **Queue prompt recovery test:** Create cycle with stored queue prompt, crash after queue consumption, verify prompt is re-injected on repair
4. **Template path crash test:** Same as (1) but for `run_template()` with patch blocks
5. **Full pipeline crash test:** Crash at each phase boundary (PreflightStarted, ResponseCaptured, WriteApplied, Committed), verify recovery at each point
6. **SimWorld coverage:** Add deterministic model for the crash-and-resume sequence

## Success Criteria

- No crash window leaves the response irrecoverable — every phase has at most one replay, never silent loss
- Preflight automatically resumes from the last successful checkpoint
- Queue prompts are never silently dropped
- `session-check` reports the crash-and-resume event for observability
- Existing repair and closeout paths remain unchanged for already-covered windows

## Relationship to Existing Plans

- Extends `commit-boundary-reliability-codex-ipc.md` (Phase 2 — resume commit after successful write) with earlier-phase coverage
- The `recover_missing_commit_boundary()` function (repair.rs:668) already handles the write-applied-but-not-committed window; this plan adds coverage for the earlier stdin-only and capture-but-not-written windows
