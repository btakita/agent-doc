---
agent_doc_session: 584ef929-37ee-47f6-b3d8-d4266ad31ee9
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review corky codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
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
- **Diarization span fix** — `max_speaker_span_ratio()` in `diarize.rs` + span check in `run_adaptive_diarization()` (engine.rs): cascade to smaller chunks (600s→120s→30s) if any speaker block >60% of total audio. Working tree changes in `src/corky`, not yet committed or released.
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
- [x] Google Docs/Sheets integration — main 9e67e35
- [x] crates.io publish — v0.28.2; v0.28.1 superseded by v0.28.2
- [ ] PyPI trusted publishing — remaining blocker: workflow run 26195923707 still fails `invalid-publisher`; PyPI 0.28.2 artifacts are present but do not close the trusted-publisher setup item.
- [x] `corky doc upload` — main 5bbd614
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/3fe810fc180ec90ce4bbb9c2bdb013766843d9a8224cac4431150341ae259853-20260530-033821.md`*

Compacted content:
- Archived 1 response topic(s): corky crate split
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/3fe810fc...
Register the GCP App with Google so I don't get the unsafe menu.

### Re: GCP OAuth app verification — gpt-5

I could not register the Google-side app from this shell. `gcloud` is not installed here, and Google OAuth app verification has to be submitted in Google Cloud Console by a project owner/editor with access to the project that owns Corky's OAuth client. I did not modify the Cloud project.

What matters for the unsafe/unverified screen: Google says apps that request sensitive or restricted OAuth scopes can show the unverified-app warning until the app has been reviewed, and the warning can also appear when requested scopes are missing from the consent-screen configuration. Personal/testing use can continue by clicking through, but it does not remove the warning. Sources: [Unverified apps](https://support.google.com/googleapi/answer/7454865?hl=en), [Sensitive scope verification](https://developers.google.com/identity/protocols/oauth2/production-readiness/sensitive-scope-verification), and [OAuth verification requirements](https://support.google.com/cloud/answer/13464321?hl=en).

For Corky, the Google client needs verification/registration for the scopes it can request. I checked the code and docs; the built-in Google path uses Gmail, Drive, Docs, Sheets, Chat, Tasks, and People/Contacts scopes. At minimum, make sure the Google Auth Platform **Data Access** page declares the exact scopes Corky requests, including:

- `https://www.googleapis.com/auth/gmail.readonly`
- `https://www.googleapis.com/auth/gmail.compose`
- `https://www.googleapis.com/auth/gmail.settings.basic`
- `https://www.googleapis.com/auth/gmail.labels`
- `https://www.googleapis.com/auth/drive.file`
- `https://www.googleapis.com/auth/drive.readonly`
- `https://www.googleapis.com/auth/documents`
- `https://www.googleapis.com/auth/spreadsheets.readonly`
- `https://www.googleapis.com/auth/spreadsheets`
- `https://www.googleapis.com/auth/contacts`
- `https://www.googleapis.com/auth/chat.messages`
- `https://www.googleapis.com/auth/tasks`

Concrete Cloud Console path:

1. Select the project that owns Corky's built-in OAuth client. The session notes mention project number `873280095295`; confirm it against the OAuth client ID prefix from `pass corky/gcp/client_id`.
2. Go to **Google Auth Platform → Branding**. Set app name `Corky`, support email, developer contact, homepage URL, privacy policy URL, and terms URL.
3. Use a verified domain you own for the homepage and policy pages. Corky already has source docs at `src/corky/docs/reference/privacy-policy.md` and `src/corky/docs/reference/terms-of-service.md`, but Google expects the public URLs to be hosted under the verified app domain.
4. Go to **Audience**. Move to production/public only if the goal is to remove the warning for arbitrary Google users. Keep testing only for a small personal/test-user flow where clicking through is acceptable.
5. Go to **Data Access**. Add every scope Corky requests and provide per-scope justifications. For example, `gmail.readonly` is for syncing mail into local files; `gmail.compose` is for draft creation/sending; Drive/Docs/Sheets scopes are for document upload/read/write workflows; `contacts` is for People API contact push/delete.
6. Submit verification. Expect Google to ask for a demo video showing the OAuth flow and each sensitive/restricted scope in use.

Local proof:

- Changed paths: none.
- Commands run: `agent-doc --version`, `agent-doc preflight`, `agent-doc read`, `agent-doc plan`, `tsift status --fix`, targeted `rg`/`sed` reads, and `gcloud auth list` / `gcloud config get-value project`.
- Verification: no code verification was needed because no repo files changed. The Google Cloud check was blocked because `gcloud` exits `127` here.
- Confidence: high that verification is the required path to remove the unsafe/unverified screen; medium on the project number until you confirm it from the client ID.
- Escalation: none.

### Re: Register the GCP App with Google so I don't get the unsafe menu. — gpt-5

Handled in the preceding response. I could not complete the Google-side registration from this shell because `gcloud` is unavailable here and Google requires a project owner/editor to submit OAuth app verification in Cloud Console. The actionable checklist above is the path to remove the unsafe/unverified warning; no local repo or Cloud project files were changed.
<!-- agent:boundary:c079bfb7 -->
<!-- /agent:exchange -->
###
<!--
Why is 1 test ignored?
Please give me a report the performance
-->

## Queue

<!-- agent:queue auto -->
- Register the GCP App with Google so I don't get the unsafe menu.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [/] [#pypi-trusted-publisher] Configure PyPI trusted publishing for btakita/corky workflow .github/workflows/pypi.yml with environment pypi. Repo workflow is configured and pushed in Corky commit feb074e; PyPI run 26195923707 now builds all artifacts but is blocked by PyPI invalid-publisher until the project has a trusted publisher matching repo:btakita/corky:environment:pypi.
<!-- /agent:backlog -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/corky.done.md -->
<!-- /agent:done -->
