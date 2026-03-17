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
**CUDA build fixed.** Bumped `whisper-rs` from 0.15 to 0.16 in `Cargo.toml`.

**Root cause:** whisper-rs-sys v0.14.1 uses bindgen 0.71.1, which generates opaque structs for `_IO_FILE` and `whisper_full_params` (size 1 byte) but still emits C-derived size assertions expecting 216/296 bytes. The subtraction underflows → compile error. This is a known bindgen issue (rust-lang/rust-bindgen#2691, #3264).

**Fix:** whisper-rs 0.16.0 pulls in whisper-rs-sys 0.15.0 which sets `bindgen::RustTarget::stable(88, 0)` to prevent incompatible layout checks.

**No code changes needed** — corky's usage of the whisper-rs API is compatible with 0.16 (`.full()` return type changed from `Result<c_int, _>` to `Result<(), _>`, but we only `?`-propagate it).

**Verified:**
- `cargo check --features transcribe-cuda` — compiles clean
- All tests pass with `--features transcribe-cuda`
- Dep changes: bindgen 0.71.1→0.72.1, whisper-rs-sys 0.14.1→0.15.0

Ready to commit in corky submodule and release when you say.

release
<!-- /agent:exchange -->
****
