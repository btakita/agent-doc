# Harness Invocation Patterns

This runbook covers the harness-specific differences in how agent-doc is invoked.
The core workflow (preflight, respond, persist the response) is identical across all harnesses; see `commit.md` for the shared commit-boundary contract.

## Directive Semantics

- Imperative user edits inside an `agent-doc` session document are executable directives, not just topics to comment on.
- `do #qj5w now`, `fix this`, `run tests`, `build + install`, `commit + push`, and similar document edits authorize the same underlying repo work they would authorize in chat.
- The agent should either perform that work before `finalize` / `write --commit`, or stop on a concrete blocker. Do **not** emit status-only progress prose while doing neither.

## Post-Preflight Planning

- After `agent-doc preflight <FILE>`, the next dispatch step should consume a binary-owned planning record rather than improvise from raw prose alone.
- Run `agent-doc plan <FILE>` and execute the cycle from its `prompt_targets`, `repo_actions`, `required_commands`, `pending_mutations`, `handoff`, and `blockers`.
- If the plan says `handoff=orchestrate`, run the emitted `agent-doc orchestrate ...` command before attempting a manual response.

## Manual Repair Default

- For both **Claude Code** and **Codex**, the default documented manual-repair path is `agent-doc write --commit <FILE>` once the user prompt is already present in the document.
- Do **not** document or follow a manual-repair flow that stops after bare `agent-doc write`; that leaves the response on the wrong side of the commit boundary.
- If the user prompt itself is missing, insert that prompt into `exchange` first, then return to `agent-doc write --commit <FILE>` for the assistant response.

## Response Header Attribution

- Always attribute `### Re:` headings with the resolved model short name, for example `### Re: topic — gpt-5` or `### Re: topic — opus-4-6`.
- Never use the harness label as the suffix. `### Re: topic — codex` and `### Re: topic — claude` are wrong.

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
- **Write-back:** Pipe the normal response cycle via `Bash` using `agent-doc finalize ...`; use `agent-doc write --commit ...` only for manual repair when the prompt already exists.
- **Manual repair / missed patchback:** Use the shared default above. In Claude Code that still means piping the response through `Bash`, but the command should be `agent-doc write --commit <FILE>` once the prompt already exists in the document.
- **Built-in commands** (e.g., `/compact`, `/clear`): Cannot invoke via Skill. Write a document note instructing the user to run it at the terminal.

## Codex

- **Invocation:** Direct prompt injection or user instruction referencing the document. Do **not** type `/agent-doc`; Codex CLI reserves leading `/` for its own built-in slash commands and will reject project-defined `/agent-doc`.
- **Use this form instead:** `agent-doc <FILE>` as a normal message, for example `agent-doc tasks/agent-doc/agent-doc-bugs.md`.
- **Slash commands:** Codex has no slash commands. If preflight returns `slash_commands`, skip them. If `builtin_commands`, write a document note.
- **Auto-update prompt:** Print a message asking the user to restart.
- **Installed hook backstop:** `agent-doc skill install` also writes `.codex/hooks.json` and enables `features.codex_hooks = true` in `.codex/config.toml`. The installed commands are `agent-doc hook codex-user-prompt-submit` for `UserPromptSubmit` and `agent-doc hook codex-stop` for `Stop`. `UserPromptSubmit` tracks the active document for the Codex session across nested `.agent-doc` roots in the same workspace; `Stop` first tries to finish the response cycle deterministically from `last_assistant_message`, but only when that payload validates as a single assistant closeout. Transcript-shaped payloads (for example full `agent:exchange` dumps, prompt-target lines, or repeated response headings) are blocked and saved only for diagnostics instead of being replayed, even when the stop arrives on a later turn in that same Codex session.
- **Ordering:** finish the turn's requested coding / testing / build-install work before the response persistence command. Do not patch the document early and then keep working for the same turn.
- **Write-back:** Execute `agent-doc finalize` directly for the normal response cycle (Codex runs shell commands natively), then immediately run `agent-doc session-check <FILE>`.
- **Fail closed:** If `agent-doc session-check <FILE>` exits nonzero after write-back, the cycle is still open. Do **not** report success or stop; continue recovery instead.
- **Manual repair / missed patchback:** Use the shared default above. Do **not** patch the assistant response directly into the file. After `agent-doc write --commit <FILE>`, run the same `agent-doc session-check <FILE>` guard before ending the turn. That repair write-back should also be the last substantial action of the turn.
- **Session resume:** Codex uses `codex resume --last` instead of `--continue`.

## Cursor / Generic

- **Invocation:** Follow the same pattern as Codex (direct execution).
- **Slash commands:** Not available. Skip `slash_commands`; note `builtin_commands`.
- **Write-back:** Execute `agent-doc finalize` directly for the normal response cycle.
