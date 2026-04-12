---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
agent_doc_model_tier: low
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
✅ agent-doc v0.32.1 — CRDT compact fix released, all tests passing, live in production
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260412-155559.md`*
### Re: Compact complete

Exchange archived (446 lines) → `pre-compact-8` tag. Previous content recoverable via `agent-doc show tasks/agent-doc/agent-doc-bugs.md` at that tag.

**Summary of archived work:**
- Bidirectional IPC (Option A) with PostToolUse hook — fully implemented and tested
- Config consolidation (`components.toml` → `config.toml`) — done with migration
- Frontmatter flags (`no_mcp`, `enable_tool_search`) — implemented
- Context audit skill scaffolded at `.claude/skills/context-audit/`
- "compress doc" discussion — resolved: no `compress` subcommand exists, `compact` is the correct command

**Open item:** Add `compress` as a binary alias for `compact`?
❯ Please add an `env` frontmatter, which will be read to add the environment valiables into the claude session and tool calls. The values of the env variable should support shell expansion/execution.
❯ So the following env variables set in the agent doc fontmatter would guide Claude Code to run qwen3.6-plus on openrouter:
```
env:
  OPENROUTER_API_KEY: "$(passage btak/OPENROUTER_API_KEY)"
  ANTHROPIC_BASE_URL: "https://openrouter.ai/api"
  ANTHROPIC_AUTH_TOKEN: "$OPENROUTER_API_KEY"
  ANTHROPIC_MODEL: "qwen/qwen3.6-plus"
  ANTHROPIC_SMALL_FAST_MODEL: "qwen/qwen3.6-plus"
