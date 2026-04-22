# Session

## [Ontology](../../../existence-lang/ontology/src/ontology.md)

A stateful agent-document conversation identified by a UUID, binding a [Document](./document.md) path, an agent backend process, and a tmux pane into a unified whole. A Session is a [System](../../../existence-lang/ontology/src/system.md) — its members are the document file, the running Claude CLI process, the tmux pane, and the session registry entry in `sessions.json`.

## [Axiology](../../../existence-lang/ontology/src/axiology.md)

The Session matters because it provides persistent identity across invocations. When the agent-doc binary is called again on the same document, the session UUID allows the Claude CLI to resume context (`--resume <id>`) rather than starting fresh. The [Claim](./claim.md) binding ensures that commands typed in the editor are routed to the correct tmux pane without ambiguity.

Session lifecycle:
1. **Start** — `agent-doc start` spawns a Claude CLI process in a tmux pane and registers the session.
2. **Active** — `agent-doc submit` sends diffs; the agent replies; the binary writes back.
3. **Resync** — `agent-doc resync` validates live pane state, removes dead entries, detects wrong-process panes.
4. **Reset** — `agent-doc reset` clears session and [Snapshot](./snapshot.md) state.

## [Epistemology](../../../existence-lang/ontology/src/epistemology.md)

### Pattern Expression

A Session is the [Context](../../../existence-lang/ontology/src/context.md) applied to a [Document](./document.md). It bounds what the agent needs to know: the document path, the resume ID, the tmux target, and the [Claim](./claim.md). Sessions are stored in `~/.local/share/agent-doc/sessions.json` and identified by document path hash. Each document maps to at most one active session at a time — the registry enforces this invariant.
