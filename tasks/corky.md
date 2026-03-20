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

When installing corky, can we detect if gpu support is available on the machine?
If there is a gpu available, please attempt to install support for `corky transcribe --gpu`.
If there is a failure on installing the gpu support, continue.

### GPU Auto-Detection for `corky transcribe`

Implemented auto-detection in both install paths. Changes in corky submodule:

**Makefile** — `make install` now checks `nvidia-smi`:
- GPU detected → tries `cargo install --path . --features transcribe-cuda`
- Build fails (missing CUDA toolkit, driver mismatch) → falls back to CPU-only
- No GPU → standard `cargo install --path .`

**install.sh** — prebuilt binary installer:
- GPU detected → tries downloading `corky-<target>-cuda.tar.gz` variant
- Binary not available for release → falls back to standard `corky-<target>.tar.gz`
- No GPU → standard binary

**SPEC.md** § 5.30 — added "GPU auto-detection" subsection documenting both paths.

All three files changed (54 insertions, 9 deletions). No CLI `--gpu` flag needed — GPU is compile-time via `whisper-rs/cuda`. The detection is purely at install time with graceful fallback.

Ready to commit in corky submodule when you approve.
Add to SPEC.md

audit docs

### Audit Docs Results

Ran `corky audit-docs` — initially flagged AGENTS.md and README.md as stale vs `src/filter/gmail_auth.rs`.

**Root cause:** v0.18.0 added built-in GCP OAuth credentials (`DEFAULT_GCP_CLIENT_ID`/`DEFAULT_GCP_CLIENT_SECRET` injected at compile time) but docs still showed manual-only GCP setup.

**Fixes applied (4 files, 68 insertions):**

| File | Change |
|------|--------|
| **SPEC.md** § 5.25 | Documented credential resolution order: config → CLI flags → env vars → built-in defaults |
| **SPEC.md** § 5.30 | GPU auto-detection subsection (already added previous cycle) |
| **README.md** | Gmail API section simplified — built-in credentials are now the default, manual GCP setup shown as optional override. Removed step 1 (Cloud Console setup), renumbered steps |
| **README.md** | Transcription section updated — GPU auto-detection noted for `make install` and `install.sh` |
| **Makefile** | GPU auto-detection in `make install` (already added previous cycle) |
| **install.sh** | GPU binary variant download (already added previous cycle) |

**Re-ran `corky audit-docs` — all clean:**
```
No issues found ✓
```

VERSIONS.md is complete (47 entries, v0.1.0–v0.18.1, no gaps).
Are the recent updates in SPEC.md, VERSIONS.md, README.md?
Are the recent updates in SPEC.md, VERSIONS.md, README.md?

### Verification: SPEC.md, VERSIONS.md, README.md

**VERSIONS.md** — Complete. All 47 entries (v0.1.0–v0.18.1), no gaps. Every version from the backfill is present.

**README.md** — Updated. Gmail API section shows built-in credentials as default, manual GCP setup as optional override. GPU auto-detection documented in transcription section. Links to VERSIONS.md for release history.

**SPEC.md** — Partially updated:
- ✅ § 5.25 — credential resolution order (config → CLI → env → built-in defaults)
- ✅ § 5.30 — GPU auto-detection (Makefile + install.sh paths)
- ❌ Gmail API sync provider — no standalone section (only IMAP sync in §6). `provider = "gmail-api"` dispatch exists in code but isn't spec'd
- ❌ v0.18.0 features — `corky doctor`, self-hosted guide, privacy policy not documented in SPEC.md

**Recommendation:** Add SPEC.md §6.2 for Gmail API sync and §19 (or similar) for `corky doctor` + self-hosted deployment. Want me to draft those sections?

Yes

When this document was autoclaimed, there was `/agent-doc tasks/agent-doc.md` left in the input.

