# Commit Boundary

Every appended `agent-doc` response must be committed unless the user explicitly tells you otherwise.

A harness-native `agent-doc` entrypoint (`/agent-doc <FILE>` in Claude Code, `agent-doc <FILE>` in Codex/direct-exec, or an equivalent direct entry in another harness) starts the binary-owned response cycle. It is not permission to patch the document manually and stop short of closeout.

## Default Paths

- **Normal session response:** use `agent-doc finalize <FILE>` with the same write flags you would otherwise pass to `agent-doc write`.
- **Manual repair / missed patchback:** when the user prompt is already present in the document, use `agent-doc write --commit <FILE>`.

## Normal Happy Path

- Finish the turn's requested implementation / verification / build-install work before the response-persistence command. `finalize` is the close-out boundary, not a mid-turn checkpoint.
- The default response-cycle command is `agent-doc finalize <FILE> --baseline-file <preflight.baseline_file> --stream --origin skill`.
- `finalize` is the binary-owned happy path: it writes the response, runs commit, and fails closed unless the cycle reaches `committed`.
- If the turn also includes ordinary repo `commit + push`, keep the active session document out of that manual git commit. Resolve the exact intended non-session path set first, run stage commands only for that set, stop immediately if any stage step fails, verify `git diff --cached --name-only` (or a stricter submodule-pointer inspection) still matches the intended set, then commit only that validated non-session set before `finalize` or `write --commit` creates the session-document closeout commit. Push only after the binary-owned closeout so the response commit is included.
- Use `finalize` for the normal preflight → respond → persist flow across Claude Code, Codex, Cursor, and generic harnesses. Harness-specific command dispatch lives in `harness-invocation.md`.
- For direct-exec harness paths such as Codex, run `agent-doc session-check <FILE>` immediately after the persistence command returns. A nonzero check means the cycle is still open, so do not report success.
- Do **not** describe a normal harness-native `agent-doc` turn as successful while also saying the response is still uncommitted, unless the user explicitly requested that exception.
- After `finalize` returns, do not continue with more long-running task work for that same turn. Only `session-check`, failure recovery, and final reporting should remain.

## Explicit Exceptions

- Bare `agent-doc write` is acceptable only when the user explicitly wants the response left uncommitted, or when you are writing an intermediate checkpoint rather than the final response.
- On real session documents, that path is deliberately nonterminal: the response/capture may be preserved for recovery, but the command now fails closed instead of reporting success while the cycle is still at `response_captured` / `write_applied`.
- If you intentionally leave a response uncommitted, say so clearly and do not describe the cycle as complete.
- `agent-doc write --commit` remains the documented repair path because it preserves the older CLI surface while still crossing the write/commit boundary in one invocation.
- For real session documents (`agent_doc_session` / legacy `session`) that command now fails closed like `finalize`: non-git docs are rejected before mutation, commit errors fail the command, and success means the cycle reached `committed`. Best-effort behavior remains only for non-session docs and `--pending-only` maintenance.
- The same post-write `agent-doc session-check <FILE>` guard applies after manual repair with `agent-doc write --commit`.
- Manual repair uses the same ordering rule: do the repair write-back last, then `session-check`, then stop.

## Anti-Patterns

- Do **not** stop after bare `agent-doc write` for a final response.
- Do **not** patch the assistant response directly into the file when `finalize` or `write --commit` should carry it through the commit boundary.
- Do **not** stage the active session document into an ordinary repo `git commit` before `finalize` / `write --commit`.
- Do **not** continue to `git commit` after a narrowed `git add` / stage failure, and do **not** trust unrelated pre-existing staged entries to "probably be fine" for the same turn.
- Do **not** replace `agent-doc finalize` / `agent-doc commit` with manual `git commit` commands.
