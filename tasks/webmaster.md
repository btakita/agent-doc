---
agent_doc_session: 4bd915ed-e927-4d64-8b49-f5a3c5e66153
tmux_session: '0'
agent_doc_mode: template
---

Are there any skills installed by the webmaster crate?

<!-- agent:exchange -->
No. The webmaster crate (`src/webmaster/`) is a standalone Rust CLI tool — it has no Claude Code skills installed.

**Current state:**
- **Location:** `src/webmaster/` — Rust crate, v0.1.0
- **Purpose:** Unified CLI for search engine webmaster tools (sitemap submission, search analytics, URL inspection)
- **Engines:** Google (OAuth2, working), Bing (API key, working), Yandex (stub)
- **Commands:** `webmaster auth`, `webmaster submit-sitemap`, `webmaster list-sitemaps`
- **Binary:** Built at `src/webmaster/target/release/webmaster` but NOT installed in PATH
- **Skills:** None — only agent-doc, existence, and tagpath have registered skills in `.claude/skills/`

To add a webmaster skill, you'd create `.claude/skills/webmaster/SKILL.md` with instructions for sitemap management workflows.

Does webmaster need a SKILL.md? Similar to how agent-doc has an installable SKILL.md.

### Probably not yet

**agent-doc needs a SKILL.md because** it has a complex multi-step workflow (diff, respond, 3-way merge write-back) that requires detailed agent instructions. The agent operates *inside* the document.

**webmaster doesn't need one because:**
- It's a straightforward CLI -- `webmaster submit-sitemap <url>` is self-explanatory
- No agent-in-the-loop workflow; just fire-and-forget commands
- Claude can use it via Bash tool calls; `--help` output is sufficient
- No document coordination needed

**When it would make sense:**
- A `/webmaster` slash command for multi-step workflows (discover sites, submit all sitemaps, verify indexing, report status)
- Integration with agent-doc template components (e.g., a dashboard showing sitemap status per site)

For now, just installing the binary (`cargo install --path src/webmaster`) and mentioning it in CLAUDE.md is sufficient.

More functionality will be added to webmaster.

> and mentioning it in CLAUDE.md is sufficient

Since we need to explicitlay add to CLAUDE.md, we should add a SKILL.md with the next release.

### Agreed -- SKILL.md with next release

Good point. If the CLI is growing, a bundled SKILL.md is the right vehicle -- self-updating (`webmaster skill install`), same pattern as agent-doc. Avoids manual CLAUDE.md maintenance.

**SKILL.md scope for webmaster:**
- Available commands and flags
- Authentication setup (Google OAuth, Bing API key)
- Site property format conventions
- Common workflows (submit all sitemaps, check indexing status)

Simple reference doc -- no complex agent workflow instructions needed. Just enough that Claude knows what's available and how to call it.

Ok...since I now have multiple packages & cli tools that install a SKILL.md...this is a good time to extract a common package dependency that manages SKILL.md.

Also, this new package should manage interactions with the agent loop...regardless of the model and program running the agent loop. This means compatability with a diverse range of environments such as:
- Pi
- OpenCode
- Claude Code
- Claude
- Codex
- Grok

What should we name this package? Verify the suggested names are available on crates.io

### Naming — "Ver" prefix

You started typing "Ver" — interesting direction. Some possibilities:

- **Verba** (Latin: "words") — the instruction layer that speaks to any agent
- **Vertex** — connection point between tools and agent loops
- **Verso** — a page/document term; fits the SKILL.md document management angle

But given the actual function — **managing how tools describe themselves to agent loops** — I'd suggest:

### `agent-skill`

Direct, descriptive, no ambiguity. It's what it does: manage agent skills across environments.

**What it provides as a Rust crate dependency:**

1. **`SkillManifest`** — structured representation of a tool's capabilities (commands, flags, auth requirements, workflows)
2. **`skill install`** subcommand scaffolding — `include_str!("SKILL.md")` pattern, target directory detection
3. **Environment detection** — which agent loop is running (Claude Code → `.claude/skills/`, OpenCode → its equivalent, Pi/Grok → generic markdown injection)
4. **Instruction file format abstraction** — emit SKILL.md for Claude Code, system prompt fragments for others, MCP tool descriptions for MCP-aware loops

**Environment adapter pattern:**
```
agent-skill/
  src/
    lib.rs           # SkillManifest, install/upgrade API
    detect.rs        # Detect active agent environment
    adapters/
      claude_code.rs # .claude/skills/<name>/SKILL.md
      opencode.rs    # OpenCode equivalent
      generic.rs     # Plain markdown fallback
      mcp.rs         # MCP tool_description format
```

Each tool crate (`agent-doc`, `webmaster`) depends on `agent-skill` and calls `agent_skill::install(manifest)` from its `skill install` subcommand. The adapter handles placement.

**vs. the "Ver" direction** — if you had a broader vision with that prefix (a framework beyond just skill management), I'd want to hear it. What were you going for with "Ver"?

### crates.io availability

| Name | Available? | Notes |
|------|-----------|-------|
| `agent-skill` | **Available** | Not taken |
| `verba` | Taken | Latin words library (9.4k downloads) |
| `verso` | Taken | Literate programming tool (6.5k downloads) |
| `vertex` | Taken | 3D game engine (1.4k downloads) |

`agent-skill` is the clear winner — available, self-documenting, and exactly describes the purpose.

Ready to scaffold `src/agent-skill/` when you give the go-ahead, or if you had something else in mind with "Ver", let me know.

I may want to expand the scope of agent-skill a bit. Trying to think of what other common needs that CLI tools have in integrating with different agent loops & models. 
<!-- /agent:exchange -->
