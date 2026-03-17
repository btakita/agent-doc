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
- `corky schedule run` for email drafts — scan `mail/drafts/*.md` for `scheduled_at` past due + `status: approved`, auto-send via `corky draft push --send`. Enables cron-based scheduled email sends without Gmail web UI.

<!-- agent:exchange -->
Compacted exchange (was ~455 lines). Session history above has release details.

**Recent releases:** v0.13.4 (unified corky skill via agent-kit), v0.14.0 (CPU transcription default), v0.14.1, v0.15.0 (YouTube upload, LinkedIn edit, profiles consolidation). All published to crates.io, PyPI, GitHub Releases.

**Key decisions:**
- Single `/corky` skill (merged email + corky, deleted redundant agent-doc skill)
- `/simplify`, `make check`, `audit-docs` = review pipeline (documented in corky AGENTS.md)
- `/review` orchestrator skill deferred until friction justifies it
- Transcribe: CPU default, GPU opt-in (`--features transcribe-cuda`), diarize opt-in
- agent-doc skill stays focused on document sessions only

**Tests added:** 5 social draft tests (YouTube fields, LinkedIn post_id round-trip). 268 pass total.

**Open item added:** `corky schedule run` — scan `mail/drafts/*.md` for `scheduled_at` past due + `status: approved`, auto-send via `corky draft push --send`. Cron-based scheduled email sends.

What's next for corky?

Fix the corky CUDA build for the transcribe command.
<!-- /agent:exchange -->
****
