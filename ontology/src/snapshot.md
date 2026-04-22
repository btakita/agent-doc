# Snapshot

## [Ontology](../../../existence-lang/ontology/src/ontology.md)

A frozen copy of [Document](./document.md) content captured at a specific point in time. A Snapshot is a [State](../../../existence-lang/ontology/src/state.md) — it records the exact bytes of the document at the moment before an agent response is applied, forming the stable baseline for subsequent diff computation.

Snapshots are stored at `.agent-doc/snapshots/<hash>` relative to the document, keyed by document path hash.

## [Axiology](../../../existence-lang/ontology/src/axiology.md)

The Snapshot matters because it makes diff computation deterministic and user-edit-safe. When the user modifies the document between agent responses, the binary computes `diff(snapshot, current)` to isolate user changes from the prior agent response. Only the user's new edits are sent to the agent as the prompt — previous agent content is not re-submitted.

Snapshots also enable undo: `agent-doc undo` restores the pre-response snapshot, reverting the agent's last write while preserving user edits made before the submission.

## [Epistemology](../../../existence-lang/ontology/src/epistemology.md)

### Pattern Expression

A Snapshot is the agent-doc instance of a universal [Pattern](../../../existence-lang/ontology/src/pattern.md): any system that needs to detect change must first record a known-good baseline. In version control, a commit is a snapshot. In databases, a transaction checkpoint is a snapshot. In agent-doc, the pre-response file copy is the snapshot.

The key property: a Snapshot is immutable once written. The binary writes it atomically (write-to-temp, rename) and never modifies it — only replaces it with a newer snapshot after a successful agent response cycle.
