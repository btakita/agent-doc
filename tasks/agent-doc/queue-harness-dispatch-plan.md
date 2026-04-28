# Queue Harness Command Dispatch Plan

## Overview

Allow `agent:queue` to include harness CLI commands (`/clear`, `/compact`, `/model sonnet`, etc.) alongside document prompts. When the queue consumer encounters a command, it dispatches it to the running harness session rather than treating it as a document prompt.

## Problem

Currently, harness commands like `/clear` require the user to type them in the terminal. If a queue contains `- /clear` followed by document prompts, the queue consumer has no way to execute that command — it would be treated as a document prompt and fail or produce nonsense.

## Design

### Detection

Queue items starting with `/` are harness commands. The parser classifies each item as either `prompt` or `command`:

```markdown
<!-- agent:queue -->
- /clear
- /compact tasks/agent-doc/agent-doc-bugs.md
- do #fix1
- /model sonnet
- Review the pending items
<!-- /agent:queue -->
```

Parsed as: `[command:/clear, command:/compact ..., prompt:"do #fix1", command:/model sonnet, prompt:"Review the pending items"]`

### Dispatch Mechanisms

Two dispatch paths, selected by runtime context:

#### Path A: Supervisor IPC (preferred)

If a supervisor process is running (detected via socket at `.agent-doc/supervisor.sock` or env var `AGENT_DOC_SUPERVISOR_PID`):

1. Queue consumer sends `{ "type": "command", "command": "/clear" }` over the IPC socket
2. Supervisor executes the command in the harness session
3. Supervisor replies with `{ "status": "ok" }` or `{ "status": "error", "message": "..." }`
4. Queue consumer waits for completion before proceeding to the next item

#### Path B: tmux send-keys (fallback)

If no supervisor is available but the harness runs in a tmux pane:

1. Detect the target pane from `AGENT_DOC_TMUX_PANE` env var or `.agent-doc/pane`
2. Run `tmux send-keys -t <pane> '/clear' Enter`
3. Poll for completion: watch for the harness prompt to reappear (or use a sentinel file)
4. Proceed to next queue item

#### Path C: Inline execution (subset)

Some commands can be executed directly by the queue consumer without harness interaction:
- `/model <tier>` — set the model for subsequent queue items (modifies queue-local state)
- `/compact` — invoke `agent-doc compact` directly

Commands that require the harness session (like `/clear` which resets conversation context) must use Path A or B.

### Ordering Guarantees

Queue items execute strictly in order. A command must complete before the next item (prompt or command) starts. If a command fails, the queue halts and reports the failure — remaining items stay queued.

## Implementation Phases

### Phase 1: Parser classification

- Extend queue parser to classify items as `prompt` or `command`
- Items starting with `/` → `command`; all others → `prompt`
- Emit classified items in preflight JSON: `queue_items: [{ type: "prompt"|"command", text: "..." }]`

### Phase 2: Inline-executable commands

- `/model <tier>` — apply to queue-local state for subsequent prompts
- `/compact <file>` — call `agent-doc compact` directly
- These don't need supervisor or tmux

### Phase 3: tmux send-keys dispatch

- Detect tmux pane from env/config
- Send command + Enter
- Poll for completion (sentinel file or prompt detection)
- Error handling: timeout after configurable duration

### Phase 4: Supervisor IPC dispatch

- Define IPC protocol for command dispatch
- Supervisor accepts command, executes in harness session, returns status
- Queue consumer blocks until response

## Open Questions

- Should `/clear` be a special case that also clears the queue itself?
- Timeout for tmux send-keys polling — 30s? 60s? Configurable?
- Should the queue support conditional commands (e.g., "if last prompt failed, /clear")?
