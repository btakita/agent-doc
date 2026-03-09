---
agent_doc_session: 584ef929-37ee-47f6-b3d8-d4266ad31ee9
tmux_session: '0'
agent_doc_mode: template
---

## Session History

### v0.12.1 — Email attachments ([PR #17](https://github.com/btakita/corky/pull/17))
- `--attach` flag, `attachments:` YAML frontmatter, `multipart/mixed` with `mime_guess`

### v0.12.2 — `corky social` → `corky linkedin` ([PR #18](https://github.com/btakita/corky/pull/18))
- BREAKING: platform-specific top-level subcommands (`corky linkedin`, `corky bluesky`)

### v0.12.3 — Filter drift detection
- `corky filter check` — read-only comparison of local `.corky.toml` vs live Gmail filters
- `corky watch` runs hourly drift check, warns on mismatch

### v0.12.4 — Rust 2024 edition ([PR #19](https://github.com/btakita/corky/pull/19))
- `set_var`/`remove_var` → `unsafe {}`, `collapsible_if = "allow"` lint, instruction-files v0.1.4

### v0.13.0 — Document building
- `corky doc build` — markdown → PDF (pandoc + weasyprint) or DOCX (pandoc native)
- CSS template system, `check_tool()` with install links, SPEC.md § 14

### v0.13.1 — Google Calendar + SMS import
- `corky cal auth/list/delete` — Google Calendar OAuth2, event listing, deletion
- `corky sync sms-import` — SMS Backup & Restore XML import, groups by phone number

### v0.13.2 — OAuth fixes
- Non-interactive OAuth in watch mode (never opens browser from background daemon)
- Auth callback timeout: 120s → 300s

### v0.13.3 — Calendar create + check
- `corky cal create <SUMMARY> <START> <END>` — RFC 3339 or YYYY-MM-DD all-day
- `corky cal check <START> <END>` — availability check with busy/free summary
- SPEC.md § 15 (full calendar section)

### Decisions
- Gmail filters stay manual-only (`corky filter push` is destructive, no undo)
- Keep shelling out to pandoc/weasyprint (Rust PDF crates don't support CSS)
- SMS import via SMS Backup & Restore XML (de facto Android standard)
- `[imports]` config in `.corky.toml` for multi-source import (SMS, Telegram, Slack) — designed, not yet implemented

### Open items
- Attachment phases 2-3 (inline images, clipboard)
- `collapsible_if` clippy refactor (48 instances)
- `corky sync imports` config dispatcher (designed in session, not implemented)
- `corky doc upload` (stashed, coming soon)

<!-- agent:exchange -->
Compacted 650 lines → ~50. Converted to template mode. What's next for corky?

Does the corky library have a SKILL.md...similar to agent-doc and webmaster?
If not, does it make sense to have a corky SKILL.md?
Corky already has two skills in `src/corky/.claude/skills/`:

- **`email`** (`SKILL.md`) — drafting, inbox review, contact enrichment, thread sync
- **`agent-doc`** (`SKILL.md`) — the document session skill (shared with agent-doc repo)

There is no top-level "corky" skill. **Yes, it makes sense to add one** — a `corky` skill would be the umbrella that orchestrates across all corky capabilities:

**What a `corky` SKILL.md would cover:**
- Calendar management (`corky cal list/create/check/delete`)
- Document building (`corky doc build`)
- Filter management (`corky filter check/push`)
- SMS import (`corky sync sms-import`)
- LinkedIn posting (`corky linkedin`)
- Watch daemon (`corky watch`)
- The email skill already handles correspondence — the corky skill would be for everything else

**Two options:**
1. **Single `corky` skill** — replaces `email`, covers all commands. Invoke as `/corky`.
2. **Keep `email` separate, add `corky`** — email stays focused on correspondence, corky covers calendar, docs, filters, imports. Users invoke `/email` for drafting, `/corky` for everything else.

Option 2 is cleaner — email has a distinct workflow (read thread → draft → review) that benefits from a focused skill. The corky skill handles utility commands.

Want me to create the `corky` SKILL.md?

Yes
Use agent-kit
Use agent-kit

Why would corky have a separate agent-doc skill? agent-doc already has a skill. Is the corky agent-doc skill redundant?
**Agent-doc skill in corky: yes, redundant.** The corky copy at `src/corky/.claude/skills/agent-doc/SKILL.md` is 98 lines — an older version. The canonical one (installed by `agent-doc skill install`) is 170 lines with claim detection, recovery, template mode, and auto-update. Corky's copy should be deleted — `agent-doc skill install` handles installation to wherever the agent session runs.

**Creating corky SKILL.md with agent-kit:**

Plan:
1. Add `agent-kit` dependency to corky's `Cargo.toml`
2. Bundle a `SKILL.md` via `include_str\!("../SKILL.md")` in corky's source
3. Wire `corky skill install/check/uninstall` commands using `agent_kit::skill::SkillConfig`
4. Write the actual `SKILL.md` content covering calendar, docs, filters, imports, linkedin, watch
5. Delete `src/corky/.claude/skills/agent-doc/` (redundant)
6. Keep `src/corky/.claude/skills/email/` (distinct workflow, installed by corky)

The email skill should also be bundled via agent-kit and installed via `corky skill install` alongside the main corky skill.

Shall I implement?

The email skill is separate from the main corky skill? Why?
Fair point — no good reason to keep them separate. The email skill was created before corky had calendar/docs/filters. Now corky is one tool, so one skill.

**Revised plan — single `corky` skill:**
1. Add `agent-kit` dependency to corky's `Cargo.toml`
2. Write `SKILL.md` at corky repo root — covers ALL capabilities:
   - Email: inbox review, drafting, threading, contact enrichment
   - Calendar: list, create, check, delete events
   - Documents: build PDF/DOCX
   - Filters: check drift, push
   - Imports: SMS, Telegram, Slack
   - LinkedIn: posting
   - Watch daemon
3. Bundle via `include_str\!("../SKILL.md")` + `agent_kit::skill::SkillConfig`
4. Wire `corky skill install/check/uninstall`
5. Delete `src/corky/.claude/skills/agent-doc/` (redundant)
6. Delete `src/corky/.claude/skills/email/` (merged into corky skill)
7. Users invoke `/corky` for everything

Implementing now.

Do you have anything about diarized transcription of audio recordings?
<!-- /agent:exchange -->
****
