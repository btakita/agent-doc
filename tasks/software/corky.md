---
agent_doc_session: 584ef929-37ee-47f6-b3d8-d4266ad31ee9
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
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
- **Gmail API threading fix (Bug 1)** — `in_reply_to` in draft YAML currently accepts a Gmail thread ID (e.g., `19d479af292d8d99`), but the `In-Reply-To` email header requires an RFC 2822 Message-ID (e.g., `<CAxxxxxx@mail.gmail.com>`). Corky should either: (a) store Message-IDs in conversation markdown files during sync, or (b) resolve thread IDs to Message-IDs via the Gmail API when composing replies. Files: `src/sync/markdown.rs`, `src/draft/mod.rs`.
- **Gmail API threading fix (Bug 2)** — `send_via_gmail_api` and `push_to_drafts_via_gmail_api` only send `{ "raw": encoded }`. Gmail API requires `"threadId"` in the request body to thread replies into existing conversations. Add `thread_id` field to draft YAML frontmatter and include it in the API request JSON. Files: `src/draft/mod.rs` (lines 371-433).

<!-- agent:status mode=replace -->
- [x] HTML email sending (multipart/alternative) — v0.16.0
- [x] `collapsible_if` refactor — v0.16.0
- [x] `corky sync imports` config dispatcher — v0.16.0
- [x] Attachment phases 2-3 — v0.16.0
- [x] Gmail API sync provider — v0.17.0
- [x] Watch bug fixes (routing scope, 404 resilience, IMAP shutdown) — v0.19.1
- [x] Write destination + pull improvements — v0.19.2
- [x] SPEC.md §6.8, §6.9, §18 — v0.19.1
- [x] Metal GPU + audio resilience — v0.19.3
- [x] YouTube playlist management — v0.21.0
- [x] Diarize default feature — v0.22.1
- [x] `corky contact push google` — v0.22.1
- [x] CLAUDE.md fallback in contact push — v0.22.1
- [x] Gmail API body decode (attachment + base64 padding) — v0.22.2
- [x] `corky sync refetch` — v0.23.0
- [ ] crates.io publish — needs token refresh
- [ ] `corky doc upload` — deferred (Google Drive OAuth design)
- [ ] Google Docs/Sheets integration — read/write Google Docs, read Google Sheets (extends Gmail OAuth scopes)
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
*Compacted. Covers v0.24.0 through v0.26.0.*

### This session

**v0.24.0** — Gmail API threading (Message-ID extraction, `thread_id` in draft YAML + API requests), account-based doc auth (`--account` flag), Google Drive upload/Docs read-write/Sheets read, contact delete. Published: crates.io, PyPI (`license-files` fix), GitHub Release.

**v0.25.0** — Multi-skill install: `corky skill install` writes 3 skills (corky, email, linkedin) + 5 runbooks. 85-92% context reduction per invocation vs monolithic.

**v0.25.1** — Skill runbook extraction: Gmail config, browser-paste, imports, transcription moved to on-demand runbooks. 236→185 lines always-loaded (21% reduction).

**v0.26.0** — Confidence-based re-transcription: scans for low-confidence blocks (< 0.4), re-transcribes with smaller audio windows, replaces only if higher confidence. Config: `confidence_threshold`, opt-out: `--no-confidence-retranscribe`.

### Key decisions
- `license-files = ["LICENSE"]` in pyproject.toml for PyPI sdist compliance
- No-hallucination rule added to CLAUDE.md Core Principles
- Multi-skill architecture: separate SKILL.md files for focused Claude Code skill matching
- Runbook extraction: procedural/reference content deferred to on-demand loading
- Confidence re-transcription: automated post-pass, not a runbook (code feature)

### Open/deferred
- Verify Google Sheets sharing / GCP test users for `brian.takita@gmail.com`
- Re-test sheet read (403 on last attempt)
- Manylinux binary wheel for PyPI (needs container/CI)
do
- Manylinux binary wheel (nice-to-have, needs container/CI)
### Re: Manylinux binary wheel — CI fixed

**Root causes (2):**
1. CI workflows only cloned `instruction-files` but not `agent-kit` (path dependency added in v0.24.0)
2. `env!("CORKY_DEFAULT_GCP_CLIENT_ID")` fails at compile time without the env var — changed to `option_env!()` with empty string fallback

**Fixes:**
- All 3 CI workflows (ci, pypi, release) now clone both `instruction-files` and `agent-kit`
- GCP credentials optional at build time — CI users get empty defaults, auth prompts at runtime if needed
- Committed `b9b56b9`, pushed, re-tagged v0.26.0

**PyPI workflow triggered:** https://github.com/btakita/corky/actions/runs/23985173123

This should produce manylinux wheels for x86_64 Linux, macOS (Intel + ARM), and Windows.

Can we use a local .cargo/config.toml override (see src/agent-doc repo) while keeping references to the crate version in the corky cargo.toml?

Add this heuristic as a skill->runbook where appropriate? Likely at the agent-loop level.
### Re: .cargo/config.toml patch pattern + skill update

