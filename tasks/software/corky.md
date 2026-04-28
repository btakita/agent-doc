---
agent_doc_session: 584ef929-37ee-47f6-b3d8-d4266ad31ee9
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
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
- [ ] crates.io publish — needs token refresh
- [ ] `corky doc upload` — deferred (Google Drive OAuth design)
- [ ] Google Docs/Sheets integration — read/write Google Docs, read Google Sheets (extends Gmail OAuth scopes)
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
Compacted. Covers CI fixes (v0.26.0 retags), dependency upgrades (agent-kit 0.4, instruction-files 0.2), and corky v0.27.0 release.

## Decisions
- `.cargo/config.toml` patch pattern: Cargo.toml uses crate versions; local path overrides gitignored. CI resolves from crates.io, no cloning. Documented in `dev/runbooks/cargo-path-deps.md` + `rust-package/SKILL.md`.
- `option_env!()` (not `env!()`) for compile-time GCP client ID — allows CI build without credentials.
- Cross.toml pre-installs host OpenSSL in aarch64 cross container.
- `corky skill install` now calls `instruction_files::init()` for transitive skill install.
- `SkillConfig` removed from agent-kit v0.4 — inlined with `Environment::ClaudeCode` for CI tests.
- Top-level `corky docs` / `corky sheets` aliases replace old `corky doc` sub-routes.
- gws skill + runbooks removed; commands moved to `corky docs` / `corky sheets`.

## Releases
- **v0.26.0** — CI fixed, .cargo/config.toml pattern, dependency upgrades, manylinux PyPI wheel.
- **v0.27.0** — `corky docs read/write`, `corky sheets read/write`, `corky chat send`, `corky tasks list/add/done`, `corky draft send` (file attachments), `corky draft new --thread-id`. PyPI: manylinux_2_28_x86_64.

## Open items
- `corky gsc` user-OAuth fallback (step 6) — **in progress on branch `20260413/corky-gsc`**. Plan: port `cal/auth.rs` user flow into `gsc/auth.rs`, reuse `[gmail]` credentials, port 8485, token prefix `gsc:`. Scope `webmasters.readonly` is sensitive — user must be added as GCP test user. Stray file `src/corky/src/gsc/20260413/corky-gsc` (misdirected git output) — not yet cleaned up.
- brian@dreamable.com OAuth: GCP project 718955483040 — Sheets API must stay enabled.

❯ I'm getting insufficient permissions to write to a google doc
❯ See /home/brian/work/btakita/agent-loop/src/session-share/tasks/aws.md
```
❯ corky sheets write "https://docs.google.com/spreadsheets/d/1Jmr9rYxkmT2DKB6UX1CrC8eyCijtqqteslsWcvJ1JCM/edit?gid=412269394#gid=412269394" "A1" /tmp/aws-project-plan.csv --account brian@dreamable.com
Writing 17 rows to A1...
Error: Sheets API error (HTTP 403): {
  "error": {
    "code": 403,
    "message": "Request had insufficient authentication scopes.",
    "status": "PERMISSION_DENIED",
    "details": [
      {
        "@type": "type.googleapis.com/google.rpc.ErrorInfo",
        "reason": "ACCESS_TOKEN_SCOPE_INSUFFICIENT",
        "domain": "googleapis.com",
        "metadata": {
          "method": "google.apps.sheets.v4.SpreadsheetsService.UpdateValues",
          "service": "sheets.googleapis.com"
        }
      }
    ]
  }
}
```
### Re: sheets write insufficient scope — opus-4-6

The 403 `ACCESS_TOKEN_SCOPE_INSUFFICIENT` is a bug in `get_access_token_for_user` — it returns cached tokens without checking if their scopes cover the requested scope. A token obtained via `corky sheets read` (scope `spreadsheets.readonly`) gets reused for `corky sheets write` (scope `spreadsheets`), and Google rejects it.

**Fix applied** in `src/filter/gmail_auth.rs`:

1. Added `scope_covered()` helper — checks if stored token scopes cover the requested scope. A read-write scope (e.g., `spreadsheets`) subsumes its `.readonly` variant.
2. `get_access_token_for_user` now validates scope coverage before returning a cached token. If insufficient, skips refresh (refresh preserves original scopes) and triggers full re-auth with the correct scope.
3. Added 4 unit tests covering exact match, rw-subsumes-readonly, readonly-does-not-subsume-rw, and multi-scope strings.

