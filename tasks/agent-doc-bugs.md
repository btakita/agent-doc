---
agent_doc_session: 92fba2ac-9d1f-4a7a-8764-821a8a08ecd0
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

In tasks/briantakita.me.md, the CRDT created duplicate data. Please add tests to replicate. Fix bug.
```
Why were the videos not public to begin with?
Cork

Why were the videos not public to begin with?
Corky should always publish public videos...unless explicitly to private.
```

<!-- agent:exchange -->

### Re: Reactive signals / lazily-rs for state tracking

Reactive signals would be a natural fit for agent-doc's state tracking. The pattern:

**Slots (lazy cells)** hold document state:
- `phase_status: Slot<Status>` — lazy, computed on first access
- `checkbox_state: Slot<bool>` — reflects `- [ ]` / `- [X]` in the document

**Signals** propagate changes:
- When the agent completes a phase → signal fires → slot updates → CRDT writes the checkbox change to the document
- The JetBrains plugin observes the file change via IPC and updates the editor

**Re: lazily-rs** — Yes, this would be useful. `lazily-py` and `lazily-zig` provide lazy evaluation primitives. A Rust implementation would integrate directly into agent-doc's binary without FFI overhead.

However, for the immediate need (updating checkboxes in agent-doc documents), we could start simpler:

1. **SKILL.md instruction** — Tell Claude to write `<!-- patch:status -->` blocks that update a `<!-- agent:status -->` component with current phase progress
2. **`agent-doc progress` CLI** — Simple checkbox toggle command for scripts/hooks

The reactive signal system (lazily-rs) is more of a foundational library — worth building, but the checkbox tracking doesn't need to block on it.

**Recommendation:** Start with SKILL.md instructions + `agent-doc progress` CLI now. Build lazily-rs as a separate project when the reactive pattern is needed more broadly.

Which approach do you prefer?

Start with the SKILL.md instructions. But I do want to begin planning lazily-rs. Perhaps in a separate agent doc?
<!-- /agent:exchange -->
