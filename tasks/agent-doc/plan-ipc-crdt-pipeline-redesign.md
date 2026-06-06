# Plan: IPC/CRDT pipeline redesign — non-blocking main loop + per-document queues

**Source:** agent-doc-bugs2.md — "Why are there so many IPC disk writes causing
File Cache Conflicts? Why is the CRDT pipeline so often blocked? Can we make the
main loop responsive + non-blocking and have per-document queues?"

This is the durable root behind the finalize-divergence / visible-buffer /
snapshot-reset churn. The point fix `#finalize-divergence-orphans-committed-head`
(consume trusts the merge) removed the worst symptom; this is the architecture
that removes the class.

## Diagnosis (from the code)

### Why so many IPC disk writes → File Cache Conflicts
- Every write path (`run`, `stream`, `finalize`, `write`) tries `try_ipc` first:
  it drops a patch JSON in `.agent-doc/patches/<hash>.json` for the IDE plugin to
  apply via the Document API (no "externally modified" dialog), then **waits up to
  2s** for the ack (`ipc_socket.rs:95`, `recv_timeout(Duration::from_secs(2))`).
- On timeout it **falls back to a direct disk write** (exit 75 / `EX_TEMPFAIL`).
  A direct disk write while IntelliJ holds the file open is exactly what raises
  the **File Cache Conflict** ("file modified externally") dialog.
- The 2s ack misses whenever the editor is busy, unfocused, mid-keystroke, or the
  socket races — i.e. constantly during live editing. Each miss = one disk write
  = one potential conflict. So the conflicts scale with *how often IPC times out*,
  not with the work.

### Why the CRDT pipeline is so often blocked
- The runtime is **synchronous and sequential** (`CLAUDE.md`: "No async —
  sequential per-run"). A finalize does read → diff → CRDT merge → IPC write
  (**blocks ≤2s**) → queue consume → commit, in order. The IPC round-trip stalls
  the whole turn.
- A single document lock (`acquire_doc_lock`) plus the synchronous flow means one
  document's slow IPC serializes everything.
- Downstream guards historically **re-rejected a successful CRDT merge**: the
  visible-buffer guard and the queue-consume divergence check failed-closed on the
  editor drift the merge had already reconciled (the `#finalize-divergence`
  symptom — now fixed for consume; the visible-buffer guard remains).

## Redesign

### 1. Per-document work queue + worker
- One in-process queue + single-writer worker **per document hash**. Writes for
  doc A never block doc B. Within a doc, writes serialize (preserving the existing
  per-doc lock invariant) but across docs they run independently.
- The supervisor already tracks per-pane/per-doc state; extend it with a
  per-document write channel instead of a global synchronous path.

### 2. Non-blocking IPC (fire-and-reconcile)
- Treat the **CRDT state as the source of truth**, the editor as one more
  concurrent replica. Send the IPC patch **without** synchronously blocking 2s;
  record a pending-ack and continue. The editor applies and acks asynchronously;
  a background reconciler merges the ack (or times out to a *coalesced* disk write
  far less often than per-write).
- This removes the per-write 2s stall and collapses many disk writes into at most
  one coalesced write per quiescent window.

### 3. Trust the merge downstream (in progress)
- `#finalize-divergence-orphans-committed-head` (done): consume reconciles instead
  of re-rejecting the merge. Apply the same principle to the **visible-buffer
  guard**: after a successful CRDT merge, do not fail-closed on the drift the
  merge already resolved — verify via the merged CRDT state, not a raw
  disk-vs-buffer byte compare.

### 4. Debounce / coalesce writes
- Batch rapid edits (streaming chunks, live typing) into one write per debounce
  window instead of an IPC round-trip per chunk. The stream path already has a
  200ms timer; generalize coalescing to all write paths.

## Phasing (so it ships incrementally, each independently verifiable)
1. **DONE** — consume trusts the merge (`#finalize-divergence-orphans-committed-head`).
2. Visible-buffer guard: verify via merged CRDT state, not raw byte compare (removes the remaining hard-block class). Live-IPC verify.
3. Non-blocking IPC ack + background reconciler (the per-write 2s stall). Highest risk; SimWorld-cover the ack/timeout/coalesce states first.
4. Per-document work queues in the supervisor (cross-doc isolation).
5. Coalesced/debounced writes across all paths.

## Verification
- A live IntelliJ session under active typing finalizes without File Cache
  Conflict dialogs and without per-write multi-second stalls.
- Concurrent sessions on different documents don't block each other.
- `#queue-edit-lost-during-finalize-crdt` (the queue-add drop) resolves naturally
  once IPC is fire-and-reconcile (no block-and-drop path).

## Related (all symptoms of this root)
`#finalize-divergence-orphans-committed-head` (done) · `#finalize-visible-buffer-ipc-timeout-race` · `#queue-edit-lost-during-finalize-crdt` · `#ipcfullprompt-recur2` · `#ipc-dup-tracking-double-merge` · `#adoc-queue-ipc-buffer-divergence` · `#prompt-duplicated-while-typing`
