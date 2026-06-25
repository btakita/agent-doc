---
agent_doc_session: 584ef929-37ee-47f6-b3d8-d4266ad31ee9
agent: opencode
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
queue: stop
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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/3fe810fc180ec90ce4bbb9c2bdb013766843d9a8224cac4431150341ae259853-20260625-030019.md`*

Compacted content:
❯ - Archived 15 response topic(s): #ckyorphan; IPC proof diagnostic (interrupted-cycle recovery); #ckypanic; 12 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/3fe810fc180ec90ce4bbb9c2bd...

### Re: #next-steps — glm-5.2

> **Queue prompt:**
>
> What are #next-steps to fix corky bugs?


**Backlog verified — all 3 items legitimately open; the semantic-completion matches are false positives.** Preflight flagged #ckymimeinline, #ckymimestream, #ckythreadmerge as possibly resolved by their shipped parents. Checked origin/main (fetched) — each parent fix was intentionally partial and explicitly carved these out:

- `#ckymime` (15c0498) fixed only filename sanitization + In-Reply-To normalization. Its commit body states the RAM-buffering and inline-CID-image sub-issues are "split into their own backlog items (#ckymimestream, #ckymimeinline) as they are feature/architecture-sized."
- `#ckythread` (6894c88) fixed IMAP Message-ID population + partial-reply warning. Its commit body states the cross-provider thread merge "is architectural and split into its own backlog item (#ckythreadmerge)."
- **`find_thread_file`** (sync) and **`find_thread_file_by_id`** (refetch) are alias-aware, so a merged thread resolves by any claimed key.
- **`merge_message_to_file`** resolves the existing file by the provider key (alias-aware), then falls back to a **Message-ID lookup** so the same message from two providers lands in one file; the incoming key is recorded as an alias. Alias recording runs *before* the dedup short-circuit, so it persists even when the bridged message is a duplicate.
- **`Thread.aliases`** — a file "claims" key `K` when `Thread ID == K` or `aliases` contains `K`. New `**Thread Aliases**:` line (serialize + parse); `parse_thread_identity` / `claims_key` helpers complete the foundation that was half-staged in the working tree (it parsed `aliases` but dropped it on construction — fixed).

Nothing reaped.

**Priority reorder (applied this cycle).** Backlog was ordered P3, P3, P2 — the lone correctness item was last. Reordered to P2 → P3 → P3: `#ckythreadmerge`, `#ckymimestream` (crash/OOM risk: ~1.8 GB peak for 500 MB), `#ckymimeinline` (silent feature gap).

**Top of backlog: `#ckythreadmerge`**

No new items added — the prior #code-review sweep already surfaced these; every other #cky* finding shipped on origin/main (6894c88 thread, 6c4c831 slug, 63cbb52 durability, 15c0498 mime). Remaining open work: the three splits above plus the operator-gated #pypi-trusted-publisher in review.

Note: the local checkout at `~/work/btakita/corky` is stale at v0.13.1 (monolithic `src/`) with 3 uncommitted modifications (`filter/check.rs`, `filter/gmail_auth.rs`, `watch.rs`); the v0.29.0 workspace lives on origin/main. A fresh #code-review should run against an updated checkout.

### Re: #ckythreadmerge — glm-5.2

