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
- [x] HTML email sending (multipart/alternative)
- [x] `collapsible_if` refactor (61 instances across 26 files)
- [ ] Commit & release changes
- [ ] Attachment phases 2-3 (inline images, clipboard)
- [ ] `corky sync imports` config dispatcher
- [ ] `corky doc upload`
- [ ] `corky schedule run`
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
**Released v0.15.3** — CUDA transcription build fix.

- `whisper-rs` 0.15→0.16, `whisper-rs-sys` 0.14.1→0.15.0, `bindgen` 0.71.1→0.72.1
- All tests pass, no code changes
- Published: crates.io, PyPI, GitHub Releases
- GitHub release: https://github.com/btakita/corky/releases/tag/v0.15.3
- Binary installed locally (v0.15.3)
- Parent submodule pointer update

Implement multipart/alternative

Do `collapsible_if` refactor.

Add agent:status component to this file.

### HTML Email Sending — Implementation Plan

Investigated `src/draft/mod.rs` — confirmed the issue. `compose_email()` sends raw markdown as `text/plain` (lettre default). Bold, links, headers all appear as literal markdown syntax in email clients.

**Changes needed:**

1. **Add `pulldown-cmark` dependency** to `Cargo.toml`
2. **Modify `compose_email()`** in `src/draft/mod.rs`:
   - Convert markdown body → HTML via `pulldown-cmark`
   - Without attachments: `MultiPart::alternative()` with `SinglePart::plain(body)` + `SinglePart::html(html_body)`
   - With attachments: `MultiPart::mixed()` containing `MultiPart::alternative(plain + html)` + attachment parts
3. **No new flags** — default behavior, always sends both plain text and HTML

**Key details from code review:**
- `lettre 0.11` already supports `MultiPart::alternative()` and `SinglePart::html()`
- `mime_guess` already present for attachments
- Both `send_email()` (SMTP) and `push_to_drafts()` (IMAP) use the same `Message`, so HTML works for both paths
- Tests in `tests/test_draft.rs` need HTML output assertions

Ready to implement when you want to start this. Estimated ~50 lines changed across 2 files.

go

### Implementation Complete

**1. HTML Email Sending (multipart/alternative)** — Done

- Added `pulldown-cmark = "0.12"` to `Cargo.toml`
- New `markdown_to_html()` helper in `src/draft/mod.rs` — uses strikethrough, tables, smart punctuation
- `compose_email()` now always builds `MultiPart::alternative()` (plain + HTML)
- With attachments: `MultiPart::mixed()` wraps `alternative` + attachment parts
- 4 new tests: `test_markdown_to_html_basic`, `test_markdown_to_html_links_and_lists`, `test_compose_email_contains_html`, `test_compose_email_with_attachment_contains_html`

**2. `collapsible_if` Refactor** — Done

- `cargo clippy --fix` collapsed 61 nested `if` statements across 26 source files
- Uses Rust 2024 `let` chains (`if let ... && let ...`)
- Removed `[lints.clippy] collapsible_if = "allow"` override from `Cargo.toml`
- Files: `accounts.rs`, `app_config.rs`, `cal/auth.rs`, `contact/from_conversation.rs`, `contact/info.rs`, `draft/migrate.rs`, `draft/mod.rs`, `filter/gmail_auth.rs`, `help.rs`, `mailbox/add.rs`, `mailbox/remove.rs`, `mailbox/rename.rs`, `mailbox/validate_draft.rs`, `main.rs`, `resolve.rs`, `schedule.rs`, `social/auth.rs`, `social/mod.rs`, `social/profiles.rs`, `sync/imap_sync.rs`, `sync/manifest.rs`, `sync/sms_import.rs`, `transcribe/audio.rs`, `transcribe/model.rs`, `upgrade.rs`, `watch.rs`

**3. Status Component** — Added below.

`make check` passes clean — all 199+ tests pass, clippy clean. Changes are uncommitted in `src/corky/`, ready for review.

Commit & release changes
<!-- /agent:exchange -->
