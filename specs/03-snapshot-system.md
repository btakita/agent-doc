> Extracted from SPEC.md — see [index](../SPEC.md)

# Snapshot System

## Storage

Snapshots live in `.agent-doc/snapshots/` relative to CWD. Path: `sha256(canonical_path) + ".md"`.

## Lifecycle

- **Save**: After successful run, full content saved as snapshot
- **Load**: On next run, loaded as "previous" state for diff
- **Delete**: On `reset`, snapshot removed
- **Missing**: Diff treats previous as empty (entire doc is the diff)