```
### Re: `env` frontmatter feature request

This is a solid feature — it cleanly separates per-document environment config from the global shell. Here's how I'd approach it:

**Implementation plan:**

1. **Frontmatter parsing** — extend the YAML frontmatter parser in agent-doc to recognize an `env` map field. Each key is an env var name, each value is a string that may contain shell expansions.

2. **Shell expansion** — when agent-doc spawns a Claude session (or when the skill reads frontmatter), expand values through the shell:
   - `$(command)` — command substitution
   - `$VAR` / `${VAR}` — variable expansion
   - Order matters: process entries top-to-bottom so `$OPENROUTER_API_KEY` in `ANTHROPIC_AUTH_TOKEN` resolves from the already-expanded value above it

3. **Injection point** — two options:
   - **Option A (binary-side):** `agent-doc` expands env vars and passes them to Claude Code via `--env` flags or by setting them in the spawned process environment
   - **Option B (skill-side):** The SKILL.md reads `env` from frontmatter, expands values via `eval`, and exports them before tool calls

   Option A is cleaner — the binary handles expansion and the skill doesn't need to parse YAML env blocks.

4. **Security** — values go through `passage` (your secret manager), so cleartext secrets never appear in the document. The expanded values should only exist in process memory, never logged.

**Questions before building:**
❯ - Should env vars apply to **all** Bash tool calls in the session, or only to agent-doc-spawned subprocesses?: What do you suggest? I'm inclined to apply to **all** bash tool calls by default. We could have an override frontmatter option to opt out of this behavior.
❯ - Should `agent-doc preflight` output the expanded env in its JSON (so the skill can `export` them), or should `agent-doc` handle this when spawning sessions via tmux?: Explain the differences.
### Re: Env scope and injection point

**All Bash tool calls — agreed.** The whole point of setting `ANTHROPIC_MODEL` etc. is to affect the session globally. Making it all-by-default with an opt-out (`env_scope: subprocess` or similar) is the right call.

**Preflight JSON vs. binary-side — the differences:**

❯ | | Preflight JSON (skill exports) | Binary-side (tmux spawn)                                                                                                                                          |
❯ |---|---|-------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **How it works** | `preflight` returns `"env": {"KEY": "expanded_value"}` in its JSON. The SKILL.md reads these and runs `export KEY=value` before subsequent tool calls. | `agent-doc` expands env vars itself and sets them in the tmux pane environment when spawning a Claude session (`tmux send-keys "export KEY=value && claude ..."`) |
❯ | **Applies to** | Current Claude session's Bash tool calls (the skill runs `export` in the shell) | Only the spawned subprocess tree (the tmux pane)                                                                                                                  |
❯ | **Requires skill changes** | Yes — SKILL.md needs a new step after preflight to parse and export env vars | No skill changes — binary handles everything                                                                                                                      |
❯ | **Secrets exposure** | Expanded values appear in preflight JSON output (visible in tool call results). But since `--dangerously-skip-permissions` is on, tool results aren't shown to viewers... except during livestreams where console is visible | Expanded  values only exist in the tmux pane environment — never in Claude's tool output                                                                          |
❯ | **Works for /agent-doc skill** | Yes — the skill can export before any Bash call | Partially — only affects tmux-spawned sessions, not the current interactive session                                                                               |
❯ | **Works for tmux spawn** | No — preflight runs inside the already-spawned session | Yes — this is its primary use case                                                                                                                                |

❯ **Secrets exposure**: Would the secrets be viewable when the tmux command is entered into the prompt via send-keys? I am able to see the tmux command when agent-doc calls send-keys.

**Recommendation:** Use **both**, serving different flows:

- **Preflight JSON** for the `/agent-doc` skill flow (interactive session). The skill exports env vars into the current shell. For security, the JSON should contain the **unexpanded shell commands** (e.g., `$(passage btak/OPENROUTER_API_KEY)`) so the skill runs the expansion inside a Bash call — the expanded secret value never appears in JSON output.
- **Binary-side** for the `agent-doc spawn` flow (tmux session creation). The binary expands and sets env vars in the new pane's environment.

This way `env:` works regardless of how the session starts.
### Re: Secrets exposure via tmux send-keys

Yes — this is a real risk. If the binary does `tmux send-keys "export OPENROUTER_API_KEY=actual_secret_value && claude ..."`, the expanded secret is visible in the tmux pane prompt, scrollback, and to livestream viewers.

**Mitigation options:**

1. **Pass unexpanded `$(passage ...)` through send-keys** (don't pre-expand):
   ```bash
   tmux send-keys 'export OPENROUTER_API_KEY="$(passage btak/OPENROUTER_API_KEY)" && claude ...' Enter
   ```
   The target pane's shell expands `$(passage ...)` at runtime. The secret never appears in the tmux command string or scrollback. The expanded value exists only in the shell's environment — unavoidable for env vars.

2. **Use `tmux setenv`** instead of `send-keys export`:
   ```bash
   tmux setenv -t $pane OPENROUTER_API_KEY "$(passage btak/OPENROUTER_API_KEY)"
   ```
   Sets the env var in tmux's server environment — child shells inherit it without it appearing on screen. But: the binary must expand the value to pass it to `tmux setenv`, so the secret appears in `/proc/*/cmdline`.

3. **Write a temp env file, source it, delete it** — more complex, marginal benefit.

**Best approach:** Option 1. Pass the unexpanded shell expression through send-keys. The target shell handles expansion internally. This matches your existing `passage` subshell pattern and keeps secrets out of both the tmux visual layer and the binary's argument list.

For the skill flow (preflight JSON), the same principle applies — return the unexpanded `$(passage ...)` strings and let the skill's Bash call expand them. Secrets never appear in JSON output or tool results.

❯ do
❯ - **`env` frontmatter** — add env map to frontmatter; scope: all Bash calls by default; dual injection: preflight JSON (unexpanded) for skill flow + send-keys with unexpanded `$(passage ...)` for tmux spawn
### Re: Implementing `env` frontmatter

Done. Here's what was built:

**1. Frontmatter (`frontmatter.rs`):**
- Added `env: IndexMap<String, String>` field — order-preserving map from the `indexmap` crate
- Serialized/deserialized as a YAML mapping, empty by default (omitted from output when empty)
- 3 new tests: parse, empty default, write roundtrip

**2. Preflight (`preflight.rs`):**
- Added `env: IndexMap<String, String>` to `PreflightOutput`
- Populated from frontmatter — values are **unexpanded** (raw shell expressions)
- Verified: `$(echo hello)` appears as literal string in JSON output

**3. Start (`start.rs`):**
- Added `expand_env_values()` function — builds a single shell script that exports all vars in document order, then prints each expanded value with null-byte delimiters
- Cross-references work: `$OPENROUTER_API_KEY` in a later value resolves from the earlier export
- Expanded values are set on the child `claude` process via `cmd.env()`
- 3 new tests: plain values, shell expansion, cross-reference

**4. SKILL.md:**
- Added step 0c2 after model tier gate: export env vars from preflight JSON into the current shell
- Updated preflight JSON example to show `"env": {}` field

**Test results:** All 72 new + existing tests pass. Release binary installed to `~/.cargo/bin/agent-doc`.

**Usage in a document:**
```yaml
---
env:
  OPENROUTER_API_KEY: "$(passage btak/OPENROUTER_API_KEY)"
  ANTHROPIC_BASE_URL: "https://openrouter.ai/api"
  ANTHROPIC_AUTH_TOKEN: "$OPENROUTER_API_KEY"
  ANTHROPIC_MODEL: "qwen/qwen3.6-plus"
  ANTHROPIC_SMALL_FAST_MODEL: "qwen/qwen3.6-plus"
---
```

Ready for you to test in a live session. The binary is installed — try adding `env:` to a document's frontmatter and running `/agent-doc` on it.

❯ Will the env variables be applied to the Claude Code session when the JB plugin run `Claim for Tmux Pane` on the document or when Sync is called where new terminal is opened?
### Re: Env vars in Claim vs Sync flows

**Claim (`Claim for Tmux Pane`)** — No. Claim just registers the pane-to-document mapping in `sessions.json`. It doesn't start Claude or set any environment.

**Sync (new terminal opened)** — Yes. The chain: Sync → `route.rs` → `auto_start_in_session()` → `tmux send-keys "agent-doc start <FILE>"` → `start.rs` reads frontmatter `env:`, expands values via shell, sets them on the `claude` child process via `cmd.env()`.

**`/agent-doc` skill (existing session)** — Yes. The skill reads `env` from preflight JSON and runs `export` commands in the Bash tool.

| Flow | Env applied? | How |
|------|-------------|-----|
| `Claim for Tmux Pane` | No | Just registers pane |
| Sync (new terminal) | Yes | `start.rs` expands + sets on child |
| `/agent-doc` skill | Yes | Skill exports from preflight JSON |
| `agent-doc stream` | Not yet | Would need separate wiring |

Want me to also wire env into the stream path?
❯ Yes. **all** ways of running claude or any other tool should include the env variables.
❯ Make sure we have a running list of every place where agent-doc uses tmux send-keys.
### Re: Env in all paths + tmux send-keys audit (HEAD)

**Every place agent-doc uses tmux send-keys:**

| # | File | Line | What it sends | Flow / Trigger |
|---|------|------|---------------|----------------|
| 1 | `start.rs:268` | `/agent-doc <file>` + Enter | `--continue` restart auto-trigger (raw `Command::new("tmux")`, bypasses Tmux struct) |
| 2 | `route.rs:394` | `/agent-doc <file>` + Enter | `send_command()` — routes slash command to existing Claude pane via `tmux.send_keys()` |
| 3 | `route.rs:434` | `Enter` only | `send_command()` retry loop — re-sends Enter if command text still visible in pane |
| 4 | `route.rs:733` | `agent-doc start <file>` + Enter | `auto_start_in_session()` — launches agent-doc in a newly split pane |
| 5 | `parallel.rs:166` | `claude -p --output-format json < prompt.md > result.json 2>task.log; exit` | `parallel` fan-out — launches Claude CLI in each worker pane |
| 6 | `prompt.rs:219,225,232` | Up / Down / Enter keys | TUI navigation for permission prompts (via `sessions::send_key()`) |

**Env var coverage status across all code paths:**

| Path | Has env? | Mechanism | What to wire |
|------|----------|-----------|-------------|
| `start.rs` (interactive Claude) | **Yes** | `expand_env_values()` → `cmd.env()` | Already done |
| `/agent-doc` skill | **Yes** | Preflight JSON → shell `export` | Already done (step 0c2 in SKILL.md) |
| `stream.rs` (streaming Claude) | **No** | Agent spawned via `Claude::send_streaming()` → `Command::new("claude")` | Need to pass expanded env to `StreamingAgent` |
| `parallel.rs` (fan-out) | **No** | Sends raw command string via `tmux send-keys` | Need to prepend exports to the send-keys command |
| `route.rs` auto_start | **Indirect** | Sends `agent-doc start <file>` → start.rs handles env | Already covered (start.rs reads frontmatter) |
| `route.rs` send_command | **Indirect** | Sends `/agent-doc <file>` → skill handles env | Already covered (skill exports from preflight) |

**Implementation plan for the two missing paths:**

**1. `stream.rs` — thread env through the agent trait:**
- Add `env: Vec<(String, String)>` parameter to `StreamingAgent::send_streaming()` (and `Agent::send()` for consistency)
- In `claude.rs`, apply env pairs via `cmd.env(key, value)` on the spawned `Command`
- In `stream.rs::run()`, read `fm.env`, call `expand_env_values()` (move it to a shared module or make it `pub(crate)`), pass expanded pairs to `send_streaming()`

**2. `parallel.rs` — prepend exports to the send-keys command:**
- Read frontmatter env from the document
- Build an export prefix: `export K1=V1 && export K2=V2 && <existing command>`
- Values should be **unexpanded** shell expressions (like `$(passage ...)`) since the target pane's shell will expand them — same pattern as start.rs auto-trigger

**Shared refactor:** Move `expand_env_values()` from `start.rs` to a new location (either `frontmatter.rs` or a small `env.rs` module) so both `start.rs` and `stream.rs` can call it.

Ready to implement?
<!-- agent:boundary:47c48ed1 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- **`env` frontmatter** — ([done]) implemented in frontmatter.rs, preflight.rs, start.rs, SKILL.md
- **`env` in stream path** — wire `expand_env_values` into `stream.rs` → `StreamingAgent` trait
- **`env` in parallel path** — prepend unexpanded exports to tmux send-keys command string in `parallel.rs`
- **Refactor `expand_env_values`** — move from `start.rs` to shared location (`frontmatter.rs` or `env.rs`)
- **`compress` alias** — decide: add as binary alias for `compact`, or skip
<!-- /agent:pending -->
## Assistant
