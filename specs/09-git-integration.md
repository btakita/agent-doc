> Extracted from SPEC.md — see [index](../SPEC.md)

# Git Integration

- With opt-in per-component convergence, pre-stage commit rebases the
  binary-owned snapshot onto current Lazily/editor authority only after every
  component named by exact retained expected→target transitions matches between
  the snapshot and that authority cut. The current cut then becomes the private
  index candidate, preserving unowned operator edits byte-for-byte. Owned
  mismatch fails closed. Missing legacy expectations or malformed transitions
  use the whole-document fallback. This is not authority to merge a disk
  candidate under a live editor.
- Commit: select the snapshot-owned session-document blob, build it in a private index rooted at the exact observed `HEAD`, create the commit tree, and advance `HEAD` with compare-and-swap retries. The user's real index is never the commit source: unrelated staged files remain staged and outside the response commit, while only the committed session document's index entry is aligned afterward. A git-dir advisory lock serializes cooperating agent-doc transactions; the HEAD compare-and-swap protects against unrelated concurrent commits. Before staging the selected blob, commit must check the candidate path against `.gitignore` and skip untracked ignored files with a visible diagnostic rather than silently adding them. Tracked files remain committable. For submodule documents, commit must also update and commit the parent repository gitlink; strict closeout and `session-check` compare parent `HEAD:<submodule>` with the submodule `HEAD` and fail closed when the inner snapshot is committed but the parent pointer is still stale. Harness launches must auto-grant access to any extra writable roots the lifecycle needs outside the submodule root: the superproject working tree for parent-repo document patchbacks plus any external git metadata directories (the current repo's `.git/modules/...` descendants for nested child submodules, the submodule repo's own external gitdir when applicable, and the superproject `.git` for pointer updates). Harness-specific handling stays the same: both Claude Code and `codex exec` accept `--add-dir`, but `codex exec resume` does not — the Codex backend strips `--add-dir` entries from resume args since the resumed session inherits writable roots from the original `exec`.
- Harness/manual repo commits must keep the active session document on the binary-owned closeout path. When a document prompt includes ordinary repo `commit + push`, agents may commit non-session repo files before closeout, but they must not stage or commit the session document itself until `agent-doc finalize` / `write --commit` performs the response closeout commit. For narrowed path-scoped repo work, agents must resolve the intended non-session path set first, run stage commands only for that set, stop immediately if any stage step fails, prove the staged diff still matches the intended set before `git commit`, and use an explicit pathspec commit or equivalent isolated-index strategy so unrelated pre-staged entries cannot leak into the commit. Push happens after that closeout so the response commit is included. `session-check` must warn on likely partial manual staging closeouts: if the latest repo commit (or dirty submodule repo surfaced by the owning worktree) changes relevant source/test paths and tracked dirty or staged companion changes remain with overlapping changed string literals, the closeout is suspect because local verification may have run against the dirty worktree while CI sees only the committed tree.
- Answered-prompt prefix canonicalization must start at the first prompt-like line in the contiguous block above a `### Re:` heading. Earlier assistant tail prose in that same block stays unprefixed even when a later `do #...` / question line in the block is the real answered prompt. Markdown list items inside the answered prompt block stay bare; the binary must not rewrite bullets or ordered-list lines as `❯ - ...` / `❯ 1. ...`.
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

**Codex bridge (`#codex-user-hook-owner`):** `agent-doc skill install` writes `UserPromptSubmit` / `Stop` once into user-level `$CODEX_HOME/hooks.json` and writes repo-local `.codex/config.toml` (`[features] hooks = true` and `[mcp_servers.agent-doc] command = "agent-doc"`, `default_tools_approval_mode = "approve"`, `args = ["mcp", "serve"]`). Codex composes user and project hook layers, so reinstall retires only agent-doc's managed lifecycle commands from an existing project `.codex/hooks.json`, preserving unrelated hooks and preventing duplicate preflight. The user-level owner remains discoverable when nested Git roots, submodules, or worktrees hide a project hook. The MCP args carry **no** `--project-root` (`#skillinstallconfigpath`): `.codex/config.toml` is project-local and usually tracked, so an absolute install-machine path made it churn per operator, and `agent-doc mcp serve` walks up from its own working directory to the nearest project instead. Reinstalling migrates a legacy absolute entry away. Installed JSON/TOML artifacts end with a newline and are rewritten only on a real content change. The approval mode must be server-level so newly installed agent-doc binaries and newly exposed MCP tools do not stall active Codex turns for repeated agent-doc MCP permission prompts. That bridge routes:

- `UserPromptSubmit` → `agent-doc hook codex-user-prompt-submit`
- `Stop` → `agent-doc hook codex-stop`

