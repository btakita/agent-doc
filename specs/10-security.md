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

The `PreToolUse` command (`agent-doc hook coined-id-pre-tool-use`) runs this
guard first, before the coined-id guard, in every pane, with or without a
session document. A bare-root `agent-doc skill install` (no `--root`; `--all`
covers every harness; `agent-doc init` runs the same install) wires it into each harness's user layer, so it covers
every project on the machine, governed by a session document or not
(`#tmuxguardclaudeinstall`, `#tmuxguardclaudeuser`):

- **Claude Code:** `~/.claude/settings.json` (`$CLAUDE_CONFIG_DIR/settings.json`
  when set) gets an unmatched `PreToolUse` entry and nothing else. The
  consequential hooks (preflight, Stop, steering) stay project-scoped in
  `.claude/settings.json`, which also keeps its own copy of the guard. Claude
  Code runs a handler defined in more than one settings file once, so the two
  copies do not double. The user layer also reaches subagents: settings-file
  hooks fire for a subagent's tool calls.
- **Codex:** `$CODEX_HOME/hooks.json`, beside the other agent-doc Codex hooks.
- **Grok Build:** no file of its own. Grok runs the hooks in
  `~/.claude/settings.json` (always) and in a trusted project's
  `.claude/settings.json`.
- **Cursor:** a native `beforeShellExecution` entry
  (`{"command": "agent-doc hook coined-id-pre-tool-use"}`) in
  `~/.cursor/hooks.json`, with `"version": 1` added when the file has none.
  Cursor also imports `~/.claude/settings.json` while its third-party import
  setting is on (the default), so the guard can run twice per shell call
  there; both copies reach the same decision. The native entry keeps the guard
  when that import is off.
- **OpenCode:** OpenCode has no command-hook config. The plugin
  `plugins/agent-doc-tool-guard.js` in `~/.config/opencode`
  (`$XDG_CONFIG_HOME/opencode` when set) handles `tool.execute.before`. For the
  `bash` tool it pipes a `PreToolUse` payload (`tool_name: "bash"`,
  `tool_input` = the tool's args, `cwd` = `workdir` resolved against the
  session directory) to the same command and throws when it exits 2, which
  blocks the call. Any other outcome lets the call run: `agent-doc` missing
  from `PATH`, a spawn error, the 10s timeout, or another exit status.

Every merge keeps the operator's own entries, their matchers, and unrelated
settings, and a re-run adds no duplicate. A run with `--root` writes no user
layer, and neither does a test process whose runtime scope excludes the user
config dir. A Claude user file that already holds project hooks
(preflight, Stop, steering) from an older `turn-status install --user` keeps
them. The install names them on stderr instead of removing them:
Claude Code runs a handler defined in two settings files once, and each stays
silent without an agent-doc cycle. `agent-doc turn-status install --user`
writes only the turn-status hooks (`turn-status active`/`idle`) into the
Claude user layer. The full project set is `skill install`'s. OpenCode loads
`{plugin,plugins}/*.{ts,js}` (1.19.4), but its docs name only `plugins/`, so
agent-doc writes its plugins there and removes its own byte-identical copy
from the singular `plugin/` dir. A copy the operator edited stays.

The handler reads every payload shape these harnesses send:
`tool_name`/`tool_input` (Claude Code, Codex, Cursor `preToolUse`, the OpenCode
adapter), Grok's camelCase `toolName`/`toolInput` when the snake_case aliases
are absent, and Cursor's `beforeShellExecution`, whose command is the
top-level `command` string, read as a `Shell` call. A payload carrying
`cursor_version` or naming `beforeShellExecution` is Cursor's. Cursor's docs
say a permission hook blocks on output that is not valid JSON, and they do
not say whether empty output counts. So the handler answers a Cursor payload
with `{"permission":"allow"}` on stdout, or with
`{"permission":"deny","user_message":…,"agent_message":…}` plus the reason
on stderr and exit 2. Claude Code, Codex, Grok and OpenCode get no stdout.
The guard behaves as follows:

- **Scope.** Only shell commands: tool name `Bash` (Claude Code, Codex),
  `run_terminal_command` (Grok Build), `Shell` (Cursor, and Grok's Cursor
  preset), or `bash` (the OpenCode adapter). Grok runs hooks from Claude
  settings files but puts its own tool name in the payload and maps Claude
  names (`Bash`) only in a `matcher`. Cursor's docs map `Bash` to `Shell` and
  show `Shell` in payloads, but do not say which name an imported Claude hook
  receives, so both are accepted. Matching `Bash` alone loaded the guard in
  Grok panes and allowed everything. The guard follows quote-aware segments,
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

### Harness wiring verified against (`#tmuxguardclaudeuser`)

The wiring above depends on each harness's hook config location, payload
shape, tool names, and blocking contract. None of that is a stable interface,
and two harnesses already diverged from Claude's payload while loading
Claude's files. Each row records the harness version installed where the
wiring was checked, and the doc source. "Not installed" means the row rests
on the docs and the release source alone.

| Harness | Version checked (2026-10-27) | Source |
|---|---|---|
| Claude Code | 2.1.310 (installed) | https://code.claude.com/docs/en/hooks: hook locations, merge across settings levels, "If you define the same handler in more than one settings file, it runs once", hooks run inside subagents, exit 2 blocks `PreToolUse` |
| Codex CLI | 0.164.0 (installed) | https://learn.chatgpt.com/docs/hooks (formerly developers.openai.com/codex/hooks): `~/.codex/hooks.json`, all layers load, shell tool "Match as `Bash`", exit 2 blocks |
| Grok Build | 1.0.62 (installed) | bundled `~/.grok/docs/user-guide/10-hooks.md`: Hook Locations (`~/.claude/settings.json`, trusted project `.claude/settings.json`), Tool Name Aliases (matcher-only), Input (`toolName: "run_terminal_command"`, snake_case aliases), Exit Codes (2 denies) |
| OpenCode | not installed; release v1.19.4 source | https://opencode.ai/docs/plugins/ (`plugins/` dirs, `tool.execute.before`, throw to block); `packages/plugin/src/index.ts` (`input.tool`, `output.args`), `packages/opencode/src/config/plugin.ts` (`{plugin,plugins}/*.{ts,js}`), `packages/opencode/src/tool/shell/id.ts` (tool id `bash`), `tool/shell/prompt.ts` (`command`, `workdir`) |
| Cursor | not installed | https://cursor.com/docs/agent/hooks (`~/.cursor/hooks.json`, `beforeShellExecution` input `command`/`cwd`/`sandbox`, `preToolUse` `tool_name: "Shell"`, exit 2 blocks, invalid JSON blocks a permission hook); https://cursor.com/docs/reference/third-party-hooks (imports `~/.claude/settings.json` by default, maps `Bash` → `Shell`) |

Re-verify the wiring for a harness whenever a new version of it ships:
re-read its hook docs, run a payload for a destructive tmux command through
the installed hook, and update this table. Where a harness's docs leave the
payload uncertain, the adapter stays fail-open for everything except a
recognized live-server kill. Cursor's empty-output case is the open
uncertainty, and the explicit JSON reply above covers it. agent-doc has no
mechanism today that reads harness versions, so nothing warns when a row goes
stale; this rule is the reminder.

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
