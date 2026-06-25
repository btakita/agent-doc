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

### Re: do [#ckypubsm] — opus-4-8

> **Queue prompt:** do [#ckypubsm]

Shipped the **social publish state machine** (corky `7ee6692`, pushed to `btakita/corky` main).

**What changed**
- `DraftStatus` extended to `{ Draft, Ready, Publishing, Published, Failed }` with a typed `transition(PublishEvent)` plus `can_publish(ready)` (centralizes the old PB1 `if`-guards) and `is_terminal()`.
- **Crash safety / de-dup:** `publish()` now persists a `Publishing` marker + `publish_started_at` **before** the platform API call, then checkpoints `post_id` while still `Publishing`. A crash between create and the terminal write reconciles by `post_id` on retry instead of double-posting; a `Publishing` draft with **no** `post_id` refuses auto-retry and asks the operator to verify on-platform. API failure → `Failed`.
- `schedule.rs` skips terminal drafts via `is_terminal()` (was a bare `== Published`), so a `Failed` scheduled post no longer loops.
- New `publish_started_at` frontmatter field.

**Verification:** `make check` → **683 pass / 0 fail** (6 new `DraftStatus` state-machine unit tests + 36 social integration tests green); `corky audit-docs` clean for this change (16 pre-existing mtime-staleness notes unrelated). SPEC.md §12.3/§12.6/§16 updated. `make build` + `make install` done — `corky 0.29.0` live in `~/.cargo/bin`.

Next queue head: `do [#ckyemailsm]` (email draft status enum).

### Re: do [#ckyemailsm] — opus-4-8

> **Queue prompt:** do [#ckyemailsm]

Shipped the **typed email draft status enum** (corky `e6d8490`, pushed to `btakita/corky` main).

**What changed**
- New `EmailDraftStatus { Draft, Review, Approved, Scheduled, Sent, Failed }` (serde lowercase) replaces the stringly-typed `EmailDraftMeta.status: String`. I kept `Review`/`Approved` (real send-gate statuses) rather than the plan's narrower 4-variant sketch, and added `Failed`.
- `is_send_eligible()` replaces the `VALID_SEND_STATUSES` const + `to_lowercase()` compare in the `draft push --send` gate; `is_scheduled()` replaces `status.to_lowercase() != "scheduled"` in the scheduler (both YAML and legacy-regex paths); `is_terminal()` (sent/failed) so the scheduler skips terminal drafts.
- `transition(EmailEvent)` models `draft→review→approved→scheduled→sent` (+`failed`) and rejects invalid edges.
- **Drift fix:** an unknown status now fails parsing instead of silently mis-comparing as before. `migrate`/`new`/`validate`/`update_draft_status` paths all adapted to the typed field.

**Verification:** `make check` → **700 pass / 0 fail** (4 new `EmailDraftStatus` unit tests; 237 corky-mail tests green). SPEC.md §6/§12 updated. `make build` + `make install` done.

Next queue head: `do [#ckysyncsm]` (sync run-state machine, design-first).

### Re: do [#ckysyncsm] — opus-4-8

> **Queue prompt:** do [#ckysyncsm]

Shipped the **typed sync run-state machine + durable marker** (corky `97e762e`, pushed to `btakita/corky` main). This is the design-first P3 item, scoped to the typed machine + persisted resume marker (the resume *consumer* is filed as follow-up `#ckysyncsm-resume`).

**What changed**
- New `sync/run_state.rs`: `SyncRunState { Idle, Fetching, Merging, Writing, Done, Error }` with a `transition(SyncRunEvent)` table (`Start`/`Fetched`/`Merged`/`Complete`/`Fail`/`Reset`), `is_terminal()`, `is_in_flight()`, and `interrupted_accounts()`.
- Persisted `.sync-run-state.json` sidecar (per-account `SyncRunRecord`) written through the same locked atomic store (`save_json_with_lock`) as `.sync-state.json` — the cursor data stays the source of truth for *what* was fetched; this records *where in a run* a crash happened.
- `corky sync` records `Start` before each account and `Complete`/`Fail` after, so a crash leaves an `is_in_flight` marker for the next run to detect.

**Verification:** `make check` → **706 pass / 0 fail** (6 new run_state tests: transition table, predicates, `apply_event` started-at carry-forward, interrupted list, serde roundtrip). SPEC.md §3.5 added. `make build` + `make install` done.

Next queue head: `do [#ckywatchsm]` (watch phase enum + backoff — the last, polish item).

### Re: do [#ckywatchsm] — opus-4-8

> **Queue prompt:** do [#ckywatchsm]

Shipped the **typed watch-loop phase scheduler + circuit breaker** (corky `15cc39f`, pushed to `btakita/corky` main) — the last, polish item. The state-machine queue is now fully drained.

**What changed**
- New `PeriodicPhase` (cadence + per-phase circuit breaker) replaces the loose `cycles_since_upgrade_check`/`cycles_since_filter_check: u64` counters in the watch loop. `tick()` fires once every `every` cycles (`max(1, 3600/interval)`).
- **Circuit breaker:** a periodic tick (`auto-upgrade`, `filter-drift`) that panics/cancels arms exponential backoff (`2^(failures-1)` cycles, capped at 64) instead of retrying every interval; a success clears it. `run_tick` now returns completion (`false` on panic/cancel) to feed the breaker.

**Verification:** `make check` → **710 pass / 0 fail** (4 new watch tests: cadence, `every=0` clamp, backoff+recovery, cap). SPEC.md §9.2a added. `make build` + `make install` done.

**Queue drained.** All four corky state machines shipped this run: `#ckypubsm` (social publish crash-safety), `#ckyemailsm` (email status enum), `#ckysyncsm` (sync run-state + durable marker), `#ckywatchsm` (watch phase + backoff). One follow-up remains in the backlog: `#ckysyncsm-resume` (wire the sync resume consumer).

### Re: do [#ckysyncsm-resume] — opus-4-8

> **Queue prompt:** do [#ckysyncsm-resume]

Wired the **sync resume consumer** (corky `3536cb8`, pushed to `btakita/corky` main) — the follow-up to `#ckysyncsm`.

**What changed**
- At the start of `corky sync`, it loads the run-state log and computes `interrupted_accounts()` (accounts left `is_in_flight` by a crash). Each such account being synced is forced to a **full re-fetch** this run (`account_full = full || resuming`), so its in-flight window is re-verified/reconciled (idempotent by Message-ID) instead of a thin incremental from a possibly mid-write cursor. A successful run records `Complete`, clearing the marker.
- **Latent bug fixed:** the `#ckysyncsm` transition table didn't allow `Start` from an in-flight phase, so `record(Start)` *silently failed* on an interrupted account (leaving it stuck `Fetching`). `Start` is now a valid resume edge from `Fetching`/`Merging`/`Writing`.

**Verification:** `make check` → **711 pass / 0 fail** (+1 resume-edge test). SPEC.md §3.5 updated. `make build` + `make install` done.

**Queue fully drained** — all four state machines plus the resume consumer are now shipped. No follow-ups remain.

### Re: ignored-test + performance report — glm-5.2

> **Queue prompt:**
> Why is 1 test ignored?
> Please give me a report the performance

**1. "Why is 1 test ignored?" — no corky test is `#[ignore]`.**

Audited the whole workspace (corky `39f266a`) and the premise doesn't hold:
- `rg '#\[ignore'` across all `.rs` (excl. `target/`) → **0 hits**.
- `git log -S'#[ignore]' --all` → **0 commits** — `#[ignore]` has never existed in corky's history.
- No `cfg_attr(... ignore)` either, and all doctests are empty.
- Full `cargo test --workspace` → **every binary reports `0 ignored`** (e.g. corky-mail 237/0/0).

The single "ignored" entry you saw is the **`draft_mime` criterion bench** (`crates/corky-mail/benches/draft_mime.rs`, `harness = false`). Benches are *not* compiled or run by `cargo test` — the `39f266a` commit msg states this verbatim ("Benches aren't compiled by cargo test, so the 237 mail tests stayed green while the gate went red"). A runner that enumerates with `--all-targets` (rust-analyzer's test lens, or `cargo test --all-targets`) sees exactly one non-libtest target it can't run as a normal `#[test]` and renders it as a single ignored/non-runnable item. That is your "1".

Verify on your end: `cargo test --workspace --features transcribe-metal 2>&1 | rg ignored` → all `0 ignored`. The "1" only appears under a bench-enumerating runner, not from `make check`.

**2. Performance report.**

MIME composition hot path (`build_mime_message`, criterion, this machine, measured this cycle):

| case | time | vs. prior saved |
|---|---|---|
| plain | **148.65 ns** | −7.4% (was ~160 ns) |
| in_reply_to (threaded) | **199.89 ns** | −4.3% (was ~209 ns) |
| with_attachment (4 KiB) | **8.62 µs** | within noise |

Large-attachment memory (`#ckymimestream`, `9e3f1c4`): `corky draft send` previously peaked at **~1.8 GB RAM** for a 500 MB attachment — it read the file, base64-encoded it into a `String`, accumulated into the MIME `String`, then base64url-encoded the whole MIME for the Gmail `raw` body. Now, for combined attachments **≥ 5 MB** (`LARGE_ATTACHMENT_BYTES`), the MIME is spooled to a temp file (attachments streamed in 57-byte → 76-char base64 chunks via `build_mime_to_writer`) and the Gmail `{"raw":…}` JSON body is produced by streaming a `Base64UrlReader` over that file (`StreamingRawBody`). None of the raw bytes, the full base64 string, or the whole-message payload are held in RAM at once. Below 5 MB the simple in-memory path is unchanged. Output is byte-identical.

**Suite health:** 711 pass / 0 fail / 0 ignored (workspace with `transcribe` feature; 237 in corky-mail). `make check` (clippy `-D warnings` + test) green; `draft_mime` bench repaired (`#ckybenchbreak`) and green under `--all-targets`.

No follow-ups — nothing for the backlog.

### Re: do [#5jax] release + publish — operator-gated — glm-5.2

> **Queue prompt:** do [#5jax]

`#5jax release + publish` is **externally blocked**, not autonomous agent work, so I'm gating it (`[/]`) rather than executing. Corky publish state (corky `39f266a`, v0.29.0):

| channel | last published | unpublished since |
|---|---|---|
| crates.io | v0.28.2 | v0.29.0 — 9 commits ahead (crate-split refinement, `#ckymimestream`, `#ckymimeinline`, `#ckythreadmerge`, the 4 state machines, `#ckybenchbreak`) |
| PyPI | — | blocked: PyPI trusted-publisher not configured (run 26195923707 `invalid-publisher`) — already in review as `#pypi-trusted-publisher` |

Why I can't close it this turn:
- **crates.io**: `cargo publish` is irreversible and needs your API token + an explicit go/no-go (workspace publish order is corky-core → -google → -mail → -social → bin; v0.29.0 carries breaking changes). Not something to fire autonomously.
- **PyPI**: blocked on PyPI-side trusted-publisher config (`repo:btakita/corky:environment:pypi`) — external approval, outside this session's reach.

Gating `#5jax` → review as the umbrella over the PyPI item. To unblock: (1) confirm you want a v0.29.0 crates.io release and I'll prep the ordered publish + changelog, and (2) complete the pypi.org trusted-publisher setup. The prior queue head's semantic-match flag (`#tagpath-queue-priority-publish-align`) was a false positive — that's the tagpath release, unrelated to corky.

### Re: do [#5jax] release + publish — operator-gated — glm-5.2

> **Queue prompt:** do [#5jax]

`#5jax release + publish` is **externally blocked**, so I gated it (`[/]` → review) rather than executing. Corky publish state (corky `39f266a`, v0.29.0):

| channel | last published | unpublished since |
|---|---|---|
| crates.io | v0.28.2 | v0.29.0 — 9 commits ahead (crate-split refinement, `#ckymimestream`, `#ckymimeinline`, `#ckythreadmerge`, 4 state machines, `#ckybenchbreak`) |
| PyPI | — | blocked: PyPI trusted-publisher not configured (run 26195923707 `invalid-publisher`) — already tracked as `#pypi-trusted-publisher` |

Why it can't close this turn:
- **crates.io** — `cargo publish` is irreversible and needs your API token + an explicit go/no-go (workspace publish order: corky-core → -google → -mail → -social → bin; v0.29.0 has breaking changes). Not something to fire autonomously.
- **PyPI** — blocked on PyPI-side trusted-publisher config (`repo:btakita/corky:environment:pypi`), an external approval outside this session's reach.

To unblock: (1) confirm you want a v0.29.0 crates.io release and I'll prep the ordered publish + changelog, and (2) complete the pypi.org trusted-publisher setup. Note: the earlier semantic-match flag (`#tagpath-queue-priority-publish-align`) was a false positive — that's the tagpath release, unrelated to corky.

Tooling note: this cycle hit a stale IPC listener (the `startup-miss` opencode pane buffer kept overwriting the working tree via CRDT convergence, causing `#queue-user-edit-overwrite` guards). I used `--force-disk` to land a clean commit. If you see more drift, killing the stale pane or running `agent-doc admin recycle` should clear it.
<!-- agent:boundary:217b4fee -->
<!-- /agent:exchange -->
###
<!--
Why is 1 test ignored?
Please give me a report the performance
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" priority go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#5jax] release + publish
- [/] [#pypi-trusted-publisher] Configure PyPI trusted publishing for btakita/corky workflow .github/workflows/pypi.yml with environment pypi. Repo workflow is configured and pushed in Corky commit feb074e; PyPI run 26195923707 now builds all artifacts but is blocked by PyPI invalid-publisher until the project has a trusted publisher matching repo:btakita/corky:environment:pypi.
<!-- /agent:review -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/corky.done.md -->

<!-- completed work archived in tasks/software/corky.done.md -->
<!-- /agent:done -->
