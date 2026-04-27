# Codex Support — Multi-Harness agent-doc

Plan for supporting both Claude Code and Codex (OpenAI) as agent-doc harnesses.

## Current State

### Already Multi-Harness

| Layer | Status | Notes |
|-------|--------|-------|
| `agent-kit/detect.rs` | Done | Detects Codex via `CODEX_CLI`/`CODEX` env vars |
| `agent-kit/skill.rs` | Done | `install_for(Codex)` writes to `.codex/AGENTS.md` |
| `agent/mod.rs` | Done | `Agent` trait + `resolve()` dispatches by name |
| `model_tier.rs` | Done | Harness-agnostic tier selection |
| `config.toml` | Done | `[agents.codex]` section can override command/args |

### Claude-Code-Specific (Coupling Points)

| Component | Coupling | Impact |
|-----------|----------|--------|
| **SKILL.md** | References Claude Code `Skill` tool, `/agent-doc` slash command | Codex has no skill/slash-command system — needs `.codex/AGENTS.md` adapter |
| **start.rs** | Hardcoded `claude` binary, `--continue`, prompt detection (`❯`) | Must parameterize binary + restart flags per harness |
| **route.rs** | Sends `/agent-doc <file>` via tmux `send-keys` (Claude Code slash command) | Codex uses different invocation — needs harness-aware command injection |
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

Key differences from Claude Code:
- No `-p` (pipe mode) — `exec` is the non-interactive equivalent
- No `--permission-mode` — uses `-s` sandbox mode instead
- No `--append-system-prompt` — system instructions via `.codex/AGENTS.md` or `AGENTS.md`
- JSON output: `--json` flag produces JSONL (not `--output-format json/stream-json`)
- Session resume: `codex resume <id>` (not `--resume <id>` flag)
- Session fork: `codex fork <id>` (separate subcommand, not `--continue --fork-session`)
- No `CLAUDECODE` env var — uses `CODEX_CLI`/`CODEX`
- Interactive mode: `codex [PROMPT]` (TUI, not `claude` with no flags)

## Implementation Plan

### Phase 1: Codex Agent Backend (agent/codex.rs)

**Goal:** `agent-doc run --agent codex <FILE>` and `agent-doc stream --agent codex <FILE>` work.

1. Create `src/agent/codex.rs` implementing `Agent` trait:
   - Command: `codex exec`
   - Flags: `--json` for structured output, `-s workspace-write` default
   - Parse JSONL output → extract final response text + session ID
   - Session resume: shell out to `codex resume <id>` with prompt on stdin
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
   - `RestartBehavior::Append` for Claude (`["--continue"]`), `RestartBehavior::Replace` for Codex (`["resume", "--last"]`)
   - Factory methods: `claude()`, `codex()`, `from_agent_name()`, `from_context()`
   - Helper methods: `restart_args()`, `trigger_command()`, `matches_prompt()`

2. ✅ `HarnessConfig::from_context(fm, config)` resolves from:
   - Frontmatter `agent` field > Config `default_agent` > fallback `"claude"`

3. ✅ Refactored `start.rs::run()`:
   - Binary name parameterized (spawn, error messages, log events)
   - Restart args use `harness.restart_args()` (append vs replace)
   - Prompt detection uses `harness.matches_prompt()`
   - Trigger command uses `harness.trigger_command()`
   - `--no-mcp` and `ENABLE_TOOL_SEARCH` gated by `supports_*` flags
   - Fresh-route timeout no longer silently idles: `route.rs` now attempts one bounded fallback trigger injection before logging `fresh_route_trigger_missing` and failing closed

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
