# Codex Support — Multi-Harness agent-doc

Plan for supporting both Claude Code and Codex (OpenAI) as agent-doc harnesses.

## Current State

### Already Multi-Harness

| Layer | Status | Notes |
|-------|--------|-------|
| `agent-kit/detect.rs` | Done | Detects Codex via `CODEX_CLI`/`CODEX` env vars |
| `agent-kit/skill.rs` | Done | `install_for(Codex)` writes to `.codex/AGENTS.md` |
| `agent/mod.rs` | Done | `Agent` trait + `resolve()` dispatches by name |
| `agent-doc-model-tier` | Done | Harness-agnostic tier selection |
| `config.toml` | Done | `[agents.codex]` section can override command/args |

### Shared Response Persistence

Codex differs from Claude Code at the invocation, routing, and CLI/backend
layers only. Once a Codex turn has produced a final assistant response, it must
cross the same binary-owned response persistence path as Claude Code and
OpenCode:

1. `preflight` / `plan` establish the prompt and execution contract.
2. The harness/backend returns response text; it does not directly mutate the
   session document.
3. `agent-doc finalize <FILE>` or strict `agent-doc write --commit <FILE>`
   applies the response through the normal template/write pipeline, commit
   transaction, and `session-check`.

Codex may also enter that same boundary through `agent-doc mcp serve`, whose
stdio MCP tools expose document read/session-check/finalize operations to the
host. The MCP server is a transport adapter only: its finalize tool must call
the strict shared write/commit/session-check machinery, and a failed MCP tool
call must leave the same recovery surface as the equivalent CLI closeout.

Do not standardize on the Claude Code slash command or Skill-tool path across
all harnesses. That path is not available to Codex. Standardize on the
harness-neutral closeout and document-mutation path instead: component patches
first, full-content editor IPC disabled, fail-closed retry when an editor IPC
patch cannot be proven, and a mandatory commit/session-check boundary. If a
corruption appears only in Codex sessions, treat it as evidence
that the Codex invocation, hook recovery, or final-response capture bypassed the
shared closeout path, not as permission to add a Codex-specific write-back path.

### Claude-Code-Specific (Coupling Points)

| Component | Coupling | Impact |
|-----------|----------|--------|
| **SKILL.md** | References Claude Code `Skill` tool, `/agent-doc` slash command | Codex has no skill/slash-command system — needs `.codex/AGENTS.md` adapter |
| **start.rs** | Hardcoded `claude` binary, `--continue`, prompt detection (`❯`) | Must parameterize binary + restart flags per harness |
| **route.rs** | Sends the active harness trigger via tmux `send-keys` or supervisor IPC | Codex uses a bare `agent-doc <file>` trigger and hex-text+CR tmux submit, so command injection must stay harness-aware |
| **agent/claude.rs** | `-p`, `--output-format json`, `--permission-mode`, `--resume`, `--fork-session` | Codex uses `exec --json`, `resume <id>`, different sandbox flags |
| **stream.rs** | `--output-format stream-json` (Claude-specific) | Codex has `--json` (JSONL) with different event schema |
| **frontmatter** | `claude_args` field | Rename to generic `agent_args` (keep `claude_args` as compat alias) |
| **sync.rs / sessions.rs** | Tmux pane management + Claude Code session model | Core of the problem — see Phase 3 |

## Codex CLI Interface

```
codex exec [PROMPT]           # non-interactive, stdin prompt
codex exec --json             # JSONL event stream to stdout
codex exec -m <MODEL>         # model override
codex exec -s <SANDBOX>       # sandbox: read-only | workspace-write | danger-full-access
codex resume <SESSION_ID>     # resume by UUID
codex resume --last           # resume most recent
codex fork <SESSION_ID>       # fork (≈ Claude's --continue --fork-session)
codex fork --last
```

`agent-doc` also treats `CODEX_SANDBOX_NETWORK_DISABLED` as a first-class Codex launch input:
- `codex_network_access: enabled` removes the variable from the child env
- `codex_network_access: disabled` forces `CODEX_SANDBOX_NETWORK_DISABLED=1`
- `inherit` preserves the ambient launcher setting

