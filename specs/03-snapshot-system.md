> Extracted from SPEC.md — see [index](../SPEC.md)

# Snapshot System

## Storage

Snapshots live in `.agent-doc/snapshots/` relative to CWD. Path: `sha256(canonical_path) + ".md"`.

## Lifecycle

- **Save**: After successful run, full content saved as snapshot
- **Load**: On next run, loaded as "previous" state for diff
- **Safe absorb on commit**: If the working tree is ahead of the snapshot because of a missed
  agent-doc-style mutation (`status` changed and/or `exchange` gained a new `### Re:` block and/or
  `pending` gained a superset of stable IDs) while the redacted document structure is unchanged,
  `agent-doc commit` refreshes the snapshot from the live file before staging. This repairs lost
  patchbacks/pending ops without turning plain user prompts into committed content.
- **Delete**: On `reset`, snapshot removed
- **Missing**: Diff treats previous as empty (entire doc is the diff)
