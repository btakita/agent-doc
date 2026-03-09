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

### Expanded `agent-skill` scope

Beyond SKILL.md management, here's what CLI tools commonly need when integrating with agent loops:

**1. Skill Management** (already discussed)
- `skill install` / `skill check` / `skill uninstall`
- Environment-aware placement (Claude Code, OpenCode, generic)
- Version tracking and auto-update detection

**2. Structured Output for Agents**
- Consistent JSON/markdown output format that agents can parse reliably
- Error reporting in agent-friendly format (not just stderr text)
- Progress indicators that agents can interpret (not just spinners)
- `--agent-output` flag support — machine-readable mode for any command

**3. Context Injection**
- `CLAUDE.md` / `AGENTS.md` management — append/remove sections programmatically
- MCP tool registration — generate `tool_description` JSON from the manifest
- System prompt fragments for non-file-based loops (Claude API, Grok API)

**4. Configuration Discovery**
- Unified config location pattern (`~/.config/<tool>/config.toml`)
- Project-local config (`.agent-doc/`, `.webmaster/`)
- Credential management abstraction (pass, keyring, env vars)

**5. Agent Loop Detection & Adaptation**
- Detect which agent is running (Claude Code? OpenCode? API call?)
- Adapt output verbosity, format, and error handling per environment
- Feature flags per environment (e.g., Claude Code has file tools; Codex doesn't)

**6. Instruction File Auditing**
- The `instruction-files` crate already does this — could become a dependency of `agent-skill`
- Shared audit logic for any tool that ships instruction files

**Which of these resonate?** The core question is whether `agent-skill` stays narrow (skill management + environment detection) or becomes a broader "agent integration SDK" for CLI tools. The name `agent-skill` fits the narrow scope well; if it grows into an SDK, a different name might suit better.

I think the scope should be broader. What are good names? Please verify availability.

### Broader SDK names — crates.io verified

For a crate covering skill management, structured output, context injection, environment detection, and config discovery:

| Name | Available? | Fit | Notes |
|------|-----------|-----|-------|
| `agent-kit` | **Available** | Excellent | "Kit" = toolkit — broader than one feature, practical, composable |
| `agent-wire` | **Available** | Good | "Wire" = connection layer between tools and agent loops |
| `agent-harness` | **Available** | Good | "Harness" = integration framework metaphor |
| `agent-loop-sdk` | **Available** | Decent | Explicit but verbose; couples to "agent-loop" naming |
| `tool-bridge` | **Available** | Good | Tool-centric perspective |
| `toolmaker` | **Available** | Creative | Evocative but could be confused with general dev tooling |
| `agent-sdk` | Taken | — | "Rust Agent SDK for building LLM agents" (active, v0.5.3) |
| `agent-bridge` | Taken | — | "CLI to read/compare context across Codex, Claude, Gemini" |

**Recommendation: `agent-kit`** — it's the right scope signal. A "kit" is a collection of utilities for building something; it doesn't claim to be the whole framework. It says "here's everything a CLI tool needs to integrate with agent loops."

`agent-skill` still works if you want to keep it narrow initially and rename later, but `agent-kit` gives room to grow without the name becoming a misnomer.

agent-kit

Please extract common skill logic from agent-doc. Use agent-kit in agent-doc and webmaster.

### Done — `agent-kit` extracted and integrated

**Created `src/agent-kit/`** (v0.1.0) — a shared Rust crate for CLI tools integrating with agent loops.

**What was extracted:**
- `agent_kit::skill::SkillConfig` — generic skill install/check/uninstall logic
- Takes `name`, `content` (from `include_str!`), and `version` as config
- Handles directory creation, idempotent install, content comparison, cleanup

**Updated `agent-doc`** — now depends on `agent-kit`:
- `skill.rs` reduced from 150 to 55 lines (delegates to `SkillConfig`)
- All 218 tests pass, clippy clean

**Updated `webmaster`** — now has `skill` subcommand:
- `webmaster skill install` / `webmaster skill check`
- Created `SKILL.md` with command reference, auth setup, site property format
- Depends on `agent-kit` for the install/check logic

**Test results:**
- agent-kit: 10/10 tests pass
- agent-doc: 218/218 tests pass (197 unit + 21 integration)
- webmaster: builds clean, clippy clean
Please add tests for webmaster.
Write the initial installable SKILL.md for webmaster.

**Next steps when ready:**
- Publish `agent-kit` to crates.io (reserve the name)
- Add environment detection (Claude Code vs OpenCode vs generic)
- Add more kit modules (structured output, context injection)

Make sure SKILL.md is compatible with Codex, Pi, OpenCode, and other models + agent loop runtimes.
<!-- /agent:exchange -->
