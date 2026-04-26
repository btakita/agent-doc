# Example: agent-doc developing itself

This is a real task document used to develop agent-doc itself. The document served as both the planning surface and the conversation interface between a human developer and an AI agent (Claude) throughout the project's lifecycle.

## What you're looking at

**`task.md`** is a template-mode agent-doc session document. It contains:

- **`agent:status`** — live project status, updated by the agent after each release
- **`agent:architecture`** — system architecture maintained through the document
- **`agent:releases`** — complete release history from v0.5.5 to v0.23.0
- **`agent:lessons`** — 16 hard-won lessons discovered during development
- **`agent:history`** — compacted session summaries (each represents hundreds of lines of conversation)
- **`agent:exchange`** — the active conversation surface (compacted after each major milestone)
- **`agent:backlog`** — roadmap items with GitHub issue links (legacy alias: `agent:pending`)

## How it works

1. The developer edits `task.md` in their IDE (IntelliJ IDEA with the agent-doc plugin)
2. They run `/agent-doc task.md` in a Claude Code session
3. The agent reads the diff since the last snapshot, responds in the document, and commits
4. The developer sees the response appear in their editor in real-time via IPC patching

### Template mode

Unlike append mode (which alternates `## User` / `## Assistant` blocks), template mode uses named components. The agent responds with patch blocks that target specific components:

```markdown
<!-- patch:status -->
Updated status line here.
<!-- /patch:status -->

<!-- patch:exchange -->
### Re: Your Question (HEAD)
Response content here.
<!-- /patch:exchange -->
```

This allows the agent to update multiple parts of the document atomically — status, architecture, and exchange in a single response.

### Boundary markers

When the agent reads the document, it inserts an invisible `<!-- agent:boundary:UUID -->` marker at the end of the exchange component. This ensures responses appear after the prompt that triggered them, even if the developer types new text while waiting.

### Compaction

When the exchange grows too large (hundreds of lines of conversation), the developer types "compact exchange" and the agent archives the content, replacing it with a summary. The session history section preserves the high-level narrative.

## Key patterns demonstrated

- **Document-as-UI**: The markdown file IS the user interface. Edits are prompts.
- **Iterative problem-solving**: The boundary marker feature evolved through four approaches (caret-based -> byte offset -> content hash -> marker) before settling on the final design.
- **Lesson accumulation**: Bug patterns are captured as lessons (#13 was triggered three times before the rule was internalized).
- **Release tracking**: The agent maintains version history as a living document, not just git tags.
- **Privacy boundary**: Personal conversation details are compacted away; only product-relevant summaries remain.

## Running your own session

```bash
# Install agent-doc
cargo install agent-doc

# Create a new task document
cat > my-project.md << 'EOF'
---
agent_doc_format: template
---

# My Project

<!-- agent:status -->
Starting...
<!-- /agent:status -->

<!-- agent:exchange patch=append -->
What should we build first?
<!-- /agent:exchange -->
EOF

# Start a session (requires Claude Code)
# /agent-doc my-project.md
```
