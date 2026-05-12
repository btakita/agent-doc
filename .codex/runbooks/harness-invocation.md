# Harness Invocation Patterns

This runbook covers the harness-specific differences in how agent-doc is invoked.
The core workflow (preflight, respond, persist the response) is identical across all harnesses; see `commit.md` for the shared commit-boundary contract.

## Harness-Native Entrypoints

- A harness-native `agent-doc` invocation is an executable workflow entry, not a generic request to edit the markdown file by hand.
- Claude Code `/agent-doc <FILE>`, Codex `agent-doc <FILE>`, OpenCode `/agent-doc <FILE>`, and equivalent direct-entry forms all start the same binary-owned response cycle.
- Do **not** treat that turn as successful until the response crosses `agent-doc finalize <FILE>` for the normal path or `agent-doc write --commit <FILE>` for a repair path.
- Do **not** end a normal harness-native `agent-doc` turn with "not committed" or equivalent wording unless the user explicitly asked to leave the response uncommitted.
- MCP auth / OAuth steps are part of that same turn. If a tool pauses for authentication or browser approval, resume the managed response afterward and still finish with `finalize` / `write --commit` plus `session-check`; the auth step is not the success boundary.

## Directive Semantics

- Imperative user edits inside an `agent-doc` session document are executable directives, not just topics to comment on.
- `do #qj5w now`, `fix this`, `run tests`, `build + install`, `commit + push`, and similar document edits authorize the same underlying repo work they would authorize in chat.
- If that work includes an ordinary repo `commit + push`, the manual repo commit must exclude the active session document. Resolve the intended non-session path set first, run stage commands only for that set, stop immediately on any stage failure, verify the staged diff still matches the intended set, then commit only that validated set. `agent-doc finalize` / `write --commit` still owns the session-document closeout commit, and the push happens after that closeout so the response commit is included.
- The agent should either perform that work before `finalize` / `write --commit`, or stop on a concrete blocker. Do **not** emit status-only progress prose while doing neither.
- **Direct-chat preset write-back:** when a session-document preset (for example `#commit-push`, `#spec-test-build-install-commit-push`) triggers repo work through a direct chat turn rather than through `agent-doc finalize`, the turn is not complete until the response is written into the session document with `agent-doc write --commit <FILE>` and `agent-doc session-check <FILE>` passes. Completing side-effect repo work (commit, push) in chat without writing the response patchback is the same closeout violation as skipping `finalize`. The Codex Stop hook cannot auto-close a plain-text response that lacks `<!-- patch:exchange -->` blocks, so the agent must explicitly write back before reporting success.

## Post-Preflight Planning

- After `agent-doc preflight <FILE>`, the next dispatch step should consume a binary-owned planning record rather than improvise from raw prose alone.
- Run `agent-doc plan <FILE>` and execute the cycle from its `prompt_targets`, `repo_actions`, `required_commands`, `pending_mutations`, `handoff`, and `blockers`.
- If the plan says `handoff=orchestrate`, run the emitted `agent-doc orchestrate ...` command before attempting a manual response.

## Manual Repair Default

