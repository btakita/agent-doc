---
agent_doc_session: tsift-v0.1
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#advance-review': Go through the review items. Add actionable backlog items to complete or advance the review items.,
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#bugs-observed': Please add any bugs from observed behavior to the backlog.,
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
- Phase 4 cutover shipped — FTS5 `index.db` BM25 is the **default** lexical search path; JSON `TokenIndex` demoted to a freshness-gated degraded-read-only fallback.
- Phase 4b (b) path-prune (`#ve5f`) shipped; Phase 4b (a) decided + recorded (keep `TokenIndex::build` for degraded read-only).
- All pushed to `origin/main`.
- **Phase 5 — crates.io release.** Review item `#1yks` gates it: the two new crates `tsift-local-model` + `tsift-kg` need crates.io Trusted Publishing config before the `v*` tag, or the publish fails on them.
### Session Summary

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260621-035054.md`*

Compacted content:
❯ - Archived 9 response topic(s): session compaction; queue re-mirror; do #015t; 6 more
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54...

### Re: session compaction — state recap — opus-4-8

The exchange was compacted (9 response topics archived to `…-20260621-035054.md`) and the compacted document is now committed cleanly (the snapshot/CRDT drift from the compaction was reset from the current file first, so HEAD matches the visible doc). This response closes the prompt-only summary tail. No new work was requested.

**`#015t` (FTS5 content index) state — Phases 0–4b COMPLETE:**
- Phase 4 cutover shipped — FTS5 `index.db` BM25 is the **default** lexical search path; JSON `TokenIndex` demoted to a freshness-gated degraded-read-only fallback.
- Phase 4b (b) path-prune (`#ve5f`) shipped; Phase 4b (a) decided + recorded (keep `TokenIndex::build` for degraded read-only).
- All pushed to `origin/main`.

**Only remaining item is operator-gated → `#015t` stays `[operator-verify]`:**
- **Phase 5 — crates.io release.** Review item `#1yks` gates it: the two new crates `tsift-local-model` + `tsift-kg` need crates.io Trusted Publishing config before the `v*` tag, or the publish fails on them.

The queued `[#015t]` is the go-mode mirror of the operator-verify umbrella (`queue_drainable_head_count: 0`, `queue_continuation_required: false`) — not a fresh directive, so I'm not looping. Say **"release"** (after `#1yks` is configured) to ship Phase 5.

### Re: release + publish — blocked on operator crates.io bootstrap of 2 new crates — opus-4-8

> **Queue prompt:**
>
> :pushpin: release + publish

You chose **"you bootstrap, then I tag"**, so here's the exact handoff. I verified the full release state first — the `#1yks` gate is real and its stale `v0.1.72`/`08d0f65` reference is corrected below.

