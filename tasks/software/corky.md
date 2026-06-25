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

Nothing reaped.

**Priority reorder (applied this cycle).** Backlog was ordered P3, P3, P2 — the lone correctness item was last. Reordered to P2 → P3 → P3: `#ckythreadmerge`, `#ckymimestream` (crash/OOM risk: ~1.8 GB peak for 500 MB), `#ckymimeinline` (silent feature gap).

**Top of backlog: `#ckythreadmerge`**

No new items added — the prior #code-review sweep already surfaced these; every other #cky* finding shipped on origin/main (6894c88 thread, 6c4c831 slug, 63cbb52 durability, 15c0498 mime). Remaining open work: the three splits above plus the operator-gated #pypi-trusted-publisher in review.

Note: the local checkout at `~/work/btakita/corky` is stale at v0.13.1 (monolithic `src/`) with 3 uncommitted modifications (`filter/check.rs`, `filter/gmail_auth.rs`, `watch.rs`); the v0.29.0 workspace lives on origin/main. A fresh #code-review should run against an updated checkout.
<!-- agent:boundary:896ad8c1 -->
<!-- /agent:exchange -->
###
<!--
Why is 1 test ignored?
Please give me a report the performance
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
- 🚧 [#ckythreadmerge]
- [#ckymimestream]
- [#ckymimeinline]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#ckythreadmerge] [P2 correctness] threading provider-switch merge: IMAP threads keyed by subject (thread_key_from_subject) and Gmail-API threads keyed by Gmail threadId never merge when the same conversation is synced from both providers, producing duplicate thread files. Design a unified thread-identity reconciliation (e.g. index by Message-ID/References across providers, or map Gmail threadId<->subject-key). Architectural — needs its own focused cycle. Split from #ckythread. crates/corky-mail/src/sync/{imap_sync.rs,gmail_api_sync.rs}, markdown.rs
- [ ] [#ckymimestream] [P3] corky draft send buffers the whole attachment in RAM then base64-encodes it (~1.8GB peak for a 500MB file). Use the Gmail API resumable/multipart upload endpoint for large attachments and stream the base64 encoding instead of building one giant String. crates/corky-mail/src/draft/send.rs:build_mime_message + send_via_gmail_api. Split from #ckymime.
- [ ] [#ckymimeinline] [P3] corky draft send silently drops inline images: YAML (only draft push handles images+CID via multipart/related). Add CID inline-image support to send build_mime_message (multipart/related wrapping the text part + image parts with Content-ID), reusing draft push image-resolution logic. crates/corky-mail/src/draft/send.rs, mod.rs (push image handling). Split from #ckymime.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#pypi-trusted-publisher] Configure PyPI trusted publishing for btakita/corky workflow .github/workflows/pypi.yml with environment pypi. Repo workflow is configured and pushed in Corky commit feb074e; PyPI run 26195923707 now builds all artifacts but is blocked by PyPI invalid-publisher until the project has a trusted publisher matching repo:btakita/corky:environment:pypi.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/corky.done.md -->

<!-- completed work archived in tasks/software/corky.done.md -->
<!-- /agent:done -->
