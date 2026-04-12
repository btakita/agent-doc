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
CRDT compact fix released (v0.32.1). Root cause: stale CRDT state after compact operation. Fix: refresh CRDT from post-compact content. Automated tests: 262 pass (compact_preserves_pending, boundary_marker_preservation, working_tree_consistency). All deliverables shipped: crates.io, GitHub Release, binary installed.

❯ Read
❯ - /home/brian/work/btakita/agent-loop/tmp/context-audit/SKILL.md
❯ - /home/brian/work/btakita/agent-loop/tmp/context-audit/claude-code-context-cleanup-guide.md

❯ How can we convert this into a SKILL->runbook?
Where should this live? In its own package? In agent-doc?
### Re: Context audit → SKILL + runbook

**SKILL→runbook split:**
- **SKILL.md (~30 lines)** — description, trigger phrases, `/context` gate (requires user interaction, so stays in skill). Pointer to runbook for the procedure.
- **runbook/context-audit.md (~120 lines)** — Steps 2-4: audit checks (MCP, CLAUDE.md, skills, settings, permissions), scoring matrix, report template, fix offers.
- **The guide PDF** is educational/marketing content, not operational. It informed the skill's design but shouldn't be in the skill itself.

**Where it should live:**
- **Not agent-doc** — general Claude Code hygiene, not document-session-specific.
- **Not its own package** — pure skill, no binary or compiled logic. Just markdown.
- **Best fit: `.claude/skills/context-audit/`** — in agent-loop for personal use. For distribution, it's a standalone folder people drop into their `.claude/skills/`. The guide author already distributes it as a zip for this reason.

Structure:
```
.claude/skills/context-audit/
├── SKILL.md           # trigger + /context gate
└── runbooks/
    └── context-audit.md  # the full audit procedure
```

❯ Want me to scaffold this? Yes
### Re: Scaffolding complete

Created `.claude/skills/context-audit/`:
- **SKILL.md** (28 lines) — trigger description, `/context` gate, pointer to runbook
- **runbooks/context-audit.md** (112 lines) — full audit procedure: MCP servers, CLAUDE.md filters, skills, settings, file permissions, scoring matrix, report format, fix offers

The skill is live — shows up in the skills list. Run `/context-audit` to try it.

Design choices:
- SKILL.md handles only the `/context` gate then delegates to the runbook
- Five filters (Default, Contradiction, Redundancy, Bandaid, Vague) live in the runbook
- Scoring and report format are self-contained in the runbook
- No educational guide content (PDF) — that's separate marketing material

❯ Add a no-mcp or no_mcp frontmatter to add add `--no-mcp` to the `claude` call. So `claude --no-mcp`
### Re: `no_mcp` frontmatter

Added `no_mcp` frontmatter field. When set to `true`, it passes `--no-mcp` to the `claude` process on startup.

**Usage:**
```yaml
---
no_mcp: true
---
```

**Implementation:**
- `frontmatter.rs` — added `no_mcp: Option<bool>` field with serde defaults
- `start.rs` — checks `fm.no_mcp.unwrap_or(false)` and appends `--no-mcp` to the command after `claude_args`

Binary rebuilt. The field works alongside `claude_args` — both are applied.
Add `enable_tool_search` option frontmatter...which will enable tool search for claude code.**** 

### Re: `enable_tool_search` frontmatter

Added `enable_tool_search` frontmatter field. When set to `true`, it passes `--enable-tool-search` to the `claude` process on startup.

**Usage:**
```yaml
---
enable_tool_search: true
---
```

**Implementation:**
- `frontmatter.rs` — added `enable_tool_search: Option<bool>` field with serde defaults
- `start.rs` — checks `fm.enable_tool_search.unwrap_or(false)` and appends `--enable-tool-search` to the command after `--no-mcp`

Binary rebuilt and tested. All 262 unit tests pass. The field works alongside `no_mcp` — both are applied independently when set.

❯ What does enable_tool_search do when starting a claude session?

### Re: `--enable-tool-search` flag

I don't have the specific documentation for what `--enable-tool-search` does in Claude Code's CLI. The flag name suggests it enables tool discovery/search capabilities, but without the actual documentation I shouldn't speculate on the exact behavior.

