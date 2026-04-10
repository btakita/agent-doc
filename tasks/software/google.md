---
agent_doc_session: 39ffe113-30d6-4186-8eee-e50634d306c3
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ Please look at https://www.skills.google/
❯ Can we install the google skills to interact with google docs?
Would corky benefit from the google skills?
### Re: skills.google

[skills.google](https://www.skills.google/) is Google's **cloud learning platform** — Vertex AI, Gemini, certifications, hands-on labs. It is not a plugin or MCP system; it offers no installable integrations.

### Re: Can we install google skills to interact with Google Docs?

Not from skills.google — it's a training site, not an integration hub.

The correct path for Google Docs integration is the **Google Workspace MCP** or the Google Drive/Docs REST API. This session already has Gmail and Calendar MCPs (`mcp__claude_ai_Gmail__authenticate`, `mcp__claude_ai_Google_Calendar__authenticate`) — Google Docs/Drive would follow the same pattern (an MCP server, not a skill file).

To add Google Docs support you'd need to install or configure a Workspace MCP that covers Drive/Docs, then authenticate it similarly to Gmail.

### Re: Would corky benefit from the google skills?

Corky handles correspondence: email (Gmail API), LinkedIn, YouTube. A Google Drive/Docs MCP would let corky read drafts or content directly from Google Docs — useful if you author correspondence or content in Docs before sending.

Current corky workflow: local files → Gmail/LinkedIn/YouTube APIs. If drafts live in Google Docs rather than local files, the integration would be valuable. If not, the benefit is minimal.

❯ Lets add a feature to add useful skills + runbooks from https://github.com/WadeWarren/gws-claude-plugin into corky
### Re: Add gws skills + runbooks into corky

[WadeWarren/gws-claude-plugin](https://github.com/WadeWarren/gws-claude-plugin) wraps a `gws` CLI with 92 skills across 13 Google Workspace services (Gmail, Docs, Drive, Sheets, Slides, Calendar, Chat, Forms, Tasks, Keep, Meet, Classroom, Admin). Key gap it fills vs. native Claude: full Gmail send with attachments, full Docs/Sheets/Slides read-write, Google Chat/Tasks/Keep.

**What's useful for corky specifically:**
- Gmail: real send + attachment support (corky currently drafts only)
- Docs/Drive: read drafts from Docs, write outputs back
- Personas: "Executive Assistant" — close match to corky's correspondence role
- Recipes: `team-announcement`, `attachment-management`, `expense-tracking` all map to corky use cases

**Proposed integration (additive, no new Rust required):**

1. **New skill:** `src/corky/.claude/skills/gws/SKILL.md` — routes to gws personas/recipes, documents prerequisites (`gws` CLI installed, OAuth configured)
2. **New runbooks** under `src/corky/.agent/runbooks/`:
   - `gws-gmail-send.md` — send with attachments via `gws gmail send`
   - `gws-docs-draft.md` — create/update a Google Doc draft before sending
   - `gws-workspace-announcement.md` — team announcement recipe
3. **Install prerequisite:** document `gws` CLI install path in corky's `AGENTS.md`

The `gws` CLI handles execution; the skill layer in corky orchestrates the workflow. No Rust code changes needed for an initial integration.

**Next step:** Clone/review the gws plugin repo to confirm the CLI interface before writing the skill and runbooks.

❯ do
❯ - [ ] Review gws CLI interface from https://github.com/WadeWarren/gws-claude-plugin
❯ - [ ] Add `src/corky/.claude/skills/gws/SKILL.md`
❯ - [ ] Add `src/corky/.agent/runbooks/gws-gmail-send.md`
❯ - [ ] Add `src/corky/.agent/runbooks/gws-docs-draft.md`
❯ - [ ] Document gws install prerequisite in corky AGENTS.md
### Re: do (HEAD)

All tasks complete.

**[done] Review gws CLI interface** — gws is a Claude Code *plugin* (not standalone CLI), invoked via `/gws-*` slash skills. 92 skills across 13 Workspace services. Install: `claude plugin install gws`. Auth: `gws auth login`.

**[done] `src/corky/.claude/skills/gws/SKILL.md`** — created. Documents all services (Gmail, Docs, Drive, Sheets, Calendar), personas, recipes, prerequisites, and links to runbooks.

**[done] `src/corky/.agent/runbooks/gws-gmail-send.md`** — created. Covers when to use gws vs. corky native, send steps, threading for replies, confirmation gate, safety rules.

**[done] `src/corky/.agent/runbooks/gws-docs-draft.md`** — created. Three workflows: read-from-Docs→send-via-corky, create-new-Doc, post-mortem/announcement recipes.

**[done] Document gws prerequisite in corky AGENTS.md** — added `## Google Workspace Integration (gws)` section with install commands and when-to-use decision table.
<!-- agent:boundary:b0428dbe -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->

<!-- /agent:pending -->
