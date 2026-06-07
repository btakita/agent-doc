> Extracted from SPEC.md — see [index](../SPEC.md)

# Snapshot System

## Storage

Snapshots live in `.agent-doc/snapshots/` relative to CWD. Path: `sha256(canonical_path) + ".md"`.

## Lifecycle

- **Save**: After successful run, full content saved as snapshot
- **Pre-response undo snapshot**: local write paths capture the undo snapshot from the live
  document while the advisory doc lock is held, so `undo` restores the exact on-disk state that
  existed immediately before the response write
- **Load**: On next run, loaded as "previous" state for diff
- **Safe absorb on commit**: If the working tree is ahead of the snapshot because of a missed
  agent-doc-style mutation (`status` changed and/or `exchange` gained a new `### Re:` block and/or
  `pending` gained a superset of stable IDs) while the redacted document structure is unchanged,
  `agent-doc commit` refreshes the snapshot from the live file before staging. This repairs lost
  patchbacks/pending ops without turning plain user prompts into committed content.
- **Delete**: On `reset`, snapshot removed
- **Missing**: Diff treats previous as empty (entire doc is the diff)

## Auto-Migration on Rename

When a document is renamed/moved, its path hash changes, orphaning all `.agent-doc/` state
files, including both CRDT sidecars (`<hash>.yrs` and `<hash>.overlay.yrs`). `ensure_initialized` (called from `start`, `preflight`, `claim`, and `sync`) detects
this automatically:

1. Document has a `agent_doc_session` UUID in frontmatter
2. No snapshot exists for the current path hash
3. Scan `.agent-doc/snapshots/*.md` for a snapshot whose frontmatter has the same session UUID

If an orphaned snapshot is found, all state files are migrated from the old hash to the new
hash: snapshots, baselines, locks, pending, legacy CRDT, overlay CRDT, and pre-response. The sessions registry is also updated.

Explicit preflight baseline paths are part of the active closeout contract. If a document
is moved after preflight and rename migration moves `.agent-doc/baselines/<old-hash>.md`
to the new path hash, `finalize` / `write --commit` must recover by reading the migrated
current-hash baseline instead of falling back to `content_ours` or failing on the missing
old path.

**Fallback:** `agent-doc rename <old> <new>` performs the same migration explicitly when
the old path is known.
