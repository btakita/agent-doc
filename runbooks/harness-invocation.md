# Harness Invocation Patterns

This runbook covers the harness-specific differences in how agent-doc is invoked.
The core workflow (preflight, respond, write, commit) is identical across all harnesses.

## Harness Detection

Identify your harness from your environment:

| Signal | Harness |
|--------|---------|
| You have a `Skill` tool and slash commands (`/agent-doc`) | **Claude Code** |
| You are Codex CLI / `CODEX_CLI` env var is set | **Codex** |
| You are Cursor / `CURSOR_SESSION_ID` env var is set | **Cursor** |
| None of the above | **Generic** |

## Claude Code

- **Invocation:** User types `/agent-doc <file>` which triggers the `Skill` tool.
- **Slash commands:** Execute via the `Skill` tool. Strip the leading `/`; pass remaining args.
- **Auto-update prompt:** Use `AskUserQuestion` to prompt the user to run `/compact`.
- **Write-back:** Pipe via `Bash` tool using heredoc (`cat <<'RESPONSE' | agent-doc write ...`).
- **Built-in commands** (e.g., `/compact`, `/clear`): Cannot invoke via Skill. Write a document note instructing the user to run it at the terminal.

## Codex

- **Invocation:** Direct prompt injection or user instruction referencing the document. Do **not** type `/agent-doc`; Codex CLI reserves leading `/` for its own built-in slash commands and will reject project-defined `/agent-doc`.
- **Use this form instead:** `agent-doc <FILE>` as a normal message, for example `agent-doc tasks/agent-doc/agent-doc-bugs.md`.
- **Slash commands:** Codex has no slash commands. If preflight returns `slash_commands`, skip them. If `builtin_commands`, write a document note.
- **Auto-update prompt:** Print a message asking the user to restart.
- **Write-back:** Execute `agent-doc write` directly (Codex runs shell commands natively).
- **Manual repair / missed patchback:** If the prompt is already in the document and you are repairing a missed assistant response, use `agent-doc write --commit <FILE>` for the response itself. Do **not** patch the assistant response directly into the file. If the user prompt is missing, insert that prompt into `exchange` first, then return to `agent-doc write --commit <FILE>` for the response path.
- **Session resume:** Codex uses `codex resume --last` instead of `--continue`.

## Cursor / Generic

- **Invocation:** Follow the same pattern as Codex (direct execution).
- **Slash commands:** Not available. Skip `slash_commands`; note `builtin_commands`.
- **Write-back:** Execute shell commands directly.
