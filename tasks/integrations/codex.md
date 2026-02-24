# Codex Frontend Skill — agent-doc

## Status

Research complete. Not yet implemented.

## Problem

agent-doc only has a Claude backend. Need a Codex backend following the same pattern.

## Plan

### 1. Create `agent-doc/src/agent/codex.rs`

Implement the `Agent` trait for Codex CLI:
- Non-interactive: `codex exec --json --full-auto -` (prompt via stdin)
- Session resume: `codex exec resume <session_id>` (vs Claude's `--resume`)
- Output: JSONL stream (vs Claude's single JSON blob)
- Extract `thread_id` from `thread.started` event, response text from `item.completed` events
- System prompt: Prepend instructions to user prompt (no `--append-system-prompt` equivalent)

### 2. Update `agent-doc/src/agent/mod.rs`

- Add `pub mod codex;`
- Add `"codex"` match arm in `resolve()`

### 3. Update `agent-doc/CLAUDE.md`

Add `codex.rs` to module layout diagram under `agent/`.

## Key Differences: Claude vs Codex

| Feature | Claude CLI | Codex CLI |
|---------|-----------|-----------|
| Non-interactive | `claude -p` | `codex exec` |
| JSON output | single JSON blob | JSONL stream |
| Session resume | `--resume <id>` | `codex exec resume <id>` |
| Session fork | `--continue --fork` | not available in exec mode |
| Response field | `.result` | `item.completed` events |
| Session ID | `.session_id` | `thread.started.thread_id` |

## Config

Recommended `~/.config/agent-doc/config.toml`:

```toml
[agents.codex]
command = "codex"
args = ["exec", "--json", "--full-auto"]
```

## Risks

1. JSONL schema uncertainty — exact `item.completed` structure needs verification against real output
2. Session fork not available in exec mode — ignore `fork` parameter for Codex
3. No `--append-system-prompt` — mitigated by prepending instructions to user prompt
4. Codex not installed — produce clear error message