When the effective child env still has `CODEX_SANDBOX_NETWORK_DISABLED=1` while the sandbox request is `danger-full-access`, `start` and orchestrated fresh Codex runs fail closed with a launch-policy mismatch instead of silently starting a network-disabled session.

Managed Codex panes do not treat correct process args as capability proof, but
the proof is now off by default. A document must set `managed_proof: true` in
frontmatter before network access, required SSH targets, explicit extra
writable roots, or auto-injected submodule/superproject git metadata roots can
create a pending proof gate. An omitted or false value records `not_required`;
project/global config cannot silently opt the document in. When enabled,
explicit network access requires host DNS and either a bounded sandboxed child
probe or a same-process cached success. Writable-root proof checks launcher and
sandbox access, including git `index.lock` creation/removal. Probe children add
`--ephemeral`, `--ignore-rules`, and low reasoning effort; simultaneous network
and writable-root checks share one child invocation and validate both markers.
The existing contract fingerprints, status reporting, and dispatch-gate
semantics remain unchanged.

A transient failure of an opted-in proof retries with exponential back-off
before committing the dispatch gate to `Failed`; while retrying it remains
`Pending`. Retry count, back-off, and timeout retain their frontmatter-over-
config policy and 3 / 2s / 45s defaults.

Operator stop/clear/recovery is exempt from the dispatch gate, so a session whose capability proof failed can still be stopped or cleared without `kill -9` (`#codex-capability-proof-unrecoverable`). The supervisor IPC layer classifies only `Inject` (real prompt dispatch) as gated; the operator control methods (`Clear`, `Stop`, `Restart`) and the read-only methods (`State`, `Pid`) bypass the gate. `agent-doc session clear` and `agent-doc session interrupt-clear` deliver `/clear` through the gate-exempt `Clear` IPC method (and the tmux interrupt sequence), so they succeed against a proof-`Failed` session instead of failing with `prompt dispatch is disabled`. The auto-trigger and auto-queue dispatch paths remain gated, because those are real prompt dispatches rather than operator recovery.

Codex startup UI that asks the operator to review hooks is also a dispatch blocker. The warning text such as `hook needs review` / `Open /hooks to review it` requires interactive approval before the TUI can consume routed input, even if a later capability-proof diagnostic records `status=proven`. Route/start readiness checks must fail closed with that blocker instead of treating the proof line or model/context footer as an idle prompt. Dispatch-only refusals for this blocker must include recovery guidance to open `/hooks`, approve or disable the pending hook change, wait for the idle composer, and rerun the route/editor action.

Managed Codex instructions and generated prompt context must also scope remote-host evidence to the active project. Approved command prefixes, ambient SSH config, or a host name seen in another workspace/client are launch-environment facts only; they are not evidence that the current document should use that host. Codex may use a named remote host only when the current user prompt, session document/frontmatter, project-local `.agent-doc/config.toml`, or project-local runbook explicitly identifies it. Otherwise the session must ask or record follow-up work to confirm the intended host before using it in answers, probes, or backlog items.

For resumed Codex sessions, agent-doc must not pass legacy sandbox flags through blindly. `codex resume` / `codex exec resume` policy is expressed with `-c sandbox_mode="..."`; supervisor restart and backend resume paths translate `-s <SANDBOX>` / `--sandbox=<SANDBOX>` into that form, strip `--add-dir` entries that resume cannot accept, and fail closed on malformed or conflicting sandbox args before a resumed session can run task work. A direct backend turn that has a saved Codex resume id but whose current launch args require `--add-dir` roots must fresh-start with the full root set, because resume cannot add missing git metadata roots. This prevents a document requesting `danger-full-access` or extra writable roots from silently resuming under Codex's older launch policy.

