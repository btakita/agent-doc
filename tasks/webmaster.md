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
<!-- /agent:exchange -->
