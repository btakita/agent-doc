# Run Flow

## Overview

```
┌──────────┐  hotkey ┌────────────┐  diff + prompt  ┌───────┐
│  Editor  │ ──────> │ agent-doc  │ ──────────────> │ Agent │
│          │         │            │ <────────────── │  API  │
│  reload  │ <────── │ write+snap │                 └───────┘
└──────────┘         │ git commit │
                     └────────────┘
```

## Step by step

1. **Read document** and load snapshot (last-known state from previous run)
2. **Compute diff** — if empty, exit early (double-run guard)
3. **Pre-commit** user's changes via `git add -f` + `git commit` (baseline for diff gutters)
4. **Send** diff + full document to agent, resuming session if one exists
5. **Build response** — original content + session ID update + `## Assistant` block + `## User` block
6. **Check for concurrent edits** — re-read the file
7. **Merge if needed** — 3-way merge via `git merge-file` if file changed during agent response
8. **Write** merged content back to file
9. **Save snapshot and close out** — the response is committed in the same cycle, then post-commit cleanup brings the live document, snapshot, and editor-facing state back in line with committed `HEAD`. Only genuine later local edits remain uncommitted.

## Session continuity

- **Empty `session:`** — forks from the most recent agent session (inherits context)
- **`session: <uuid>`** — resumes that specific session
- **Delete `session:` value** — next run starts fresh

## Merge-safe writes

If you edit the document while the agent is responding:

- **Clean merge** (edits in different regions) — merged automatically. Message: "Merge successful — user edits preserved."
- **Conflict** (edits in the same region as the response) — conflict markers written to the file with labels `agent-response`, `original`, `your-edits`. Message: "WARNING: Merge conflicts detected."

The merge uses `git merge-file -p --diff3`, which handles edge cases (whitespace, encoding, partial overlaps) better than a custom implementation.

## Git integration

| Flag | Behavior |
|------|----------|
| `-b` | Auto-create branch `agent-doc/<filename>` on first run |
| (none) | Pre-commit user changes to current branch |
| `--no-git` | Skip git entirely |

The closeout flow still commits the user's baseline before generating a response, but a successful response turn now also crosses the binary-owned commit boundary before it exits. Agent-owned `(HEAD)` / boundary churn is cleaned up in the same cycle instead of being left behind for the next run. Only real follow-up edits after that closeout remain as uncommitted local changes.

Cleanup: `agent-doc clean <file>` squashes all session commits into one.
