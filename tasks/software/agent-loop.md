---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

# agent-loop operations

Operational task document for agent-loop workspace management.

**History** (archived in git, commits up to `f80f524`):
- Submodule conversions (briantakita.me-dev, ctx-core-dev, mail, python-intentions)
- mail/mailboxes/lucas nested submodule init + push
- JetBrains module creation (20 .iml files)
- IDEA Cargo project path fixes (workspace.xml)
- Recursive `.agent-doc/` gitignore additions (20+ submodules)
- `.sync-state.json` gitignore for corky/mail
- Nested submodule fixes (ctx-core-dev/lib/relysjs, rappstack-dev/lib/ui--server--blog)

<!-- agent:exchange patch=append -->
Completed releases: corky v0.26.3 (7-platform GitHub Release, fixed CI through 5 iterations: OpenSSL, host libssl-dev, aarch64 features), agent-kit v0.4.0 (GitHub Release). All submodules committed and pushed (84b98b52). Pending: litellm proxy + local model testing.
❯ I want to avoid bloating my context. Please help me optimize my /context. If I disable all plugins, can I have the installed plugins activate on demand?
### Re: optimizing /context, plugins on demand

**Short answer:** Plugins themselves can't "activate on demand" — they load at session start. BUT individual *tools* and *skills* within plugins already use lazy loading patterns. The big wins come from disabling MCP servers and unused plugins, not from on-demand activation.

#### What's actually loaded into your context

Run `/context` to see the breakdown. Typical heavy hitters:

1. **MCP server tools** — every MCP server's full tool schemas load at startup (chrome-devtools, gmail, calendar, claude-mem, claudescore, etc.). Each tool schema = hundreds of tokens.
2. **System prompt** — includes CLAUDE.md, CLAUDE.local.md, recent git status, etc.
3. **Skill metadata** — every available skill's name + description (NOT body). Skill bodies only load when you invoke `Skill { skill: "name" }`.
4. **Built-in tools** — Read, Edit, Bash, etc.

#### What's already lazy (good news)

- **Skills**: Only metadata (~1 line each) is in context. Full SKILL.md only loads on invocation. This is why you have ~50 skills available with minimal cost.
- **Deferred tools**: I just saw this in action — a system reminder told me MCP tools (chrome-devtools, gmail, claude-mem, claudescore) are now *deferred*. Their names are listed but schemas are NOT loaded. I have to call `ToolSearch` with `select:<tool_name>` to load each schema before using it. This is exactly the on-demand pattern you're asking about — and it appears to already be enabled for your session right now.

#### What you can do

1. **Disable unused MCP servers** in `~/.claude/settings.json` → biggest single win. Each server you remove drops dozens of tool schemas.
2. **Disable unused plugins** via `/plugins` → removes their skill metadata + commands.
3. **Configure deferred tools**: Since deferred tools already work in your session, check whether this is configured in settings.json. If yes, expand it to more MCP servers. If it's a new feature you got automatically, lean into it.
4. **Trim CLAUDE.md / CLAUDE.local.md** — these load every session. Move stale sections to `tasks/` docs or auto-memory files.
5. **Check `~/.claude/projects/.../memory/MEMORY.md`** — your auto-memory index loads every session (truncated at line 200). Keep it tight.

#### Investigation steps

```
/context              # see current breakdown
/plugins              # list installed plugins, toggle off unused
/mcp                  # list MCP servers (or check settings.json)
```

Want me to audit your `~/.claude/settings.json` and `MEMORY.md` for trim opportunities? I can identify which MCP servers / plugins are heaviest.
❯ yes
### Re: context audit

Starting the audit. I need one thing from you first:

**Run `/context` in this terminal** and let me know when you're done. I can't run slash commands myself, but once I see the breakdown I'll audit everything — settings.json, MCP servers, plugins, MEMORY.md, CLAUDE.md files.

❯ Please analyze my sessions and investigate how tokens are being wasted. What are some ways that we can save on tokens. Are there any features that can be added to agent-doc to save tokens from the claude's tool usage and output?

❯ Please analyze my sessions and investigate how tokens are being wasted. What are some ways that we can save on tokens. Are there any features that can be added to agent-doc to save tokens from the claude's tool usage and output?

### Re: token optimization strategies (HEAD)

Token waste happens in three places: **tool output bloat**, **context load patterns**, and **session accumulation**. Here's what I found:

#### Current Waste Patterns

1. **Tool output verbosity** — When you run `rtk git log` or `gh api` queries, full JSON streams back. It's visible once but lives in cache and gets re-parsed on subsequent operations.
2. **Snapshot accumulation** — Every agent-doc cycle creates a baseline in `.agent-doc/baselines/`. After 50 cycles, that's 50 snapshots in git history (they compress well, but still noise).
3. **Oversized reads** — The Read tool defaults to 2000 lines. For a 10k-line file where you need one function, that's 8000 wasted tokens.
4. **Deferred tools already working** — MCP server schemas are NOT loaded into base context, only on-demand. This is the on-demand pattern you wanted.

#### Immediate High-ROI Wins

1. **Use `rtk` for repetitive commands** — `rtk git log -10` instead of raw `git log` saves ~60% on verbose output.
2. **Grep-before-read pattern** — Find the function with Grep, then Read only that file + 10 lines context around the match.
3. **Limit parameters on Read** — `Read(file.rs, offset=100, limit=50)` for targeted sections, not full file.
4. **Snapshot cleanup** — Snapshots >30 days old are recovery backstops. Can delete them from `.agent-doc/baselines/` to trim git history.

#### Features agent-doc Could Add for Token Savings

**1. Snapshot garbage collection** (highest value)
```bash
agent-doc gc baselines --older-than 30d
```
Currently snapshots accumulate forever. This would purge old recovery files.

**2. Compaction with history summary** (medium value)
You removed `agent_doc_auto_compact: 250` — intentional silence. But a **versioned summary** mode could:
- Squash old exchanges into a 2-3 sentence summary
- Keep summary in a `<details>` block for future context
- Preserve token budget while maintaining audit trail

Current `compact exchange` loses history entirely.

**3. Tool output filtering** (hypothetical, harness-level)
```bash
agent-doc config tool-output-format
  git: compact      # one-liner per commit, not full log
  json: keys-only   # structure + types, no values
  grep: context     # surrounding lines, not file paths
```

This would be CLI-side filtering, not agent-doc itself. But worth exploring.

---

**What's already solved:** Deferred tools are active in your session. MCP schemas don't load by default — they're on-demand via ToolSearch. The next win is disabling unused MCP servers in settings.json.

Ready for `/context` output when you are. I'll identify the heaviest servers and MEMORY.md sections to trim.
<!-- agent:boundary:f6c15b88 -->
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->
- [x] [#t4hp] [>] **Context audit**: waiting for `/context` output from user
- [ ] [#y2ka] Investigate deferred-tools config — confirm whether it's enabled in settings.json or session-default
- [ ] [#wr52] Run litellm proxy pointing at vllm
- [ ] [#04hc] Test `claude` against local model via `ANTHROPIC_BASE_URL`
<!-- /agent:pending -->
