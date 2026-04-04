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
*Compacted exchange covering v0.15.3 through v0.24.0.*

### Releases in this session

**v0.21.0** — YouTube playlist CRUD (`playlist create/add/list/remove`), 5 tests, 13 total YouTube tests.

**v0.22.0** — Unified `corky search` with sift (local) + ragie (cloud) backends. Diarize now default feature. `corky contact push google` (People API).

**v0.22.1** — LinkedIn URL fix in `contact push` (CLAUDE.md fallback). Duplicate contact issue noted.

**v0.22.2** — Gmail API body decode: `attachmentId` support for `multipart/related`, padded base64 fallback. 555 emails recovered.

**v0.23.0** — HTML-first email body extraction (htmd), markdown cleanup (CSS/tracking/styles), tracking pixel detection (`**Tracking:**` metadata), `corky sync refetch <THREAD_ID>`, META_RE regex fix. 252 tests.

**v0.23.1** — Transcription quality: speaker count constraint in cross-chunk merging, frequency spike repetition filter (66KB→49KB), CUDA build fix, adaptive chunking cascade. Cube ACR recommendation: MPEG-4 LQ.

**v0.24.0** — Gmail API threading (Message-ID extraction, `thread_id` in draft YAML + API requests), account-based doc auth (`--account` flag), Google Drive upload/Docs read-write/Sheets read, contact delete. Published: crates.io, PyPI (license-files fix), GitHub Release.

### Key decisions
- HTML-first body extraction (prefer `text/html` → htmd over `text/plain`)
- Tracking pixel detection: domain-only in `**Tracking:**` metadata line
- Adaptive chunking + resolve-unknown default-on, opt-out via config/CLI
- `--resolve-unknown` implies `--diarize`
- LLM inference via current agent harness (not new dependency)
- `license-files = ["LICENSE"]` in pyproject.toml for PyPI sdist compliance
- No-hallucination rule added to CLAUDE.md Core Principles

### Transferred from briantakita.me
- `corky skill install` should manage both email and linkedin skills (separate SKILL.md files)
- Replace `.agent/runbooks/linkedin-post.md` with corky-managed skill
- Shared conventions: never send without approval, links in first comment

### Open/deferred
- crates.io token was expired (fixed in v0.24.0)
- btak.dev Gmail API OAuth needs re-auth (interactive)
- Google Sheets 403: verify sharing / GCP test users for `brian.takita@gmail.com`
- Manylinux binary wheel for PyPI (needs container/CI)

do
- Push corky license-files fix to origin
- Re-auth btak.dev Gmail API OAuth (interactive)
### Re: Push & OAuth

**Pushed** `3daf664` (license-files fix) to corky origin.

**btak.dev OAuth:** Needs interactive browser auth. Run in terminal:
```
corky sync account btak.dev
```
reauthed
do
- Corky skill architecture: `corky skill install` for email + linkedin skills
 
### Re: Corky skill architecture

**Current state:**
- `.claude/skills/email/SKILL.md` — manually created (109 lines), covers email drafting
- `.claude/skills/corky/` — not installed
- `corky skill install` — bundles one SKILL.md, installs to `.claude/skills/corky/`
- LinkedIn: `.agent/runbooks/linkedin-post.md` (manual runbook, not skill-managed)

**Proposed design (multi-skill install):**
`corky skill install` writes 3 SKILL.md files:
1. `.claude/skills/corky/SKILL.md` — general corky (sync, contacts, docs, cal, transcribe)
2. `.claude/skills/email/SKILL.md` — email drafting (replaces manual one)
3. `.claude/skills/linkedin/SKILL.md` — LinkedIn posting via `corky linkedin`

Each has focused frontmatter for Claude Code skill matching. Shared conventions (never send without approval) in each relevant skill.

**Implementation:** Bundle 3 SKILL.md files via `include_str!`, `corky skill install` writes all 3 + runbooks.

Ready to implement on your go.
Are there any generalizable context in email/SKILL.md that should be moved into corky's installable skills?
Any runbook extractions that should happen?
### Re: Generalizable context and runbook extractions

**What moves into corky-managed skills:**

| Content | From | To |
|---------|------|-----|
| "Never send without approval" | email/SKILL.md | Shared convention in both email + linkedin skills |
| `file:///` protocol paths | email/SKILL.md | Shared convention |
| Verify facts vs contact file | email/SKILL.md | Shared convention |
| Secret handling (`pass`) | email/SKILL.md | Already in CLAUDE.md — remove from skill |
| LinkedIn post runbook | `.agent/runbooks/linkedin-post.md` | `linkedin/runbooks/linkedin-post.md` |

**What stays put:**
- Account config (btak.dev, personal, proton) — email-specific
- Draft/threading workflow — email-specific
- HTML/browser paste — email delivery path
- All 4 email runbooks — email-specific

**Proposed installed structure:**
```
.claude/skills/
├── corky/SKILL.md           # general: sync, contacts, docs, cal, transcribe, search
├── email/SKILL.md + runbooks/  # email drafting (replaces manual)
└── linkedin/SKILL.md + runbooks/  # linkedin posting (new)
```

All installed by `corky skill install`. Ready to implement on go.
go
### Re: Multi-skill install implemented

**Created in corky repo:**
- `skills/email/SKILL.md` + 4 runbooks (email-send, draft-reply, review-inbox, enrich-contact)
- `skills/linkedin/SKILL.md` + 1 runbook (linkedin-post)

**Modified:**
- `SKILL.md` — trimmed to general corky commands, added Related Skills section
- `src/skill.rs` — bundles all 3 skills + 5 runbooks via `include_str!`

