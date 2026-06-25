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

### Re: #ckyorphan — P0 orphan-cleanup data loss — opus-4-8

Fixed and shipped (corky `9f74291` on `origin/main`). The fix was already implemented in the working tree from a prior interrupted cycle; this cycle verified, built, installed, and committed it.

**Both data-loss paths guarded** in `crates/corky-mail/src/sync/mod.rs`:

- **(a) Scoped `--account X` wipe** — a scoped full sync records only that account's touched files, but conversations live in one global `conversations/` dir, so global cleanup deleted every *other* account's data. Scoped syncs now **skip cleanup entirely** with a printed notice (`run()`, ~`:265`).
- **(b) Transient empty/partial fetch wipe** — an empty/short fetch left `touched` empty, classifying every file as an orphan and reaping the whole store with no self-heal (state unchanged → no re-fetch). `cleanup_orphans` now **skips with a warning** when it would delete more than `ORPHAN_CLEANUP_MAX_DELETE_FRACTION` (50%) of conversations; files stay put for the next good sync. `CORKY_SYNC_FORCE_ORPHAN_CLEANUP=1` overrides for intentional bulk deletes.

**Verification (`#spec-test-build-install-commit-push`):**
- Tests: 3 new unit tests (empty-touched preserve, majority-orphan skip, below-threshold delete) — `cargo test -p corky-mail` → **219 passed**.
- Clippy: clean (only pre-existing patch-not-used + imap-proto future-incompat warnings).
- Build + install: `cargo build --release` + `cargo install --path .` → `corky 0.29.0` replaced in `~/.cargo/bin`.
- Spec: SPEC.md §6.5 + AGENTS.md document both guards.
- Commits: `a6ca9f0` (fix) + `9f74291` (lock criterion dev-deps for the `#ckybench` bench + bench clippy fix) — pushed.

