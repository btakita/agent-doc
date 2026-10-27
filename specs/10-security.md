> Extracted from SPEC.md — see [index](../SPEC.md)

# Security

agent-doc is designed for single-user, local operation. There is no authentication, authorization, or multi-user access control.

## Threat Model

- **Trusted user, untrusted content.** The user is trusted; document content may contain prompt injection from external sources (pasted emails, web pages, chat logs).
- **Local filesystem scope.** All data (documents, snapshots, exchange history, links cache) resides on the local filesystem. No network services are exposed.
- **Git as audit trail.** All agent responses are committed to git, providing a complete audit trail. However, git history may contain sensitive content if documents reference private data.

## Known Risks

- **Prompt injection via document content.** Content pasted from external sources could contain injection attempts. The agent processes all document content as user input with no injection scanning. Mitigation: user awareness; planned content scanning in `agent-doc write`.
- **`--dangerously-skip-permissions` exposure.** When running with this flag (common in agent-doc sessions via `claude_args` or `opencode_args` frontmatter), the agent has full filesystem access. Injected prompts could read files or execute commands.
- **Data divulgence through the response channel.** Even with sandboxing, the agent's response IS the output channel. If the model has sensitive data in context, injection can convince it to include that data in the document response. The only real defense is context isolation (see ragie-web-doc security analysis).
- **Links cache may contain sensitive fetched content.** URL content fetched via `links` frontmatter is cached at `.agent-doc/links_cache/`. This cache is not encrypted and persists until manually cleared.
- **Cross-document plan / backlog reads can leak user context.** Moving exchange content, transferring backlog or icebox items, or following a plan-backed backlog item into another `.md` file can expose one user's work queue to another if the document is being used collaboratively.

## Recommendations

- Use a **private git repository** for the project containing session documents.
- Avoid putting secrets (API keys, credentials) in documents or agent context.
- If you are experimenting with shared/collaborative docs, mark the document with `agent_doc_collaboration: shared` and require an auditable `agent_doc_security_review: <review-id>` before cross-document `extract` / `transfer` or plan-backed `do #id` work that reads another `.md` file.
- For broader shared/collaborative use cases, wait for the planned multi-user security model (access control, session isolation, content scanning).
- Review agent responses before sharing or publishing document content.

## Live tmux server guard (`#tmuxkittyterminal`)

Agent panes, their subagents, and the terminal hosting them all depend on one
tmux server: the operator's. Killing it ends every live session at once. On
2026-10-26 22:26:47 -04:00 a background subagent tore down its test server with
`TMUX_TMPDIR=<dir> tmux kill-server`. Its harness ran inside the operator's
tmux, and tmux resolves the server as `-L`/`-S`, then `$TMUX`, then
`$TMUX_TMPDIR/tmux-<uid>/default`. The command killed the operator's server,
every Claude and Codex pane, and kitty, which runs `kitty -e tmux` and closed
when its client exited.

The `PreToolUse` command installed for Claude Code and Codex
(`agent-doc hook coined-id-pre-tool-use`) runs this guard first, before the
coined-id guard, in every pane, with or without a session document.
`agent-doc skill install` wires it for both harnesses (`#tmuxguardclaudeinstall`):
for Claude Code into the project's `.claude/settings.json`, and for Codex into
the user-level `$CODEX_HOME/hooks.json` beside the other agent-doc Codex hooks.
The entry has no matcher, because the handler filters by tool name itself. The
install merges it: the operator's own `PreToolUse` entries, their matchers, and
unrelated settings survive, and a re-run adds no duplicate. A Claude project
where `agent-doc skill install --harness claude` never ran has no guard; the
install does not write the operator's user-level `~/.claude/settings.json`.
The guard behaves as follows:

- **Scope.** Only shell commands: tool name `Bash` (Claude Code, Codex),
  `run_terminal_command`, or `Shell` (`#tmuxguardclaudeuser`). Grok Build
  runs the hooks in `~/.claude/settings.json` and, once a folder is trusted,
  the project's `.claude/settings.json`, but its payload names its own shell
  tool (`run_terminal_command`, `Shell` under its Cursor preset) in
  `toolName`/`toolInput`, with `tool_name`/`tool_input` as aliases. Grok maps
  Claude tool names only in a `matcher`. The handler reads both key spellings
  and treats every shell tool name alike; otherwise the guard would load in a
  Grok pane and allow everything. The guard follows quote-aware segments,
  prefix assignments, `env`/`sudo`/`command`/`exec`/`nohup`/`timeout`/`nice`/
  `rtk proxy` wrappers, and `sh -c`/`bash -lc` scripts. It inspects the tmux
  commands that destroy state: any `kill*` (`kill-server`, `kill-session`,
  `kill-window`, `kill-pane`, `killp`, `killw`, unique prefixes), plus
  `respawn*` and `unlink*` with `-k`, including commands chained with `\;`.
  It also inspects `pkill`/`killall` patterns that match `tmux`, `tmux:
  server`, `tmux: client`, `kitty`, or `kitten`. Read-only tmux commands
  (`ls`, `list-panes`, `show-options`, `capture-pane`) are never blocked.