All 293 tests pass, clippy clean, both binaries rebuilt (`cargo install` + `cargo build --release`).

**To fix your current token:** re-run the same `corky sheets write` command. It will detect the insufficient scope, print a message, and open the browser for OAuth consent with the full `spreadsheets` scope. After authorizing, the write should succeed.

❯ do Codex Gmail support plan. spec-test-build-install-commit-push
### Re: Codex Gmail support plan — gpt-5

I audited the current `src/corky` worktree against `tasks/software/corky/plan-codex-gmail-support.md`. The connector-facing CLI surfaces are already present locally: `corky doctor gmail --json`, `corky sync refetch THREAD_ID --json`, `corky draft push --json`, and `corky draft send --json`, along with the Gmail threading fix that separates RFC 2822 `Message-ID` (`in_reply_to`) from Gmail `thread_id`.

I fixed the remaining doc and instruction drift around that work, including one real workflow bug in the email reply runbook that still told agents to pass a Gmail thread ID as `--in-reply-to`. After that:
- `cargo clippy -- -D warnings` passed
- full Rust test suite passed
- `cargo run --quiet -- audit-docs` passed clean
- `make release` succeeded
- `make install` succeeded (`transcribe-cuda` path)

I did not make the requested `src/corky` commit or push yet. The concrete blocker is the repo state: branch `20260413/corky-gsc` already has a large pre-existing dirty worktree across roughly 100 tracked files plus untracked `src/gsc/`. A clean commit now would either bundle unrelated branch work under this Codex Gmail task or violate the repo's "no dangling unstaged files" rule for split commits. The existing pending Codex Gmail support item stays open until that commit boundary is clarified.

❯ do #1bgy. spec-test-build-install-commit-push
### Re: Codex Gmail support plan — gpt-5

I completed the part of `#1bgy` that was still missing outside `src/corky`.

`src/corky` is already in the right state: branch `20260413/corky-gsc` is clean and `cf63185` is already pushed to `origin/20260413/corky-gsc`. I re-ran the task-level verification there:
- `cargo clippy -- -D warnings`
- `cargo test` (293 tests plus the full integration suite)
- `cargo run --quiet -- audit-docs`
- `make release`
- `make install`

I then added the repo-local Codex adapter in `plugins/corky-gmail/`:
- plugin manifest plus marketplace entry in `.agents/plugins/marketplace.json`
- a `corky-gmail` skill for auth, refetch, draft, push, and send flows
- thin wrapper scripts for `doctor gmail --json`, `sync refetch --json`, guarded `draft new`, `draft push --json`, and `draft send --json`

The `draft-new.sh` wrapper specifically blocks the bad threading path this task was about: it rejects using the Gmail thread ID as `--in-reply-to` and requires the RFC 2822 `Message-ID` there.

I also validated the adapter locally: both JSON manifests parse cleanly, all wrapper scripts pass `bash -n`, the `draft-new.sh` guard rejects the wrong thread-id flow, and `auth-status.sh` resolves to the local `corky` binary correctly.

I did not make the requested root-repo commit or push. The blocker is the current `agent-loop` repo state: `main` is already ahead of `origin/main` by 10 commits and the worktree is dirty across many unrelated tracked and untracked paths, so a clean adapter-only `commit + push` from here is not safe.

❯ Is #1bgy complete?
### Re: Codex Gmail support plan — gpt-5

Not under the exact meaning of `#1bgy` as written.

The implementation part is complete:
- Gmail reply/thread correctness in `src/corky`
- connector-grade JSON/auth-status CLI surfaces in `src/corky`
- the repo-local Codex adapter outside `corky` in `plugins/corky-gmail/`
- spec/test/build/install verification

What is still not complete is the final `commit + push` part for the root `agent-loop` repo. Those adapter files are present locally, but this repo is still dirty across many unrelated paths and `main` is already ahead of `origin/main`, so I do not consider `#1bgy` fully done yet in the operational sense.

So the right status is: implementation complete, delivery boundary still blocked. I gated `#1bgy` instead of closing it.

❯ commit + push agent-loop.
### Re: Codex Gmail support plan — gpt-5

Done.

I made a bounded root-repo commit that includes only the `#1bgy` adapter deliverables:
- `.agents/plugins/marketplace.json`
- `plugins/corky-gmail/`
- `tasks/software/corky/plan-codex-gmail-support.md`

