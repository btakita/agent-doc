---
agent_doc_session: 584ef929-37ee-47f6-b3d8-d4266ad31ee9
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: opus
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review corky codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
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
- [ ] crates.io publish — needs token refresh
- [ ] `corky doc upload` — deferred (Google Drive OAuth design)
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/3fe810fc180ec90ce4bbb9c2bdb013766843d9a8224cac4431150341ae259853-20260520-173822.md`*

Compacted content:
- Archived 1 response topic(s): Corky log review
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/3fe810fc...
❯ do [#google-docs-sheets-integration]. #spec-test-build-install-commit-push
### Re: Google Docs/Sheets integration — gpt-5

Implemented `#google-docs-sheets-integration` in Corky and pushed it to `btakita/corky` `main` as `9e67e35` (`fix: use Docs API for Google Docs reads`).

The main correction was making `corky docs read` use the Google Docs API `documents.get` payload instead of Drive `files.export`, so the documented `documents` OAuth scope is sufficient for Docs reads. I also added paragraph/table extraction coverage, CLI help coverage for `docs` and `sheets read`, updated SPEC/README/guide/privacy docs, and refreshed the installed Corky skill copy.

Verification completed:
- `cargo test doc::gdocs::tests`
- `make precommit` (clippy, full tests, `corky audit-docs`)
- `make release`
- `make install` (installed with `transcribe-cuda`)

CI/tmux note: Corky has no `tmux-ci` target and no tmux-test workflow in `Makefile` or `.github/workflows`, so there was no tmux CI leg to inspect. The implementation is based on the official Google Docs `documents.get`, Drive `files.export`, and Sheets `spreadsheets.values.get` scope references.
❯ Is corky able to handle writes to https://docs.google.com/spreadsheets/d/1k52arliMCqbe40_CHWUCinV59EACIoYTG1gVMvnlKrA/edit?gid=1675400873#gid=1675400873 using brian@dreamable.com? To test, create a temporary new sheet then delete immediately.
### Re: Corky Sheets write access — gpt-5

Corky has a Sheets write path in the installed CLI: `corky sheets write` updates a range, and `corky sheets push` can create a missing tab, clear it, and write CSV data from `A1`.

I could not complete the live write test for the requested account yet. The cached Corky token for `brian@dreamable.com` only has `spreadsheets.readonly`, so a Sheets write requires re-authorizing Corky with the read/write `spreadsheets` scope. I started `corky auth --scope sheets --email brian@dreamable.com`; it opened the Google OAuth flow and waited on the localhost callback, but it timed out after 300 seconds with `Timed out waiting for OAuth callback (300s)`.

Because auth did not complete, I did not create a temporary sheet and there was nothing to delete. Once the account grants the `spreadsheets` scope, the intended test is to use `corky sheets push` against a uniquely named temporary tab, verify the create/write succeeds, then clean up the tab. One limitation I found: Corky currently exposes tab creation through `sheets push`, but it does not expose a `sheets delete-tab` command, so deletion would need a direct Sheets API cleanup unless we add that Corky subcommand.
❯ try again please. Make sure the cross-platform notify-desktop triggers when during oauth.
❯ #spec-test-build-install-commit-push
### Re: Sheets OAuth retry and cleanup support — gpt-5

Implemented and pushed Corky changes to `btakita/corky` `main` as `6c66014` (`fix: make Sheets OAuth test cleanup reliable`).

What changed:
- Linux OAuth notifications now prefer `notify-desktop` but fall back to `notify-send` when `notify-desktop` is not installed. On this host, `notify-desktop` is absent and `/usr/bin/notify-send` is present, so the retried OAuth flow exercised the fallback path.
- Added `corky sheets delete-tab <SHEET> <TAB> [--account EMAIL]`, which resolves the tab title to its numeric Sheets `sheetId` and deletes it via `batchUpdate`. This lets Corky create/write a temporary tab with `sheets push` and clean it up with Corky rather than a separate API script.
- Updated SPEC/README/guide docs, instruction runbooks, version notes, and CLI tests.

Verification completed:
- `cargo test doc::sheets::tests -- --nocapture`
- `cargo test desktop_notify -- --nocapture`
- `make precommit` (clippy, full tests, `corky audit-docs`)
- `make release`
- `make install` (installed with `transcribe-cuda`)
- `corky sheets delete-tab --help`

CI/tmux note: Corky still has no `tmux-ci` target and no tmux-test workflow in `Makefile` or `.github/workflows`, so there was no tmux CI leg to inspect.

I retried the live Sheets authorization with `corky auth --scope sheets --email brian@dreamable.com`. The flow opened the browser URL and waited on `http://127.0.0.1:8484/callback`, but it timed out after 300 seconds with `Timed out waiting for OAuth callback (300s)`. The cached token for `gmail:brian@dreamable.com` is still only `spreadsheets.readonly`.

Because the read/write OAuth grant did not complete, I did not create the temporary tab and there was nothing to delete. The remaining blocker for the live write test is completing the Google browser consent callback for the `spreadsheets` scope.
❯ I need to add brian@dreamable.com as an approved email for the Google development account.
```
Access blocked: correspondence has not completed the Google verification process

brian@dreamable.com
correspondence has not completed the Google verification process. The app is currently being tested, and can only be accessed by developer-approved testers. If you think you should have access, contact the developer.
If you are a developer o f correspondence, see error details.
Error 403: access_denied
```
### Re: Google OAuth test user — gpt-5

This is an OAuth consent-screen tester issue for the Google Cloud project that owns the `correspondence` OAuth client, not a Corky code failure.

I could not add `brian@dreamable.com` directly from this machine: `gcloud` is not installed here, and adding external test users is controlled in the Google Cloud Console for the app's Google Auth platform configuration.