Key differences from Claude Code:
- No `-p` (pipe mode) — `exec` is the non-interactive equivalent
- No `--permission-mode` — uses `-s` sandbox mode instead
- No `--append-system-prompt` — system instructions via `.codex/AGENTS.md` or `AGENTS.md`
- JSON output: `--json` flag produces JSONL (not `--output-format json/stream-json`)
- Session resume: `codex resume <id>` (not `--resume <id>` flag)
- Session fork: `codex fork <id>` (separate subcommand, not `--continue --fork-session`)
- No `CLAUDECODE` env var — uses `CODEX_CLI`/`CODEX`
- Interactive mode: `codex [PROMPT]` (TUI, not `claude` with no flags)
- Network policy is partially env-driven — sandbox args alone do not guarantee socket access

## Implementation Plan

### Phase 1: Codex Agent Backend (agent/codex.rs)

**Goal:** `agent-doc run --agent codex <FILE>` and `agent-doc stream --agent codex <FILE>` work.

1. Create `src/agent/codex.rs` implementing `Agent` trait:
   - Command: `codex exec`
   - Flags: `--json` for structured output, `-s workspace-write` default
   - Parse JSONL output → extract final response text + session ID
   - Session resume: shell out to `codex exec resume <id> --json` with prompt on stdin, translating legacy sandbox flags to `-c sandbox_mode="..."`
   - Session fork: `codex fork --last` with prompt on stdin
   - Model override: `-m <model>`
   - System prompt: write to temp `.codex/AGENTS.md` or use `AGENTS.md` in project root
   - Remove `CODEX_CLI` from child env (prevent recursive detection, same pattern as `CLAUDECODE`)

2. Implement `StreamingAgent` for Codex:
   - `codex exec --json` streams JSONL events
   - Map Codex event types to `StreamChunk` (text delta, thinking, final)
   - Event schema TBD — needs testing with actual `codex exec --json` output

3. Register in `agent/mod.rs`:
   ```rust
   "codex" => Ok(Box::new(codex::Codex::new(cmd, args).with_env(env))),
   ```

4. Tests: unit tests mirroring claude.rs test patterns (send_success, send_is_error, streaming_chunks)

### Phase 2: Frontmatter + Config Generalization ✅

**Goal:** Frontmatter and config support harness-neutral agent selection.

**Status:** Implemented in v0.33.12.

1. ✅ Added `agent_args` frontmatter field (generic replacement for `claude_args`):
   - `agent_args: "--json -s workspace-write"` — passed to whatever agent is active
   - `claude_args` kept as backward-compatible alias
   - `codex_args` added as a Codex-only alias for explicit Codex session config
   - Claude precedence: `fm.agent_args > fm.claude_args > config.agent_args > config.claude_args > AGENT_DOC_CLAUDE_ARGS`
   - Codex precedence: `fm.agent_args > fm.codex_args > config.agent_args > config.codex_args`
   - `claude_args` remains Claude-only; Codex ignores it
   - Frontmatter/config/start tests cover the harness-specific chains

2. ✅ Config `default_agent` already works for `"codex"` value — verified in `run.rs`, `stream.rs`, `init.rs`

3. ✅ Frontmatter `agent: codex` dispatches through `agent::resolve("codex")` — verified via Phase 1 registration

### Phase 3: Supervisor Harness Abstraction (start.rs) ✅

**Goal:** `agent-doc start <FILE>` works with Codex interactive sessions.

**Status:** Implemented in v0.33.12.

1. ✅ Defined `HarnessConfig` struct in `harness.rs`:
   - `binary`, `restart_behavior` (Append or Replace), `prompt_patterns`, `trigger_command_template`, `env_remove`, `supports_no_mcp`, `supports_enable_tool_search`
   - `RestartBehavior::Append` for Claude (`["--continue"]`), `RestartBehavior::Prepend` for Codex (`["resume", "--last"]`)
   - Factory methods: `claude()`, `codex()`, `from_agent_name()`, `from_context()`
   - Helper methods: `restart_args()`, `trigger_command()`, `matches_prompt()`

2. ✅ `HarnessConfig::from_context(fm, config)` resolves from:
   - Frontmatter `agent` field > Config `default_agent` > fallback `"claude"`

