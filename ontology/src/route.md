# Route

## [Ontology](../../../existence-lang/ontology/src/ontology.md)

The decision process that maps an incoming agent-doc command to the correct tmux pane for execution. A Route is an [Algorithm](../../../existence-lang/ontology/src/algorithm.md) — given a document path and a command, it produces a tmux target (session:window.pane) by consulting the [Claim](./claim.md) registry and live pane state.

Implemented in `route.rs`; also exposed as `auto_start` for the sync path.

## [Axiology](../../../existence-lang/ontology/src/axiology.md)

Routing matters because agent-doc operates across multiple tmux panes simultaneously. An editor plugin that intercepts a slash command (e.g. `/agent-doc submit`) must dispatch to the pane running the Claude CLI for that specific [Document](./document.md), not to any arbitrary pane. Without a reliable route, commands land in the wrong agent context and produce incorrect or corrupted responses.

The Route algorithm resolves in this order:
1. Look up the document's [Claim](./claim.md) in `sessions.json`.
2. Verify the claimed pane is alive and running the expected process.
3. If no valid claim exists, optionally auto-start a new [Session](./session.md).
4. Return the tmux target for command dispatch.

## [Epistemology](../../../existence-lang/ontology/src/epistemology.md)

### Pattern Expression

A Route is the agent-doc instance of a [Pattern](../../../existence-lang/ontology/src/pattern.md) found in every dispatch system: a function from identity to destination. In HTTP routing, a URL maps to a handler. In message queues, a topic maps to a consumer. In agent-doc, a document path maps to a tmux pane. The route must be computed at dispatch time, not baked in at session start — pane addresses change when tmux sessions are reattached or windows are rearranged, and `agent-doc resync` updates the [Claim](./claim.md) registry to reflect current reality.
