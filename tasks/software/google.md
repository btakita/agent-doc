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
### Re: do

All tasks complete.

**[done] Review gws CLI interface** — gws is a Claude Code *plugin* (not standalone CLI), invoked via `/gws-*` slash skills. 92 skills across 13 Workspace services. Install: `claude plugin install gws`. Auth: `gws auth login`.

**[done] `src/corky/.claude/skills/gws/SKILL.md`** — created. Documents all services (Gmail, Docs, Drive, Sheets, Calendar), personas, recipes, prerequisites, and links to runbooks.

**[done] `src/corky/.agent/runbooks/gws-gmail-send.md`** — created. Covers when to use gws vs. corky native, send steps, threading for replies, confirmation gate, safety rules.

**[done] `src/corky/.agent/runbooks/gws-docs-draft.md`** — created. Three workflows: read-from-Docs→send-via-corky, create-new-Doc, post-mortem/announcement recipes.

**[done] Document gws prerequisite in corky AGENTS.md** — added `## Google Workspace Integration (gws)` section with install commands and when-to-use decision table.

❯ I don't want corky to depend on gws. I want corky to fork gws. Plan to implement SKILL->runbooks and binary Rust logic
### Re: Fork gws into corky — implementation plan

**Key finding:** Corky already has significant Google Workspace coverage:
- `corky doc read/write` — Google Docs read + write ✅
- `corky doc sheet` — Sheets read ✅
- `corky doc upload` — Drive upload ✅
- `corky cal` — Calendar full CRUD + auth ✅
- `src/sync/gmail_api_sync.rs` — Gmail API sync ✅

**What's missing (gap to fill):**
- Gmail **send** with attachments (draft only today)
- Sheets **write** (`doc sheet` only reads)
- Google Chat (`src/social/chat.rs`)
- Google Tasks
- Google Slides (write)
- Google Forms (low priority)

**Plan: fork gws as native corky Rust modules**

No new `corky gws` umbrella — extend existing command structure organically.

---

#### Phase 1: Revise skill + runbooks (no gws plugin dependency)

Remove all `/gws-*` skill references from the files we just created. Replace with `corky` CLI commands.

- `src/corky/.claude/skills/gws/SKILL.md` → rename/revise to document `corky doc`, `corky cal`, `corky social chat`, `corky tasks`
- `src/corky/.agent/runbooks/gws-gmail-send.md` → revise to use `corky draft send --attachment`
- `src/corky/.agent/runbooks/gws-docs-draft.md` → revise to use `corky doc read/write`
- `src/corky/AGENTS.md` → remove gws plugin install instructions

#### Phase 2: Gmail send with attachments

New: `corky draft send --attachment <file> [--attachment <file>...]`

- `src/corky/src/draft/send.rs` — Gmail API send via `POST /gmail/v1/users/me/messages/send`
- MIME multipart encoding (base64 + RFC 2822 headers)
- Reuse OAuth token from `filter/gmail_auth.rs`
- CLI: `corky draft send <draft-file> [--attachment path]`

#### Phase 3: Sheets write

New: `corky doc sheet-write <sheet> <range> <csv-file>`

- Extend `src/corky/src/doc/sheets.rs` with `write()` function
- `PUT /v4/spreadsheets/{id}/values/{range}` with `valueInputOption=USER_ENTERED`
- Needs `SHEETS_SCOPE` (not readonly) — add to `filter/gmail_auth.rs`

#### Phase 4: Google Chat

New: `corky social chat send <space> <message>`

- `src/corky/src/social/chat.rs`
- `POST https://chat.googleapis.com/v1/spaces/{name}/messages`
- OAuth scope: `chat.messages`
- CLI: `corky social chat send <space-name> "<message>"`

#### Phase 5: Google Tasks

New: `corky tasks list/add/done`

- `src/corky/src/tasks/` module
- Tasks API: `https://tasks.googleapis.com/tasks/v1/lists/{tasklist}/tasks`
- OAuth scope: `tasks`
- CLI: `corky tasks list [--tasklist id]`, `corky tasks add "<title>"`, `corky tasks done <id>`