```
 ▐▛███▜▌   Claude Code v2.1.80
▝▜█████▛▘  Opus 4.6 (1M context) · Claude Max
  ▘▘ ▝▝    ~/work/btakita/agent-loop
  ⎿  SessionStart:startup says: [agent-loop] recent context, 2026-03-20 9:20am EDT
     ────────────────────────────────────────────────────────────

     Legend: session-request | 🔴 bugfix | 🟣 feature | 🔄 refactor | ✅ change | 🔵 discovery | ⚖️ decision

     Column Key
       Read: Tokens to read this observation (cost to learn it now)
       Work: Tokens spent on work that produced this record ( research, building, deciding)

     Context Index: This semantic index (titles, types, files, tokens) is usually sufficient to understand past work.

     When you need implementation details, rationale, or debugging context:
       - Fetch by ID: get_observations([IDs]) for observations visible in this index
       - Search history: Use the mem-search skill for past decisions, bugs, and deeper research
       - Trust this index over re-reading code for past decisions and learnings

     Context Economics
       Loading: 50 observations (15,939 tokens to read)
       Work investment: 116,519 tokens spent on research, building, and decisions
       Your savings: 86% reduction from reuse

     Mar 19, 2026

     src/agent-doc/src/git.rs
       #7493  3:27 PM  🟣  Added VCS refresh signal to agent-doc commit command
       #7494  3:34 PM  🔴  Multi-section HEAD markers for all new headings

     tasks/agent-doc.md
       #7495  3:39 PM  🔵  Heading marker limited to h3 tags, needs support for any root-level heading

     src/agent-doc/src/git.rs
       #7496           🟣  Heading marker now supports any markdown heading level with root-level detection

     src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt
       #7497  3:40 PM  🔴  Fixed VCS gutter not updating by refreshing VFS before marking VCS dirty

     General
       #7498           ✅  Built release binaries with flexible heading markers and VFS refresh fix

     src/agent-doc/Cargo.toml
       #7499  3:50 PM  ✅  Version bump to 0.22.1

     src/agent-doc/VERSIONS.md
       #7500           ✅  Added v0.22.1 changelog entry

     General
       #7501           ✅  Built agent-doc v0.22.1 release binary

     src/agent-doc/Cargo.toml
       #7502  3:51 PM  ✅  Released and published v0.22.1 to git repository

     General
       #7503           ✅  Published agent-doc v0.22.1 to crates.io
       #7504           ✅  Installed v0.22.1 locally and created GitHub Release

     tasks/laura-klask.md
       #7505  3:52 PM  ✅  Amulet Technologies research and collaboration strategy documented
       #7506  3:54 PM  ✅  Task documentation committed for laura-klask
       #7507  3:55 PM  ✅  Minor follow-up edit to laura-klask task document
       #7508           🔵  Complete laura-klask task workflow revealed through diff
       #7509  4:04 PM  🟣  Email workflow automation with Gmail integration

     tasks/judith-krug.md
       #7510  4:17 PM  ✅  Judith Krug project update - client response and advisor feedback captured

     tasks/agent-doc.md
       #7511  4:32 PM  🔴  Cursor-aware ordering bug fixed in agent-doc JetBrains plugin

     mail/contacts/robert-ward/recordings/phone_20260317-144243_3109684542.txt
       #7512  4:35 PM  🔵  Phone call transcript processing workflow detected

     mail/contacts/robert-ward/recordings/phone_20260317-165018_3109684542.txt
       #7513           ⚖️  Judith Krug mortgage website architecture and build approach decided

     mail/contacts/judith-krug/CLAUDE.md
       #7514  4:36 PM  🔵  Judith Krug contact context and project requirements documented

     #S2181 Agent documentation for Judith Krug task - email response analysis with communication coaching feedback (Mar
      19, 4:37 PM)

     #S2182 Generated documentation for Judith Krug task - transcription processing initiated (Mar 19, 4:39 PM)

     #S2183 Fixed agent-doc JetBrains plugin to default exchange and findings components to append mode (Mar 19, 4:52
     PM)

     General
       #7515  4:52 PM  ✅  First transcription process completed for Judith Krug task

     mail/contacts/robert-ward/recordings/phone_20260318-024757_3109684542.txt
       #7516           🟣  Phone recording transcription completed using Whisper AI

     tasks/ken-klask.md
       #7517  4:54 PM  ✅  Agent documentation committed to repository

     #S2184 Generate documentation for Judith Krug task and prepare draft reply (Mar 19, 4:55 PM)

     tasks/agent-doc.md
       #7518  4:55 PM  ✅  Task file claimed for active work in pane
       #7519           ✅  Agent response committed to task documentation
       #7520           🔵  Agent-doc workflow state issues identified

     #S2185 Diagnosed agent-doc issues with file operations and cursor-aware ordering across Claude sessions (Mar 19,
     4:56 PM)

     tasks/judith-krug.md
       #7521  4:56 PM  ✅  Documentation committed for Judith Krug task
       #7522  4:57 PM  ✅  Comprehensive documentation update for Judith Krug sales engagement

     mail/drafts/20260319-judith-retreat-reply.md
       #7523  4:58 PM  ✅  Draft email reply created for Judith Krug retreat response

     src/corky/Cargo.toml
       #7524           🔵  GPU transcription support exists via transcribe-cuda feature flag

     mail/drafts/20260319-judith-retreat-reply.md
       #7525  4:59 PM  ✅  Email sent to Judith Krug acknowledging retreat availability

     General
       #7526  5:00 PM  🔵  NVIDIA GPU confirmed available with CUDA 13.2

     #S2186 Send reply to Judith and enable GPU-accelerated transcription in Corky (Mar 19, 5:00 PM)

     src/corky/CLAUDE.local.md
       #7527  5:00 PM  ✅  Local configuration created for GPU-accelerated corky builds

     src/agent-doc/SKILL.md
       #7528  5:01 PM  🔵  Found "Do NOT commit after writing" instruction in bundled SKILL.md files
       #7529           ✅  Updated SKILL.md to commit immediately after writing agent responses

     src/agent-doc/Cargo.toml
       #7530           ✅  Bumped agent-doc version to 0.22.2

     src/agent-doc/VERSIONS.md
       #7531  5:02 PM  ✅  Added version 0.22.2 changelog entry

     #S2187 Document agent-doc v0.22.2 release fixing SKILL.md commit instruction bug (Mar 19, 5:02 PM)

     General
       #7532  5:02 PM  ✅  Built and tested agent-doc v0.22.2 release binary

     src/agent-doc/SKILL.md
       #7533           ✅  Released and deployed agent-doc v0.22.2 to GitHub

     General
       #7534  5:03 PM  ✅  Published agent-doc v0.22.2 to crates.io and created GitHub release

     .claude/skills/agent-doc/SKILL.md
       #7535           ✅  Installed updated SKILL.md v0.22.2 to local project

     tasks/agent-doc.md
       #7536  5:25 PM  🔴  agent-doc v0.22.2 fixes SKILL.md commit instruction

     #S2188 Process Ken-Klask task and draft communication response (Mar 19, 5:25 PM)

     #S2189 Document GPU auto-detection implementation for corky installation process (Mar 19, 5:28 PM)

     tasks/ken-klask.md
       #7537  5:28 PM  ✅  Committed agent-doc response for Ken-Klask task

     tasks/corky.md
       #7538  5:30 PM  ✅  Agent documentation committed to corky task tracking
       #7539  5:31 PM  🔵  Corky project task tracking reveals comprehensive release history and current GPU work

     General
       #7540           🔵  Corky compilation requires GCP OAuth credentials at build time

     AGENTS.md
       #7541  5:32 PM  🔵  Documentation audit reveals stale files after gmail_auth.rs changes

     tasks/agent-doc.md
       #7542  5:35 PM  ✅  Agent documentation updated in task file

     #S2190 Generate and commit agent documentation for tasks/agent-doc.md (Mar 19, 5:35 PM)

     Investigated: The agent-doc tool processed tasks/agent-doc.md and updated project documentation files including
     SPEC.md and README.md. A documentation audit was performed to identify completeness issues.

     Learned: The agent-doc tool performs comprehensive documentation updates across multiple files and includes audit
     functionality to detect documentation gaps or inconsistencies. It can add structured specification sections and
     update command references.

     Completed: SPEC.md now includes section 7.14 with the full selective commit specification. README.md updated with
     terminal command documentation and corrected commit description. Documentation audit completed, reducing issues
     from 5 to 3 non-blocking items. All changes committed to git with 30 lines added to tasks/agent-doc.md.

     Next Steps: Documentation update cycle complete. The 3 remaining non-blocking audit issues are identified but not
     critical for current functionality.


     Access 117k tokens of past research & decisions for just 15,939t. Use the claude-mem skill to access memories by
     ID.

     View Observations Live @ http://localhost:37777

────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
❯ /agent-doc tasks/agent-doc.md
────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
  ⏵⏵ bypass permissions on (shift+tab to cycle)
```

