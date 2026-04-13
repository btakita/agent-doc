# Bidirectional IPC: Binary-to-Claude Callback Protocol

## Problem

Currently IPC is one-way: the Claude skill sends patch payloads to the agent-doc binary (via socket or file). The binary cannot request work from the running Claude session. This means operations like exchange compaction that need summarization must either:
- Be handled entirely by the skill (current approach, fragile)
- Spawn a new `claude --print` subagent (expensive, no context)

## Goal

Enable the agent-doc binary to send requests back to the Claude session that claimed the document. This allows the binary to orchestrate complex operations (cleanup, compaction, pending management) while delegating content-generation tasks (summarization) to the running agent.

## Architecture

### Current Flow (one-way)
```
Claude Skill → agent-doc write → IPC patch → JetBrains plugin
                                → file write → snapshot update
```

### Proposed Flow (bidirectional)
```
Claude Skill → agent-doc write → IPC patch → JetBrains plugin
                                              ↓ (ack)
agent-doc cleanup ← callback request → Claude session (via hook/watcher)
                  → summarized content → document update
```

## Design Options

### Option A: File-based callback (simplest)

The binary writes a request file; the skill polls for it during idle cycles.

```
.agent-doc/callbacks/<doc_hash>/
├── request.json     # binary writes: { "type": "summarize", "content": "...", "callback_id": "uuid" }
└── response.json    # skill writes: { "callback_id": "uuid", "result": "..." }
```

**Flow:**
1. `agent-doc cleanup <FILE>` writes `request.json` with the exchange content to summarize
2. Binary polls for `response.json` (with timeout)
3. The Claude session's next preflight detects the pending callback and handles it
4. Skill writes `response.json` with the summary
5. Binary reads response, applies the compacted exchange, updates snapshot

**Pros:** Simple, no new infrastructure, works with existing file-based IPC patterns
**Cons:** Latency (depends on next preflight cycle), can't interrupt a running session

### Option B: Socket-based callback (real-time)

The Claude session listens on a socket; the binary sends requests to it.

```
.agent-doc/sockets/<pane_id>.sock    # Claude session's listening socket
```

**Flow:**
1. Skill registers a callback socket at session start (preflight creates it)
2. `agent-doc cleanup <FILE>` connects to the socket, sends request
3. Skill receives request, generates summary, sends response on same connection
4. Binary receives response, applies changes

**Pros:** Real-time, no polling, natural request-response pattern
**Cons:** Requires the skill to maintain a long-running listener (not how Claude Code skills work today)

### Option C: Claude Code hook-based callback

Use Claude Code's hook system to trigger on file changes.

```json
// .claude/settings.json
{
  "hooks": {
    "PostToolUse": [{
      "matcher": "Bash",
      "command": "agent-doc check-callbacks"
    }]
  }
}
```

**Flow:**
1. `agent-doc cleanup <FILE>` writes a callback request file
2. The hook fires after each tool use in the Claude session
3. `agent-doc check-callbacks` detects the request, outputs instructions
4. Claude session processes the callback as part of its normal flow

**Pros:** Integrates with existing Claude Code infrastructure, no polling
**Cons:** Hook output interpretation is indirect, timing depends on tool use frequency

**Detailed analysis:**

- **"Indirect interpretation"** means the hook's stdout becomes just another tool result that the model *may* act on. There's no guaranteed execution path — it's prompt engineering, not a syscall. The model must infer the right action from formatted output text. You need strong SKILL.md rules to make this reliable.
- **"Timing variance"** — `PostToolUse` fires after every tool call, but NOT during text generation. Callbacks can sit unnoticed during: (a) thinking/reading turns before any tool call, (b) long streaming responses. Latency ranges from seconds (active tool-use phases) to 3+ minutes (text-heavy turns with no tool calls).
- **Design tension:** Option C makes the binary a *passive requester* instead of an *active orchestrator*. A hybrid with timeout fallback (5s → `claude --print` subagent) mitigates both concerns.

### Option D: tmux send-keys injection

The binary injects a command into the Claude session's tmux pane.

**Flow:**
1. `agent-doc cleanup <FILE>` looks up the claiming pane for the document
2. Binary uses `tmux send-keys` to inject `/agent-doc callback <FILE> <callback_id>`
3. The skill processes it as a normal invocation

**Pros:** Uses existing tmux infrastructure, skill handles it normally
**Cons:** Fragile (depends on terminal state), can interrupt user typing, security concerns

## Recommended Approach

**Option A (file-based) for v1, with Option C (hook-based) as the upgrade path.**

Option A is buildable today with zero new infrastructure. The latency concern (waiting for next preflight) can be mitigated by having `agent-doc cleanup` trigger a preflight-like cycle itself:

```bash
# User or skill runs:
agent-doc cleanup <FILE>

# Binary:
# 1. Identifies content to summarize
# 2. Writes callback request
# 3. Spawns `claude --print -p "Summarize this exchange: ..."` as fallback
# 4. If running session responds first (via next cycle), use that
# 5. Otherwise, use subagent result
```

This hybrid approach gives immediate results (subagent fallback) while preferring the cheaper in-session path when available.

## Callback Protocol Spec

### Request format
```json
{
  "callback_id": "uuid-v4",
  "type": "summarize",
  "doc_path": "tasks/agent-doc/agent-doc-bugs.md",
  "content": "the exchange content to summarize",
  "max_tokens": 500,
  "created_at": "2026-04-12T01:35:00Z",
  "ttl_seconds": 120
}
```

### Response format
```json
{
  "callback_id": "uuid-v4",
  "result": "summarized content here",
  "model": "claude-opus-4-6",
  "completed_at": "2026-04-12T01:35:05Z"
}
```

### Callback types (extensible)
- `summarize` — compress exchange content
- `pending_curate` — review and clean up pending items
- `status_update` — generate a new status line from current state

## Implementation Tasks

- [ ] Define callback request/response JSON schema in Rust structs
- [ ] Add `.agent-doc/callbacks/` directory management (creation, cleanup, GC)
- [ ] Implement `agent-doc cleanup <FILE>` command (replaces `compact`)
- [ ] Add callback detection to preflight (check for pending requests)
- [ ] Add callback handling to skill workflow (between preflight and response)
- [ ] Implement subagent fallback (`claude --print`) for when no session responds
- [ ] Add TTL-based callback expiry and garbage collection
- [ ] Migrate `compact exchange` runbook to use new cleanup command

## Open Questions

1. Should callbacks be per-document or per-session? (per-document seems cleaner)
2. Should the subagent fallback use `--model haiku` for cost savings on summarization?
3. How to handle callback during an active response cycle? (queue for next cycle)
4. Should `agent-doc cleanup` be blocking or background? (blocking with timeout)
