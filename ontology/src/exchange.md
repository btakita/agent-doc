# Exchange

## [Ontology](../../../existence-lang/ontology/src/ontology.md)

The shared document surface where user and agent write inline, constituting the visible conversation history. An Exchange is a [Communication](../../../existence-lang/ontology/src/communication.md) — it is the medium through which the user's prompts and the agent's responses coexist in the same file, read by both parties.

In template-format documents, the Exchange is the `<!-- agent:exchange -->` [Component](./component.md). In inline-format documents, the Exchange is the entire document body, structured as alternating User/Assistant blocks.

## [Axiology](../../../existence-lang/ontology/src/axiology.md)

The Exchange matters because it makes conversation history a first-class artifact. Unlike chat UIs that store history in a database, the Exchange lives in a version-controlled markdown file. The user can edit it directly — correcting prior prompts, adding context, or pruning irrelevant turns. The git history of the Exchange file IS the conversation audit trail.

Exchange compaction (`agent-doc compact`) archives old turns and truncates the visible exchange when it grows too large for the agent's context window. This is the Exchange's [Evolution](../../../existence-lang/ontology/src/evolution.md) mechanism — history accumulates, compaction prunes it back to a manageable [Scope](../../../existence-lang/ontology/src/scope.md), and the [Story](../../../existence-lang/ontology/src/story.md) continues.

## [Epistemology](../../../existence-lang/ontology/src/epistemology.md)

### Pattern Expression

The Exchange is the agent-doc expression of a universal [Pattern](../../../existence-lang/ontology/src/pattern.md): any sustained [Communication](../../../existence-lang/ontology/src/communication.md) between two parties requires a shared medium that both can read and write. In letters, the paper is the exchange. In a whiteboard session, the board surface is the exchange. In agent-doc, the markdown file is the exchange — persistent, editable, version-controlled, and visible in the same editor the user is already working in.

The [Boundary](./boundary.md) within the Exchange marks the turn boundary between agent-written and user-written content. The [Snapshot](./snapshot.md) records the Exchange state before each agent turn. Together they enable the diff-based prompt construction that keeps agent context focused on new user input only.
