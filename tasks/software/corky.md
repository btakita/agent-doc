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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/3fe810fc180ec90ce4bbb9c2bdb013766843d9a8224cac4431150341ae259853-20260625-055419.md`*

Compacted content:
❯ - Archived 11 response topic(s): #next-steps; #ckythreadmerge; #ckymimestream; 8 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/3fe810fc180ec90ce4bbb9c2bd...

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/software/corky.md source=socket_ack_content patch_id=41bfdd00-e7c9-470c-90cc-a6c6deabbde5 invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=8193 candidate_hash=4a964b2f0f1939fc82bcb6f7a93769e617192c2285687632e220939fc628bd39 content_ours_len=27101 content_ours_hash=9ff52e12de2feb53d49742ca7f3588995ec8b4715536fa014c143b7a69b22ca2 doc=corky session=584ef929-37ee-47f6-b3d8-d4266ad31ee9 turn=cycle-1782366951130
```

### Re: #next-steps to implement state machines — glm-5.2

> **Queue prompt:**
> What are #next-steps to implement state machines to manage corky state?

**Audited corky's four stateful lifecycles** (origin/main `39f266a`). One is already a typed enum; the rest are stringly-typed or implicit. Full plan written to `tasks/software/corky-state-machines.md`.

| Lifecycle | Today | Gap |
|---|---|---|
| Social publish | `DraftStatus` enum ✅ (`draft.rs:12`) but mutated directly (`publish.rs:127`), no transition fn, **no `Publishing` state** | mid-publish crash can double-post on retry |
| Email draft | **stringly-typed** `"scheduled"` (`schedule.rs:146,178`) | no enum; no terminal `Sent`/`Failed` |
| Sync run | `SyncState` = cursor data only (`types.rs:76`) | no resume/progress phase |
| Watch tick | implicit phase order + loose `u64` counters (`watch.rs:320`) | no backoff on repeated failure |

**Next steps → backlog (priority order):**
1. **`#ckypubsm` (P2)** — `PublishState` + `transition()` + crash-safe `Publishing` marker persisted *before* the API call. Goes first: it's the only machine guarding an irreversible side effect (a duplicate public post).
2. **`#ckyemailsm` (P2)** — `EmailDraftStatus` enum replacing the `"scheduled"` string compares + legacy `**Status**` regex.
3. **`#ckysyncsm` (P3)** — `SyncRunState` for resume/progress (design-first).
4. **`#ckywatchsm` (P3)** — `WatchPhase` enum + backoff/circuit-breaker (polish).

Typed machines only — no lazily-rs/reactive graphs (confirmed overkill for corky's batchy shape last cycle). Each item ships as its own `#spec-test-build-install-commit-push` cycle.

**Top of backlog: `#ckypubsm`**

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=/home/brian/work/btakita/agent-loop/tasks/software/corky.md source=socket_ack_content patch_id=- invariant=missing_response_probe recovery=retry_without_disk_write response_sha256=929d6505cca8c761944be9926df20685ae6dfe0753453741c1b7e71767d2d3c6 content_len=28479 content_hash=956742e7a1360a06969351a17379b2ff4266904fa460a0d4602be3c6b075d867 doc=corky session=584ef929-37ee-47f6-b3d8-d4266ad31ee9 turn=cycle-1782367673222
```
<!-- agent:boundary:1294af98 -->
<!-- /agent:exchange -->
###
<!--
Why is 1 test ignored?
Please give me a report the performance
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 do [#ckypubsm]
- do [#ckyemailsm]
- do [#ckysyncsm]
- do [#ckywatchsm]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#ckypubsm] P2 — Social publish state machine: PublishState{Draft,Ready,Publishing,Published,Failed}+transition() centralizing PB1 guards (publish.rs:26-42); persist crash-safe Publishing marker BEFORE the API call so a mid-publish crash reconciles by post_id instead of double-posting. Plan: tasks/software/corky-state-machines.md
- [ ] [#ckyemailsm] P2 — Email draft status enum (EmailDraftStatus Draft/Scheduled/Sent/Failed) replacing the stringly-typed 'scheduled' compares (schedule.rs:146) + legacy **Status** regex (schedule.rs:178-189); terminal Sent/Failed so scheduler skips them. Plan: tasks/software/corky-state-machines.md
- [ ] [#ckysyncsm] P3 — Sync run-state machine (SyncRunState Idle/Fetching/Merging/Writing/Done/Error) persisted as sidecar so a crashed watch tick / corky sync refetch resumes instead of re-fetching; design-first. Plan: tasks/software/corky-state-machines.md
- [ ] [#ckywatchsm] P3 — Watch phase enum (WatchPhase)+typed tick schedule replacing loose cycles_since_* counters (watch.rs:320-325)+backoff/circuit-breaker for repeated phase failures (polish). Plan: tasks/software/corky-state-machines.md
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#pypi-trusted-publisher] Configure PyPI trusted publishing for btakita/corky workflow .github/workflows/pypi.yml with environment pypi. Repo workflow is configured and pushed in Corky commit feb074e; PyPI run 26195923707 now builds all artifacts but is blocked by PyPI invalid-publisher until the project has a trusted publisher matching repo:btakita/corky:environment:pypi.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/corky.done.md -->

<!-- completed work archived in tasks/software/corky.done.md -->
<!-- /agent:done -->