### Drafted SPEC.md Sections

Added three new sections to SPEC.md (94 lines):

**§6.8 Gmail API Sync** — Full spec for the `provider = "gmail-api"` sync path:
- Config format (`[[accounts]]` with provider/user/labels/sync_days)
- OAuth2 authentication with credential resolution per §5.25
- `GmailLabelState` with `last_history_id` for incremental sync
- historyId-based incremental flow with fallback to full sync
- Message parsing, label resolution, multi-label routing
- Account verification, shutdown handling
- References §6.4/§6.5 for merge and orphan cleanup (shared with IMAP)

**§6.9 Doctor** — Spec for `corky doctor [PROVIDER]`:
- Always-run checks (config + mail directory)
- Per-provider check table (gmail-api, imap, gmail filters, linkedin, youtube)
- Output format (✓/✗ markers, 2-space indent, sections)
- Exit codes (0 pass, 1 fail)

**§18 Self-Hosted Deployment** — New top-level section:
- §18.1 Credential modes (built-in defaults vs self-hosted GCP)
- §18.2 Privacy model (local-only, no telemetry, scope list, revocation)
- References `docs/guide/self-hosted-gmail.md` and `docs/reference/privacy-policy.md`

Changes are in the corky submodule working tree (not committed). Ready to commit when you approve. Want me to also run `corky audit-docs` to verify consistency?
### SPEC.md Sections Drafted

