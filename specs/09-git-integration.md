> Extracted from SPEC.md — see [index](../SPEC.md)

# Git Integration

- Commit: stage the snapshot-selected blob (fallback `git add -f {file}` when needed), hold a blocking advisory lock per resolved git dir / submodule git dir for the short closeout critical section, and retry the full stage+commit transaction when git reports `index.lock` contention. For submodule documents, harness launches must auto-grant access to any extra writable roots the lifecycle needs outside the submodule root: the superproject working tree for parent-repo document patchbacks plus any external git metadata directories (the current repo's `.git/modules/...` descendants for nested child submodules, the submodule repo's own external gitdir when applicable, and the superproject `.git` for pointer updates). Harness-specific handling stays the same: both Claude Code and `codex exec` accept `--add-dir`, but `codex exec resume` does not — the Codex backend strips `--add-dir` entries from resume args since the resumed session inherits writable roots from the original `exec`.
- Harness/manual repo commits must keep the active session document on the binary-owned closeout path. When a document prompt includes ordinary repo `commit + push`, agents may commit non-session repo files before closeout, but they must not stage or commit the session document itself until `agent-doc finalize` / `write --commit` performs the response closeout commit. For narrowed path-scoped repo work, agents must resolve the intended non-session path set first, run stage commands only for that set, stop immediately if any stage step fails, prove the staged diff still matches the intended set before `git commit`, and use an explicit pathspec commit or equivalent isolated-index strategy so unrelated pre-staged entries cannot leak into the commit. Push happens after that closeout so the response commit is included.
- Answered-prompt prefix canonicalization must start at the first prompt-like line in the contiguous block above a `### Re:` heading. Earlier assistant tail prose in that same block stays unprefixed even when a later `do #...` / question line in the block is the real answered prompt.
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

**Codex bridge:** `agent-doc skill install` now also writes repo-local `.codex/hooks.json` plus `.codex/config.toml` (`[features] hooks = true`). That bridge routes:

- `UserPromptSubmit` → `agent-doc hook codex-user-prompt-submit`
- `Stop` → `agent-doc hook codex-stop`

The Codex stop hook does not replace the documented `finalize` / `write --commit` + `session-check` path. It is a backstop: when Codex reaches `Stop` with an open `agent-doc` cycle, the binary validates `last_assistant_message` before replay. A single assistant closeout may be captured into the existing pending/capture ledger and replayed through the normal recover/write/commit path automatically. Transcript-shaped payloads such as full `agent:exchange` dumps, prompt-target lines, or repeated response headings must not enter the replay ledger; they are captured only as diagnostics and the hook blocks or fails closed instead. If `last_assistant_message` is empty because a tool-only/authentication step ended the turn before the assistant emitted the final closeout, the hook must also fail closed, save a diagnostic record with the tracked prompt, and require the normal `finalize` / `session-check` recovery path. If the validated auto-close succeeds, `session-check` should be green and the tracked hook state should be cleared.
