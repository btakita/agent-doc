# Document

## [Ontology](../../../existence-lang/ontology/src/ontology.md)

A markdown file that serves as the shared UI surface for agent-human interaction. A Document carries both visible content and YAML frontmatter metadata that governs how the agent-doc binary reads, diffs, and writes back to it.

A Document is an [Entity](../../../existence-lang/ontology/src/entity.md). It is the atom of the agent-doc [Session](./session.md) — every session is anchored to exactly one Document.

## [Axiology](../../../existence-lang/ontology/src/axiology.md)

The Document matters because it collapses the interface between human and agent into a single, version-controlled file. The editor is the UI; the file is the state. This design means any editor that can read and write markdown participates in agent interaction without custom protocol support.

A Document's frontmatter (`agent_doc_format`, `agent_doc_write`, `session_id`) encodes the [Context](../../../existence-lang/ontology/src/context.md) the binary needs to resolve write strategy, component layout, and CRDT merge behavior. The visible content encodes the [Story](../../../existence-lang/ontology/src/story.md) — the accumulated exchange between user and agent.

## [Epistemology](../../../existence-lang/ontology/src/epistemology.md)

### Pattern Expression

A Document manifests in two formats:

- **Inline** (`agent_doc_format: inline`) — User/Assistant blocks written sequentially. The document is a linear conversation transcript.
- **Template** (`agent_doc_format: template`) — Named [Components](./component.md) bounded by HTML comment markers. Agent writes target specific components; user edits the rest.

The [Snapshot](./snapshot.md) of a Document is the baseline for diff computation. The [Boundary](./boundary.md) within a Document is the signal separating agent-written content from user input space. The [Exchange](./exchange.md) is the primary component where human-agent [Communication](../../../existence-lang/ontology/src/communication.md) accumulates.
