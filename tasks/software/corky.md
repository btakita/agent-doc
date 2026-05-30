---
agent_doc_session: 584ef929-37ee-47f6-b3d8-d4266ad31ee9
agent_doc_format: template
agent_doc_write: crdt
agent: claude
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/3fe810fc180ec90ce4bbb9c2bdb013766843d9a8224cac4431150341ae259853-20260530-013743.md`*

Compacted content:
- Archived 1 response topic(s): do #h465
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/3fe810fc...
❯ What are #next-steps to split out crates in corky?

### Re: next-steps to split out crates in corky — opus-4-8

corky today is a single ~32k-LOC crate (`src/lib.rs` re-exports 30 modules), 41 deps, one bin. The build I just ran took 2m36s — nearly all of it `whisper-rs-sys` + CUDA. That number is the headline: the biggest payoff from splitting isn't independent publishing, it's **build-time isolation** of the native/GPU toolchain plus enforced module boundaries.

I measured the coupling before recommending a sequence (`grep crate::` across modules):

- **`transcribe/`** is the cleanest seam in the codebase — it imports only `crate::config::corky_config`, and the *only* caller is `main.rs`. It also owns every heavy optional dep (whisper-rs, pyannote-rs, ort, symphonia, rubato, hound) behind the `transcribe`/`transcribe-cuda`/`transcribe-metal`/`diarize` features.
- **`filter::gmail_auth`** is the shared OAuth/token store that the Google cluster (`doc`, `cal`, `gsc`, `tasks`) and `social` all depend on — so it has to land in the foundation crate, not in `corky-mail`.
- The Google cluster also has minor cross-edges (`doc`→`social`, everything→`util`/`resolve`/`config`/`oauth_loopback`), so those domain extractions come last.

#### Recommended sequence (lowest-risk → highest)

**Phase 1 — workspace scaffold.** Add `[workspace]`; keep `corky` as the sole member (still lib+bin). No code moves. This is just the container and lets you move one module at a time afterward without a big-bang refactor.

**Phase 2 — extract `corky-transcribe` (do this first).** Highest ROI, lowest coupling. Move `transcribe/` + its deps into a feature-gated crate; re-export the feature flags from the parent (`corky/transcribe-cuda = ["corky-transcribe/cuda"]`). Result: `cargo build` of everything except transcription drops from minutes to seconds, and only the bin (or whoever opts in) pulls the CUDA toolchain. **Trap to plan for:** moving a module to a sibling crate hides its `#[cfg(test)]` items from the parent's test binaries — use a `test-support` cargo feature for runtime short-circuits and keep crate-local fixture copies (this bit us before in the agent-doc extraction).

**Phase 3 — extract `corky-core`.** The shared foundation: `config`, `accounts`, `resolve`, `oauth_loopback`, `filter::gmail_auth` (token store), `file_store`, `util`, `app_config`, `desktop_notify`. Everything else will depend on this. Critically, the `tokens.json` / `.sync-state.json` accessors must live here so all domain crates share one locked, atomic-write store.

**Phase 4+ — domain crates on top of core.** `corky-google` (doc/cal/gsc/tasks), then `corky-mail` (sync/draft/mailbox/filter/contact/search/topics/label — the largest, 5.3k LOC in sync alone), then `corky-social`. Finally slim `corky` to a thin CLI (`cli.rs` 1.4k + `main.rs` dispatch + doctor/schedule/watch/upgrade) depending on the library crates.

#### Decisions to make before starting

- **Publish scope.** You don't need to publish each member to crates.io. Mark workspace members `publish = false` and publish only the `corky` bin — you keep the compile isolation without quadrupling release ceremony (otherwise each release means `cargo publish` in dependency order + lockstep version bumps across all crates).
- **PyPI/maturin packaging** targets the bin crate — `pyproject.toml` and `.github/workflows/pypi.yml` must keep pointing at the workspace root bin, and config templates / skill files / `install.sh` ship with the bin, so don't relocate those into library crates.
- **Sequencing guard:** land Phase 1+2 and ship a release before touching core/domain splits — that proves the workspace + feature re-export plumbing on the easy case before you move the load-bearing modules.

Queued the four phases as `[recommended]` backlog items so they're tracked without committing you to the work.
<!-- agent:boundary:a7e15fd9 -->
<!-- /agent:exchange -->
###
<!--
Why is 1 test ignored?
Please give me a report the performance
-->

## Backlog

<!-- agent:backlog -->
- [ ] [#5wnq] [recommended] corky crate-split phase 4+: extract corky-google (doc/cal/gsc/tasks), corky-mail (sync/draft/mailbox/filter/contact/search/topics/label), corky-social; slim corky bin to CLI dispatch (cli.rs/main.rs). Keep workspace members publish=false except the corky bin; update maturin/pyproject + multi-crate release order
- [ ] [#0k50] [recommended] corky crate-split phase 3: extract corky-core foundation (config, accounts, resolve, oauth_loopback, filter::gmail_auth token store, file_store, util, app_config, desktop_notify) so domain crates share auth + the tokens.json/.sync-state.json stores
- [ ] [#69ge] [recommended] corky crate-split phase 2: extract corky-transcribe (transcribe/ + whisper-rs/pyannote-rs/ort/symphonia/rubato/hound, feature-gated transcribe/cuda/metal/diarize). Only main.rs calls it, sole coupling crate::config; isolates the 2m+ CUDA build from core. Watch cross-crate #[cfg(test)] — use a test-support feature
- [ ] [#vbpm] [recommended] corky crate-split phase 1: convert corky to a cargo workspace, root crate keeps lib+bin — scaffolding only, no module moves yet
- [/] [#pypi-trusted-publisher] Configure PyPI trusted publishing for btakita/corky workflow .github/workflows/pypi.yml with environment pypi. Repo workflow is configured and pushed in Corky commit feb074e; PyPI run 26195923707 now builds all artifacts but is blocked by PyPI invalid-publisher until the project has a trusted publisher matching repo:btakita/corky:environment:pypi.
<!-- /agent:backlog -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/corky.done.md -->
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
- 2026-05-29 [#h465] draft push on gmail-api accounts silently DROPS attachments. Repro (corky 0.28.2): draft with 'attachments:' frontmatter + 'account' = gmail-api provider, run 'corky draft push <file>' — it prints 'Attach: N file(s)' but the created Gmail draft is multipart/alternative with NO attachment parts (verify: GET /users/me/drafts/<id>?format=full → payload.mimeType, parts). The 'draft send' path (src/draft/send.rs) builds multipart/mixed with attachments correctly; the push/drafts.create path (push_to_drafts_via_gmail_api in src/draft/mod.rs) does not include them despite compose_email receiving the attachment list. Fix: ensure push uses the same multipart/mixed builder as send so attachments land in drafts.create; add a regression test asserting a pushed gmail-api draft's MIME is multipart/mixed with the expected part filenames. Workaround in use: build the MIME + POST drafts.create directly via the Gmail API.
<!-- /agent:done -->