Add the account here:

1. Open Google Cloud Console for the project that owns the `correspondence` OAuth client.
2. Go to **Google Auth platform -> Audience**.
3. Under **Test users**, choose **Add users**.
4. Add `brian@dreamable.com` and save.

In the older Console navigation, the same setting may appear under **APIs & Services -> OAuth consent screen -> Test users**. Google's current Workspace OAuth docs describe the current flow as **Google Auth platform -> Audience -> Test users -> Add users**: https://developers.google.com/workspace/guides/configure-oauth-consent

Once that tester entry is saved, the next retry should be:

```bash
corky auth --scope sheets --email brian@dreamable.com
```

After the read/write `spreadsheets` grant completes, Corky should be able to run the temporary Sheets create/write/delete test against the target spreadsheet. I did not change code or run the build/test suite in this cycle because this blocker is outside the repo.
❯ I don't see "Test Users" in https://console.cloud.google.com/auth/clients/718955483040-0aru4vvij1ug85ga8o3tjsqpk7gtb7tj.apps.googleusercontent.com?project=corky-490604. Use chromium-bridge on the open Chromium browser to add
### Re: Google OAuth test user added — gpt-5

Added `brian@dreamable.com` as a Google Auth Platform test user for the OAuth app Corky is actually using.

The client URL you opened was for project `corky-490604` / app `corky`, and that app is already `In production`, so it does not show the old `Test users` table. Corky's mailbox config points at a different OAuth client: `873280095295-...apps.googleusercontent.com`, which belongs to project `unop-d3871` / app `agent-loop`. That app is still in `Testing`, and its Audience page has the tester list.

Using `chromium-bridge`, I opened `https://console.cloud.google.com/auth/audience?project=unop-d3871`, opened **Add users**, entered `brian@dreamable.com`, saved it, and verified the Audience page now shows:

```text
3 users (3 test, 0 other) / 100 user cap
brian.takita@gmail.com
brian@dreamable.com
btak.dev@gmail.com
```

This matches Google's Testing-mode rule: only accounts listed as test users can access a testing OAuth app. Source: https://developers.google.com/identity/protocols/oauth2/production-readiness/brand-verification

I did not change code or run the build/test suite in this cycle. The next Corky auth retry is:

```bash
corky auth --scope sheets --email brian@dreamable.com
```
❯ I authenticated. Please run test.
### Re: Corky Sheets live write test — gpt-5

Live Sheets write test passed for `brian@dreamable.com` against the target spreadsheet `1k52arliMCqbe40_CHWUCinV59EACIoYTG1gVMvnlKrA`.

I used a temporary tab named `corky-live-test-20260520-gpt5` and a two-row CSV:

```csv
test_id,result
corky-live-sheets-write,ok
```

Commands/results:

```bash
corky sheets push 'https://docs.google.com/spreadsheets/d/1k52arliMCqbe40_CHWUCinV59EACIoYTG1gVMvnlKrA/edit?gid=1675400873#gid=1675400873' 'corky-live-test-20260520-gpt5' /tmp/corky-sheets-live-test.csv --account brian@dreamable.com
```

Result: Corky created the tab, cleared it, wrote 2 rows, and reported `4 cells updated`.

```bash
corky sheets read 'https://docs.google.com/spreadsheets/d/1k52arliMCqbe40_CHWUCinV59EACIoYTG1gVMvnlKrA/edit?gid=1675400873#gid=1675400873' --range 'corky-live-test-20260520-gpt5!A1:B2' --format csv --account brian@dreamable.com
```

Readback returned:

```csv
test_id,result
corky-live-sheets-write,ok
```

```bash
corky sheets delete-tab 'https://docs.google.com/spreadsheets/d/1k52arliMCqbe40_CHWUCinV59EACIoYTG1gVMvnlKrA/edit?gid=1675400873#gid=1675400873' 'corky-live-test-20260520-gpt5' --account brian@dreamable.com
```

Result: Corky reported `Deleted tab corky-live-test-20260520-gpt5.` I then retried reading that deleted range and got the expected Sheets API `HTTP 400` invalid-range error, confirming the temporary tab was gone.

I removed the local temporary CSV afterward. No code changed in this cycle, so I did not run the build/test suite.
<!-- agent:boundary:f06d5670 -->
<!-- /agent:exchange -->
###
<!--
-->

## Backlog

<!-- agent:backlog -->
- [ ] [#pypi-trusted-publisher] Configure PyPI trusted publishing for btakita/corky workflow .github/workflows/pypi.yml with environment pypi, rerun the PyPI workflow, and verify the PyPI latest version matches the released crate.
- [ ] [#release-status-cleanup] Reconcile the Corky session/status notes after v0.28.2: mark crates.io publish complete, note v0.28.1 as superseded, and keep PyPI as the remaining publish blocker.
- [ ] [#doc-upload-drive-oauth] Implement corky doc upload with Google Drive OAuth design, upload behavior, docs, and tests.
<!-- /agent:backlog -->

## Completed / Reaped

<!-- agent:done -->
- 2026-05-12 [#tokenstore-delete-preservation] [recommended] Make TokenStore saves deletion-safe so stale writers cannot restore tokens cleared by another process
- 2026-05-12 [#watch-gmail-history-notify] [recommended] Teach watch to count Gmail API last_history_id movement and fire mailbox sync and desktop notifications for gmail-api accounts
- 2026-05-12 [#oauth-callback-url-decode] [recommended] URL-decode OAuth callback params before state comparison and token exchange
- 2026-05-20 [#google-docs-sheets-integration] Add Google Docs/Sheets integration for Corky: required OAuth scopes, read/write Google Docs, read Google Sheets, docs, and tests.
<!-- /agent:done -->
