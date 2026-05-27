# Plan: `already_applied` IPC path saves stale `content_ours` as snapshot, diverging HEAD from working tree

**Bug class:** write/IPC/snapshot-consistency
**Severity:** high — produces stuck captured cycles, duplicate responses, and requires `agent-doc reset --from-current` recovery
**Related:** `plan-full-document-ipc-corruption-typing-prompt.md`, `plan-ipc-corruption-and-duplicate-during-typing.md`, `plan-ipc-timeout-duplicate-patchback.md`, `plan-stuck-cycle-causes-duplicated-uncommitted-response.md`

## Problem

When `try_ipc()` receives an `already_applied` ack from the plugin, it calls `persist_already_applied_socket_content_ours_snapshot()` (write.rs:8060) which saves `content_ours` (baseline + patches, computed at write.rs:6507-6514) as the snapshot without verifying that the actual disk content matches. The git::commit path then stages this snapshot via `hash-object + update-index`, producing a HEAD that diverges from the working tree.

Observed in tsift.md session (commits c7094309e through 9ed6cfaa1, 2026-05-27):

1. Finalize sends patches via socket IPC
2. Plugin replies `already_applied` (ack `{"type":"ack","status":"error","reason":"already_applied"}`)
3. Binary saves `content_ours` as snapshot (write.rs:8125)
4. `persist_already_applied_socket_content_ours_snapshot()` detects divergence between `content_ours` and disk (write.rs:8078-8107) but saves `content_ours` as snapshot **regardless** (write.rs:8125)
5. Sets `ipc_snapshot_adoption_blocked` flag (write.rs:8107)
6. git::commit stages snapshot (`content_ours`) as the committed blob (git.rs:1854)
7. Working tree = plugin's buffer (may contain stuck-cycle duplicates or user edits)
8. HEAD = `content_ours` (clean but stale)
9. session-check detects: `snapshot_len != head_len` or "direct response patchback without agent-doc cycle"
10. Recovery requires manual edits + `agent-doc reset --from-current`

The `ipc_snapshot_adoption_blocked` flag (git.rs:1504-1506) prevents absorbing working tree edits into the snapshot, which is correct in isolation but means HEAD permanently diverges from the working tree until manual reset.

## Root cause

The `already_applied` path trusts the plugin's in-memory state as ground truth but saves a computed `content_ours` that may never have been on disk. There are three race windows:

### Race A: Plugin buffer vs disk read (primary)

```
T1: Binary reads file from disk (write.rs:9029)
T2: Binary sends patches via socket (write.rs:9207)
T3: Plugin applies patches to in-memory buffer
T4: Plugin replies "already_applied"
T5: Plugin writes buffer to disk (asynchronous, may happen later)
T6: Binary saves content_ours as snapshot (write.rs:8125)
    → content_ours may differ from what's on disk at T5/T6
T7: git::commit stages snapshot → HEAD = content_ours
T8: Working tree = plugin buffer from T5 (may have stale duplicates)
```

### Race B: Stuck prior cycle + new cycle overlap

```
T1: Cycle A's try_ipc() sends patches, times out (2s)
T2: Cycle A exits code 75 or commits with local write
T3: Plugin actually DID receive Cycle A's patches, applied to buffer
T4: Cycle B's try_ipc() sends NEW patches
T5: Plugin sees response heading already in buffer → replies "already_applied"
T6: But plugin means "I have Cycle A's response" NOT Cycle B's
T7: Binary saves content_ours (Cycle B's response) as snapshot
T8: Plugin buffer has: Cycle A response + Cycle B response → duplicate heading
T9: HEAD = content_ours (Cycle B only), working tree = plugin buffer (A + B)
```

### Race C: User edits between preflight and finalize

```
T1: Preflight captures baseline
T2: User edits the document (adds prompt, changes backlog, etc.)
T3: Finalize computes content_ours from baseline + patches (does NOT include user edits from T2)
T4: Plugin replies "already_applied"
T5: Binary saves content_ours as snapshot (missing user edits from T2)
T6: HEAD = content_ours (missing user edits)
T7: Working tree = plugin buffer (has user edits)
```

## Fix plan

### Phase 1: Verify disk content before accepting `already_applied`

