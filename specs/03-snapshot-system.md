> Extracted from SPEC.md — see [index](../SPEC.md)

# Typed Document Baselines (No Filesystem Sidecars)

## Storage

The typed state ledger in `.agent-doc/state.db` owns document baselines, undo
checkpoints, and CRDT recovery checkpoints.

No command writes, reads, scans, imports, or repairs a content-bearing
`.agent-doc/snapshots/<hash>.md` file. Files left by an older version are inert
legacy residue and may be garbage-collected. Copying a session document into a
project-state path created a second document-shaped object which an editor could
mistake for a session; the ledger stores the same recovery fact without creating
another path capable of acquiring document authority.

For this contract, a **sidecar** is a per-document filesystem object that mirrors
mutable document/session state and can be consulted or projected by a later
runtime. Those objects are forbidden. The remaining `.agent-doc/` filesystem
classes are not document sidecars: `state.db` is the typed controller/Lazily
ledger; locks, sockets, and transport envelopes are bounded coordination
artifacts and never state authority; logs and `repair-blocked/` are diagnostic
evidence and never replay authority; `archives/` are immutable operator-visible
history; and `config.toml`, installed plugins, and `dashboard.md` are explicit
project artifacts/projections. None may be admitted as a session document.

## Lifecycle

- **Checkpoint**: After a successful run, the committed baseline is appended as
  a typed ledger fact. There is no content-bearing filesystem effect.
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
- **Reset**: `reset` appends typed clear facts. It does not need to locate or
  remove a content copy beside the ledger.
- **Missing**: Diff treats a missing ledger baseline as first submit. Git may
  supply that first-submit baseline when HEAD differs from the current
  worktree; legacy snapshot files are not a fallback.

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
registry is updated. Legacy snapshot files remain inert; migration neither
reads nor moves them.

Explicit preflight baseline ids are part of the active closeout contract. If a document is
moved after preflight, `finalize` / `respond` / `write --commit` resolve the rekeyed baseline
from `state.db` instead of falling back to `content_ours` or failing on the old path.

`agent-doc rename <old> <new>` performs the same ledger migration explicitly
when the old path is known. It is an explicit command, not a sidecar fallback.
