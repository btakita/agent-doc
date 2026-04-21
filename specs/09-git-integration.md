> Extracted from SPEC.md — see [index](../SPEC.md)

# Git Integration

- Commit: `git add -f {file}` (bypasses .gitignore) + `git commit -m "agent-doc: {timestamp}" --no-verify`
- Branch: `git checkout -b agent-doc/{filestem}`
- Squash: soft-reset to before first `agent-doc:` commit, recommit as one

## Hook System

Cross-session event coordination via `agent-kit` hooks (v0.3).

**CLI:** `agent-doc hook fire|poll|listen|gc`

- `fire <EVENT> <FILE>` — write event JSON to `.agent-doc/hooks/<event>/`, auto-reads session ID from frontmatter
- `poll <EVENT> [--since SECS]` — read events newer than timestamp, clean expired
- `listen [--root PATH]` — start Unix socket listener at `.agent-doc/hooks.sock`
- `gc [--root PATH]` — clean expired events across all hooks

**Lifecycle hooks fired by agent-doc:**
- `post_write` — after IPC write succeeds (from `write.rs`)
- `post_commit` — after successful git commit (from `git.rs`)
- `claim` / `layout_change` — available via CLI, not yet wired into binary paths

**Transport:** `HookTransport` trait with `FileTransport` (default), `SocketTransport` (Unix socket), `ChainTransport` (fallback chain). Socket transport connects to `.agent-doc/hooks.sock` and expects `ok\n` ack.

**Claude Code bridge:** Add `PostToolUse` hook to `settings.json`:
```json
{"hooks":{"PostToolUse":[{"matcher":"Write|Edit","command":"agent-doc hook fire post_write \"$TOOL_INPUT_FILE\""}]}}
```