Two items to address from this cycle:

---

**1. SPEC.md §6.8 + §18 — Ready to implement**

Based on code analysis of `src/sync/gmail_api_sync.rs`, `src/sync/mod.rs`, and `src/doctor.rs`:

**§6.8 Gmail API Sync Provider** (insert after §6.7 Contact Sync, line 951):
- Provider config: `provider = "gmail-api"` in `[accounts.<name>]`
- OAuth scope: `GMAIL_SYNC_SCOPE` via `gmail_auth::get_access_token_for_user()`
- Incremental sync: `historyId`-based via Gmail History API, stored in `.sync-state.json` as `gmail_labels.<label>.last_history_id`
- Fallback: History API failure → automatic full sync within `sync_days` window
- Thread identity: Gmail's native `threadId` (not subject-based heuristic like IMAP)
- Message parsing: `payload.headers` (case-insensitive), `base64url` body decode, `internalDate` epoch millis
- Graceful shutdown support via `Arc<AtomicBool>` signal

**§18 Doctor + Self-Hosted Deployment** (append after §17 YouTube):
- CLI: `corky doctor [PROVIDER]` — optional filter: `gmail`, `gmail-api`, `imap`, `protonmail-bridge`, `linkedin`, `youtube`
- Checks: config loaded, mail dir exists, per-account OAuth/IMAP credentials, token validity (valid/expired-with-refresh/expired/missing)
- Exit codes: 0 = pass, 1 = any failure
- Self-hosted: IMAP-only setups work with zero third-party API dependencies, tokens stored locally (`tokens.json`, 0600 perms)

Want me to write these sections directly into `src/corky/SPEC.md` now?