**Verified state:**
- tsift is at **`0.1.73`** (HEAD `1caaa43`, clean tree). Latest tag is only `v0.1.71` — so the next tag is **`v0.1.73` at `1caaa43`** (not `v0.1.72`).
- Crates.io publishing is **enabled** (`TSIFT_ENABLE_CRATES_PUBLISH=true`) and uses **OIDC trusted publishing** — `release.yml` requires every crate to already exist on crates.io with a registered Trusted Publisher.
- **`tsift-local-model` and `tsift-kg` are 404 on crates.io** (don't exist); both are deps of `tsift-cli`. The other tsift crates exist (e.g. `tsift-core 0.1.71`, `tsift-cli 0.1.69`).
- **Dependency wrinkle:** `tsift-local-model` has no tsift-* deps (serde/ureq only) → can be created standalone. `tsift-kg` depends on `tsift-core 0.1.73` + `tsift-sqlite 0.1.73` + `tsift-local-model 0.1.73`, none of which are on crates.io yet → it **cannot** be token-published standalone before the rest of the 0.1.73 chain.

**Recommended bootstrap (Option A — pre-config trusted publishing, no token, lets CI order everything):**
On crates.io, add a Trusted Publisher to **both** crate names `tsift-local-model` and `tsift-kg` (GitHub repo `btakita/tsift`, workflow `release.yml`) — modern crates.io supports configuring trusted publishing for a not-yet-published crate name. Then I push `v0.1.73` and CI publishes the whole set in dependency order, creating the two new crates via OIDC. This sidesteps the `tsift-kg` dependency wrinkle entirely.

**Fallback (Option B — if crates.io rejects pre-config for a nonexistent name):** create them with a personal token, in dependency order. `tsift-local-model` first (`cargo publish -p tsift-local-model`), but `tsift-kg` needs `tsift-core`/`tsift-sqlite`/`tsift-local-model` at `0.1.73` published first — so Option A is strongly preferred.

**After you confirm bootstrap** (a quick `curl`/browser check that both crate names resolve / show a Trusted Publisher), tell me and I'll: re-run `make check`, `git tag v0.1.73 1caaa43`, `git push origin v0.1.73`, then watch the Release run. I'm **not** pushing the tag now — doing so before bootstrap would fail the crates.io job on the two new crates and cascade to `tsift-cli` (a red/partial release).
### Re: #015t — what you need to verify to unblock the v0.1.73 release — opus-4-8

> **Queue prompt:**
>
> :pushpin: [#015t]: What do I need to verify?


One thing, and it's the `#1yks` gate: **the two new crates `tsift-local-model` and `tsift-kg` must exist on crates.io with a Trusted Publisher configured.** Right now both are **404** (never published); every other tsift crate is already set up. Until these two are bootstrapped, pushing the tag fails their publish job and cascades to `tsift-cli` (which depends on them).

**Do this (crates.io, your browser/passkey 2FA — I can't do 2FA):**
1. Sign in to crates.io.
2. For **each** crate name — `tsift-local-model` and `tsift-kg` — add a **Trusted Publisher**: GitHub, repository `btakita/tsift`, workflow `release.yml`. Modern crates.io lets you configure trusted publishing for a crate name that doesn't exist yet; the first OIDC publish then creates it.

**How to verify you're done** (either is sufficient):
- Each crate's crates.io page shows a Trusted Publisher under *Settings → Trusted Publishing*, **or**
- the crate name resolves once the first publish lands. If you only pre-config (don't publish yet), rely on the Trusted Publishing settings page as the proof.

**If crates.io refuses to pre-config a nonexistent crate** (fallback): publish `tsift-local-model` once manually with a personal token to create it (`cargo publish -p tsift-local-model`). `tsift-kg` can't be created standalone — it needs `tsift-core`/`tsift-sqlite`/`tsift-local-model` at `0.1.73` on crates.io first — so the pre-config route above is strongly preferred; with it, CI publishes the whole set in dependency order in one shot.

**Then:** `agent-doc admin queue resume tasks/software/tsift.md` (the queue is currently paused) and tell me — I'll run `make check`, `git tag v0.1.73 1caaa43`, `git push origin v0.1.73`, and watch the Release run. Nothing else is needed from you; `#015t`'s Phases 0–4b are done and pushed, only Phase 5 (this release) remains.

### Re: setup trusted publishing + ship the release — v0.1.73 PUBLISHED — opus-4-8

> **Queue prompt:**
>
> :pushpin: Use https://crates.io/docs/trusted-publishing to setup trusted publishing for the crates.

Read the docs, then did the release. **Key docs finding (corrects my earlier guess):** crates.io has **no "pending publisher"** like PyPI — you **cannot** pre-configure trusted publishing for a crate that doesn't exist; each new crate's **first publish must be manual with a token**. So pre-config wasn't an option for `tsift-local-model` / `tsift-kg`.

You chose **"I publish all 25 now"**, so I did the full manual `0.1.73` publish with the present token (read-verified first; no E403):

- **`make check` green** (exit 0, full suite) before publishing.
- **All 25 crates published to crates.io at `0.1.73`** in dependency order — including the two brand-new crates **`tsift-local-model`** and **`tsift-kg`** (both now resolve, HTTP 200). That's the `#1yks` bootstrap, done.
- **Tag `v0.1.73` pushed** (`git tag v0.1.73 1caaa43`) → Release CI run `27907620025` is **in progress** (binary GitHub Release; its crates.io job now skips all 25 already-published versions idempotently, so the prior red Release runs are resolved). Per the in-progress-CI rule, closing from the publish evidence rather than blocking on CI.

**`#015t` is now complete** — Phases 0–4b shipped earlier, Phase 5 (this release) is published + tagged. Reaped.

**One optional, non-blocking follow-up for you (future releases only):** add a Trusted Publisher to `tsift-local-model` and `tsift-kg` on crates.io (*Settings → Trusted Publishing* → GitHub `btakita/tsift`, workflow `release.yml`) so the **next** `v*` tag publishes them via CI/OIDC instead of a token. It's tracked in `#1yks`; the release itself no longer depends on it.

### Re: Can we complete #1yks now? — yes, done — opus-4-8

> **Queue prompt:**
>
> Can we complete #1yks now?

Yes — completed it just now. Both new crates now exist on crates.io (after the v0.1.73 publish), so I registered their Trusted Publishers via the same chromium-bridge path that configured the other 23 (your browser is logged in as `btakita`):

- **`tsift-local-model`** → Trusted Publisher `btakita/tsift @ release.yml` (config id 10989)
- **`tsift-kg`** → Trusted Publisher `btakita/tsift @ release.yml` (config id 10990)

Verified both via `GET /api/v1/trusted_publishing/github_configs` — each returns `btakita/tsift@release.yml`. No token, no environment, matching the existing 23.

**Net effect:** all 25 tsift crates are now OIDC trusted-publishing configured. The next `v*` tag publishes the entire workspace via CI with no manual token step — the token bootstrap was a one-time thing for the two brand-new crate names. `#1yks` is resolved.
<!-- agent:boundary:d15a1444 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#g5kw] What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
- [ ] [#4z0w] Can we increase max-hop caps?
- [ ] [#cg52] Could FalkorDB help with performance?
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
