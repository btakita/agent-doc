# Plan: Live typing duplicate insertion

## Problem

While the user types into `agent:exchange`, `agent-doc.md` can receive duplicate
content from an active writer. The latest report asks whether an orphaned
process is still writing into the document and whether the previous duplicate
patchback fix missed a live-editor path.

## Evidence to Gather

- Inspect active `agent-doc` / editor / IPC listener processes while the
  document is open.
- Compare `.agent-doc/patches`, snapshots, and recent cycle metadata for stale
  patch files that can still be replayed after finalize.
- Reproduce a live edit during `finalize --stream` and during IPC timeout or
  fallback paths.
- Check whether boundary reposition, patch delivery, autosync, or fallback patch
  replay can write the same exchange content after the user has already typed.

## Investigation Plan

- [ ] Add diagnostics that identify the process, patch id, cycle id, and write
      mode for every write touching the session document.
- [ ] Add a stale-patch/orphan-listener check that can distinguish a live IPC
      listener from an abandoned one.
- [ ] Trace the direct binary path, IPC ack path, IPC timeout path, and plugin
      replay path against the exact document content written to disk.
- [ ] Verify whether concurrent user text inside `agent:exchange` is being
      normalized as prompt content, response content, or both.

## Fix Plan

- [ ] Make patch replay idempotent across active typing by keying each patch to
      the committed boundary/cycle and rejecting already-applied response bodies.
- [ ] Guard stale fallback patch files so a later listener cannot apply them to
      a newer document generation.
- [ ] Surface a clear diagnostic when an orphaned listener or stale patch writer
      is detected instead of silently mutating the document.
- [ ] Ensure session-check catches duplicate live-typing insertions before the
      cycle can close as successful.

## Regression Coverage

- [ ] Add a CRDT/finalize integration test where the user appends exchange text
      while a response patch is applied.
- [ ] Add an IPC fallback replay test with a stale patch file and assert no
      duplicate content is inserted.
- [ ] Add a process/patch diagnostic test if the orphan detection is implemented
      in the CLI.

## Exit Criteria

- Live typing in `agent:exchange` does not duplicate the assistant response or
  the user's typed text.
- Stale fallback patches and abandoned listeners are detected or ignored
  deterministically.
- The closeout path leaves one response block, one current boundary, and a
  session-check pass without follow-up repair.
