# Boundary

## [Ontology](../../../existence-lang/ontology/src/ontology.md)

A marker inserted into a [Document](./document.md) that separates the agent's response space from the user's input space. A Boundary is a [Signal](../../../existence-lang/ontology/src/signal.md) — it carries no content of its own, but its position encodes the structural division between what the agent wrote and where the user may write next.

Boundary markers take the form of an HTML comment, e.g. `<!-- agent:boundary -->`. The binary manages their lifecycle: insert on agent response start, reposition as response grows, remove on session reset or document cleanup.

## [Axiology](../../../existence-lang/ontology/src/axiology.md)

The Boundary matters because it solves the attribution problem in a shared document. Without it, the binary cannot distinguish agent-authored text from user-authored text when computing the next diff. The [Snapshot](./snapshot.md) records what the document contained before the agent responded; the Boundary marks where in the current file agent content ends. Together they enable precise diff computation and safe undo.

The Boundary also provides a UX cue to the user: content above it is agent-written (and may be overwritten on next submit); content below it is the user's current prompt area.

## [Epistemology](../../../existence-lang/ontology/src/epistemology.md)

### Pattern Expression

A Boundary is the agent-doc expression of a universal pattern: any [Communication](../../../existence-lang/ontology/src/communication.md) medium needs a way to mark turn boundaries. In a terminal, the shell prompt is the boundary. In a chat UI, the message bubble is the boundary. In a shared markdown document, the HTML comment marker is the boundary — invisible to rendered output, visible to the binary's parser.

Boundary lifecycle is fully deterministic and owned by the binary — the skill never inserts or removes boundary markers directly.