In one binary-owned invocation, `UserPromptSubmit` first makes session tracking durable and then runs preflight for a valid trigger. Its stdout cycle contract ends with the admission success marker and an explicit directive to continue in the current turn without shell-running `agent-doc <FILE>`; Codex injects that context into the arriving turn. Tracking or preflight failures emit diagnostics without the marker. It recognizes both bare `agent-doc <FILE>` reopen prompts and session invocations with same-line or following-line directive bodies, such as `agent-doc <FILE> #code-review` or `agent-doc <FILE>` followed by `do #id ...`. A bare invocation with no body remains a no-op when the document snapshot is unchanged unless an active `agent:queue` supplies the next prompt. Tracked directive bodies and active queue head prompts are exposed to preflight/plan as synthetic prompt diffs so they cannot be lost behind `no_changes=true`.

The admission invariant is one harness-native prompt, admitted once, in its owning pane. The user-level hook is the policy owner; the process-entry recursion guard remains a late imperative exception for genuine nested process starts. Required transitions are:

| Input state | Transition |
| --- | --- |
| User-level hook admits the prompt | Continue the current model turn in-pane; do not spawn `agent-doc <FILE>`. |
| Hook tracking or preflight fails | Emit no success marker and fail admission closed. |
| A real nested process reaches the same owning pane | Reject it with the recursive-direct-invocation guard; do not claim the unresolved prompt was handled. |

Regression coverage must include the full follow-up lifecycle: a routed dispatch leaves a prompt-bearing user edit on top of a committed document, `finalize` or `compact --commit` responds through the normal binary closeout path, and the next `preflight` reports `no_changes=true` without stale prompt-cycle repair, prior-patchback, or snapshot/head guard mismatch diagnostics.

The Codex stop hook does not replace the documented `finalize` / `write --commit` + `session-check` path. It is a backstop for the exact Codex `session_id` that `UserPromptSubmit` durably bound to the document. When Codex reaches `Stop` with an open bound `agent-doc` cycle, the binary validates `last_assistant_message` before replay. A single assistant closeout may be captured into the existing pending/capture ledger and replayed through the normal recover/write/commit path automatically. Transcript-shaped payloads such as full `agent:exchange` dumps, prompt-target lines, or repeated response headings must not enter the replay ledger; they are captured only as diagnostics and the hook blocks or fails closed instead. If `last_assistant_message` is empty because a tool-only/authentication step ended the turn before the assistant emitted the final closeout, the hook must also fail closed, save a diagnostic record with the tracked prompt, and require the normal `finalize` / `session-check` recovery path. If the exact session has no tracked binding, the hook is a no-op even when another document in the project has a durable queue marker. If the validated auto-close succeeds, `session-check` should be green and the tracked hook state should be cleared.

Prompt writeback debt is cycle-scoped. `UserPromptSubmit` records the cycle ID and
whether that cycle was open when it observed the prompt. Once that same cycle has
a captured response and reaches `committed`, the Stop hook must retire the prompt
debt even when a shortened console handoff does not repeat the prompt text. A
prompt first observed after the cycle is terminal remains new work and must still
cross a binary-owned response boundary. When that exact-thread prompt has a
replayable Stop payload, the hook must mint a fresh cycle from `HEAD` before it
captures the payload, then drive the normal write/commit path. It must never
attach a new response capture to the preceding terminal cycle. Legacy bindings without
the cycle observation may use strict cycle start / prompt observation / commit
timestamp ordering as compatibility proof.

After a cycle is committed, a visible queue line is not by itself proof that the
same turn still owes document work. Frontmatter `queue: stop` explicitly parks
that head, so the Stop hook must agree with `session-check`'s
`no_drainable_work` outcome, clear the tracked hook state, and return
`continue=true`. A manual queue head without an explicit stop remains a
writeback guard for chat-only responses, and active go/auto queue continuation
is unchanged.

When the repo-local Codex config registers the `agent-doc` MCP server, Stop-hook queue-continuation blocks must prefer the MCP tool path for the active turn: `agent_doc_admit`, `agent_doc_plan` / `agent_doc_read` as needed, `agent_doc_finalize` for the strict write/commit closeout, and `agent_doc_session_check` for the final gate. If MCP tools are unavailable in the current Codex run, the same block must retain the in-pane CLI fallback (`agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>`) and must still forbid running `agent-doc <FILE>` from the owner pane. Claude, OpenCode, and direct CLI flows remain unchanged.

When the Codex Stop hook blocks to continue an active go-mode `agent:queue` head, it must also write file-scoped ops-log proof with `codex_stop_queue_continuation`, the continuation source (`tracked_state`), MCP availability, prompt byte count, and prompt hash. The hook must not log the raw prompt body.

When the Stop hook recovers a direct Codex/chat response for a session document, strict closeout still owns every git layer. If the response is committed in a submodule but the parent gitlink update fails, the hook must block the turn with the `session-check`/`agent-doc commit <FILE>` recovery hint, leave hook tracking in place for retry, and avoid reporting `continue=true` until the parent pointer commit is complete. If an earlier strict-closeout invariant fails before the submodule commit advances, the hook must still block and preserve tracking for retry, but parent gitlink drift is not required because the parent may still match the unadvanced submodule `HEAD`.
