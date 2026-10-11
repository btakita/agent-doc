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
- **Agent-write attribution (`#backlogiceboxreview`)**: a standalone tracked-work
  command (`agent-doc backlog|icebox|review <FILE> ...`) is an agent edit. Once
  one of its writes has landed (a converged editor write, or a `--force-disk`
  write), the baseline advances by that write's *structural delta* only: the
  component-body change from the document the command planned against
  (`before`) to the target it wrote (`after`) is replayed onto the stored
  baseline (`agent_doc_element::baseline_attribution::attribute_component_write`,
  applied by `agent_doc_snapshot_io::attribute_agent_write_to_document_baseline`).
  The whole document is never re-snapshotted, so an operator edit the baseline
  has not absorbed (anywhere, including elsewhere in the same component) stays
  visible to the next diff as operator input. The replay is a line-level
  three-way merge over `before`; it is refused, leaving the baseline untouched,
  when the write is not expressible as component-body edits (it created or
  removed a component, or moved text between components), when a changed
  component is missing from the baseline, or when the write's change overlaps
  or touches a difference the baseline has not absorbed. Fail safe: an agent
  edit shown as an operator edit is noise; an operator edit hidden by the
  baseline is lost intent. A deferred (exit 75, still retained) write is not
  attributed, so a write that never lands cannot be pre-absorbed. Every
  attempt logs `document_baseline_agent_write_attribution outcome=<patched |
  already_carried | no_component_change | no_baseline | unscoped_write |
  component_missing_from_baseline | overlaps_unabsorbed_edit>`. The scope is
  installed by `main` for those commands only; the response/closeout path does
  not enter it (its commit owns the cycle's baseline), and `queue` commands keep
  their own snapshot steps.
  Why preflight's own absorption was not enough: pending maintenance resyncs
  the tracked surfaces into the baseline before diffing
  (`#pending-gate-snapshot-desync`), but when its write is retained by a live
  editor's converging delivery projection it defers and reverts that resync
  too (`#retaineddeferisnotafailure`), so the CLI write reached the diff as
  `[user+]`.
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