- **Allowed only when provably private.** The command passes `-L <name>`
  (not `default`) or `-S <path>` (not the `$TMUX` socket or the default
  socket). Or it clears `$TMUX` (`env -u TMUX`, `TMUX=`, `unset TMUX`,
  `env -i`) and exports a non-empty `TMUX_TMPDIR` in the same command line.
  Or it sets `$TMUX` to another socket. A label or path built from a shell
  variable counts as private.
- **Refused otherwise,** with exit status 2 and a reason that names the server
  the command would reach and the private form to use instead. A `TMUX_TMPDIR`
  with an inherited `$TMUX` is refused, and the reason says tmux reads `$TMUX`
  first. An unexported `TMUX_TMPDIR` assignment is ignored, and so is a
  `source`d env file, because the guard cannot see what it exports. The guard
  fails closed on a destructive command: a refusal costs a retry with `-L`, and
  a wrong allow costs every live pane. There is no override flag. A change to
  the operator's own server is the operator's call.
- **Required practice is stricter than the floor.** The guard allows exactly
  what tmux resolves to a private socket. Agents and scripts must still use
  both `env -u TMUX` and `-L <unique-name>` (or `-S <path>`) on every tmux
  command against a test server, setup and teardown included, and the refusal
  reason names that form.

The subagent brief (`agent-doc queue brief`) and the exchange-subagent dispatch
guidance restate the full rule (`subagent_tmux_isolation_rule!`), including send-keys
and resize, which the hook does not inspect.

## Secret Redaction

The `agent-doc-secret-redact` crate is a backend hygiene layer that scrubs common plaintext secret shapes from anything written to `.agent-doc/` state files or stream/finalize stdout messages. It is **always on** with no operator-facing flag.

### Patterns (most-specific first)

- `sk-proj-[A-Za-z0-9_-]{20,}` → `[REDACTED_OPENAI]` (OpenAI project keys)
- `sk-svcacct-[A-Za-z0-9_-]{20,}` → `[REDACTED_OPENAI]` (OpenAI service-account keys)
- `sk-[A-Za-z0-9]{40,}` → `[REDACTED_OPENAI]` (legacy OpenAI keys)
- `AKIA[0-9A-Z]{16}` → `[REDACTED_AWS]` (AWS access keys)
- `xoxb-[0-9]+-[0-9]+-[A-Za-z0-9]+` → `[REDACTED_SLACK]` (Slack bot tokens)
- `ghp_[A-Za-z0-9]{36}` / `gho_[A-Za-z0-9]{36}` → `[REDACTED_GITHUB]`
- Named env-var family: `OPENAI_API_KEY|ANTHROPIC_API_KEY|AWS_SECRET_ACCESS_KEY|CLOUDFLARE_API_TOKEN|XAI_API_KEY|OPENROUTER_API_KEY|STRIPE_API_KEY` followed by `:` or `=` — the value half is replaced with `[REDACTED]`; the variable name and separator are preserved so debugging still works.

Order matters: longer patterns run first so a project / service-account token is never partially eaten by the generic legacy rule.

### `passage` exemption

Any match whose surrounding ±16-byte window contains the substring `passage ` is left UNCHANGED. `passage open …`, `passage show …`, and `$(passage …)` are the canonical safe patterns and round-trip verbatim. This protects the project's recommended pattern (read secrets from `passage` rather than env dumps) from accidental redaction.

### Call sites

- `src/capture.rs::capture_response` and `checkpoint_partial_response_for_cycle` — redact `response_body` before it is appended to the cycle-scoped `state.db` capture/recovery ledger. The `response_sha256` retains the original in-memory hash so cycle-state correlation stays consistent.
- typed baseline and pre-response checkpoint writers — redact document bodies before appending them to the transactional `state.db` ledger. No markdown snapshot or pre-response sidecar is written.
- `src/stream.rs` — redact flush-error and thinking-flush-error messages before they hit stderr so a failed write that interpolates a streamed chunk cannot leak a token to the terminal.

The streaming-safe variant `redact_streamed(carry, chunk)` holds the last ~128 chars across calls so a token split across a chunk boundary still matches once the second chunk arrives.

### Threat model boundary

This is **best-effort hygiene**, not a security boundary against a determined adversary. It addresses accidental leakage of common token shapes (e.g., a user running `npx convex env list` while a session document is open and the dump landing in the recovery ledger or terminal scrollback). Defense-in-depth still relies on (a) keeping secrets in `passage` and (b) never running tools that dump env to stdout.
