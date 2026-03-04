---
agent_doc_session: 584ef929-37ee-47f6-b3d8-d4266ad31ee9
tmux_session: claude
---

## Session History

### v0.12.1 — Email attachments ([PR #17](https://github.com/btakita/corky/pull/17))

- `--attach` flag on `corky draft new`, `attachments:` YAML frontmatter field
- Sends as `multipart/mixed` with `mime_guess` for content-type detection
- Files validated at send time, not draft time
- Future phases: inline image embedding (Phase 2), clipboard attachment (Phase 3)

### v0.12.2 — `corky social` → `corky linkedin` ([PR #18](https://github.com/btakita/corky/pull/18))

- BREAKING CHANGE: all social commands scoped under platform-specific subcommand
- Convention: each platform gets its own top-level subcommand (`corky linkedin`, `corky bluesky`)
- Enforced by SPEC.md § 12.7, not code abstractions — complexity grows only when a second platform arrives
- Internal `src/social/` module unchanged, still platform-aware

### Cleanup: `bin/build-mail-filters` deleted

- Superseded by `corky filter build` and `corky filter push`

### Decision: Gmail filters stay manual-only

- `corky filter push` is destructive (deletes all, recreates). No Gmail undo.
- Manual `corky filter push` is the safety feature.

### v0.12.3 — Filter drift detection ([PR](https://github.com/btakita/corky/releases/tag/v0.12.3))

- `corky filter check` — read-only comparison of local `.corky.toml` vs live Gmail filters
- `corky watch` runs hourly drift check, warns on mismatch, never auto-pushes
- 120 lines in `filter/check.rs`, 4 tests

### v0.12.4 — Rust 2024 edition ([PR #19](https://github.com/btakita/corky/pull/19))

- `set_var`/`remove_var` → `unsafe {}`, removed explicit `ref mut` patterns
- Added `collapsible_if = "allow"` clippy lint (48 instances across 21 files — future cleanup)
- instruction-files bumped to v0.1.4
- `imap-proto` macro issue is future-compat warning only (deps use their own edition)

## Assistant

**Document compacted.** 384 lines → ~45 lines. Preserved all version releases, key decisions (manual filters, social platform convention), and open follow-ups (attachment phases 2-3, collapsible_if refactor).

## User

Use the corky logo in all of the docs. Such as the readme, mdbook, etc.

## Assistant

Added the corky logo (SVG, 128px) to:

- **README.md** — centered `<img>` above the `# Corky` heading
- **docs/introduction.md** — same centered logo at the top of the mdbook landing page
- **docs/assets → ../assets** — symlink so mdbook can serve the logo from the existing `assets/` directory

The logo assets already existed (`assets/logo.svg`, `assets/logo-64.png`, `assets/logo-128.png`) — just weren't referenced anywhere.

## User