> **Queue prompt:**
>
> `#spec-test-build-install-commit-push` — `[#ckythreadmerge]` (P2 correctness, split from #ckythread)

**Shipped: cross-provider thread merge via Message-ID bridge** — corky `535594f` → `origin/main` (`15c0498..535594f`).

The same conversation synced from IMAP (subject-keyed) and Gmail API (`threadId`-keyed) forked into two files. Reconciled by keying the merge on the globally unique `Message-ID`:

- **`Thread.aliases`** — a file "claims" key `K` when `Thread ID == K` or `aliases` contains `K`. New `**Thread Aliases**:` line (serialize + parse); `parse_thread_identity` / `claims_key` helpers complete the half-staged foundation (it parsed `aliases` but dropped it on construction — fixed).
- **`merge_message_to_file`** resolves the existing file by the provider key (alias-aware), then falls back to a **Message-ID lookup** so the same message from two providers lands in one file; the incoming key is recorded as an alias. Alias recording runs *before* the dedup short-circuit, so it persists even when the bridged message is a duplicate.
- **`find_thread_file`** (sync) and **`find_thread_file_by_id`** (refetch) are alias-aware, so a merged thread resolves by any claimed key.

Files: `crates/corky-mail/src/sync/{types,markdown,imap_sync,mod}.rs`, `tests/test_sync.rs`, `SPEC.md` (§3.1 + new §4.5; renumbered Label Routing→§4.6, Manifest→§4.7).

**Spec + tests.** +3 integration tests (`test_cross_provider_merge_by_message_id`, `test_alias_lookup_merges_followup_by_key`, `test_no_merge_without_message_id`) + 4 markdown unit tests (`claims_key`, `parse_thread_identity`, aliases round-trip). `test_sync` 30/30, markdown unit 10/10, `cargo clippy -p corky-mail --tests` clean.

**Build + install.** `make build` (transcribe-cuda) green; `make install` → `corky 0.29.0` from this tree at `~/.cargo/bin/corky`.

**Verification notes.** CI run `28146555728` (CI workflow) in_progress at closeout — proceeding on local evidence per the in-progress rule. The 6 `test_doctor` failures are **pre-existing/environmental** (they read `~/work/btakita/mail/.corky.toml`, which lacks gmail-api/LinkedIn/YouTube setup); confirmed identical on a stashed baseline (5 passed / 6 failed both with and without my changes — they touch no sync code). `Cargo.lock` churn (agent-kit source/checksum + an unused `instruction-files` patch) is unrelated and was left **unstaged**.

**Limitation (documented §4.5):** a message with no `Message-ID` cannot bridge, so two distinct keys for it still yield two files (pre-fix behavior); replies unique to one provider attach via the alias once the parent files are merged.

Queue advances to **#ckymimestream** (draft-send buffers whole attachment in RAM — stream base64 / use Gmail resumable upload).

### Re: #ckymimestream — glm-5.2

> **Queue prompt:**
>
> `#spec-test-build-install-commit-push` — `[#ckymimestream]` (P3, split from #ckymime)

**Shipped: streaming large-attachment base64** — corky `9e3f1c4` → `origin/main` (`535594f..9e3f1c4`).

`corky draft send` read each attachment fully, base64-encoded it into a `String`, accumulated it into the MIME string, then base64url-encoded the whole MIME again for the JSON `raw` field — peaking at ~1.8 GB for a 500 MB file. Now it streams:

- **`build_mime_to_writer`** writes attachments in 57-byte (→ 76-char) chunks straight to a writer (byte-identical to the old in-memory builder), so neither the raw bytes nor their full base64 string sit in RAM.
- For combined attachments ≥ 5 MB, the MIME is spooled to a temp file and the Gmail `{"raw":"…"}` body is streamed through a base64url encoder (`Base64UrlReader` + `StreamingRawBody`); the whole-message payload is never materialized as one string. Below the threshold the simple in-memory JSON path is unchanged.
- GPU transcription behind `transcribe-cuda` / `transcribe-metal` features; `make build` requires a local GPU.
- `#ckymimestream` (9e3f1c4): attachment base64 streams in 57-byte chunks; ≥5 MB MIME spools to a temp file and the Gmail `{"raw":…}` body streams through `Base64UrlReader` + `StreamingRawBody`. Peak RAM for a 500 MB attachment: **~1.8 GB → streaming buffers** (spool on disk, not RAM).
- `build_mime_message/with_attachment` (4 KiB) — **~8.71 µs** (231k iter; cost is base64-encoding the attachment)
- `build_mime_message/in_reply_to` — **~209 ns** (9.6M iter)
- `build_mime_message/plain` — **~160 ns** (13M iter)

Files: `crates/corky-mail/src/draft/send.rs`, `SPEC.md` §5.5.1.

**Tests.** +4 (`base64url`-streamer parity across 0/1/2/57/4096/8192-byte inputs; JSON-body framing with/without `threadId`; streamed attachment round-trips to the original bytes with ≤76-char lines). `corky-mail` lib 235/235, `test_sync` 30/30, clippy clean.

**Build + install.** `make build` (transcribe-cuda) green; `make install` → `corky 0.29.0`.

**Design note.** The backlog suggested the Gmail resumable/multipart media-upload endpoint; I kept the proven `messages.send` + `raw` endpoint and streamed the base64url body instead — identical wire format, fully unit-testable offline (live send can't be exercised here without OAuth). Peak RAM for a 500 MB attachment drops from ~1.8 GB to roughly the streaming buffers (spool on disk, not RAM).

Queue advances to **#ckymimeinline** (draft send drops inline CID images — reuse draft push `multipart/related` logic in `build_mime_message`).

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

> **Queue prompt:**
>
> :pushpin: [#ckymimeinline]


```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/software/corky.md source=socket_ack_content patch_id=460ac752-e608-4362-9b45-7b6ff9a7475b invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=13957 candidate_hash=cdb4277d2d8b094340cef2d1650c6a73258d444cedfef72528ff9d7d8152d929 content_ours_len=10789 content_ours_hash=6e667648259b6a0c4f665f6f845103c1b74033c914b3f0cad62b2048528fd13e doc=corky session=584ef929-37ee-47f6-b3d8-d4266ad31ee9 turn=cycle-1782344910865
```

### Re: "1 test ignored" — glm-5.2

No test is actually ignored. corky has **zero** `#[ignore]` attributes anywhere in the workspace, and every test binary reports `0 ignored` (verified via `cargo test --workspace --features transcribe-cuda --no-fail-fast`).

What you saw is the test **name** `s9_non_md_ignored` (`crates/corky-mail/src/schedule.rs:580`). cargo prints:

    test schedule::tests::s9_non_md_ignored ... ok

The trailing `_ignored` is part of the name — the test asserts that `scan_social_dir` **ignores non-markdown files** (it drops a `readme.txt` in the social dir and asserts the scheduler yields no items). It passes. At a glance "ignored" reads like a runner status, but it's the S9 behavioral test, not an ignored test.

### Re: performance report — glm-5.2

**Test suite** (~690 tests, `--features transcribe-cuda --no-fail-fast`): 684 passed / 6 failed / **0 ignored**. Nearly every binary finishes in <10 ms; slowest are `test_cli` (0.53 s), `test_resolve`/`test_sync` (~0.1 s). The 6 `test_doctor` failures are **pre-existing/environmental** — they read `~/work/btakita/mail/.corky.toml` (no gmail-api/LinkedIn/YouTube setup), identical on a stashed baseline.

**Release binary:** 49 MB; debug+CUDA 316 MB. Release profile is aggressively tuned — `opt-level=3`, `lto="fat"`, `codegen-units=1`, `panic="unwind"` (unwind kept so `corky watch` survives a panicking tick via tokio's `JoinError`, #ckywatchpanic).

**MIME hot-path bench** (`criterion` `benches/draft_mime.rs`, after the fix below):
- `build_mime_message/plain` — **~160 ns** (13M iter)
- `build_mime_message/in_reply_to` — **~209 ns** (9.6M iter)
- `build_mime_message/with_attachment` (4 KiB) — **~8.71 µs** (231k iter; cost is base64-encoding the attachment)

**Runtime perf wins already shipped:**
- `#ckymimestream` (9e3f1c4): attachment base64 streams in 57-byte chunks; ≥5 MB MIME spools to a temp file and the Gmail `{"raw":…}` body streams through `Base64UrlReader` + `StreamingRawBody`. Peak RAM for a 500 MB attachment: **~1.8 GB → streaming buffers** (spool on disk, not RAM).
- GPU transcription behind `transcribe-cuda` / `transcribe-metal` features; `make build` requires a local GPU.

**Regression found — `make check` was RED on main.** `#ckymimeinline` (6056584) added a 7th `images: &[PathBuf]` param to `build_mime_message` (`crates/corky-mail/src/draft/send.rs:443`) but never updated `benches/draft_mime.rs` — 3 callsites still pass 6 args, so `cargo clippy --all-targets` (= `make clippy`/`make check`) fails with `E0061`. Benches aren't built by `cargo test`, which is why the 237 mail tests stayed green while the gate went red.

Fix applied **locally** (empty `images` slice added to the 3 bench callsites); clippy `--all-targets` is green again and the bench numbers above come from that fixed tree. I did **not** commit it — you asked for a report, not a ship. It's logged as `#ckybenchbreak` (P2), fix ready, just needs commit + push to restore the gate.

**Queue note:** the active head `#ckymimeinline` already shipped (6056584), so the queue was stale — marked it done. `#ckybenchbreak` is the only new open item.

### Re: fix test suite — glm-5.2

> **Queue prompt:**
>
> fix test suite

**Test suite fully green: 690 passed / 0 failed / 0 ignored** (was 684/6), and `make check` (clippy `--all-targets`) is green again. Shipped to corky origin/main (`6056584..39f266a`).

**Root cause — `test_doctor` isolation broken by a `./mail` symlink.** The corky repo has `mail -> ~/work/btakita/mail`. `resolve::data_dir()` checked the CWD `mail/` heuristic — `PathBuf::from("mail").is_dir()` follows symlinks → true — **before** `CORKY_DATA`, so every `test_doctor` case loaded the developer's real config instead of its fixture. The 5 "passing" tests passed by coincidence (negative assertions); the 6 failures asserted positive content the real config lacks (gmail-api "main", LinkedIn/YouTube sections, checkmarks).

**Fix `f3098ec` (#ckysuite):** `CORKY_DATA` — an explicit override (`--mailbox`, `corky init`, tests) — now wins over implicit CWD/app-config discovery (`crates/corky-core/src/resolve.rs`). SPEC §2.2 updated. All 11 `test_doctor` tests pass against their fixtures.

**Fix `39f266a` (#ckybenchbreak):** the `draft_mime` criterion bench now passes the `images` slice added by #ckymimeinline (the E0061 that made `make check` red). Bench numbers: `build_mime_message` plain ~160 ns, in_reply_to ~209 ns, with_attachment(4 KiB) ~8.71 µs.

**Build + install:** `make build` (transcribe-cuda) green; `make install` → `corky 0.29.0`. Both commits pushed (`6056584..39f266a`). `Cargo.lock` churn left unstaged (unrelated).

### Re: do [#ckysuite] — glm-5.2

> **Queue prompt:**
>
> do [#ckysuite]

Already shipped as part of the "fix test suite" turn — corky `f3098ec` (prioritize `CORKY_DATA` over the `./mail` symlink heuristic in `resolve::data_dir`), SPEC §2.2 updated, on origin/main. No further work; reaping `#ckysuite`. Full suite: 690 passed / 0 failed / 0 ignored.

### Re: #next-steps to fix corky bugs — glm-5.2

> **Queue prompt:**
>
> What are #next-steps to fix corky bugs?

All corky bug items are shipped; suite is green (690/0/0, origin/main through `39f266a`): `#ckythreadmerge`, `#ckymimestream`, `#ckymimeinline` earlier, plus `#ckybenchbreak` (`39f266a`, make-check gate) and `#ckysuite` (`f3098ec`, test_doctor isolation) this turn.

**Open corky work:**
1. `#pypi-trusted-publisher` (review/gated) — the only remaining open item; operator-gated (PyPI must add a trusted publisher matching `repo:btakita/corky:environment:pypi`). No code action until then.
2. Optional hardening: add an `inline_images` case to the now-compiling `draft_mime` bench; extend `test_doctor`-style fixture isolation to any other integration test touching the real `~/work/btakita/mail` config.
3. Deeper coverage: property/round-trip tests for sync alias-merge + base64 streaming; live OAuth paths (untestable offline today).

No new corky bugs are known open — next steps are closing #pypi-trusted-publisher, then hardening rather than more bug fixes.

### Re: lazily-rs / reactive graphs + state machines — glm-5.2

> **Queue prompt:**
>
> Would corky benefit from lazily-rs? Reactive graphs + state machines?

**Mixed — typed state machines = modest win; reactive graphs = likely overkill.** (Evaluating against your "reactive graphs + state machines" framing of lazily-rs.)

Stateful logic today:
- `corky watch` (`src/watch.rs`) — tokio daemon ticking hourly: filter-drift check, `schedule run`, sync. A poll loop with a few statuses.
- Publish/sync lifecycle — **stringly-typed** (`"ready"`/`"publishing"`/`"published"`, `manifest.toml` flags); OAuth + sync have implicit phase transitions.

**Reactive graphs (propagated/derived signals):** corky is a batchy CLI/daemon, not a UI or a system with many derived interdependent values. The watch "poll → check → act" loop doesn't gain from signal propagation — it'd add indirection without payoff.

**Explicit state machines (typed `enum State` + transitions):** real win. Modeling the draft/social publish lifecycle and sync phases as typed enums (instead of `String` statuses + ad-hoc checks) catches invalid transitions at compile time and makes guards like `s8b_social_published_skipped` (double-publish) and sync token/refetch phases provably correct. Localized refactor, not a framework.

**Recommendation:** don't adopt lazily-rs/reactive graphs for corky's current shape. Do introduce `enum`-based state machines for publish + sync lifecycles (focused P3 hardening). Revisit reactive only if corky grows a long-lived observer graph (e.g., a TUI over watch state).
<!-- agent:boundary:4043f75c -->
<!-- /agent:exchange -->
###
<!--
Why is 1 test ignored?
Please give me a report the performance
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- From a code + architecture perspective, would lazily-rs add much overhead vs a non-reactive graph?
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#pypi-trusted-publisher] Configure PyPI trusted publishing for btakita/corky workflow .github/workflows/pypi.yml with environment pypi. Repo workflow is configured and pushed in Corky commit feb074e; PyPI run 26195923707 now builds all artifacts but is blocked by PyPI invalid-publisher until the project has a trusted publisher matching repo:btakita/corky:environment:pypi.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/corky.done.md -->

<!-- completed work archived in tasks/software/corky.done.md -->
<!-- /agent:done -->