- For **Claude Code**, **Codex**, and **OpenCode**, the default documented manual-repair path is `agent-doc write --commit <FILE>` once the user prompt is already present in the document.
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
| You are OpenCode / `OPENCODE` env var is set | **OpenCode** |
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
- **Entry semantics:** once that message is accepted, the turn is inside the binary-owned response cycle. Do **not** downgrade it into a manual document-editing task or report a useful-but-uncommitted success state.
- **Direct prompt body:** extra chat text after that invocation still counts. For example, `agent-doc tasks/agent-doc/agent-doc-bugs.md #agent-doc-bug` or a following-line `do #id ...` prompt is now captured by the Codex hook and reused by `preflight` / `plan` even when the document itself has no new diff. Bare `agent-doc <FILE>` with no trailing body remains a no-op until the document changes.
- **Slash commands:** Codex has no slash commands. If preflight returns `slash_commands`, skip them. If `builtin_commands`, write a document note.
- **Auto-update prompt:** Print a message asking the user to restart.
- **Installed hook backstop:** `agent-doc skill install` also writes `.codex/hooks.json` and enables `features.hooks = true` in `.codex/config.toml`. The installed commands are `agent-doc hook codex-user-prompt-submit` for `UserPromptSubmit` and `agent-doc hook codex-stop` for `Stop`. `UserPromptSubmit` tracks the active document for the Codex session across nested `.agent-doc` roots in the same workspace; `Stop` first tries to finish the response cycle deterministically from `last_assistant_message`, but only when that payload validates as a single assistant closeout. Transcript-shaped payloads (for example full `agent:exchange` dumps, prompt-target lines, or repeated response headings) are blocked and saved only for diagnostics instead of being replayed, even when the stop arrives on a later turn in that same Codex session. If `last_assistant_message` is empty because the turn stopped after a tool-only/authentication step, the hook still blocks, records a diagnostic artifact, and sends the turn back through the normal `finalize` / `session-check` recovery path.
- **Ordering:** finish the turn's requested coding / testing / build-install work before the response persistence command. Do not patch the document early and then keep working for the same turn.
- **Commit/push ordering:** when the document directive includes ordinary repo `commit + push`, do not stage the active session document into that manual git commit. Resolve the intended non-session path set, stage only that set, stop on any stage failure, verify `git diff --cached --name-only` (or stricter submodule-pointer proof) still matches the intended set, commit only that validated non-session set, run `agent-doc finalize` / `write --commit`, then push after the closeout commit lands.
- **Write-back:** Execute `agent-doc finalize` directly for the normal response cycle (Codex runs shell commands natively), then immediately run `agent-doc session-check <FILE>`.
- **Fail closed:** If `agent-doc session-check <FILE>` exits nonzero after write-back, the cycle is still open or the document still has prompt-bearing user edits with no newer cycle start. Do **not** report success or stop; continue recovery instead.
- **Manual repair / missed patchback:** Use the shared default above. Do **not** patch the assistant response directly into the file. After `agent-doc write --commit <FILE>`, run the same `agent-doc session-check <FILE>` guard before ending the turn. That repair write-back should also be the last substantial action of the turn.
- **Session resume:** Codex uses `codex resume --last` instead of `--continue` for ordinary continue flows.
- **Tracked `/clear` recovery:** if the latest tracked Codex prompt for a document was `/clear`, the next managed `agent-doc route` rerun (not `--dispatch-only`) must restart the live session fresh before injecting `agent-doc <FILE>` so the original sandbox, writable roots, and network policy are reapplied instead of trusting post-clear resume inheritance. Editor `Run Agent Doc` actions still use `agent-doc route --dispatch-only`, which must send the bare `agent-doc <FILE>` reopen into the live session instead of turning a post-clear reroute into a restart shortcut.

## OpenCode

- **Invocation:** User types `/agent-doc <FILE>` which triggers the installed OpenCode command. The command template emits `agent-doc <FILE>` as the prompt, which the agent recognizes and loads the skill.
- **Entry semantics:** once that message is accepted, the turn is inside the binary-owned response cycle. Do **not** downgrade it into a manual document-editing task or report a useful-but-uncommitted success state.
- **Slash commands:** The `/agent-doc` command is an OpenCode custom command installed by `agent-doc skill install --harness opencode` into `.opencode/commands/agent-doc.md`. If preflight returns `slash_commands`, skip them (OpenCode handles them natively). If `builtin_commands`, write a document note.
- **Write-back:** Execute `agent-doc finalize` directly for the normal response cycle, then immediately run `agent-doc session-check <FILE>`. Do **not** output the response to the CLI without piping it through `agent-doc finalize` — response text visible in the console but absent from the session document is the same closeout violation as skipping finalize entirely.
- **Fail closed:** If `agent-doc session-check <FILE>` exits nonzero after write-back, the cycle is still open or the document still has prompt-bearing user edits with no newer cycle start. Do **not** report success or stop; continue recovery instead.
- **Manual repair / missed patchback:** Use the shared default above. Do **not** patch the assistant response directly into the file; use `agent-doc write --commit <FILE>` once the prompt already exists. After `agent-doc write --commit <FILE>`, run the same `agent-doc session-check <FILE>` guard before ending the turn. That repair write-back should also be the last substantial action of the turn.

## Cursor / Generic

- **Invocation:** Follow the same pattern as Codex (direct execution).
- **Slash commands:** Not available. Skip `slash_commands`; note `builtin_commands`.
- **Write-back:** Execute `agent-doc finalize` directly for the normal response cycle.
