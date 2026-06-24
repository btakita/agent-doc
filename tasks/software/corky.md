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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/3fe810fc180ec90ce4bbb9c2bdb013766843d9a8224cac4431150341ae259853-20260624-222347.md`*

Compacted content:
❯ - Archived 2 response topic(s): Next steps to fix corky bugs; Recovery closeout
❯ - Prior summary/context: prior compacted content: Archived 5 response topic(s): GPU-required local corky builds; Closeout repair; Sheets creat...

### Re: corky ignored-test + performance + bug next-steps — glm-5.2

**Ignored test:** none at HEAD (`76f75f2`, v0.29.0). `cargo test` = `0 ignored` across all suites; `cargo test --doc` = 0 ignored; no `#[ignore]` in `src/` or `tests/`. The "1 ignored" is no longer reproducible — likely fixed in a recent commit or a stale doctest/feature-gated artifact. Point me at the command/commit that showed it and I'll pin it.

**Performance:** test suite is fast (heaviest suite 44 tests / 0.27s; ~280 tests, all green). There are **no benchmarks in-repo** (no `benches/`, no `criterion`), so there's no numeric runtime baseline to report — filed `#ckybench`. Runtime hot spots are the `corky watch` daemon, audio diarization (GPU/Metal), and Gmail/IMAP sync. Tell me which command feels slow and I'll instrument it.

**Corky bugs (queue head):** `agent:backlog` was empty despite the "Open items" prose. Added 4 prioritized items: `#ckygmailthread` (Gmail threading Bug 1+2), `#ckydiarizespan` (diarization span fix — uncommitted working tree), `#ckybench` (benchmarks for the perf report), `#ckyschedrun` (scheduled email sends). **Top of backlog: `#ckygmailthread`** (correctness bug — replies don't thread).

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/software/corky.md source=socket_ack_content patch_id=5d96cfbd-5ae9-4578-9df1-1191291303e8 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=12002 candidate_hash=ae4a42ed9d1f3d8dab778278c3d07cf1a273a6660bb3b66f6bc8350278a82d97 content_ours_len=12362 content_ours_hash=ee3c7966174db5823da7842a7892803b2eb991fa135203e1dcfb8a9b3fbb9475 doc=corky session=584ef929-37ee-47f6-b3d8-d4266ad31ee9 turn=cycle-1782335669784
```

### Re: corky bug next-steps — reaped already-shipped items — glm-5.2

Verified against the v0.29.0 workspace crates and reaped the backlog items that are already implemented:

- **`#ckygmailthread` → done.** Gmail threading Bug 1 + Bug 2 both shipped: `crates/corky-mail/src/sync/markdown.rs:145` parses/stores `Message-ID`; `crates/corky-mail/src/draft/send.rs:101-104` adds `threadId` to the Gmail send body and emits `In-Reply-To`/`References` (send.rs:186-189). AGENTS.md codifies "set both `in_reply_to` and `thread_id`."
- **`#ckydiarizespan` → done.** `max_speaker_span_ratio()` at `crates/corky-transcribe/src/transcribe/diarize.rs:394`, used by `run_adaptive_diarization()` cascade at `engine.rs:826-843`.
- **`#ckyschedrun` → done.** `corky::schedule::run()` at `src/main.rs:261`, driven by `schedule_tick()` in the watch loop (`src/watch.rs:122-124`).

Remaining corky backlog (genuinely open, not bugs): `#ckystale` (refresh the v0.12–v0.13 session history to v0.29 + fresh `#code-review` across the 5 crates) and `#ckybench` (add criterion benchmarks — the perf report has no numeric baseline yet). **Top of corky backlog: `#ckystale`.**

> **Queue prompt:** Add #next-steps to fix corky bugs.

### Re: [#ckystale] — corky.md session history refreshed to v0.29 — glm-5.2

Replaced the obsolete "Open items" prose (every item had shipped) with a "Shipped items (v0.14–v0.29 workspace era)" section that records the current state: corky is v0.29.0, a 5-crate workspace (`corky-core/-google/-mail/-social/-transcribe`, commit `f79ba50`), and each former open item is marked shipped with its evidence path (diarization cascade, attachments, collapsible_if, sync imports, doc upload, schedule run, HTML email, Gmail threading Bug 1+2). Pointed the doc at `agent:backlog` for current open work.

`#ckystale` → done. Remaining corky backlog: `#ckybench` (criterion benchmarks — the perf report still has no numeric baseline).

> **Queue prompt:** [#ckystale]
<!-- agent:boundary:700506aa -->
<!-- /agent:exchange -->
###
<!--
Why is 1 test ignored?
Please give me a report the performance
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~Add #next-steps to fix corky bugs.~~
- ~~[#ckystale]~~
- ~~[#ckygmailthread]~~
- ~~[#ckydiarizespan]~~
- [#ckybench]
- ~~[#ckyschedrun]~~
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#ckybench] Add criterion benchmarks (no perf instrumentation exists today): cover the watch loop tick, sync incremental pull, and draft composition/HTML conversion (pulldown-cmark). Gives the operator-requested performance report a numeric baseline instead of ad-hoc timing.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#pypi-trusted-publisher] Configure PyPI trusted publishing for btakita/corky workflow .github/workflows/pypi.yml with environment pypi. Repo workflow is configured and pushed in Corky commit feb074e; PyPI run 26195923707 now builds all artifacts but is blocked by PyPI invalid-publisher until the project has a trusted publisher matching repo:btakita/corky:environment:pypi.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/corky.done.md -->

<!-- completed work archived in tasks/software/corky.done.md -->
<!-- /agent:done -->
