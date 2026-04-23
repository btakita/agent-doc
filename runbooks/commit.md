# Commit Boundary

Every appended `agent-doc` response must be committed unless the user explicitly tells you otherwise.

## Default Paths

- **Normal session response:** use `agent-doc finalize <FILE>` with the same write flags you would otherwise pass to `agent-doc write`.
- **Manual repair / missed patchback:** when the user prompt is already present in the document, use `agent-doc write --commit <FILE>`.

## Normal Happy Path

- Finish the turn's requested implementation / verification / build-install work before the response-persistence command. `finalize` is the close-out boundary, not a mid-turn checkpoint.
- The default response-cycle command is `agent-doc finalize <FILE> --baseline-file <preflight.baseline_file> --stream --origin skill`.
- `finalize` is the binary-owned happy path: it writes the response, runs commit, and fails closed unless the cycle reaches `committed`.
- Use `finalize` for the normal preflight → respond → persist flow across Claude Code, Codex, Cursor, and generic harnesses. Harness-specific command dispatch lives in `harness-invocation.md`.
- For direct-exec harness paths such as Codex, run `agent-doc session-check <FILE>` immediately after the persistence command returns. A nonzero check means the cycle is still open, so do not report success.
- After `finalize` returns, do not continue with more long-running task work for that same turn. Only `session-check`, failure recovery, and final reporting should remain.

## Explicit Exceptions

- Bare `agent-doc write` is acceptable only when the user explicitly wants the response left uncommitted, or when you are writing an intermediate checkpoint rather than the final response.
- If you intentionally leave a response uncommitted, say so clearly and do not describe the cycle as complete.
- `agent-doc write --commit` remains the documented repair path because it preserves the older CLI surface while still crossing the write/commit boundary in one invocation.
- The same post-write `agent-doc session-check <FILE>` guard applies after manual repair with `agent-doc write --commit`.
- Manual repair uses the same ordering rule: do the repair write-back last, then `session-check`, then stop.

## Anti-Patterns

- Do **not** stop after bare `agent-doc write` for a final response.
- Do **not** patch the assistant response directly into the file when `finalize` or `write --commit` should carry it through the commit boundary.
- Do **not** replace `agent-doc finalize` / `agent-doc commit` with manual `git commit` commands.
