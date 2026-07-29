> Extracted from SPEC.md — see [index](../SPEC.md)

# Baseline and Crash-State Sidecars

## Storage

The typed state ledger in `.agent-doc/state.db` owns document baselines, undo
checkpoints, and CRDT recovery checkpoints.

Filesystem files under `.agent-doc/snapshots/` are write-only crash-state
sidecars.
Their path remains `sha256(canonical_path) + ".md"` so an operator can correlate
evidence with a document identity, but no normal command may read, scan, or
import those files as document, baseline, cycle, capture, or rename authority.

## Lifecycle

- **Checkpoint**: After a successful run, the committed baseline is appended as
  a typed ledger fact. Only after that commit succeeds, a filesystem effect
  writes the same content as crash state. Sidecar-effect failure cannot make
  the sidecar an input to, or a substitute for, typed state.
- **Pre-response undo checkpoint**: local write paths capture undo content from
  the live document while the advisory doc lock is held, so `undo` restores the
  exact state that existed immediately before the response write.
- **Load**: On the next run, the projected ledger baseline is the "previous"
  state for diff.
- **Safe absorb on commit**: If the working tree is ahead of the baseline because of a missed
agent-doc-style mutation (`status` changed and/or `exchange` gained a new `### Re:` block and/or
`pending` gained a superset of stable IDs) while the redacted document structure is unchanged,
`agent-doc commit` checkpoints the live current document before staging. This repairs lost
patchbacks/pending ops without turning plain user prompts into committed content.
- **Reset**: `reset` appends typed clear facts and may remove associated crash
  evidence. Deleting a crash file never decides or changes live authority.
- **Missing**: Diff treats a missing ledger baseline as first submit. Git may
  supply that first-submit baseline when HEAD differs from the current
  worktree; crash sidecars are not a fallback.

## Auto-Migration on Rename

When a document is renamed/moved, its path hash changes, orphaning the
document-keyed typed state rows. `ensure_initialized` (called from `start`,
`preflight`, `claim`, and `sync`) detects this automatically:

1. Document has a `agent_doc_session` UUID in frontmatter
2. No typed baseline exists for the current path hash
3. The durable session registry identifies the previous document path for that
   session UUID

The typed event rows and their embedded fact hashes are rekeyed in one
transaction. Stale editor acknowledgement cursors are retired and the session
registry is updated. Crash sidecars remain untouched under their crash-time
identity; migration neither reads nor moves them.

Explicit preflight baseline ids are part of the active closeout contract. If a document is
moved after preflight, `finalize` / `respond` / `write --commit` resolve the rekeyed baseline
from `state.db` instead of falling back to `content_ours` or failing on the old path.

`agent-doc rename <old> <new>` performs the same ledger migration explicitly
when the old path is known. It is an explicit command, not a sidecar fallback.
