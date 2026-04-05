---
agent_doc_session: null
agent_doc_format: template
agent_doc_write: crdt
---

## Granola Adapter

Bidirectional integration between Granola meeting notes and agent-doc.

## Context

*Transferred from tasks/recruit/robert-ross-granola.md*

### Vision

Agent-doc as a universal interface for AI-generated artifacts. Each external source (Granola, Proof, Figma Jam, Build Party, Shimmer) gets a thin adapter skill that bridges to agent-doc's CRDT template engine.

### Architecture

Each adapter is a **Claude Code skill**, not agent-doc core:

```
[Granola] ──adapter──┐
[Figma Jam] ─adapter──┤
[Build Party] ─adapter─┼──→ agent-doc template ──→ agent harness (Claude Code / tmux)
[Shimmer] ──adapter───┤                              │
[Any web UI] ─adapter─┘                              └──→ patch response ──→ adapters ──→ sources
```

Each skill follows the same contract:
1. Calls `agent-doc preflight` and `agent-doc write --stream` for the local side
2. Handles source-specific API calls
3. Stores link metadata in `.agent-doc/<source>-links.toml`

Related: tasks/software/proof.md (Proof adapter — designed, not yet built)

### `/granola` Skill Design

**Commands:**
- `granola ingest <url>` — fetch shared note, hydrate into agent-doc template
- `granola push <file>` — push agent-doc exchange back to Granola (browser automation initially)
- `granola link <file> <url>` — associate a local file with a Granola note

**Ingest pipeline:**
1. Fetch shared note content from Granola URL
2. Hydrate into an agent-doc template (`<!-- agent:notes -->` component)
3. Create frontmatter and component structure automatically

**Push-back options:**
1. Browser automation via chromium-bridge MCP (immediate)
2. Shared link as read-only mirror (companion artifact)
3. Granola write API when available (future)

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Pending

<!-- agent:pending patch=replace -->
- Prototype `/granola ingest` command (fetch + hydrate)
- Design shared adapter contract (preflight + write + source-links.toml)
- Implement browser-paste push-back via chromium-bridge
- Spec `/granola link` for persistent associations
<!-- /agent:pending -->
