---
agent_doc_session: 25994048-f0c3-4796-bb42-2290a3318a71
tmux_session: '1'
---

# Pane Groups — Dynamic tmux layout for parallel sessions

## Problem

When multiple parallel Claude sessions run for the same document, each session gets its own tmux pane. Switching between documents should show/hide the associated pane group. Currently, all panes are visible in a flat left|right layout regardless of which document is focused.

## Design

### Pane groups bound to documents

Each document owns a **tmux window**. Parallel tasks for that document spawn extra panes within the window.

- Focus doc A (3 parallel tasks) → tmux switches to window A → 3 stacked panes visible
- Focus doc B (1 session) → tmux switches to window B → 1 pane visible
- Switch back to doc A → 3 panes reappear
- Parallel tasks finish → panes close → window collapses to 1 pane

**Key insight:** tmux window switching IS the visibility toggle. No pane hide/show API needed.

### 2D stacking (nested splits)

Within a tmux window, panes can stack in a column layout:

```
┌───────────────┬─────────────────┐
│  claude-1     │                 │
├───────────────┤  claude-right   │
│  claude-2     │  (full height)  │
├───────────────┤                 │
│  claude-3     │                 │
└───────────────┴─────────────────┘
```

Left column: 3 parallel sessions stacked vertically.
Right column: 1 session at full height.

Achieved with nested tmux splits:

```sh
# Start: %4 (left) | %11 (right)
tmux split-window -t %4 -v    # left becomes 2 stacked
tmux split-window -t %4 -v    # left becomes 3 stacked
# Right (%11) stays full height automatically
```

### Layout command extension

```sh
# Group by column, stack vertically within each
agent-doc layout plan.md corky.md plugin.md --column left
agent-doc layout agent-doc.md --column right
```

Or automatic grouping: layout reads each doc's `--position` (left/right from IDE claim), groups by column, splits vertically within. Aligns with existing claim positioning system.

## Session registry changes

- `SessionEntry` gains `window: String` (already exists) and `group: Vec<String>` (pane IDs for parallel tasks)
- `agent-doc focus <file>` switches to the document's tmux window (already does this)
- Parallel task panes register under the parent document's session entry
- When a parallel task completes, its pane closes and deregisters

## Lifecycle

1. User opens doc A → `agent-doc claim` creates window, registers primary pane
2. Agent spawns parallel tasks → new panes split within the window, registered as group members
3. User switches to doc B → `agent-doc focus` switches tmux window → doc B's panes visible
4. User switches back to doc A → window switch → all 3 panes reappear
5. Parallel tasks complete → panes close, deregister → window returns to 1 pane

## Open questions

- Should parallel task panes auto-close or stay open for review?
- Maximum panes per column before scrolling becomes impractical? (Likely 3-4)
- Should `agent-doc layout` auto-detect parallel tasks and arrange, or require explicit `--column`?