**File:** `src/write.rs`, `persist_already_applied_socket_content_ours_snapshot()` (line 8060)

Before saving `content_ours` as snapshot, read the actual disk file and compare:

1. If disk content == `content_ours`: proceed as today (fast path)
2. If disk content contains the response heading from `content_ours` plus additional content (user edits, stuck-cycle duplicates): adopt the disk content as the snapshot after deduplicating any response headings
3. If disk content does NOT contain the response heading: the plugin lied or hasn't flushed — fall through to the file-IPC write path instead of saving a stale snapshot

Add a new helper `verify_and_adopt_disk_content()`:

```rust
fn verify_and_adopt_disk_content(
    file: &Path,
    content_ours: &str,
    response_heading: &str,
) -> Result<String> {
    let disk = fs::read_to_string(file)?;
    if disk == content_ours {
        return Ok(content_ours.to_string());
    }
    // Check if disk contains our response heading
    if disk.contains(response_heading) {
        // Disk has our response (possibly with duplicates or user edits)
        // Deduplicate response headings and adopt disk content
        let deduped = deduplicate_response_headings(&disk, response_heading);
        return Ok(deduped);
    }
    // Disk doesn't have our response — plugin may not have flushed
    // Return error to fall through to file-IPC path
    bail!("disk content does not contain response heading; cannot accept already_applied")
}
```

### Phase 2: Response heading deduplication

**File:** `src/write.rs` or `src/repair.rs`

When the disk content contains duplicate response headings (Race B), detect and remove them:

1. Find all occurrences of the response heading pattern `### Re: <topic> — <model>`
2. Keep the last occurrence (the one with the freshest boundary marker)
3. Remove earlier occurrences and their body text up to the next heading or boundary marker
4. This is safe because the heading + model attribution uniquely identifies a response

### Phase 3: Adopt disk content for snapshot when `ipc_snapshot_adoption_blocked`

**File:** `src/git.rs` (line 1504-1506)

Currently, when `ipc_snapshot_adoption_blocked` is set, git::commit refuses to absorb working tree edits into the snapshot. Change this to:

1. When the flag is set, read the disk content
2. Run the deduplication from Phase 2
3. If the deduplicated content is structurally valid (has exchange, boundary, etc.), adopt it as the snapshot
4. If not structurally valid, fail closed with a clear error message suggesting `agent-doc reset --from-current`

### Phase 4: Eliminate Race B with patch-ID-aware `already_applied`

**File:** `src/ipc_socket.rs`, editor plugins

The plugin's `already_applied` check should be patch-ID-aware, not just heading-aware:

1. Include the patch ID in the `already_applied` ack response: `{"type":"ack","status":"error","reason":"already_applied","patch_id":"<id>"}`
2. Binary verifies the patch ID matches the current cycle's patch ID
3. If the patch ID doesn't match, treat it as a stale ack from a prior cycle and fall through to file-IPC

### Phase 5: SimWorld regression

Add deterministic scenarios to `src/sim_world.rs`:

1. `AlreadyAppliedWithStuckPriorCycleDuplicate` — Cycle A times out, Cycle B gets `already_applied`, verify no duplicate heading
2. `AlreadyAppliedWithUserEditsDuringFinalize` — user edits between preflight and finalize, verify user edits preserved in HEAD
3. `AlreadyAppliedWithPluginBufferNotFlushed` — plugin says `already_applied` but disk doesn't have response, verify file-IPC fallback triggers

## Tests

- Unit: `verify_and_adopt_disk_content` with disk==ours, disk has duplicate heading, disk has user edits, disk missing response
- Unit: `deduplicate_response_headings` with 2 copies, 3 copies, copies with different bodies, no copies
- Integration: finalize → already_applied → commit → session-check passes
- SimWorld: all three Phase 5 scenarios

## Acceptance criteria

1. Finalize with `already_applied` ack produces HEAD == working tree (no drift)
2. No duplicate response headings after stuck-cycle + new-cycle sequence
3. User edits between preflight and finalize are preserved in committed HEAD
4. `agent-doc reset --from-current` is no longer needed as manual recovery
5. `make check` + new SimWorld scenarios pass
