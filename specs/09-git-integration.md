> Extracted from SPEC.md — see [index](../SPEC.md)

# Git Integration

- Commit: stage the snapshot-selected blob (fallback `git add -f {file}` when needed), hold a blocking advisory lock per resolved git dir / submodule git dir for the short closeout critical section, and retry the full stage+commit transaction when git reports `index.lock` contention. For submodule documents, harness launches must auto-grant access to any external git metadata directories that the lifecycle touches (`.git/modules/...` for the submodule repo and the superproject `.git` for pointer updates).
- Branch: `git checkout -b agent-doc/{filestem}`
- Squash: soft-reset to before first `agent-doc:` commit, recommit as one

## Hook System

Cross-session event coordination via `agent-kit` hooks (v0.3).

**CLI:** `agent-doc hook fire|poll|listen|gc|codex-user-prompt-submit|codex-stop`

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

**Codex bridge:** `agent-doc skill install` now also writes repo-local `.codex/hooks.json` plus `.codex/config.toml` (`[features] codex_hooks = true`). That bridge routes:

- `UserPromptSubmit` → `agent-doc hook codex-user-prompt-submit`
- `Stop` → `agent-doc hook codex-stop`

The Codex stop hook does not replace the documented `finalize` / `write --commit` + `session-check` path. It is a backstop: when Codex reaches `Stop` with an open `agent-doc` cycle, the binary validates `last_assistant_message` before replay. A single assistant closeout may be captured into the existing pending/capture ledger and replayed through the normal recover/write/commit path automatically. Transcript-shaped payloads such as full `agent:exchange` dumps, prompt-target lines, or repeated response headings must not enter the replay ledger; they are captured only as diagnostics and the hook blocks or fails closed instead. If the validated auto-close succeeds, `session-check` should be green and the tracked hook state should be cleared.