`corky skill install` now writes all 3 skills + runbooks. 258 tests pass, clippy clean.

Next: commit, update SPEC/VERSIONS, audit docs, release.
go
How much context savings are we getting from extracting the runbooks?
### Re: Released v0.25.0 — Multi-skill install

**Published:**
- [crates.io](https://crates.io/crates/corky/0.25.0)
- [PyPI](https://pypi.org/project/corky/0.25.0/)
- [GitHub Release](https://github.com/btakita/corky/releases/tag/v0.25.0)
- `~/.cargo/bin/corky` installed with CUDA

**3 commits:**
1. `feat: multi-skill install (email, linkedin, corky) v0.25.0`
2. `fix: track skills/ directory for crate packaging` (include_str! needs git-tracked files)
3. `chore: gitignore .claude/ directory` (installed output)

**Issue fixed:** `skills/` was gitignored but `include_str!` embeds from git-tracked files. `cargo publish --verify` failed. Un-gitignored `skills/`, added `.claude/` to `.gitignore`.
### Re: Context savings from skill extraction

Skills installed. Claude Code now shows `corky`, `email`, `linkedin` as separate skills.

| Invocation | Lines loaded | vs monolithic (695) | Savings |
|-----------|-------------|---------------------|---------|
| `/email` | 107 | 85% less | ~588 lines |
| `/linkedin` | 58 | 92% less | ~637 lines |
| `/corky` | 71 | 90% less | ~624 lines |

Runbooks (459 lines) are on-demand only — never loaded unless the user requests a specific operation.

The biggest win: Claude Code skill matching only loads the *relevant* skill per invocation. A monolithic skill would load all 695 lines for every corky-related task.
Can any runbooks be extracted from these skills, to avoid over-eagerly using context tokens?
### Re: Runbook extraction from skills

**Email skill (107 lines) — two extraction candidates:**
1. **Gmail Account Configuration** (28 lines) → `runbooks/gmail-config.md` — only needed for account setup/debug, not every email task
2. **HTML/browser paste workflow** (18 lines) → `runbooks/browser-paste.md` — fallback workflow, rarely used

**Potential savings:** ~46 lines (43% reduction, 107→~61 lines)

**LinkedIn skill (58 lines):** Already lean, nothing worth extracting.

**Corky skill (71 lines):** Import/transcription commands (~15 lines) could move but marginal gain.

Want me to extract the two email runbooks?
yes...also extract corky runbooks
report the token savings
### Re: Runbook extraction — done

**Savings: 236 → 185 lines always-loaded (21% reduction)**

| Skill | Before | After | Saved |
|-------|--------|-------|-------|
| corky | 71 | 61 | 10 |
| email | 107 | 66 | 41 |
| linkedin | 58 | 58 | — |

**4 new on-demand runbooks:** gmail-config (28), browser-paste (21), imports (21), transcription (28).

All 258 tests pass, clippy clean. Not yet committed.
do
- Commit runbook extractions to corky submodule
- Release v0.25.1 (or bump)
### Re: Released v0.25.1 — Skill runbook extraction

Published to all targets:
- [crates.io](https://crates.io/crates/corky/0.25.1)
- [PyPI](https://pypi.org/project/corky/0.25.1/)
- [GitHub Release](https://github.com/btakita/corky/releases/tag/v0.25.1)
- `~/.cargo/bin/corky` installed with CUDA

21% reduction in always-loaded skill context (236→185 lines). 4 new on-demand runbooks.
Do we have transcribe + diarization in our corky skill install?

I want to look at the transcript speaker + text confidence. If it drops below a threshold, would it help to transcribe + diarize small audio chunks?

Should this be extracted as a runbook?
### Re: Transcription confidence & chunking

**Yes** — transcription + diarization is in corky's installed skills. `corky/SKILL.md` references `runbooks/transcription.md` (on-demand).

**Confidence is already tracked.** Each speaker block outputs `**Speaker** (0.85) [00:00 → 01:23]` where `0.85` is the average whisper confidence. Stored in `MergedSegment.confidence`.

**Current adaptive chunking triggers on diarization quality** (>30% unknown speakers), NOT on whisper text confidence. Your idea — re-transcribing low-confidence blocks with smaller windows — would be a separate feature.

**Design sketch:**
1. After transcription, scan for blocks with avg confidence < threshold (e.g., 0.5)
2. Re-transcribe those time ranges with 30s windows (more context per segment)
3. Replace low-confidence blocks with higher-confidence results

**Not a runbook** — this is automated logic inside `engine.rs`, not a human procedure. It would be a code feature like adaptive chunking.

Want me to design and implement confidence-based re-transcription?
yes
### Re: Confidence-based re-transcription — implemented (HEAD)

**How it works:**
1. After whisper + diarization merge, scans for contiguous low-confidence blocks (< 0.4 threshold)
2. Re-transcribes those time ranges with fresh whisper pass (±2s padding for context)
3. Replaces originals only if re-transcription has higher avg confidence
4. Speaker labels preserved via temporal proximity matching

**Config:** `[transcription] confidence_threshold = 0.4`
**Opt-out:** `--no-confidence-retranscribe`

**Files:** engine.rs (main logic + 6 tests), corky_config.rs, cli.rs, main.rs, mod.rs. 264 tests pass, clippy clean.

Ready to commit and release.
<!-- agent:boundary:83fac144 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
- Commit confidence re-transcription + release
- Verify sheet sharing / GCP test users
- Re-test sheet read
- Manylinux binary wheel (nice-to-have, needs container/CI)
<!-- /agent:pending -->