3. ✅ Refactored `start.rs::run()`:
   - Binary name parameterized (spawn, error messages, log events)
   - Restart args use `harness.restart_args()` (append vs prepend); Codex resume restarts translate sandbox flags to `-c sandbox_mode="..."`, strip resume-incompatible `--add-dir` flags, and reject conflicting sandbox modes before spawning
   - Clean exit handling is harness-aware: Codex auto-restarts in resume mode after a normal `codex exec` turn instead of dropping into the Claude-style `Enter`/`q` prompt, and stdin-forwarded EOF/Ctrl-D or a stdin-forwarded `Ctrl+C` that actually terminates the child both surface that same restart-fresh-or-quit menu so the operator can intentionally leave the supervisor, even immediately after a committed document cycle. Route/plugin-injected interrupts that bypass stdin forwarding stay on the automatic recovery path. Those supervisor prompts must force a canonical local tty mode for the prompt read itself instead of trusting the inherited parent harness stdin flags, so Enter continues to work even when the outer binding session left stdin raw-ish. Only genuinely promptless fresh/fresh-restart clean exits with no forwarded operator quit key still count as failed startup provenance and restart fresh automatically. Failed resume handoffs still stop chaining blind resumes by restarting fresh on the first failure and escalating to the resume-failure prompt on repeated failures, but prompt-time EOF on that path now also restarts fresh rather than quitting
   - A definitive Codex `No saved session found with ID <id>` response clears `resume` only when the document still names that exact ID, then immediately launches and triggers a fresh session instead of feeding the stale ID into crash backoff. Safe replica-refresh suffix duplication is normalized together with that pointer removal in one expected-current authority write, preventing malformed component scaffolding from retaining the stale ID. Before retrying a cached resume launch, the supervisor also rechecks document authority; if the operator already removed `resume`, it discards the cached resume arguments and relaunches fresh. Crash backoff remains Ctrl+C-interruptible while no child owns stdin.
   - Prompt detection now requires a prompt line that appears in pane content produced after the resumed child starts; a stale prompt still visible in tmux history no longer counts as resume proof
   - Trigger command uses `harness.trigger_command()`
   - `--no-mcp` and `ENABLE_TOOL_SEARCH` gated by `supports_*` flags
   - Fresh-route timeout no longer silently idles: `route.rs` now attempts one bounded fallback trigger injection before logging `fresh_route_trigger_missing` and failing closed
   - Fresh Codex startup now requires a real cycle ack after trigger injection: route does not treat pane input acceptance as success by itself; it waits for a new per-document cycle state and logs `fresh_route_start_acknowledged` or `fresh_route_start_missing`

4. ✅ Commit-boundary recovery distinguishes missed-start vs missed-commit:
   - `session-check` treats `ipc_write_consumed` / `snapshot_saved_file_ipc` without later `commit_*` as “write landed, commit missing”
   - `preflight` auto-attempts `resume_commit_attempt` for that state and logs `resume_commit_success` or `resume_commit_blocked_drift`

4. 14 unit tests covering defaults, resolution, restart behavior, prompt matching, trigger substitution

### Phase 4: Route + Sync Generalization

**Status:** Implemented in v0.33.12.

**Goal:** `agent-doc route <FILE>` works regardless of harness.

1. ✅ `HarnessConfig` extended with `tmux_session_fallback` and `process_names` fields:
   - Claude: fallback session `"claude"`, processes `["agent-doc", "claude", "node"]`
   - Codex: fallback session `"codex"`, processes `["agent-doc", "codex", "node"]`
   - Added `is_prompt_line()`, `is_agent_process_name()`, `cmdline_is_agent()` methods

2. ✅ `route.rs` fully parameterized:
   - `send_command()` uses `harness.trigger_command()` instead of hardcoded `/agent-doc`
   - `wait_for_claude_ready()` → `wait_for_agent_ready()` with `harness.prompt_patterns`
   - `pane_has_prompt()` uses `harness.is_prompt_line()` instead of hardcoded `❯`/`>`
   - `is_agent_process()` uses `harness.is_agent_process_name()` instead of `AGENT_PROCESSES` const
   - `resolve_target_session()` uses `harness.tmux_session_fallback` instead of `TMUX_SESSION_NAME` const
   - Removed `TMUX_SESSION_NAME` and `AGENT_PROCESSES` constants
   - Harness resolved from file frontmatter in `run_with_tmux()` and `resolve_harness_for_file()`
   - All log messages use `harness.binary` instead of hardcoded "Claude"

