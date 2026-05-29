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
- [x] crates.io publish — v0.28.2; v0.28.1 superseded by v0.28.2
- [ ] PyPI trusted publishing — remaining blocker: workflow run 26195923707 still fails `invalid-publisher`; PyPI 0.28.2 artifacts are present but do not close the trusted-publisher setup item.
- [x] `corky doc upload` — main 5bbd614
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/3fe810fc180ec90ce4bbb9c2bdb013766843d9a8224cac4431150341ae259853-20260521-055654.md`*

Compacted content:
- Archived 1 response topic(s): doc upload Drive OAuth
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/3fe810fc...
❯ do [#sheets-scope]. #spec-test-build-install-commit-push
### Re: sheets scope blocker — gpt-5

I could not start `#sheets-scope` because the planning gate failed closed before repository work. The blocker was required `tsift` graph evidence collection timing out for:

```bash
tsift graph-db --path /home/brian/work/btakita/agent-loop/tasks/software/corky.md --json evidence sheets --depth 3 --limit 8
```

No code, spec, or test files were changed. I did not run build/install, commit, or push for Corky. The `#sheets-scope` backlog item remains open because implementation did not begin.
Please add a feature to lookup a gmail url (https://mail.google.com/mail/u/0/#search/philip/FMfcgzQgLsCwhwtlQXPkzpdLhKhzPdNv) directly from a url and convert to markdown via the cli as a tool call.

❯ #spec-test-build-install-commit-push
### Re: Gmail URL refetch — gpt-5

Implemented Corky support for `corky sync refetch TARGET`, where `TARGET` can now be a Gmail API thread ID, Gmail API message ID, or a Gmail web URL like `https://mail.google.com/mail/u/0/#search/query/FMfc...`. The command extracts selected-message tokens from Gmail fragments such as `#inbox/...` and `#search/query/...`, also accepts `th=` query URLs, tries the Gmail Threads API first, falls back through the Messages API to resolve the containing thread, and then reuses the existing refetch merge path to write the conversation markdown.

Changed paths:
- `src/corky/SPEC.md`
- `src/corky/src/cli.rs`
- `src/corky/src/main.rs`
- `src/corky/src/sync/gmail_api_sync.rs`
- `src/corky/src/sync/mod.rs`
- `src/corky/tests/test_cli.rs`

Verification completed:
- `cargo test parse_refetch_target`
- `cargo test test_cli_sync_refetch_help_mentions_gmail_url`
- `make check`
- `make build`
- `make install` (installed with `transcribe-cuda` after GPU detection)
- `corky --version`
- `corky sync refetch --help`
- `git diff --check`
- GitHub CI inspection: latest Corky `CI` run `26209510559` on `main` completed successfully. Corky has no `tmux-ci`/`tmux-test` target or workflow to inspect.

Commits:
- Corky repo: `7549276 feat: support Gmail URL refetch`, pushed to `origin/main`.
- Parent agent-loop repo: `fc190ee01 chore: update corky submodule`, committed locally; parent push follows this document closeout per the session closeout order.

No escalation was needed. Confidence: high.
❯ Look at the session for /home/brian/work/btakita/agent-loop/tasks/recruit/acadian-asset-management.md
❯ The prompt I used caused corky to do an entire `corky sync account`. The latest feature should only fetch the single email from the url.
❯ #spec-test-build-install-commit-push
### Re: Gmail URL selected-message refetch — gpt-5

I updated Corky so `corky sync refetch TARGET` keeps raw Gmail API IDs on the existing full-thread refetch path, but treats Gmail web URLs as precise selected-message lookups. URL targets now call a selected-message resolver that fetches `messages/{id}` and converts exactly that email; if the URL token only resolves to a multi-message thread, Corky fails closed with an explicit error instead of silently importing the whole thread. This avoids using `corky sync account` for the Acadian-style prompt and avoids broad account syncing for a Gmail URL lookup.

Changed paths:
- `AGENTS.md`
- `README.md`
- `SPEC.md`
- `src/cli.rs`
- `src/sync/gmail_api_sync.rs`
- `src/sync/mod.rs`
- `tests/test_cli.rs`

Verification completed:
- `cargo test parse_refetch_target`
- `cargo test test_cli_sync_refetch_help_mentions_gmail_url`
- `make precommit` (clippy, full test suite, `audit-docs`)
- `make build`
- `make install` (installed with `transcribe-cuda` after GPU detection)
- `corky --version`
- `corky sync refetch --help`
- `git diff --check`
- GitHub CI: Corky `CI` run `26261569252` on `main` completed successfully in 5m25s. Corky has no `tmux-ci`/`tmux-test` target or workflow; repo search only found the session `tmux_session` metadata and the `tmux-router` dependency entry.

Commits:
- Corky repo: `162b58b fix: limit Gmail URL refetch to selected message`, pushed to `origin/main`.
- Parent agent-loop repo: `3229d09e4 chore: update corky submodule`, committed locally; parent push follows this document closeout per the session closeout order.

No escalation was needed. Confidence: high.
❯ release + publish
### Re: release + publish — gpt-5

I could not release or publish Corky yet because the repo is still set to version `0.28.2`, and that version is already published everywhere checked:

- Git tag: `v0.28.2` exists.
- GitHub release: `corky v0.28.2` exists and is marked latest.
- crates.io: `corky = "0.28.2"` is the published version.
- PyPI: latest `corky` version is `0.28.2`.

Corky `HEAD` is `v0.28.2-12-g162b58b`, so there is unreleased work on `main`, but `AGENTS.md` explicitly says not to bump versions automatically. I did not tag, publish to crates.io, run the PyPI workflow, create a GitHub release, or push anything for this prompt. To continue, specify the next release version or bump `Cargo.toml` and `pyproject.toml` yourself, then rerun `release + publish`.

Commands run:
- `agent-doc --version`
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/software/corky.md`
- `agent-doc read /home/brian/work/btakita/agent-loop/tasks/software/corky.md`
- `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/software/corky.md`
- `git -C src/corky status --short --branch`
- `git -C src/corky tag --list 'v*' --sort=-v:refname`
- `git -C src/corky describe --tags --always --dirty`
- `gh -R btakita/corky release list --limit 10`
- `cargo search corky --limit 5`
- `uvx --from packaging python -c "...pypi corky json..."`

Changed paths: none. Verification/build/publish: not run because the release version is blocked before mutation. Escalation: none. Confidence: high.
<!-- agent:boundary:86db302a -->
<!-- /agent:exchange -->
###
<!--
Why is 1 test ignored?
Please give me a report the performance
-->

## Backlog

<!-- agent:backlog -->
- [ ] [#h465] draft push on gmail-api accounts silently DROPS attachments. Repro (corky 0.28.2): draft with 'attachments:' frontmatter + 'account' = gmail-api provider, run 'corky draft push <file>' — it prints 'Attach: N file(s)' but the created Gmail draft is multipart/alternative with NO attachment parts (verify: GET /users/me/drafts/<id>?format=full → payload.mimeType, parts). The 'draft send' path (src/draft/send.rs) builds multipart/mixed with attachments correctly; the push/drafts.create path (push_to_drafts_via_gmail_api in src/draft/mod.rs) does not include them despite compose_email receiving the attachment list. Fix: ensure push uses the same multipart/mixed builder as send so attachments land in drafts.create; add a regression test asserting a pushed gmail-api draft's MIME is multipart/mixed with the expected part filenames. Workaround in use: build the MIME + POST drafts.create directly via the Gmail API.
- [/] [#pypi-trusted-publisher] Configure PyPI trusted publishing for btakita/corky workflow .github/workflows/pypi.yml with environment pypi. Repo workflow is configured and pushed in Corky commit feb074e; PyPI run 26195923707 now builds all artifacts but is blocked by PyPI invalid-publisher until the project has a trusted publisher matching repo:btakita/corky:environment:pypi.
<!-- /agent:backlog -->

## Completed / Reaped

<!-- agent:done -->
- 2026-05-12 [#tokenstore-delete-preservation] [recommended] Make TokenStore saves deletion-safe so stale writers cannot restore tokens cleared by another process
- 2026-05-12 [#watch-gmail-history-notify] [recommended] Teach watch to count Gmail API last_history_id movement and fire mailbox sync and desktop notifications for gmail-api accounts
- 2026-05-12 [#oauth-callback-url-decode] [recommended] URL-decode OAuth callback params before state comparison and token exchange
- 2026-05-20 [#google-docs-sheets-integration] Add Google Docs/Sheets integration for Corky: required OAuth scopes, read/write Google Docs, read Google Sheets, docs, and tests.
- 2026-05-20 [#google-workspace-doc-types] Implement broad Google Workspace document type support beyond Sheets: detect Drive MIME types and route Google Docs, Sheets, Slides, Forms/Drawings, PDFs, and common Office files through appropriate read/export/write/upload behavior, with spec updates, docs, tests, build/install, commit, and push.
- 2026-05-20 [#enable-drive-api-corky-oauth] Enable Google Drive API for Corky's configured Gmail OAuth project 873280095295, then rerun the live Google Workspace Drive CRUD matrix for brian.takita@gmail.com.
- 2026-05-20 [#enable-docs-api-corky-oauth] Enable Google Docs API for Corky's configured Gmail OAuth project 873280095295, then rerun Google Docs read/write live tests for brian.takita@gmail.com.
- 2026-05-20 [#75d8] [recommended] Add the missing Corky PyPI 0.28.2 Linux x86_64 and macOS x86_64 wheels after fixing the tag workflow build failures (OpenSSL/manylinux Linux and MACOSX_DEPLOYMENT_TARGET macOS).
- 2026-05-20 [#release-status-cleanup] Reconcile the Corky session/status notes after v0.28.2: mark crates.io publish complete, note v0.28.1 as superseded, and keep PyPI as the remaining publish blocker.
- 2026-05-20 [#doc-upload-drive-oauth] Implement corky doc upload with Google Drive OAuth design, upload behavior, docs, and tests.
- 2026-05-21 [#sheets-scope] [recommended] `corky sheets` OAuth scope handling. `corky sheets read` and `corky sheets write` share the same OAuth client + token cache, but `read` only requests `spreadsheets.readonly` and `write` then fails with HTTP 403 `ACCESS_TOKEN_SCOPE_INSUFFICIENT` because the cached token lacks the write scope. Observed 2026-05-21 attempting to update the Velvet Thunder MVP worksheet (`1k52arliMCqbe40_CHWUCinV59EACIoYTG1gVMvnlKrA Wed 5/20/26!K3:L3`) via `corky sheets write` — the write API call surfaced the scope error mid-operation rather than re-auth'ing first. Fix options, ranked: (a) request the full `spreadsheets` scope upfront on any first-time `sheets *` auth (single broader prompt, no UX regression since read+write are paired in this workflow); (b) detect `ACCESS_TOKEN_SCOPE_INSUFFICIENT` from the write API and auto-trigger a re-auth flow with the missing scope; (c) add `corky sheets reauth --scope write` command + a clear "your token is read-only; re-auth needed" error message. Workaround for now: delete `~/.config/corky/...token...` and rerun `corky sheets write` (will prompt for fresh OAuth — verify it requests `spreadsheets` not `spreadsheets.readonly`).
<!-- /agent:done -->