#### Phase 6: Skills + runbooks (corky-native)

After Phase 2-5, update skill and runbook files to document the complete corky-native Google Workspace surface. No external dependencies.

---

**Scope review from gws (92 skills → what corky covers):**

| gws service | corky status after phases 1-5 |
|-------------|-------------------------------|
| Gmail | full (sync + send + attachments) |
| Drive | upload ✅ (list/download: add later) |
| Calendar | full CRUD ✅ |
| Docs | read + write ✅ |
| Sheets | read + write (after phase 3) |
| Chat | send (after phase 4) |
| Tasks | list/add/done (after phase 5) |
| Slides | out of scope (low use) |
| Forms | out of scope (low use) |
| Meet | out of scope (low use) |
| Keep | out of scope |
| Classroom | out of scope |
| Admin | out of scope |

Ready to start with Phase 1 (no code changes, just update skill/runbook files)?
❯ Write the plan into a spec md file.
### Re: Write the plan into a spec md file

Written to `src/corky/tasks/integrations/google-workspace.md`.

Covers:
- Current state (what's already implemented + OAuth scopes)
- Phase 1–6 with file paths, API endpoints, function signatures, scope constants
- Full OAuth scope table (existing ✅ vs. new)
- Service coverage summary

❯ Implement src/corky/tasks/integrations/google-workspace.md
### Re: Implement google-workspace.md (HEAD)

All 6 phases complete. Build passes (0 errors).

**Phase 1 — Skill + runbook cleanup:**
- `src/corky/.claude/skills/gws/SKILL.md` — rewritten to document corky-native commands only; no gws plugin refs
- `src/corky/.agent/runbooks/gws-gmail-send.md` → rewritten using `corky draft send`
- `src/corky/.agent/runbooks/gws-docs-draft.md` → rewritten using `corky doc read/write`
- `src/corky/AGENTS.md` — `## Google Workspace Integration` section replaced with corky-native command reference; plugin install instructions removed

**Phase 2 — Gmail send with attachments:**
- `src/corky/src/draft/send.rs` (new) — Gmail API send via `POST .../messages/send`; MIME multipart/mixed for attachments; base64url raw encoding; RFC 2047 subject encoding
- `DraftCommands::Send { file, attachments, account }` added to cli.rs
- Wired in `main.rs` → `corky::draft::send::run`

**Phase 3 — Sheets write:**
- `src/corky/src/doc/sheets.rs` — `write()` added; uses `SHEETS_SCOPE`; parses CSV with quoted-field support; `PUT .../values/{range}?valueInputOption=USER_ENTERED`
- `DocCommands::SheetWrite { sheet, range, file, account }` added to cli.rs
- `SHEETS_SCOPE` constant added to `gmail_auth.rs`

**Phase 4 — Google Chat:**
- `src/corky/src/social/chat.rs` (new) — `send()` via `POST .../spaces/{id}/messages`
- `Commands::Chat(ChatCommands)` + `ChatCommands::Send` added to cli.rs
- `CHAT_SCOPE` constant added to `gmail_auth.rs`
- `pub mod chat` added to `social/mod.rs`

**Phase 5 — Google Tasks:**
- `src/corky/src/tasks/` (new module) — `mod.rs`, `list.rs`, `add.rs`, `done.rs`
- `Commands::Tasks(TasksCommands)` + `TasksCommands::List/Add/Done` added to cli.rs
- `TASKS_SCOPE` constant added to `gmail_auth.rs`
- `pub mod tasks` added to `lib.rs`

**Phase 6 — Skills + runbooks** (done alongside Phase 1)
<!-- agent:boundary:f21690a1 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Phase 1: Revise skill + runbooks to use corky CLI (no gws plugin refs)
- [x] Phase 2: `corky draft send --attachment` (Gmail send w/ attachments)
- [x] Phase 3: `corky doc sheet-write` (Sheets write)
- [x] Phase 4: `corky chat send` (Google Chat)
- [x] Phase 5: `corky tasks list/add/done` (Google Tasks)
- [x] Phase 6: Update skill + runbooks for full corky-native surface
<!-- /agent:pending -->
