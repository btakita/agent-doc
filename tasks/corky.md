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
- **HTML email sending** — `corky draft push --send` should convert markdown body to HTML (`pulldown-cmark`), send as `multipart/alternative` (HTML + plain text fallback). Currently sends raw markdown as plain text, causing `**bold**` to appear as literal asterisks. Default behavior, no flag needed. Files: `src/draft/mod.rs` (`compose_email()`), `Cargo.toml`.

<!-- agent:status mode=replace -->
- [x] HTML email sending (multipart/alternative) — v0.16.0
- [x] `collapsible_if` refactor — v0.16.0
- [x] `corky sync imports` config dispatcher — v0.16.0
- [x] Attachment phases 2-3 — v0.16.0
- [x] Gmail API sync provider — v0.17.0
- [ ] `corky doc upload` — deferred (Google Drive OAuth design)
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
### v0.15.3 → v0.16.0 Session Summary

**v0.15.3** released — CUDA transcription build fix (whisper-rs 0.16, bindgen 0.72.1). Published crates.io/PyPI/GitHub.

**v0.16.0** released — 5 commits:
1. `9d2ff4d` — Collapsed 61 nested `if` statements across 26 files, removed `collapsible_if = "allow"` lint override (Rust 2024 let-chains)
2. `9e80cc1` — HTML email via `multipart/alternative` (pulldown-cmark 0.12, `markdown_to_html()` helper, 4 tests)
3. `30bb96a` — `corky sync imports` config dispatcher (`[[imports]]` in `.corky.toml`, routes to sms/telegram/slack handlers, 6 tests)
4. `aa073e8` — Attachment phases 2-3: inline images (`images:` YAML, CID refs, `multipart/related`) + clipboard (`corky draft attach --clipboard`, wl-paste/xclip/pngpaste)
5. `c950387` — Version bump to 0.16.0

Published: [crates.io](https://crates.io/crates/corky/0.16.0) | [PyPI](https://pypi.org/project/corky/0.16.0/) | [GitHub](https://github.com/btakita/corky/releases/tag/v0.16.0)

**Noted:** `corky schedule run` was already complete (732 lines, 26 tests). `corky doc upload` deferred (needs Google Drive OAuth design).

Release

### Released v0.17.0 — Gmail API Sync

- `e603a4e` — Gmail API sync provider (705 new lines)
  - `src/sync/gmail_api_sync.rs` — full Gmail REST API sync (messages.list, messages.get, historyId incremental)
  - `gmail_auth.rs` extended with scope-aware OAuth + login_hint
  - `sync/mod.rs` + `watch.rs` dispatch on `provider = "gmail-api"`
  - `GmailLabelState` for historyId-based incremental sync
  - `base64 = "0.22"` dependency
  - Fixed 15 collapsible_if warnings in new code + too_many_arguments lint
- `14b005c` — Version bump to 0.17.0
- Published: [crates.io](https://crates.io/crates/corky/0.17.0) | [PyPI](https://pypi.org/project/corky/0.17.0/) | [GitHub](https://github.com/btakita/corky/releases/tag/v0.17.0)
 - Binary installed locally (v0.17.0), submodule pointer updated

backfill corky's VERSIONS.md to include all releases
### Backfilled VERSIONS.md — all 39 releases (v0.1.0–v0.18.0)

Added 14 missing version entries derived from git history between adjacent tags:

| Version | Summary |
|---------|---------|
| 0.9.5 | Extract audit-docs to instruction-files crate |
| 0.10.0 | Social media posting with LinkedIn (OAuth2, drafts, publish, profiles) |
| 0.11.0 | Scheduling, topics management, draft YAML migration |
| 0.11.2 | OAuth URL encoding fix, CWD-as-data-dir, dry-run for schedule/publish |
| 0.11.3 | Watch Ctrl+C fix, 22 new tests (357 total), LinkedIn API testability |
| 0.11.4 | BREAKING: `SPECS.md` → `SPEC.md`, gitignore `.agent-doc/` |
| 0.11.5 | `corky label clear` bulk IMAP label removal |
| 0.12.0 | `corky transcribe` (whisper-rs), `corky filter` (Gmail filter management) |
| 0.15.1 | YouTube default public, `corky youtube edit` |
| 0.15.2 | YouTube embeddable flag |
| 0.15.3 | CUDA transcription build fix (whisper-rs 0.16) |
| 0.16.0 | HTML email, inline images, clipboard attach, sync imports, let-chains refactor |
| 0.17.0 | Gmail API sync provider (`provider = "gmail-api"`) |
| 0.18.0 | Built-in GCP OAuth, `corky doctor`, self-hosted guide, privacy policy |

Committed in corky submodule (`72d76f0`), pushed to GitHub, submodule pointer updated in agent-loop.
<!-- /agent:exchange -->