Commit: `caeb41ec6` (`feat: add corky gmail codex adapter`)

I then pushed `main` to `origin`, so the `agent-loop` side of this work is now shipped as well.

That means `#1bgy` is complete and I closed it.

❯ Look in the /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md agent-doc session. Why did the session fail to send to gmail even with this fix applied?
### Re: why the Ron send still failed after the Codex Gmail fix — gpt-5

It failed for a different reason than the fix addressed.

The retry in `monsterrodholders.md` used a standalone draft file at `/tmp/ron-search-console-access-request.md` with `account: personal` and no `thread_id` / `in_reply_to`, so the Gmail reply-threading fix was not even on the path for that send. The Codex adapter wrappers are also thin pass-throughs to `corky draft push` / `corky draft send`, so they do not change auth or network behavior.

What actually blocked that send was the Gmail auth/runtime path for the `personal` account:
- `src/corky/src/draft/send.rs` first asks `gmail_auth::get_access_token_for_user(..., gmail.compose, ...)` for a send token before it POSTs to Gmail
- the `personal` send token on disk is expired, so `corky` had to refresh or re-auth
- in that session, refresh failed on DNS for `oauth2.googleapis.com`, and fallback re-auth failed because the local OAuth callback server could not bind (`Operation not permitted`)

So both things are true at once: the Codex Gmail fix is present, and this particular send still failed. The fix covered connector/threading correctness; it did not remove the need for a live Gmail compose token or bypass that session's DNS/OAuth callback restrictions.

Operationally, the next step is to retry the `personal` send path from a normal shell/session that can reach Google and complete the OAuth callback, or refresh that `personal` Gmail send token there first.

❯ Why does sending gmail via corky work with Claude Code but not with Codex?
### Re: why Gmail via `corky` worked with Claude Code but not with Codex — gpt-5

The evidence here points to **runtime state**, not a fundamental Claude-vs-Codex difference.

What we know from the session logs:
- the earlier Claude Code-side send in `monsterrodholders.md` succeeded on **April 26, 2026** and sent two Ron emails from `brian.takita@gmail.com`
- the later Codex-side retry failed on **April 27, 2026** when `corky` tried to send a standalone draft through the same `personal` account
- the cached `gmail:personal` compose token is now expired (`2026-04-27T03:12:02Z`), so that later send required refresh or re-auth
- in the failing session, refresh hit DNS failure for `oauth2.googleapis.com`, and fallback re-auth failed because the local OAuth callback server could not bind (`Operation not permitted`)

So the most defensible explanation is:
- **Claude Code worked because the send happened in a session/runtime where the token path was still usable**
- **Codex failed because that later session needed live refresh/re-auth and its runtime could not complete it**

In other words, this does **not** look like "`corky` works in Claude but not in Codex" at the application level. It looks like:
- same `corky`
- same mail config
- different live session conditions at send time

The two concrete differences that matter are:
1. **Token state** — the later session hit an expired Gmail compose token for `personal`
2. **Runtime permissions/network** — that later session could not reliably resolve Google endpoints and could not open the local OAuth callback listener needed for re-auth

So if you want a precise answer: **Claude Code did not win because it is Claude Code. It won because that send happened before the auth/runtime path broke for the later Codex attempt.**

❯ Can corky create a fresh token?
### Re: whether `corky` can create a fresh token — gpt-5

Yes, but only by completing the normal Google OAuth flow.

`corky` does two things when it needs a token:
- if there is an expired token with a usable refresh token, it tries to **refresh** it
- if refresh fails or the stored scope is wrong, it falls back to a full **re-auth** flow and then writes the new token into the local token store

So in principle, yes: `corky` can create a fresh token for Gmail.

The important constraint is that it cannot invent one offline. The session still needs to be able to:
- reach Google (`oauth2.googleapis.com` and related endpoints)
- open the local OAuth callback listener so the browser auth flow can return

For this repo, the practical paths are:
- `corky filter auth` for the general Gmail OAuth path
- or just retry the Gmail action that needs the token, such as `corky draft send ...`, because that path will also trigger re-auth when needed for `gmail.compose`

So the answer is: **yes, `corky` can create a fresh token, but not from a session that cannot reach Google or complete the OAuth callback bind.**

❯ I did not see a browser window popup for oauth when I attempted to send an email.
### Re: why no OAuth browser popup appeared — gpt-5

That is consistent with how `corky` is written.

