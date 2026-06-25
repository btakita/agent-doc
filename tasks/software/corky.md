---
agent_doc_session: 584ef929-37ee-47f6-b3d8-d4266ad31ee9
agent: claude
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
queue: start
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

### Shipped items (v0.14–v0.29 workspace era)
corky is now **v0.29.0**, a 5-crate workspace (`corky-core/-google/-mail/-social/-transcribe`, commit `f79ba50`). The former "Open items" below all shipped:
- Diarization span cascade — `corky-transcribe` `diarize.rs:394` + `engine.rs:826`.
- Attachment phases 2-3 — v0.16.0.
- `collapsible_if` refactor — v0.16.0.
- `corky sync imports` dispatcher — v0.16.0.
- `corky doc upload` — main `5bbd614`.
- `corky schedule run` — `src/main.rs:261` + watch loop `src/watch.rs:122`.
- HTML email (multipart/alternative) — v0.16.0.
- Gmail API threading Bug 1 (Message-ID) + Bug 2 (threadId body) — `corky-mail` `sync/markdown.rs:145`, `draft/send.rs:101-189`.

Current open work lives in `agent:backlog` (`#ckystale`, `#ckybench`), not here.

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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/3fe810fc180ec90ce4bbb9c2bdb013766843d9a8224cac4431150341ae259853-20260625-005216.md`*

Compacted content:
❯ - Archived 2 response topic(s): IPC proof diagnostic (interrupted-cycle recovery); corky bug next-steps
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/3fe810fc180ec90ce4bbb9c2bd...
<!-- agent:boundary:738471da -->
<!-- /agent:exchange -->
###
<!--
Why is 1 test ignored?
Please give me a report the performance
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- 🚧 [#ckyorphan]
- [#ckypanic]
- [#ckysheets]
- [#ckywatchpanic]
- [#ckydedup]
- [#ckythread]
- [#ckytok401]
- [#ckypeoplesync]
- [#ckyconfigunwrap]
- [#ckyatomicwrite]
- [#ckyunicodeslug]
- [#ckymime]
- What are #next-steps to fix corky bugs?
- 🚧 What are #next-steps to fix corky bugs?
- [#ckyorphan]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#ckyorphan] [P0 data-loss] sync full orphan cleanup wipes ALL conversations: (a) --account X deletes every other account files (cleanup scans global mail/conversations but touched is account-scoped), (b) transient empty Gmail/IMAP fetch leaves touched empty so every file is reaped and not self-healing (state unchanged so no re-fetch). Guard cleanup_orphans: skip when account scoped or touched empty/sub-threshold. crates/corky-mail/src/sync/mod.rs:202-260,782-798
- [ ] [#ckypanic] [P0 crash] UTF-8 byte-slice panics on realistic input: draft body preview &body[..80] (crates/corky-mail/src/draft/mod.rs:712-713, CJK/emoji replies) and tasks due &due[..10] (crates/corky-google/src/tasks/list.rs:40, short due string). One-line char-boundary-safe truncation (chars().take() or get(..n)).
- [ ] [#ckysheets] [P0 data-loss] corky sheets push clears tab THEN writes in two HTTP calls; failure between them leaves tab empty with no backup. Use batchUpdate updateCells (atomic at API level) or buffer old values for rollback. crates/corky-google/src/doc/sheets.rs:148-174
- [ ] [#ckywatchpanic] [P1] corky watch dies on any tick panic: spawn_blocking(...).await? under panic=abort (Cargo.toml:89) propagates JoinError and aborts the daemon, contradicting the never-crashes-loop contract in watch.rs doc comments. Wrap each tick in catch_unwind and log instead of ?. src/watch.rs:309-312
- [ ] [#ckydedup] [P1 correctness] dedup key (raw_from, raw_date) drops distinct same-second messages and leaks duplicates across providers (date string varies by TZ/format, From varies by display-name). Normalize address + parse date to UTC seconds + include message_id. crates/corky-mail/src/sync/imap_sync.rs:184-197
- [ ] [#ckythread] [P1 correctness] threading gaps: IMAP path hardcodes message_id None (imap_sync.rs:542-552, breaks reply threading for IMAP accounts since **Message-ID** metadata is never written); no validation that in_reply_to AND thread_id are both set (draft/send.rs:91-104, reply silently un-threads); IMAP subject-key vs Gmail threadId-key never merge on provider switch.
- [ ] [#ckytok401] [P1] 401 from Google APIs never invalidates the cached token, so a Google-revoked but clock-valid token traps the user until manual tokens.json delete; only draft send clears its send token. Drop token on 401 across cal/doc/tasks/chat. crates/corky-google/src/{cal,doc,tasks}/*.rs, crates/corky-social/src/social/chat.rs
- [ ] [#ckypeoplesync] [P1 lost-update] .people-sync-state.json bypasses the locked atomic store (file_store::save_json_with_lock used by tokens.json and .sync-state.json); concurrent corky contact push clobbers resourceName updates and re-creates Google contacts as duplicates. Route load/save_sync_state through save_json_with_lock. crates/corky-mail/src/contact/push.rs:263-280,565,637
- [ ] [#ckyconfigunwrap] [P1 panic] as_table_mut().unwrap() panics on a hand-edited .corky.toml that uses a scalar where a table is expected; Command::new(git).output() panic! if git binary missing. Replace with ok_or_else anyhow. crates/corky-core/src/config/{topic.rs:76,contact.rs:71}, crates/corky-social/src/social/auth.rs:373-391, crates/corky-mail/src/mailbox/{sync,reset}.rs
- [ ] [#ckyatomicwrite] [P2 durability] conversation markdown written via plain fs::write (truncate+write, half-written on crash loses all prior merged messages since re-parse returns None and starts fresh); file_store atomic rename is missing sync_all before rename so not power-loss durable. Reuse NamedTempFile persist + sync_all. crates/corky-mail/src/sync/imap_sync.rs:193,224; crates/corky-core/src/file_store.rs:47-63
- [ ] [#ckyunicodeslug] [P2] slug regex strips all non-ASCII so CJK/Cyrillic/emoji subjects collapse to untitled, untitled-2,... opaque filenames distinguished only by counter. Use regex Unicode classes or transliterate. crates/corky-core/src/util.rs:5,12-31
- [ ] [#ckymime] [P2] corky draft send MIME issues: raw filename in Content-Disposition (header-injection/malformed MIME), whole attachment buffered in RAM (~1.8GB peak for 500MB), inline images: YAML silently dropped (only draft push handles images+CID), In-Reply-To not normalized to angle-bracket RFC 5322 form. crates/corky-mail/src/draft/send.rs:70-98,186-237
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#pypi-trusted-publisher] Configure PyPI trusted publishing for btakita/corky workflow .github/workflows/pypi.yml with environment pypi. Repo workflow is configured and pushed in Corky commit feb074e; PyPI run 26195923707 now builds all artifacts but is blocked by PyPI invalid-publisher until the project has a trusted publisher matching repo:btakita/corky:environment:pypi.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/corky.done.md -->

<!-- completed work archived in tasks/software/corky.done.md -->
<!-- /agent:done -->