**Implemented in corky:**
- `Cargo.toml` now uses crate-only versions: `agent-kit = "0.3"`, `instruction-files = "0.1"`
- `.cargo/config.toml` (gitignored) patches to local paths for development
- CI workflows simplified — no more `git clone` for dependencies
- Committed `ab8bc20`, pushed, retagged v0.26.0
- PyPI workflow retriggered: https://github.com/btakita/corky/actions/runs/23985288239

**Skill updated:** `rust-package/SKILL.md` conventions section updated:
> Path dependencies: Use `.cargo/config.toml` `[patch.crates-io]` for local development (gitignored). `Cargo.toml` references crate versions only — CI resolves from crates.io, no cloning needed.

CI template comment also simplified.
### Re: Runbook at agent-loop level

Created `dev/runbooks/cargo-path-deps.md` documenting the `.cargo/config.toml` patch pattern:
- Pattern: `Cargo.toml` uses crate versions, `.cargo/config.toml` (gitignored) patches to local paths
- Why: CI resolves from crates.io, no cloning. `cargo publish` works. Local dev uses sibling checkouts.
- Existing usage: agent-doc (tmux-router), corky (agent-kit, instruction-files)

Added reference in `dev/SKILL.md` alongside other runbook links.

Now documented in two places:
1. `dev/runbooks/cargo-path-deps.md` — full runbook (agent-loop level)
2. `rust-package/SKILL.md` conventions — brief reference

Should we have the same convention in agent-doc...and also in agent-loop, agent-kit, and/or instruction-set?
### Re: Convention across repos

Checked all repos:

| Repo | Path deps | `.cargo/config.toml` | Status |
|------|-----------|---------------------|--------|
| agent-doc | tmux-router | Yes (already) | Already correct |
| corky | agent-kit, instruction-files | Yes (just fixed) | Done |
| agent-kit | None | N/A | No action needed |
| instruction-files | None | N/A | No action needed |

Both repos with cross-crate dependencies already follow the pattern. No further changes needed.

gh failures:
- https://github.com/btakita/corky/actions/runs/23985286793
- https://github.com/btakita/corky/actions/runs/23985286644
### Re: CI failures fixed

**Two issues found and fixed:**

1. **Skill test fails in CI** — `SkillConfig::new()` calls `Environment::detect()` which returns `Generic` in CI (no Claude Code env vars). Tests expected `.claude/skills/corky/SKILL.md` but got `AGENTS.md`. Fix: use `SkillConfig::with_environment(..., Environment::ClaudeCode)` to always target Claude Code paths.

2. **CUDA 12.6.3 unavailable** — Jimver/cuda-toolkit action doesn't have 12.6.3. Changed to 12.6.2.

3. **aarch64 cross-compile OpenSSL** — already uses `--features vendored-openssl` in the cross build step, so this should work. The error was because it was trying to build with the native (x86_64) OpenSSL headers.

Committed `4e260a3`, pushed, retagged v0.26.0.
fix https://github.com/btakita/corky/actions/runs/23985562273

upgrade instruction-files and other dependencies. Use updates to dependencies relating to SKILL installation.
### Re: CI fix + dependency upgrade (HEAD)

**CI fix (aarch64 cross-compile OpenSSL):**
The `cross` container lacks host-arch (x86_64) `libssl-dev`, causing `openssl-sys` build failure for host dependencies. Created `Cross.toml`:
```toml
[target.aarch64-unknown-linux-gnu]
pre-build = ["apt-get update && apt-get install -y libssl-dev pkg-config"]
```

**Dependency upgrades:**
- `agent-kit` 0.3 → 0.4, `instruction-files` 0.1 → 0.2
- `agent_kit::skill::SkillConfig` was removed in v0.4.0 (moved to separate crate, not published). Inlined the install/check logic in `skill.rs` using `agent_kit::detect::Environment` directly.
- `corky skill install` now calls `instruction_files::init()` for transitive SKILL installation — installs `instruction-files/SKILL.md` + default runbooks alongside corky's own skills.

**Changes:**
- `Cross.toml` — new, pre-installs host OpenSSL in cross container
- `Cargo.toml` — bumped agent-kit 0.4, instruction-files 0.2
- `src/skill.rs` — replaced `SkillConfig` with inline logic + `instruction_files::init()` call

All 10 skill tests pass, including new assertion for transitive `instruction-files/SKILL.md` installation.

Ready to commit and push + retag v0.26.0?
<!-- agent:boundary:c1c77168 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
- [x] Fix CI aarch64 cross-compile (Cross.toml)
- [x] Upgrade agent-kit 0.3→0.4, instruction-files 0.1→0.2
- [x] Transitive SKILL installation via instruction_files::init()
- [ ] Commit, push, retag v0.26.0
- [ ] Monitor CI after retag
- [ ] Verify sheet sharing / GCP test users
- [ ] Re-test sheet read
<!-- /agent:pending -->