---

**2. Autoclaim input buffer bug**

You're describing a real UX issue: when the JetBrains plugin autoclaims `tasks/corky.md`, it leaves `/agent-doc tasks/agent-doc.md` in Claude Code's input buffer. The user's next `/agent-doc tasks/corky.md` invocation gets appended to that leftover text, producing the malformed argument `tasks/agent-doc.md/agent-doc tasks/corky.md`.

This is an **agent-doc JetBrains plugin issue** — the claim command's output or the way it injects into the input buffer isn't being consumed/cleared properly. The fix would be in `PatchWatcher.kt` — the claim injection should either:
1. Clear the input buffer before injecting, or
2. Use a separate channel (not the input buffer) for claims

I'll log this as a bug in `tasks/agent-doc-bugs.md`. Want me to investigate the plugin code for a fix?
When `❯ /agent-doc tasks/agent-doc.md` is left in the input, another gets append. Resulting with `❯ /agent-doc tasks/agent-doc.md/agent-doc tasks/corky.md`

`corky watch` is incorrectly downloading the entire INBOX for brian.takita@gmail.com
It should only be downloading the entire INBOX for btak.dev@gmail.com

```
Syncing label: INBOX
  No prior state — doing full sync
  Fetching 1853399 message(s)
```
### Investigating: INBOX Full Sync for brian.takita@gmail.com

Analyzed the config and code. Here's what I found:

**Config state** (mail/.corky.toml):
- `accounts.personal` (brian.takita@gmail.com): `provider = "gmail"` (IMAP), `labels = ["correspondence", "for-lucas", "voice-sms"]` — **no INBOX**
- `accounts.btak.dev` (btak.dev@gmail.com): `provider = "gmail-api"`, `labels = ["INBOX"]`, `sync_days = 30`

**Sync state** (.sync-state.json):
- `personal`: has state for correspondence/for-lucas/voice-sms only
- `btak.dev`: has `gmail_labels.INBOX.last_history_id = 60936`

**Code analysis:**
The message `"Syncing label: INBOX"` + `"No prior state"` + `"Fetching 1853399 message(s)"` comes from `imap_sync.rs:356,379,407` — the **IMAP path**, not gmail-api. But the personal account doesn't have INBOX in its labels, and nothing in the code injects it.

**Puzzle:** 1.8M messages is too many for btak.dev (sync_days=30), but brian.takita doesn't have INBOX configured.

