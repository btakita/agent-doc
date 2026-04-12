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
### Re: `env` frontmatter feature request (HEAD)

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
- Should env vars apply to **all** Bash tool calls in the session, or only to agent-doc-spawned subprocesses?
- Should `agent-doc preflight` output the expanded env in its JSON (so the skill can `export` them), or should `agent-doc` handle this when spawning sessions via tmux?
<!-- agent:boundary:4b4aa6e8 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- **`env` frontmatter** — add env map to frontmatter, shell-expand values, inject into session environment
- **`compress` alias** — decide: add as binary alias for `compact`, or skip
<!-- /agent:pending -->
## Assistant