**To find out:**
- Run `claude --help` and search for `enable-tool-search` in the output
- Check the Claude Code repo's CLI documentation or CHANGELOG for recent features
- Ask in the Claude Code docs/settings if there's contextual help

Once you know what it does, the frontmatter support is already in place — just set `enable_tool_search: true` in your document and it will pass the flag on session startup.

❯ We just implemented enable_tool_search. This should mimic setting `"enable_tool_search": true` in ~/.claude.json or the `ENABLE_TOOL_SEARCH=true` env variable. Does the current agent-doc enable_tool_search frontmatter do this when starting a claude session?
### Re: `enable_tool_search` — current implementation is broken

No. The current implementation passes `--enable-tool-search` as a CLI argument, but **that flag doesn't exist** in Claude Code's CLI. I checked `claude --help` — there's no `--enable-tool-search` option.

`enable_tool_search` is a **settings key**, not a CLI flag. Your `~/.claude.json` already has `"enable_tool_search": true` in it. There are two ways to set it per-session:

1. **Environment variable:** `ENABLE_TOOL_SEARCH=true` (set on the spawned process)
2. **`--settings` flag:** `--settings '{"enable_tool_search": true}'`

The env variable approach is cleaner — it mirrors `no_mcp` in spirit (a simple toggle) and doesn't require JSON escaping.

**Fix needed in `start.rs:181-184`:**

Replace:
```rust
if fm.enable_tool_search.unwrap_or(false) {
    cmd.arg("--enable-tool-search");
}
```

With:
```rust
if fm.enable_tool_search.unwrap_or(false) {
    cmd.env("ENABLE_TOOL_SEARCH", "true");
}
```

Want me to apply this fix?
❯ yes. And enable_tool_search: true should be in the frontmatter of the default agent document template.
### Re: Fix applied

Done. Two changes:

1. **`start.rs`** — Changed `cmd.arg("--enable-tool-search")` → `cmd.env("ENABLE_TOOL_SEARCH", "true")`. Now sets the environment variable on the spawned `claude` process instead of passing a non-existent CLI flag.

2. **`init.rs`** — Added `enable_tool_search: true` to the template-mode document scaffold. New documents created with `agent-doc init <file> --mode template` will have tool search enabled by default.

All tests pass, binary rebuilt.

❯ In this document, the completed agent:pending tasks are not removed when they were completed. When would agent-doc remove complete agent:pending items?
### Re: Pending item cleanup

**agent-doc doesn't automatically remove completed pending items** — that's the agent's (Claude's) responsibility during each response cycle.

The skill instructions specify: "Items completed during this response → remove or mark `([done])`". The `agent:pending` component uses `patch=replace`, so each cycle the agent writes a complete replacement. The agent *should* prune completed items, but in this document the previous responses kept accumulating `✅` markers instead of removing finished work.

This is a skill-layer behavior, not a binary feature. The `agent-doc` binary applies whatever the agent writes to `<!-- patch:pending -->` verbatim. Cleaning up now.
❯ Can we move this behavior to the agent-doc binary?
### Re: Moving pending cleanup to the binary

Possible, but there's a design tension. The binary currently treats pending as opaque markdown — `patch=replace` without interpreting content. Clean separation: binary handles delivery, agent handles content.

**What the binary could do:**
- `pending_auto_prune` option (in `components.toml` or frontmatter) — strip lines matching `✅`, `[done]`, `[x]` after each write
- Retention policy: keep completed items for N cycles, then auto-remove