**Questions to resolve:**
1. Can you paste the full watch output showing the account header (`=== Account: ... ===`) before the INBOX sync line? I need to confirm which account it's happening under.
2. Is there a `corky sync --full` that might have run recently? (That resets all state but shouldn't add INBOX)
3. Is `corky watch` running from a different directory that might pick up a different `.corky.toml`?

If the account header shows `btak.dev`, then the issue is the **history API failing** and falling back to full sync with the wrong `sync_days` (it would use 30, not 3650, so 1.8M messages wouldn't match). If it shows `personal`, something is injecting INBOX into the label set outside of what I can see in the code.

I have the process suspended because Ctrl+C did not stop the sync. You can see my attempts in the output:
```
❯ cd work/btakita/agent-loop
~/work/btakita/agent-loop main ⇡28 !36 ?5                                                                                                                                                                                           3s 14:04:02
❯ corky watch
corky watch: polling every 300s (Ctrl-C to stop)

=== Account: proton-dev (dev@briantakita.me) ===
Connecting to 127.0.0.1:1143 as dev@briantakita.me
Syncing label: INBOX
  No new messages
Syncing label: for-lucas
  Label "for-lucas" not found — skipping

=== Account: btak.dev (btak.dev@gmail.com) ===
Connecting via Gmail API as btak.dev@gmail.com
Access token expired, refreshing...
  Label: INBOX
    16 messages to process
  Error syncing btak.dev: Gmail API error (HTTP 404): {
  "error": {
    "code": 404,
    "message": "Requested entity was not found.",
    "errors": [
      {
        "message": "Requested entity was not found.",
        "domain": "global",
        "reason": "notFound"
      }
    ],
    "status": "NOT_FOUND"
  }
}


=== Account: personal (brian.takita@gmail.com) ===
Connecting to imap.gmail.com:993 as brian.takita@gmail.com
Syncing label: correspondence
  No new messages
Syncing label: for-lucas
  Fetching 1 message(s)
  Wrote: synergisticit-and-others-are-hiring-for-jr-data-scientist-ja.md
  Wrote: synergisticit-and-others-are-hiring-for-jr-data-scientist-ja.md
Syncing label: voice-sms
  No new messages

1 label(s) with new messages
Syncing lucas...
  Pushed changes

=== Account: personal (brian.takita@gmail.com) ===
Connecting to imap.gmail.com:993 as brian.takita@gmail.com
Syncing label: correspondence
  No new messages
Syncing label: for-lucas
  No new messages
Syncing label: voice-sms
  No new messages

=== Account: proton-dev (dev@briantakita.me) ===
Connecting to 127.0.0.1:1143 as dev@briantakita.me
Syncing label: INBOX
  No new messages
Syncing label: for-lucas
  Label "for-lucas" not found — skipping

=== Account: btak.dev (btak.dev@gmail.com) ===
Connecting via Gmail API as btak.dev@gmail.com
  Label: INBOX
    0 messages to process

No new messages

=== Account: proton-dev (dev@briantakita.me) ===
Connecting to 127.0.0.1:1143 as dev@briantakita.me
Syncing label: INBOX
  No new messages
Syncing label: for-lucas
  Label "for-lucas" not found — skipping

=== Account: btak.dev (btak.dev@gmail.com) ===
Connecting via Gmail API as btak.dev@gmail.com
  Label: INBOX
    0 messages to process

=== Account: personal (brian.takita@gmail.com) ===
Connecting to imap.gmail.com:993 as brian.takita@gmail.com
Syncing label: correspondence
  No new messages
Syncing label: for-lucas
  Fetching 1 message(s)
  Wrote: higher-logic-thank-you-for-your-interest-in-the-senior-front.md
  Wrote: higher-logic-thank-you-for-your-interest-in-the-senior-front.md
Syncing label: voice-sms
  No new messages

1 label(s) with new messages
Syncing lucas...
  Pushed changes

=== Account: btak.dev (btak.dev@gmail.com) ===
Connecting via Gmail API as btak.dev@gmail.com
  Label: INBOX
    0 messages to process

=== Account: personal (brian.takita@gmail.com) ===
Connecting to imap.gmail.com:993 as brian.takita@gmail.com
Syncing label: correspondence
  No new messages
Syncing label: for-lucas
  No new messages
Syncing label: voice-sms
  No new messages

=== Account: proton-dev (dev@briantakita.me) ===
Connecting to 127.0.0.1:1143 as dev@briantakita.me
Syncing label: INBOX
  No new messages
Syncing label: for-lucas
  Label "for-lucas" not found — skipping

No new messages

=== Account: btak.dev (btak.dev@gmail.com) ===
Connecting via Gmail API as btak.dev@gmail.com
  Label: INBOX
    0 messages to process

=== Account: personal (brian.takita@gmail.com) ===
Connecting to imap.gmail.com:993 as brian.takita@gmail.com
Syncing label: correspondence
  No new messages
Syncing label: for-lucas
  No new messages
Syncing label: voice-sms
  No new messages

=== Account: proton-dev (dev@briantakita.me) ===
Connecting to 127.0.0.1:1143 as dev@briantakita.me
Syncing label: INBOX
  No new messages
Syncing label: for-lucas
  Label "for-lucas" not found — skipping

No new messages

=== Account: btak.dev (btak.dev@gmail.com) ===
Connecting via Gmail API as btak.dev@gmail.com
  Label: INBOX
    0 messages to process

=== Account: personal (brian.takita@gmail.com) ===
Connecting to imap.gmail.com:993 as brian.takita@gmail.com
Syncing label: correspondence
  No new messages
Syncing label: for-lucas
  No new messages
Syncing label: voice-sms
  No new messages

=== Account: proton-dev (dev@briantakita.me) ===
Connecting to 127.0.0.1:1143 as dev@briantakita.me
Syncing label: INBOX
  No new messages
Syncing label: for-lucas
  Label "for-lucas" not found — skipping

No new messages

=== Account: personal (brian.takita@gmail.com) ===
Connecting to imap.gmail.com:993 as brian.takita@gmail.com
Syncing label: correspondence
  No new messages
Syncing label: for-lucas
  No new messages
Syncing label: voice-sms
  No new messages

=== Account: btak.dev (btak.dev@gmail.com) ===
Connecting via Gmail API as btak.dev@gmail.com
  Label: INBOX
    0 messages to process

=== Account: proton-dev (dev@briantakita.me) ===
Connecting to 127.0.0.1:1143 as dev@briantakita.me
Syncing label: INBOX
  No new messages
Syncing label: for-lucas
  Label "for-lucas" not found — skipping

No new messages

=== Account: personal (brian.takita@gmail.com) ===
Connecting to imap.gmail.com:993 as brian.takita@gmail.com
Syncing label: correspondence
  No new messages
Syncing label: for-lucas
  No new messages
Syncing label: voice-sms
  No new messages

=== Account: proton-dev (dev@briantakita.me) ===
Connecting to 127.0.0.1:1143 as dev@briantakita.me
Syncing label: INBOX
  No new messages
Syncing label: for-lucas
  Label "for-lucas" not found — skipping

=== Account: btak.dev (btak.dev@gmail.com) ===
Connecting via Gmail API as btak.dev@gmail.com
  Label: INBOX
    0 messages to process

No new messages

=== Account: btak.dev (btak.dev@gmail.com) ===
Connecting via Gmail API as btak.dev@gmail.com
  Label: INBOX
    2 messages to process
  Wrote: your-new-privacy-settings-on-play.md
  Wrote: brian-finish-setting-up-your-galaxy-s25-ultra-device-with-go.md

=== Account: personal (brian.takita@gmail.com) ===
Connecting to imap.gmail.com:993 as brian.takita@gmail.com
Syncing label: correspondence
  No new messages
Syncing label: for-lucas
  No new messages
Syncing label: voice-sms
  No new messages
Syncing label: INBOX
  No prior state — doing full sync
  Fetching 1853399 message(s)
  Wrote: re-sveltejs-svelte-structured-code-generation-3539.md
  Wrote: re-lightningnetwork-lnd-lncli-incoming-and-outgoing-peer-ali.md
  Wrote: re-thinking-about-going-back-to-source-hut-mailing-list.md
  Wrote: journalism-is-in-dire-straits-we-re-beating-the-odds.md
  Wrote: hi-brian-your-score-didn-t-change.md
  Wrote: re-atom-atom-uncaught-error-pane-has-been-destroyed-20026.md
  Wrote: re-lightningnetwork-lnd-htlcswitch-routerrpc-support-unencry.md
  Wrote: re-publiclab-plots2-fix-main-image-select-field-not-function.md
  Wrote: if-cnn-didn-t-hate-trump-before-it-will-soon.md
  Wrote: re-sveltejs-svelte-fix-ensure-batch-exists-when-resetting-a-.md
  Wrote: re-neo4j-neo4j-respect-dbms-allow-upgrade-concerning-logentr.md
  Wrote: successfully-published-ctx-core-version-app-5-0-2.md
  Wrote: the-open-food-network-turns-10.md
  Wrote: new-post-free-signed-paperbacks-pay-only-shipping.md
  Wrote: art-of-living-flipkart-deal-of-the-day-rodid-solid-mens.md
  Wrote: take-a-chill-pill-it-s-time-to-relax-this-weekend.md
  Wrote: re-lightningnetwork-lnd-htlcswitch-implement-strict-forwardi.md
  Wrote: re-mrdoob-three-js-nodebuilder-fix-missing-linearencoding-22.md
  Wrote: the-new-champion-from-wrestlemania.md
  Wrote: re-atom-atom-uncaught-error-enoent-no-such-file-or-directory.md
  Wrote: re-holochain-holochain-rust-conductor-config-sanitizing-1335.md
^C
Received signal, shutting down...
  Wrote: christian-actress-sacked-blacklisted-after-refusing-to-renou.md
  Wrote: re-sveltejs-svelte-reorder-the-type-parameters-of-derived-is.md
  Wrote: re-lightningnetwork-lnd-funding-htlcswitch-enforce-min-fee-r.md
  Wrote: re-benchmark-of-rfind-vs-regex-3-bloodbath.md
  Wrote: publiclab-plots2-testing-fixes-792-846.md
^C^C^C^C^C^C^C  Wrote: re-ray-project-ray-pubsub-replace-obod-long-polling-to-the-p.md
  Wrote: sveltejs-sapper-how-install-tailwindcss-with-sapper-750.md
  Wrote: re-twbs-bootstrap-sass-deprecation-warning-using-for-divisio.md
^Z
[1]  + 507048 suspended  corky watch
```
### Analysis: Three Bugs in corky watch (HEAD)

Thanks for the full output. I found **three real bugs** and one design issue:

---

**Bug 1: INBOX appearing for personal (the original report)**

INBOX appears at line 719 as a 4th label for personal — but only in ONE poll cycle out of ~8. The config has never had INBOX for personal (verified git history and uncommitted changes). The code doesn't inject it.

**Most likely cause:** The routing merge at \`imap_sync.rs:300\` chains \`routes.keys()\` into every account's label set. If \`build_label_routes\` transiently returned an \`"INBOX"\` key (due to a config hot-reload race or a file read during an incomplete write), it would appear for ALL accounts. This is a race condition — \`poll_once\` calls \`load_accounts\` and \`build_label_routes\` separately, reading the config file twice, with no locking.

**Fix:** Load the config once per poll cycle and pass it through, instead of re-reading it in both \`load_accounts\` and \`build_label_routes\`. Also: scope \`[routing]\` entries to their intended account using the \`account:label\` syntax (change \`for-lucas\` to \`personal:for-lucas\` in .corky.toml) to prevent routing keys from leaking to all accounts.

---

**Bug 2: btak.dev 404 error (line 508-521)**

\`\`\`
Error syncing btak.dev: Gmail API error (HTTP 404): Requested entity was not found.
\`\`\`

This happens during the first poll when the access token was just refreshed. A 404 on \`messages.get\` means a message listed by \`messages.list\` was deleted before it could be fetched. The gmail_api_sync code treats this as a fatal error instead of skipping the deleted message.

**Fix:** Catch 404 on individual message fetches and skip (log warning), don't abort the entire account sync.

---

**Bug 3: Ctrl+C not stopping sync (lines 743-753)**

The user hit Ctrl+C 7+ times and had to Ctrl+Z to suspend. The IMAP sync path doesn't check the shutdown signal — only gmail_api_sync has \`shutdown: Option<&Arc<AtomicBool>>\`. Once the IMAP sync is inside a \`session.uid_fetch()\` call downloading 1.8M messages, Ctrl+C can't interrupt it.

**Fix:** Pass the shutdown signal to \`imap_sync::sync_account\` and check it between message fetches (like gmail_api_sync does at line 362).

---

**Design issue: \`for-lucas\` routing leaks to all accounts**

The \`[routing]\` entry \`for-lucas = ["mailboxes/lucas"]\` adds \`for-lucas\` as a sync label for ALL accounts (proton-dev shows "Label not found — skipping" each cycle). Use \`personal:for-lucas\` to scope it.

---

**Immediate action needed:** The suspended corky watch process (PID 507048) is still alive. Kill it with \`kill %1\` or \`kill 507048\`.

Want me to implement fixes for all three bugs?
<!-- /agent:exchange -->
