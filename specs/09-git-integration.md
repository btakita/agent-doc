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
- Commit: select the snapshot-owned session-document blob, build it in a private index rooted at the exact observed `HEAD`, create the commit tree, and advance `HEAD` with compare-and-swap retries. The user's real index is never the commit source: unrelated staged files remain staged and outside the response commit, while only the committed session document's index entry is aligned afterward. A git-dir advisory lock serializes cooperating agent-doc transactions; the HEAD compare-and-swap protects against unrelated concurrent commits. Before staging the selected blob, commit must check the candidate path against `.gitignore` and skip untracked ignored files with a visible diagnostic rather than silently adding them. Tracked files remain committable. The refusal is unchanged for every document: the ignored file is never staged, the cycle closes as `abandoned` / `commit_refused_ignored_path`, and an undeclared document reports `commit_skipped_ignored_path` plus `commit_error`. A document whose frontmatter declares `agent_doc_scratch: true` (a deliberate scratch/proof document, see [Config](06-config.md#agent_doc_scratch)) reports the same refusal as a clean skip, `commit_skipped_ignored_scratch outcome=clean_skip`, with no `commit_error` (`#splicectlcommit`). The declaration is read from the exact candidate the commit would stage, never from process environment, so the CLI, recovery processes, and the project controller handling a live-editor commit (`commit_document_via_controller` → `controller_commit_document`) reach the same verdict; there is no commit-target override, and the declaration never skips a tracked or non-ignored path. `submodule_pointer_drift` reports no drift for an untracked ignored document: it never crosses the submodule commit boundary, so a stale parent gitlink is not its to own and `agent-doc commit` could never repair it. For submodule documents, commit must also update and commit the parent repository gitlink; strict closeout and `session-check` compare parent `HEAD:<submodule>` with the submodule `HEAD` and fail closed when the inner snapshot is committed but the parent pointer is still stale. Harness launches must auto-grant access to any extra writable roots the lifecycle needs outside the submodule root: the superproject working tree for parent-repo document patchbacks plus any external git metadata directories (the current repo's `.git/modules/...` descendants for nested child submodules, the submodule repo's own external gitdir when applicable, and the superproject `.git` for pointer updates). Harness-specific handling stays the same: both Claude Code and `codex exec` accept `--add-dir`, but `codex exec resume` does not — the Codex backend strips `--add-dir` entries from resume args since the resumed session inherits writable roots from the original `exec`.
- Post-closeout publication is a default-off, best-effort effect owned by the
  commit coordinator after the local closeout transaction has completed. With
  `[commit] push = "ff-only"` in `.agent-doc/config.toml`, it resolves the
  current branch's configured upstream and pushes exactly
  `HEAD:<upstream-branch>` with force, mirror, tags, and recursive-submodule
  pushing disabled. Detached HEAD, missing/invalid upstream configuration,
  launch failure, and remote/non-fast-forward rejection are typed advisory
  outcomes: they are logged and shown as warnings, and never roll back or
  invalidate the already committed cycle. The implicit/default `"off"` policy
  performs no Git observation or remote mutation.
- Harness/manual repo commits must keep the active session document on the binary-owned closeout path. When a document prompt includes ordinary repo `commit + push`, agents may commit non-session repo files before closeout, but they must not stage or commit the session document itself until `agent-doc finalize` / `write --commit` performs the response closeout commit. For narrowed path-scoped repo work, agents must resolve the intended non-session path set first, run stage commands only for that set, stop immediately if any stage step fails, prove the staged diff still matches the intended set before `git commit`, and use an explicit pathspec commit or equivalent isolated-index strategy so unrelated pre-staged entries cannot leak into the commit. Push happens after that closeout so the response commit is included. `session-check` must warn on likely partial manual staging closeouts: if the latest repo commit (or dirty submodule repo surfaced by the owning worktree) changes relevant source/test paths and tracked dirty or staged companion changes remain with overlapping changed string literals, the closeout is suspect because local verification may have run against the dirty worktree while CI sees only the committed tree.
- Preflight owns lossless recovery of an abandoned prompt cycle whose queue/backlog mirror already became the exact snapshot before stale-cycle repair (`#abandonedpromptcommit`). When the terminal cycle has prompt targets but no response capture, no response heading was added relative to `HEAD`, and the snapshot introduces a new queue id, the next harness-native invocation must run the binary-owned document commit in place and continue to that drainable queue head. It must not stop with operator instructions to run `agent-doc commit` plus `session-check`. Mixed drift containing an assistant response, a capture, pending response mutations, no new queue id, or a live document that no longer equals the snapshot remains fail-closed.
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
- `PostToolUse` → `agent-doc hook steering-post-tool-use`
- `PreToolUse` → `agent-doc hook coined-id-pre-tool-use` (the live-tmux-server and coined-id guards, `#tmuxguardclaudeinstall`; the Claude install wires the same command into the project's `.claude/settings.json` and, on a bare-root install, alone into user-level `~/.claude/settings.json`. OpenCode and Cursor user-layer guards are described in spec 10, `#tmuxguardclaudeuser`)

The Codex Stop payload's `last_assistant_message` and `stop_hook_active` accept `null` as their defaults (`#stopnullmessage`). Codex sends `"last_assistant_message": null` when a turn ends on a tool call with no final text; a missing field was tolerated but an explicit `null` failed the whole payload as `parse stop JSON`, a fail-closed stop with no document or recovery guidance, and the hook's `missing_last_assistant_message` path for exactly that tool-only stop could never run.

In one binary-owned invocation, `UserPromptSubmit` first makes session tracking durable and then runs preflight for a valid trigger. Its stdout cycle contract ends with the admission success marker and an explicit directive to continue in the current turn without shell-running `agent-doc <FILE>`; Codex injects that context into the arriving turn. Tracking or preflight failures emit diagnostics without the marker. It recognizes both bare `agent-doc <FILE>` reopen prompts and session invocations with same-line or following-line directive bodies, such as `agent-doc <FILE> #code-review` or `agent-doc <FILE>` followed by `do #id ...`. A bare invocation with no body remains a no-op when the document snapshot is unchanged unless an active `agent:run` supplies the next prompt. Tracked directive bodies and active queue head prompts are exposed to preflight/plan as synthetic prompt diffs so they cannot be lost behind `no_changes=true`.

**Claude queue-continuation bridge:** the Claude `UserPromptSubmit` hook stores an
exact `session_id` → document binding before it admits an agent-doc cycle. A
successful `/loop agent-doc <FILE>` admission atomically refreshes the
`claude_loop` drain-owner lease before preflight reconciles the prior closeout;
lease ownership must not depend on the model remembering a separate shell
command. The installed Claude `Stop` hook consults that exact-session binding,
the durable continuation marker (or session-check stall projection), and the
shared live continuation detector. When a clean closeout still has a drainable
head, the first Stop returns `decision=block` with the exact head and directs the
same Claude session through the loop skill. A `stop_hook_active=true` re-entry
must pass through to avoid recursive hook loops; the route supervisor remains
the bounded fallback. Missing binding or missing continuation proof is a no-op,
so unrelated Claude sessions never inherit ambient queue work.

The admission invariant is one harness-native prompt, admitted once, in its owning pane. The user-level hook is the policy owner; the process-entry recursion guard remains a late imperative exception for genuine nested process starts. Required transitions are:

| Input state | Transition |
| --- | --- |
| User-level hook admits the prompt | Continue the current model turn in-pane; do not spawn `agent-doc <FILE>`. |
| Hook tracking or preflight fails | Emit no success marker and fail admission closed. |
| A real nested process reaches the same owning pane | Reject it with the recursive-direct-invocation guard; do not claim the unresolved prompt was handled. |

Regression coverage must include the full follow-up lifecycle: a routed dispatch leaves a prompt-bearing user edit on top of a committed document, `finalize` or `compact --commit` responds through the normal binary closeout path, and the next `preflight` reports `no_changes=true` without stale prompt-cycle repair, prior-patchback, or snapshot/head guard mismatch diagnostics.

The Codex stop hook does not replace the documented `finalize` / `write --commit` + `session-check` path. It is a backstop for the exact Codex `session_id` that `UserPromptSubmit` durably bound to the document. When Codex reaches `Stop` with an open bound `agent-doc` cycle, the binary validates `last_assistant_message` before replay. A single assistant closeout may be captured into the existing pending/capture ledger and replayed through the normal recover/write/commit path automatically. Transcript-shaped payloads such as full `agent:exchange` dumps, prompt-target lines, or repeated response headings must not enter the replay ledger; they are captured only as diagnostics and the hook blocks or fails closed instead. If `last_assistant_message` is empty because a tool-only/authentication step ended the turn before the assistant emitted the final closeout, the hook must also fail closed, save a diagnostic record with the tracked prompt, and require the normal `finalize` / `session-check` recovery path. If the exact session has no tracked binding, the hook is a no-op even when another document in the project has a durable queue marker. If the validated auto-close succeeds, `session-check` should be green and the tracked hook state should be cleared.

When capture publication wakes the `captured_finalize_resume` worker before the
Stop hook's own repair attempt acquires the closeout lease, the resulting
`closeout_owner_retry_at` receipt is a binary-owned deferral, not a second
unfinished-cycle failure. The hook preserves the retained capture, leaves the
controller as the sole delivery owner, and subscribes to the exact cycle's
controller projection before returning a Stop decision. A terminal or
superseded projection is re-inspected; an owner-release projection permits one
exact keyed resume before re-inspection. The live lease alone must not surface
as `Blocked by hook`, and the hook must not append stale queue-guard or manual
`repair`/`finalize` guidance to that handoff. A bounded wait with no progress,
an observation failure, or an explicit `needs_operator` result remains
fail-closed. Without retained-capture ownership, the same lease receipt remains
fail-closed. This policy applies whether the lease refusal is observed during
repair/replay or at the final closeout/commit boundary after the response was
already materialized; those phases must not classify the same owned lease
differently.

A refused UserPromptSubmit admission never binds that Codex thread to the
document. Its refusal receipt fences the matching Stop invocation, which then
retires the denied binding without inspecting, capturing, repairing, or
committing the registered owner's cycle. An ordinary later prompt cannot replace
the refusal with active document debt; only a new explicit `agent-doc <FILE>`
trigger may attempt admission again. Consequently Stop must never turn a pane
authority refusal into an instruction to run `agent-doc commit` from the same
non-owning pane.

**Same-turn repeat during the turn's own closeout (`#fixrunturn`).** Codex
steers a message typed into a busy pane into the running turn, so an operator's
Run Agent Doc can deliver the exact trigger (same session, turn, prompt, and
document) to a turn that already holds admission. When the document cycle is
still `preflight_started` with nothing captured, the hook reuses the admission
(`kind: same_turn_repeat`). When that turn's own closeout is in flight (cycle
`response_captured` or `write_applied`), the hook coalesces the repeat
(`kind: same_turn_repeat_closeout_in_flight`, ops-log
`codex_same_turn_repeat_coalesced_into_closeout`): it emits the admission
marker with a directive to let the closeout finish and not start a second
response, and it leaves the exact-thread binding untouched. It must not run a
fresh preflight there: that preflight refuses against the turn's own open cycle
("previous cycle ... is still `write_applied`"), and its refusal receipt makes
Stop retire the binding without the queue continuation, so the operator's
pending prompt is never answered (infra.md, 2026-10-12). Because the binding
keeps its admission, Stop re-dispatches the pending prompt after the closeout
commits through its ordinary ladder: auto-queue continuation, or the
active-session-prompt writeback block. A terminal cycle is not a repeat shape;
the ordinary admission path admits a new cycle there.

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
same turn still owes document work. Frontmatter `queue: pause` explicitly parks
that head, so the Stop hook must agree with `session-check`'s
`no_drainable_work` outcome, clear the tracked hook state, and return
`continue=true`. The pending-work guard also shares the queue policy's eligibility
predicate: deferred review mirrors, unmet dependencies, and head fences cannot
create fresh response debt. An eligible manual queue prompt remains a writeback
guard without granting auto activation; a deferred head does not hide eligible
work behind it. Inline operator verdicts follow the same shared policy.

When the repo-local Codex config registers the `agent-doc` MCP server, Stop-hook queue-continuation blocks must prefer the MCP tool path for the active turn: `agent_doc_admit`, `agent_doc_plan` / `agent_doc_read` as needed, `agent_doc_finalize` for the strict write/commit closeout, and `agent_doc_session_check` for the final gate. If MCP tools are unavailable in the current Codex run, the same block must retain the in-pane CLI fallback (`agent-doc finalize <FILE>` or `agent-doc write --commit <FILE>`) and must still forbid running `agent-doc <FILE>` from the owner pane. Claude, OpenCode, and direct CLI flows remain unchanged.

When the Codex Stop hook blocks to continue an active go-mode `agent:run` head, it must also write file-scoped ops-log proof with `codex_stop_queue_continuation`, the continuation source (`tracked_state`), MCP availability, prompt byte count, and prompt hash. The hook must not log the raw prompt body.

When the Stop hook recovers a direct Codex/chat response for a session document, strict closeout still owns every git layer. If the response is committed in a submodule but the parent gitlink update fails, the hook must block the turn with the `session-check`/`agent-doc commit <FILE>` recovery hint, leave hook tracking in place for retry, and avoid reporting `continue=true` until the parent pointer commit is complete. If an earlier strict-closeout invariant fails before the submodule commit advances, the hook must still block and preserve tracking for retry, but parent gitlink drift is not required because the parent may still match the unadvanced submodule `HEAD`.