Next queue head: `#ckypanic` (P0 UTF-8 byte-slice panic).

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/software/corky.md source=socket_ack_content patch_id=e79e28c7-55bf-4557-9143-488d98c5a0d7 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=12168 candidate_hash=21bf3bfbd509108ec59e33bfab7758d34334e4a7003a90ebf7e9f877b0fef77f content_ours_len=12573 content_ours_hash=7d74f5df35a669b8fed1739354259b41220827a20be0d71414862f207eb11e84 doc=corky session=584ef929-37ee-47f6-b3d8-d4266ad31ee9 turn=cycle-1782349411341
```

### Re: #ckypanic — P0 UTF-8 byte-slice panic — opus-4-8

> **Queue prompt:**
>
> [#ckypanic]


Fixed and shipped (corky `bddfab0` on `origin/main`). Two display paths sliced strings by **byte** index and panicked on realistic input:

- **`draft push` body preview** — `&body[..80]` (`corky-mail/src/draft/mod.rs:712`) panics when byte 80 lands inside a multi-byte char (CJK/emoji replies).
- **`corky tasks list`** — `&due[..10]` (`corky-google/src/tasks/list.rs:40`) panics on a `due` field shorter than 10 bytes (or non-ASCII).

**Fix:** added `corky_core::util::truncate_chars(s, max)` — takes the first `max` Unicode scalar values via `chars().take()`, never splitting a UTF-8 boundary and never panicking on short input — and routed both sites through it. The body preview now counts chars for its `>80` ellipsis guard; the tasks due keeps its `YYYY-MM-DD` prefix.

**Verification (`#spec-test-build-install-commit-push`):**
- Tests: new unit tests (ASCII, CJK `你好世界…`, emoji `👍🏽`, short/RFC3339 `due`) — `cargo test -p corky-core truncate_chars` → 2 passed; full `make check` (GPU clippy `-D warnings` + workspace tests) green.
- Build + install (GPU/CUDA per local policy): `make release` (.bin symlink) + `make install` → `corky 0.29.0` in `~/.cargo/bin`. (Also restores a GPU binary after the prior `#ckyorphan` cycle's CPU-only `cargo install`.)
- Spec: SPEC.md §5.5 (draft push preview) + §16 (tasks list `due`) document both formats.
- Commit `bddfab0` — pushed.

Next queue head: `#ckysheets` (P0 sheets-push two-call data loss).

### Re: #ckysheets — P0 sheets-push two-call data loss — opus-4-8

Fixed and shipped (corky `5e7ba56` on `origin/main`). `corky sheets push` cleared the target tab and then wrote new values in **two separate HTTP calls** (`crates/corky-google/src/doc/sheets.rs`); a failure between them (network drop, 401, quota) left the tab empty with no backup.

**Fix — snapshot + rollback** in `push_tab`:
- Snapshot the existing rows (`fetch_rows`) **before** clearing.
- Snapshot read fails → abort before clearing, tab untouched (no data loss).
- Value write fails after the clear → save the snapshot to a local `<CSV>.bak` file **and** attempt to restore it to the tab, then error with recovery instructions.

So an interrupted push never silently discards the previous contents. I chose snapshot+rollback over an atomic `batchUpdate`/`updateCells` because it preserves the exact `valueInputOption=USER_ENTERED` parse semantics (numbers/dates/formulas) the current write relies on — an `updateCells` grid would have to re-implement type coercion.

**Verification (`#spec-test-build-install-commit-push`):**
- Tests: new `backup_csv_path` unit test (sibling/nested/extension-less paths) — `cargo test -p corky-google` → 60 passed; full `make check` (GPU clippy `-D warnings` + workspace) green.
- Build + install (GPU/CUDA): `make install` → `corky 0.29.0` in `~/.cargo/bin`.
- Spec: SPEC.md §14.2.8 documents the rollback behavior + the new snapshot read.
- Commit `5e7ba56` — pushed.

Next queue head: `#ckywatchpanic` (P1 watch daemon dies on tick panic).

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/software/corky.md source=socket_ack_content patch_id=103180a9-f5fe-418e-9696-d4bad052c171 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=16016 candidate_hash=c028248700880b79c7d1886d7af3af2c2a0f5c2f039996534555ed6fd8c45e36 content_ours_len=15869 content_ours_hash=b9d785dddaf2f661963541f3163c9705f93d940ac0aaaf85c299c913c1ec9cd2 doc=corky session=584ef929-37ee-47f6-b3d8-d4266ad31ee9 turn=cycle-1782350975165
```

### Re: #ckywatchpanic — P1 watch daemon dies on tick panic — opus-4-8

> **Queue prompt:**
>
> [#ckywatchpanic]


Fixed and shipped (corky `24baa0c` on `origin/main`). `corky watch` ran each loop tick via `spawn_blocking(...).await?`; a panicking tick surfaced as a `JoinError` that `?` propagated out of the loop — and because `[profile.release]` set `panic = "abort"`, a tick panic aborted the **whole process** before tokio could even catch it. Both paths break the "never crashes the watch loop" contract documented in `watch.rs`.

**Fix (two parts — `catch_unwind` alone is a no-op under `panic = "abort"`):**
- Switch the release profile to **`panic = "unwind"`** so a tick panic becomes a catchable `JoinError` instead of an immediate abort.
- Add `run_tick(label, f)` — runs the tick on the blocking pool and, on a panic (or cancellation), logs a warning and returns so the loop continues. All four ticks (sync, schedule, auto-upgrade, filter-drift) now route through it.

**Verification (`#spec-test-build-install-commit-push`):**
- Tests: new `#[tokio::test]` proving a panicking tick is swallowed — `cargo test -p corky --lib` → 22 passed; full `make check` (GPU clippy `-D warnings` + workspace) green.
- Build + install (GPU/CUDA): `make install` → `corky 0.29.0` in `~/.cargo/bin` (now built with unwind tables).
- Spec: SPEC.md §9.2 documents the panic isolation + unwind requirement.
- Commit `24baa0c` — pushed.

Note: switching `panic = abort → unwind` slightly enlarges the release binary (unwind tables); that's the necessary cost of the never-crash contract. Next queue head: `#ckydedup` (P1 dedup key drops distinct same-second messages).

### Re: #ckydedup — P1 dedup key drops messages + leaks cross-provider dupes — opus-4-8

> **Queue prompt:**
>
> [#ckydedup]


Fixed and shipped (corky `e14f2ae` on `origin/main`). Messages were deduplicated by the raw `(from, date)` string tuple (`crates/corky-mail/src/sync/imap_sync.rs`), which **dropped distinct same-second messages** (identical from+date string collapsed to one) and **leaked duplicates across providers** (date format/TZ and From display-name vary, so the same message synced twice looked different).

**Fix — normalized `dedup_key`:**
- Prefers the globally unique **`Message-ID`** (same message across providers shares one ID).
- Falls back to **normalized sender address** (`normalize_from_address` strips display name + lowercases) + **UTC-second timestamp** (`parse_msg_date(..).timestamp()`), so TZ/format/display-name differences no longer leak dupes and distinct same-second messages stay apart.

**Verification (`#spec-test-build-install-commit-push`):**
- Tests: 4 new unit tests (display-name strip, Message-ID priority, same-second distinct, TZ/display-name fallback) — `cargo test -p corky-mail` → 223 passed; full `make check` green.
- Build + install (GPU/CUDA): `make install` → `corky 0.29.0` in `~/.cargo/bin`.
- Spec: SPEC.md §4.3 + AGENTS.md updated.
- Commit `e14f2ae` — pushed.

Note: IMAP messages currently carry `message_id: None` (the separate `#ckythread` item fixes that), so they use the normalized-fallback key today; Gmail-API messages already benefit from Message-ID dedup. Next queue head: `#ckythread`.

### Re: #ckythread — P1 threading gaps — opus-4-8

> **Queue prompt:**
>
> [#ckythread]


Fixed and shipped the two concrete reply-threading bugs (corky `6894c88` on `origin/main`); split the architectural third into `#ckythreadmerge`.

**Fixed:**
1. **IMAP Message-ID never written** — the IMAP sync path hardcoded `message_id: None` (`imap_sync.rs`), so `**Message-ID**` metadata was missing and IMAP-account replies couldn't thread. Now extracted from the parsed headers like Subject/From/Date.
2. **Silent reply un-threading** — `corky draft send` didn't validate that a reply had **both** `in_reply_to` (RFC 2822 → `In-Reply-To` header) and `thread_id` (Gmail `threadId`). Added `reply_threading_warning`: warns when only one is set (`in_reply_to`-only → Gmail starts a new thread; `thread_id`-only → `In-Reply-To` missing for non-Gmail clients).

**Split out → `#ckythreadmerge` (new backlog item):** merging IMAP subject-keyed threads with Gmail threadId-keyed threads on a provider switch is an architectural thread-identity reconciliation that deserves its own focused cycle, not a rushed bolt-on.

**Verification (`#spec-test-build-install-commit-push`):**
- Tests: new `reply_threading_warning` unit test (both/neither/partial/whitespace) — `cargo test -p corky-mail` → 224 passed; full `make check` green.
- Build + install (GPU/CUDA): `make install` → `corky 0.29.0` in `~/.cargo/bin`.
- Spec: SPEC.md §3.1/§3.2 updated.
- Commit `6894c88` — pushed.

Next queue head: `#ckytok401`.

### Re: #ckytok401 — P1 401 never invalidates cached token — opus-4-8

Fixed and shipped (corky `ec67c22` on `origin/main`). A Google-revoked but clock-valid token trapped the user: only `draft send` cleared its token on a 401, so a 401 from Calendar/Docs/Sheets/Drive/Tasks/Chat left the dead token cached and every retry failed until a manual `tokens.json` delete.

**Fix:**
- Added `TokenStore::remove_by_access_token` + the `clear_access_token(token)` free fn (corky-core `token_store`) — invalidates a token **by its value**, so the low-level HTTP helpers (which hold only the token string, not the account key) can drop it.
- Wired it into **all 19** `401` sites: `cal/{list,check,delete,create}`, `tasks/{add,list,done}`, `doc/{sheets,gdocs,drive,upload}`, `social/chat` — clear before bailing, so the next call re-authenticates. Re-exported `clear_access_token` via `gmail_auth` and `cal::auth`.

**Verification (`#spec-test-build-install-commit-push`):**
- Tests: new `token_store` unit test (clears matching entry by value, keeps others, no-op when absent) — `cargo test` → 130 passed across touched crates; full `make check` green.
- Build + install (GPU/CUDA): `make install` → `corky 0.29.0` in `~/.cargo/bin`.
- Spec: SPEC.md §5.3.1 documents the cross-surface 401 invalidation.
- Commit `ec67c22` (16 files) — pushed.

Next queue head: `#ckypeoplesync`.

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/software/corky.md source=socket_ack_content patch_id=5c6299b8-f438-419e-bb54-92f8b396f875 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=21771 candidate_hash=6c35b974c2f5922ee37076cda785deb8222fd05a98a7afba815014a6fdc1d5ef content_ours_len=22249 content_ours_hash=9af6b869ba2576cdd9a55217e64d56aa4fd3858af37870e9cb103aeeed3dc958 doc=corky session=584ef929-37ee-47f6-b3d8-d4266ad31ee9 turn=cycle-1782352942891
```

### Re: #ckypeoplesync — P1 lost-update on people-sync-state — opus-4-8

> **Queue prompt:**
>
> [#ckypeoplesync]


Fixed and shipped (corky `6d4e643` on `origin/main`). `.people-sync-state.json` (the local name→Google `resourceName` map) was read/written with plain `fs::read`/`fs::write`, bypassing the locked atomic store used by `tokens.json` and `.sync-state.json`. Two concurrent `corky contact push` runs raced — the second load→save clobbered the first writer's freshly-stored `resourceName`s, so those contacts were re-created as **duplicates** on the next push.

**Fix:**
- `load_sync_state` → `file_store::load_json_or_default`.
- `save_sync_state` → `save_json_with_lock`, with a `merge_sync_state` that **overlays** our mappings onto the current on-disk state (union merge — `contact push` only adds/updates mappings, never removes), so a concurrent writer's entries survive.

**Verification (`#spec-test-build-install-commit-push`):**
- Tests: new `merge_sync_state` unit test (concurrent-writer entry preserved, our update wins) — `cargo test -p corky-mail` → 225 passed; full `make check` green.
- Build + install (GPU/CUDA): `make install` → `corky 0.29.0` in `~/.cargo/bin`.
- Spec: SPEC.md §8 + AGENTS.md updated.
- Commit `6d4e643` — pushed.

Next queue head: `#ckyconfigunwrap`.

### Re: #ckyconfigunwrap — P1 panic on malformed config + missing git — opus-4-8

> **Queue prompt:**
>
> [#ckyconfigunwrap]


Fixed and shipped (corky `372627b` on `origin/main`).

**Two panic classes:**
- `save_topic`/`save_contact`/social profile updates did `doc[key].as_table_mut().unwrap()`, panicking when a hand-edited `.corky.toml` put a **scalar** where a `[topics]`/`[contacts]`/`[profiles.*]` table was expected. Now `ok_or_else(|| anyhow!("… is not a table; fix or remove it"))?` — actionable error.
- The shared-mailbox `run_git` helpers (`mailbox/sync.rs`, `reset.rs`) did `Command::new("git").output().unwrap_or_else(|_| panic!(…))`, aborting the whole process if `git` was missing. Now returns code `-1` with the spawn error in stderr; callers already branch on the exit code.

**Verification (`#spec-test-build-install-commit-push`):**
- Tests: new scalar-`[topics]`-errors-not-panics unit test — `cargo test` → 296 passed across touched crates; full `make check` green.
- Build + install (GPU/CUDA): `make install` → `corky 0.29.0` in `~/.cargo/bin`.
- Spec: SPEC.md §3.3 updated.
- Commit `372627b` — pushed.

Next queue head: `#ckyunicodeslug`.

### Re: #ckyatomicwrite — P2 non-durable conversation/state writes — opus-4-8

> **Queue prompt:**
>
> [#ckyatomicwrite]


Fixed and shipped (corky `63cbb52` on `origin/main`).

**Two durability gaps:**
- Conversation markdown used plain `fs::write` (truncate+write). A crash mid-write left a half-written file that re-parses to `None` — dropping **all** previously-merged messages in that thread.
- `file_store`'s atomic JSON write flushed but never `sync_all`'d before the rename, so the rename could be durable while the data blocks weren't (a power loss could expose an empty/partial file).

**Fix:** added `file_store::atomic_write(path, contents, perms)` — temp file in the same dir → write → **`sync_all`** → rename. Routed `write_json_atomic` and both `imap_sync.rs` conversation-write sites through it (gmail-api sync shares the same merge path).

**Verification (`#spec-test-build-install-commit-push`):**
- Tests: new `file_store` unit test (full write + shorter-overwrite leaves no trailing bytes, nested dirs) — `cargo test` → 291 passed across touched crates; full `make check` green.
- Build + install (GPU/CUDA): `make install` → `corky 0.29.0` in `~/.cargo/bin`.
- Spec: SPEC.md §6.6 updated.
- Commit `63cbb52` — pushed.

Next queue head: `#ckyunicodeslug`.
<!-- agent:boundary:1a9d0b40 -->
<!-- /agent:exchange -->
###
<!--
Why is 1 test ignored?
Please give me a report the performance
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~[#ckyorphan]~~
- ~~[#ckypanic]~~
- ~~[#ckysheets]~~
- ~~[#ckywatchpanic]~~
- ~~[#ckydedup]~~
- ~~[#ckythread]~~
- ~~[#ckytok401]~~
- ~~[#ckypeoplesync]~~
- ~~[#ckyconfigunwrap]~~
- ~~[#ckyatomicwrite]~~
- 🚧 [#ckyunicodeslug]
- [#ckymime]
- What are #next-steps to fix corky bugs?
- What are #next-steps to fix corky bugs?
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#ckythreadmerge] [P2 correctness] threading provider-switch merge: IMAP threads keyed by subject (thread_key_from_subject) and Gmail-API threads keyed by Gmail threadId never merge when the same conversation is synced from both providers, producing duplicate thread files. Design a unified thread-identity reconciliation (e.g. index by Message-ID/References across providers, or map Gmail threadId<->subject-key). Architectural — needs its own focused cycle. Split from #ckythread. crates/corky-mail/src/sync/{imap_sync.rs,gmail_api_sync.rs}, markdown.rs
- [ ] 🚧 [#ckyunicodeslug] [P2] slug regex strips all non-ASCII so CJK/Cyrillic/emoji subjects collapse to untitled, untitled-2,... opaque filenames distinguished only by counter. Use regex Unicode classes or transliterate. crates/corky-core/src/util.rs:5,12-31
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
