# agent-doc CRDT Demo

Two documents running simultaneously in separate tmux panes, demonstrating conflict-free concurrent editing.

## Setup

```bash
# Terminal 1: Start streaming on Document A
agent-doc stream demo/crdt-demo-a.md

# Terminal 2: Start streaming on Document B
agent-doc stream demo/crdt-demo-b.md
```

## Screencast Script

### Scene 1: Two panes, two documents (0:00-0:30)
- Split tmux horizontally: left = Doc A, right = Doc B
- Editor open in a third pane (or floating window) showing both files
- Start both streams simultaneously

### Scene 2: Concurrent streaming (0:30-1:15)
- Both documents receive responses at the same time
- Show text appearing line-by-line in both editor tabs
- Point out the 200ms flush interval — near-real-time updates

### Scene 3: Edit while streaming (1:15-1:45)
- While Doc A is still receiving its response, type a follow-up question
- Show the CRDT merge message in the terminal
- Switch to Doc B, add an inline annotation mid-response
- Both edits are preserved — no conflicts

### Scene 4: Pane switching with agent-doc (1:45-2:15)
- Run `agent-doc claim demo/crdt-demo-a.md` in pane 1
- Run `agent-doc claim demo/crdt-demo-b.md` in pane 2
- Show `agent-doc focus demo/crdt-demo-a.md` switching to the correct pane
- Demonstrate the routing: each document knows its pane

### Scene 5: The snapshot system (2:15-2:30)
- Show `.agent-doc/snapshots/` — two snapshot files, one per document
- Explain: "Each document tracks its own baseline independently"

### Scene 6: Closing (2:30-2:45)
- Show both documents with complete responses + user edits intact
- "Two documents, two AI agents, zero conflicts. That's CRDT."

## Key commands for the demo

```bash
# Claim documents for tmux routing
agent-doc claim demo/crdt-demo-a.md
agent-doc claim demo/crdt-demo-b.md

# Start streaming
agent-doc stream demo/crdt-demo-a.md
agent-doc stream demo/crdt-demo-b.md

# Focus a document's pane
agent-doc focus demo/crdt-demo-a.md

# Check diff (shows what changed)
agent-doc diff demo/crdt-demo-a.md
agent-doc diff demo/crdt-demo-b.md
```