In `run_auth_flow_with_scope`, `corky` does this in order:
1. prints `Opening browser for Gmail authorization...`
2. calls `open::that(url)` as a **best-effort** browser launch
3. prints the fallback URL (`If the browser doesn't open, visit: ...`)
4. only after that, tries to start the local callback server on `127.0.0.1:8484`

So if you did **not** see a browser window, the likely explanation is not that OAuth never started. It is that the browser-launch step did not visibly succeed in that runtime. `corky` already tolerates that case and continues by printing the URL and waiting for callback.

Then, in your failing session, the next step also broke: the callback server bind failed with `Operation not permitted`. So you had **two** problems in sequence:
- no visible browser auto-open
- then no usable local callback listener

That is why you saw neither a popup nor a completed OAuth flow.

So the precise answer is: **`corky` probably did attempt to open OAuth, but browser launch is non-guaranteed, and that session then failed on the callback-server step anyway.**

❯ release + publish
### Re: release + publish — gpt-5

I executed the release-prep work that is safe to do without changing the declared version:
- `make check` passed cleanly in `src/corky` (clippy + full test suite; 293 unit tests plus the integration suites)
- `make release` succeeded and refreshed `.bin/corky`
- `cargo publish --dry-run` succeeded through packaging and verification

The actual publish step is blocked on two concrete release-boundary issues:

1. **`0.27.0` is already published**
   - `cargo publish --dry-run` warns: `crate corky@0.27.0 already exists on crates.io index`
   - so I cannot publish the current tree under the existing version

2. **`src/corky`'s own release policy requires an explicit new version and mainline release path**
   - `src/corky/AGENTS.md` says versions are bumped explicitly by the user, not automatically
   - the same file's release process is: bump version, merge to `main`, tag `v<version>`, then `cargo publish` / `maturin publish`
   - the unreleased work is currently on branch `20260413/corky-gsc` at `cf63185`, not on `main`

So I did **not** run the real `cargo publish` / PyPI publish, because doing so now would either fail immediately on crates.io version reuse or violate the repo's stated release process.

If you want me to finish the release, the next explicit step is to tell me the new version to cut for this branch work. Then I can bump version files, update `VERSIONS.md`, merge/release per the repo policy, and publish.

❯ do #gscsa. spec-test-build-install-commit-push
### Re: GSC auth bug — gpt-5

I completed the `#gscsa` implementation locally.

I checked Google's current Search Console docs before changing the auth model:
- Search Console user management says added users must have a valid Google Account: https://support.google.com/webmasters/answer/7687615
- the Search Console API docs say the caller must already have appropriate Search Console access: https://developers.google.com/webmaster-tools/about

From that, the safe implementation is not to assume a service-account email is addable as a property user.

In `src/corky` I changed the GSC path accordingly:
- `src/gsc/auth.rs` now prefers stored/refreshed `gsc:*` user tokens first
- a configured service-account path is only kept if the minted token can successfully list at least one visible Search Console property
- if that probe fails or comes back empty, corky now warns and falls back to `corky gsc auth`
- help/spec/diagnostic text no longer tells the user to unconditionally add the SA email in Search Console
- added unit coverage for the no-sites probe and fallback messaging

Verification passed end-to-end:
- `make check`
- `cargo run --quiet -- audit-docs`
- `make release`
- `make install` (`transcribe-cuda` path)

I did **not** make the requested commit/push. The blocker is the current `src/corky` repo state: branch `20260413/corky-gsc` already has unrelated pre-existing modifications across multiple `.claude/skills/...` files, and `src/corky/AGENTS.md` requires clean commits with no dangling unstaged files. I won't bundle those unrelated edits into this GSC fix.

If you want me to finish the delivery boundary, the next safe step is either to include those existing skill-doc changes intentionally in the same commit, or to clean/split them first and then let me commit/push the GSC work.
### Re: GSC auth bug — gpt-5

The code/spec/test/build/install portion of `#gscsa` is complete locally, but the task remains open because the requested commit/push boundary is still blocked.

I checked Google's current Search Console docs before changing the auth model:
- Search Console user management says added users must have a valid Google Account: https://support.google.com/webmasters/answer/7687615
- the Search Console API docs say the caller must already have appropriate Search Console access: https://developers.google.com/webmaster-tools/about

From that, the safe implementation is not to assume a service-account email is addable as a property user.

