# Plan — Bound long-lived state pressure (`#statepressure`)

**Status:** Complete 2026-09-06.

## Goal

Keep a long-lived project bounded across controller restarts, supervisor retries,
and thousands of completed cycles without losing the cursors and recent evidence
needed for crash recovery, editor replay, or operator forensics.

## Invariants

- Open cycles, retained responses, and editor reconciliation are immutable to
  cleanup. Rotation and compaction may run only after their durable boundary.
- The project controller owns recovery scheduling and policy. SQLite owns
  transactional receipt/event deletion; filesystem code owns log rotation and
  descriptor lifecycle.
- Navigation and sync are observers, never cleanup triggers.
- Audit identities and replay cursors outlive compacted payload rows.

## Completed phases

### 1. Terminal dispatch receipt compaction

Controller restart writes one exact, deduplicated crash-recovery marker for each
accepted-only/start-unproven receipt, then deletes only those proven terminal
receipt rows in 5,000-row batches. Failed and start-proven receipts remain. A
2,000-receipt restart regression proves the second reload has no stale replay and
does not duplicate audit markers.

### 2. Acknowledged state-event retention

The existing live-registration watermark remains authoritative. Rows strictly
below the minimum acknowledged cursor are removed in 5,000-row batches in the
same transaction; the cursor row and document high-water mark remain. A
12,050-event regression proves multi-batch deletion and exact retained bounds.

### 3. Bounded logs and descriptors

Filesystem-owned rotation keeps a 32 MiB active log and one compressed retained
segment under a project lock. Completed-cycle logging schedules rotation for the
ops, cycle, and session timelines; supervisor admission catches legacy oversized
session logs. Rotation-aware readers join the retained segment to the active tail.

Long-lived session writers share a synchronized append handle, so clones do not
multiply file descriptors. The next append reopens after rotation, and resource
telemetry reports descriptors separately from byte pressure. A 2,000-clone stress
test proves constant log descriptors, complete readable history, and successful
post-rotation append.

## Verification

- focused SQLite recovery and watermark stress regressions;
- focused filesystem, ops-log, supervisor, start-runtime, and session-accretion
  suites;
- workspace `make check`, release build/install, and installed-version probe.