3. ✅ `sync.rs` updated: process detection includes "codex", comments generalized

4. ✅ `resync.rs` updated: `AGENT_PROCESSES` includes "codex"

5. 10 new harness tests (prompt line, process name, cmdline detection) + 1 new route integration test (codex prompt detection). All existing tests updated to use `HarnessConfig`.

### Phase 5: Skill File Adaptation

**Status:** Implemented in v0.33.12.

**Goal:** `agent-doc skill install` writes correct instructions for each harness.

**Design decision:** Single unified SKILL.md with harness-detection preamble (not separate per-harness files). Rationale: ~90% of content is shared; separate files cause drift.

1. ✅ **Unified SKILL.md** with harness-detection preamble:
   - Added "Harness Compatibility" section referencing `runbooks/harness-invocation.md`
   - Abstracted Claude-specific tool references (`Skill` tool, `AskUserQuestion`) behind generic + runbook dispatch
   - Slash command section (0b) now conditionally dispatches per harness
   - Shared directive semantics: imperative edits inside the session document authorize the underlying repo work directly; agents should execute that work or stop on a concrete blocker instead of replying with status-only/meta prose
   - Success criteria uses "agent console" instead of "Claude console"

2. ✅ **New runbook `runbooks/harness-invocation.md`:**
   - Harness detection table (env vars, tool availability)
   - Claude Code: slash commands → `Skill` tool, `AskUserQuestion`, heredoc
   - Codex: direct execution, no slash commands, `codex resume --last`
   - Cursor/Generic: direct execution fallback

3. ✅ **Environment-aware runbook installation:**
   - `install_runbooks_for(env, root)` installs runbooks alongside the skill file
   - Claude: `.claude/skills/agent-doc/runbooks/`, Codex: `.codex/runbooks/`, etc.
   - `install_runbooks_all(root)` installs to all environments
   - `install_for` and `install_all` now use environment-aware runbook paths
   - Fixes: previously runbooks were hardcoded to `.claude/` only

4. ✅ **7 new tests:** runbooks-per-environment (claude, codex, all), harness preamble presence, harness-invocation runbook bundled + content check

## Open Questions

1. **Codex JSONL schema:** What does `codex exec --json` actually output? Need to run it and capture the event stream to implement parsing. The `StreamChunk` mapping depends on this.

2. **Codex prompt character:** What does Codex's interactive TUI show as its idle prompt? Needed for `wait_for_agent_ready()` polling.

3. **Codex session persistence:** Does `codex resume <id>` work reliably across restarts? Claude Code's `--continue` resumes the most recent session — Codex requires an explicit ID or `--last`.

4. **Codex system prompt injection:** Codex reads `.codex/AGENTS.md` — but can we inject per-document context? Options:
   - Write a temp AGENTS.md per session (fragile, may conflict with user's own AGENTS.md)
   - Use prompt prefix in `exec` stdin
   - Use `--config` to set system instructions

5. **Concurrent harnesses:** Can a user run Claude Code sessions for some docs and Codex for others in the same project? The session registry would need a `harness` field.

## Priority

Phase 1 (Codex backend) unblocks non-interactive use (`agent-doc run/stream --agent codex`).
Phases 2-4 enable interactive sessions. Phase 5 enables self-installing skill files.

Recommend shipping Phase 1 first, then 2+3 together, then 4+5.

## Estimated Scope

| Phase | Files Changed | New Files | Tests |
|-------|--------------|-----------|-------|
| 1 | agent/mod.rs | agent/codex.rs | 6-8 |
| 2 | frontmatter.rs, config.rs, start.rs | — | 3-4 |
| 3 | start.rs | harness.rs | 5-6 |
| 4 | route.rs, sync.rs | — | 4-5 |
| 5 | skill.rs | AGENTS_CODEX.md | 2-3 |
| **Total** | **~8 files** | **~3 files** | **~22 tests** |