In `src/corky` I changed the GSC path accordingly:
- `src/gsc/auth.rs` now prefers stored/refreshed `gsc:*` user tokens first
- a configured service-account path is only kept if the minted token can successfully list at least one visible Search Console property
- if that probe fails or comes back empty, corky now warns and falls back to `corky gsc auth`
- help/spec/diagnostic text no longer tells the user to unconditionally add the SA email in Search Console
- added unit coverage for the no-sites probe and fallback messaging

Verification passed end-to-end:
- `make check`
- `cargo run --quiet -- audit-docs`
- `make release`
- `make install` (`transcribe-cuda` path)

I did **not** make the requested commit/push. The blocker is the current `src/corky` repo state: branch `20260413/corky-gsc` already has unrelated pre-existing modifications across multiple `.claude/skills/...` files, and `src/corky/AGENTS.md` requires clean commits with no dangling unstaged files. I won't bundle those unrelated edits into this GSC fix.

If you want me to finish the delivery boundary, the next safe step is either to include those existing skill-doc changes intentionally in the same commit, or to clean/split them first and then let me commit/push the GSC work.

❯ commit + push all files in src/corky
### Re: GSC auth bug — gpt-5

I completed the remaining delivery boundary.

In `src/corky` I staged **all** current tracked changes, committed them, and pushed branch `20260413/corky-gsc` to `origin`.

Commit:
- `e84e38e` — `fix: fall back to user oauth for gsc`

Push result:
- `origin/20260413/corky-gsc` advanced from `cf63185` to `e84e38e`
- the branch now tracks `origin/20260413/corky-gsc`
- `git status` is clean

That means `#gscsa` is complete, so I closed it.

❯ I followed the instructions:
```
• #corky-bug is already complete in corky, not in this repo.

  The closure is recorded in /home/brian/work/btakita/agent-loop/tasks/software/corky.md:372: the GSC auth fix was
  committed, pushed, and [#gscsa] was marked done at /home/brian/work/btakita/agent-loop/tasks/software/corky.md:386.

  The Monster Rod Holders follow-up is at tasks/monsterrodholders.md:1634. Current state:

  - corky gsc no longer depends on adding the service-account email in Search Console.
  - It now falls back to user OAuth correctly.
  - The remaining blocker is this session’s loopback callback bind on 127.0.0.1:8485, not Search Console permissions.

  Next step is to run from a normal desktop shell:

  cd /home/brian/work/btakita/agent-loop/mail
  corky gsc auth
  corky gsc sites --format table

  If that works, #cgb1 should be treated as obsolete for the corky gsc path, and the next real checkpoint is #v81s.
```

❯ When I ran `corky gsc auth`, I got an error:
```
Access blocked: This app’s request is invalid

You can’t sign in because this app sent an invalid request. You can try again later, or contact the developer about this issue. Learn more about this error
If you are a developer of this app, see error details.
Error 400: redirect_uri_mismatch
```
### Re: GSC auth bug — gpt-5

The `redirect_uri_mismatch` is another real corky bug.

Root cause: `corky gsc auth` was the only Google OAuth flow using `http://127.0.0.1:8485/callback`, while the rest of corky's reused Gmail/Calendar client surfaces use `http://127.0.0.1:8484/callback`. So a custom Google OAuth client that already works for Gmail / Calendar can still fail only on GSC with:
- `Error 400: redirect_uri_mismatch`

I fixed that locally in `src/corky`:
- `src/gsc/auth.rs` now uses the shared `127.0.0.1:8484/callback` redirect instead of `8485`
- `SPEC.md` was updated to match the real callback behavior

Verification passed again end-to-end:
- `make check`
- `cargo run --quiet -- audit-docs`
- `make release`
- `make install` (`transcribe-cuda` path)

I did **not** commit or push this follow-up yet.

The next real check is to retry from a normal desktop shell:

```bash
cd /home/brian/work/btakita/agent-loop/mail
corky gsc auth
corky gsc sites --format table
```

If that succeeds, then the earlier `#cgb1` framing is stale for the GSC auth path, and the remaining blocker is whatever comes after successful user OAuth, not Search Console permissions and not redirect registration.
<!-- agent:boundary:6faae5b4 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending -->
- [ ] [#0ptw] [recommended] Re-run `corky gsc auth` and `corky gsc sites --format table` from a normal desktop shell to confirm the shared 8484 redirect fix resolves the Google OAuth error.
<!-- /agent:pending -->