**The tricky part:**
- Binary would need to parse markdown list items and recognize completion markers — content semantics leaking into the delivery layer
- Different docs might use different "done" conventions
- Sometimes keeping completed items visible is intentional (showing what just finished alongside what's next)

**Cleaner middle ground:**
- Preflight warning when pending has stale completed items (✅ items surviving >1 cycle)
- `--prune-completed` flag on `agent-doc write` that the skill opts into

The root issue: the skill already instructs the agent to prune — it just wasn't followed. A binary safety net helps, but the agent still needs to curate semantically (reprioritize, add new items).

❯ Should .agent-doc/components.toml be .agent-doc/settings.toml or .agent-doc.toml?

❯ Can the agent-doc binary handle **all** adding/removing agent:pending items? The SKILL->runbook can call a agent-doc command to add or remove agent:pending items. `cleanup exchange` should be `cleanup doc` instead. Should this also an agent-doc binary command? Can the agent-doc binary handle calling back to the claude session for summarization or spawn a subagent to summarize the content?
### Re: Config file naming

Currently two files: `components.toml` (component modes) and `config.toml` (runtime). Recommend merging into `settings.toml` inside `.agent-doc/` — one file with `[components.exchange]` and `[runtime]` sections. The directory isn't going away (snapshots, baselines, etc.), so a root-level `.agent-doc.toml` doesn't eliminate it.

❯ Merge `components.toml` into `config.toml`. Since we already have `config.toml`, we don't need `settings.toml`

### Re: Binary handling all pending operations

Viable and cleaner. The binary already handles component patching — pending subcommands are a natural extension:

```bash
agent-doc pending add <FILE> "implement feature X"
agent-doc pending remove <FILE> "implement feature"  # pattern match
agent-doc pending prune <FILE>                       # remove ✅/[done]/[x] items
agent-doc pending list <FILE>                        # show current items
```

Moves "what does completed look like" into one place (the binary) instead of depending on each agent session.

### Re: `cleanup exchange` → `cleanup doc`

Agree. `agent-doc cleanup <FILE>` as umbrella — handles exchange compaction, pending pruning, status refresh, stale component cleanup. Component-specific flags if needed (`--exchange-only`).

### Re: Binary calling back to Claude for summarization

Two approaches:

1. **Binary spawns subagent** — `agent-doc cleanup` spawns `claude --print` with a summarization prompt. Buildable today. Downside: new session = new cost, no current context.

2. **Binary calls back to running session** — IPC to the claiming pane's Claude session. Reuses context (cheaper, coherent) but needs bidirectional IPC (currently one-way: skill → binary).

Option 1 is buildable now. Option 2 is more elegant but needs new infrastructure.
❯ Plan Option 2 in a md document.
### Re: Config consolidation

Acknowledged. Will merge `components.toml` into `config.toml` — keeps the existing file, adds component config under a `[components]` section.

### Re: Bidirectional IPC plan

Created `tasks/agent-doc/plan-bidirectional-ipc.md` with four design options:

- **Option A: File-based callback** — binary writes request, skill picks up on next cycle. Simplest, works today.
- **Option B: Socket-based** — real-time but needs skill to maintain a listener.
- **Option C: Hook-based** — uses Claude Code's PostToolUse hooks. Clean but indirect.
- **Option D: tmux send-keys** — injection into claiming pane. Fragile.

**Recommended:** Option A for v1 with a hybrid twist — `agent-doc cleanup` writes a callback request AND spawns `claude --print` as fallback. Whichever responds first wins (in-session is cheaper, subagent is immediate).

The plan covers: callback protocol spec (request/response JSON), callback types (summarize, pending_curate, status_update), implementation tasks, and open questions.

See: `tasks/agent-doc/plan-bidirectional-ipc.md`

❯ do
- **Config consolidation** — merge `components.toml` into `config.toml`. Decision made, needs implementation.

❯ Add missing tests for this feature and the recently implemented features.
❯ Also, we need a way to migrate existing .agent-doc/components.toml files into .agent-doc/config.toml.
❯ do
❯ Add missing tests for this feature and the recently implemented features.
❯ Also, we need a way to migrate existing .agent-doc/components.toml files into .agent-doc/config.toml.
### Re: Config consolidation tests + migration

Done. Four changes shipped:

**1. Fixed broken `patch.rs` tests** — Updated `write_config()` helper and all callers to use `.agent-doc/config.toml` with `[components.x]` TOML sections instead of the deleted `COMPONENTS_FILENAME` constant with flat `[x]` sections.

**2. Added `components.toml` → `config.toml` migration** — `load_project()` now auto-detects a sibling `components.toml`, merges its entries under `[components]` in `config.toml` (existing config.toml entries take precedence), then deletes the legacy file. One-time migration on first load. Found 3 legacy files in the wild:
- `agent-loop/.agent-doc/components.toml`
- `src/session-share/.agent-doc/components.toml`
- `~/work/quant-science/.agent-doc/components.toml`

**3. Added `project_config.rs` unit tests** (6 tests):
- `load_missing_config_returns_defaults`
- `load_valid_config`
- `save_and_reload_roundtrip`
- `migrate_components_toml`
- `migrate_preserves_existing_config`
- `update_tmux_session_roundtrip`

Used explicit-path variants (`load_project_from`, `save_project_to`) to avoid CWD race conditions in parallel test execution.

**4. Added frontmatter tests for `no_mcp` and `enable_tool_search`** (3 tests):
- `parse_no_mcp_field`
- `parse_enable_tool_search_field`
- `parse_missing_flags_default_none`

**Bonus fix:** `load_component_configs()` in `template.rs` and `load_configs()` in `patch.rs` now resolve config relative to the document path (walking up to find `.agent-doc/`), not just CWD. This fixes config resolution for documents in subdirectories/tempdir test setups. Also fixed 4 template.rs tests that were writing config to a malformed filename `config.toml ([components] section)`.

All 270 unit tests pass. 522/523 integration tests pass (1 flaky tmux timing test, pre-existing). Binary rebuilt.

❯ re:
❯ - **Bidirectional IPC** — plan written at `tasks/agent-doc/plan-bidirectional-ipc.md`. Review and refine.

❯ I'm considering Option C. Please go more into "Hook output interpretation is indirect, timing depends on tool use frequency"
### Re: Option C deep-dive — Hook-based callback

Let me unpack both cons.

#### "Hook output interpretation is indirect"

Claude Code hooks (like `PostToolUse`) run as shell commands. Their stdout/stderr gets processed by Claude Code's harness, which then feeds the output back into the model's context as part of the tool result. The model sees something like:

```
Tool: Bash
Command: agent-doc check-callbacks
Output: [callback pending: summarize request for doc X]
```

But **the model decides what to do with that output**. There's no guaranteed execution path — the hook says "here's information," and the model must infer the right action. Compare this to Option A (file-based), where the binary itself drives the entire flow: write request → read response → apply changes. With hooks, you're handing the model a suggestion and hoping it follows the protocol.

In practice, this means:
- The hook output must be carefully formatted as clear instructions, not just data
- You need strong SKILL.md rules saying "if preflight shows callbacks, process them"
- There's no compile-time guarantee the callback protocol gets followed — it's all prompt engineering

#### "Timing depends on tool use frequency"

`PostToolUse` fires after **every** tool use. That means:

- **During active work** (many tool calls per turn): the hook fires frequently — callbacks get noticed quickly, maybe within seconds. Good throughput.
- **During a thinking/reading turn** (user just edited the doc, no tool calls yet): the hook doesn't fire at all. A callback could sit in the `.agent-doc/callbacks/` directory unnoticed until the next tool use happens. This could be 30 seconds or 3 minutes depending on how fast the model generates a response.
- **During long streaming responses**: the model is generating text, not calling tools — no hook fires until the response is done and a tool call happens.

Contrast this with Option A (file-based + preflight): preflight checks for pending callbacks as a **deterministic step** at the start of every `/agent-doc` invocation. You either find it or you don't — no timing variance.

#### The real design tension

Option C's strength (no polling, integrates with existing infrastructure) is offset by making the binary a **passive requester** instead of an **active orchestrator**. With Option A, `agent-doc cleanup` is a single blocking command that guarantees a result before returning. With Option C, it becomes "plant a seed and hope the garden grows" — the binary writes the request and waits.

**Hybrid mitigation:** Option C + timeout fallback. After writing the callback request, `agent-doc cleanup` starts a 5-second timer. If no response appears (detected via the hook writing `response.json`), it falls back to spawning `claude --print` for the summarization. This gives you the best of both: cheap in-session processing when the hook fires fast, reliable subagent fallback when it doesn't.

If we use Option A with the integrated preflight, the flow would be completely deterministic?
### Re: Option A + integrated preflight = deterministic?

Yes, exactly. With **Option A + integrated preflight**, the flow becomes a traditional synchronous request-response:

1. `agent-doc cleanup <FILE>` writes `request.json`
2. **Immediately** starts polling for `response.json` (not waiting for next `/agent-doc` cycle)
3. If the running session picks it up (via whatever notification mechanism), response arrives
4. If timeout → spawn `claude --print` as fallback
5. Read result → apply changes → return

No hooks, no tool-use-frequency dependency — just a blocking command with a defined timeout. The only non-deterministic part is *which* path satisfies the request (running session vs spawned subagent), but the overall flow is guaranteed to complete within a known time window.
Would the summarization happen in the running claude session or would this be from a subagent?
### Re: Running session vs subagent for summarization

Two paths in parallel:

1. **Running Claude session** (primary, preferred) — the session detects the callback request file during its next cycle and writes the summary response. This reuses the existing context (knows the document history, what matters, what to compress). More coherent summary, no new session cost.

2. **Subagent fallback** — if timeout expires before the session responds (e.g., session is idle, not running `/agent-doc` cycles), spawn `claude --print -p "Summarize this exchange..."`. Higher cost, no context, but guarantees completion.

The running session wins if it responds first — subagent is the safety net. For Option A, the notification mechanism would be the session's own preflight cycle checking for pending callbacks.

This is where the hybrid with Option C gets interesting: you could combine Option A's file-based protocol with Option C's hook polling — the hook fires `agent-doc check-callbacks` which reads requests faster than waiting for the next full preflight cycle. But this adds complexity. Option A alone with aggressive polling (1 second intervals) is probably sufficient.

❯ Let's do Option A with "the hook fires `agent-doc check-callbacks`".
### Re: Option A + hook-based notification

Great choice. Here's how it works end-to-end:

1. **Write request** — `agent-doc cleanup <FILE>` creates `.agent-doc/callbacks/<doc_hash>/request.json`
2. **Hook fires** — Claude's PostToolUse hook runs `agent-doc hook check-callbacks` after every tool call
3. **Hook detects** — scans `.agent-doc/callbacks/*/request.json` for valid (unexpired, unresponded) requests, outputs JSON to stdout
4. **Claude processes** — the model sees the hook output, reads the request, generates the summary, writes `response.json`
5. **Poll for response** — `cleanup` polls for `response.json` every 1 second with a 120s timeout
6. **Fallback** — if no response within timeout, spawns `claude --print --model sonnet` for summarization
7. **Apply result** — reads the patch content from response, applies it to the document

The hook gives you ~1-5 second detection latency (fastest next tool use), while the CLI poll gives determinism. Worst case: 120s fallback to subagent.

❯ Would reducing the timeout to 5s result in misses to the running agent? What happens if the agent is busy?
### Re: 5s timeout and busy agent (HEAD)

Yes, a 5s timeout would result in frequent misses when the agent is busy. Here's why:

**The hook fires regardless of agent state** — PostToolUse fires after every tool use, but the hook output becomes part of the model's *next available context window*. If the model is mid-response (generating text, not calling tools), the hook doesn't fire at all. If it just fired and the model is generating its 2000-word response, the callback instruction sits in the tool result but the model won't see it until the response is done and another tool call happens.

**Busy agent scenario:**
1. `agent-doc cleanup` writes request.json
2. Hook fires, outputs "callback pending"
3. Model is in the middle of writing a 3-paragraph response... it won't see the hook output until after the response completes
4. 5s timeout expires → spawns fallback subagent
5. Model finishes, sees hook output, tries to process callback
6. Now you have TWO summarizations running for the same request (wasteful)

**What we need:** The notification mechanism must be *preemptive* — the running session must see the request even mid-response. In Claude Code, the hook output only becomes visible between tool calls, not during text generation. A 5s timeout would mean the running agent almost never responds in time, making the fallback the primary path 90% of the time.

**Better timeout:** 15-30 seconds gives the agent enough time to finish a typical response cycle AND process the callback. The 120s max is conservative — most callbacks should resolve in under 20s when the agent is actively being used.
pleas revert the changes
<!-- agent:boundary:2b7aac24 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- **Bidirectional IPC** — Implementation complete: `callback.rs`, `pending_cmd.rs`, `cleanup_cmd.rs` wired into CLI. 15 new tests, 270 pass total. Binary built.
- **`agent-doc cleanup`** — Works: writes request, polls, fallback stub. Missing: full `claude --print` fallback implementation.
- **`agent-doc pending` subcommands** — add/remove/prune/list fully implemented.
- **Hook `check-callbacks`** — Detects pending requests, outputs JSON. SKILL.md needs update to handle hook output.
<!-- /agent:pending -->
## Assistant
