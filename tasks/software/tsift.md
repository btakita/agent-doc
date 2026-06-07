---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260606-175807.md`*

Compacted content:
- ****Archived 13 response topic(s): Does a runbook re-read duplicate context across turns?; Would "retain-don't-reread" be implemented in the skill-harness?; Is the soft-discipline effective at reducing redundant runbook reads?; 10 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: is [#rtsw] done? — opus-4-8

Yes — done (marked `[x]`). The goal (get `opencode-tsift` publishing working and automated) is fully achieved, though the *mechanism* ended up different from what #rtsw originally proposed:

- #rtsw assumed the fix was a **granular npm token**. That turned out wrong — the `btakita` account enforces 2FA-bypass, so classic *and* granular tokens both hit `E403` (verified locally and in CI). Tokens are a dead end for this account.
- The actual resolution was **OIDC trusted publishing** (no token at all): `opencode-tsift@0.1.63` bootstrapped to npm, `npm trust` configured (`btakita/tsift` → `release.yml`, verified via `npm trust list`), `release.yml` switched to OIDC (`id-token: write`), and the dead `NPM_TOKEN` secret deleted.

Net result: every future `v*` release auto-publishes opencode-tsift via OIDC — no token, no OTP, no authenticator app. The npm-related review items (`#0y9e`, `#astnav-npm`, `#npm-auth-setup`) are effectively satisfied by this too; `#npm-publish-verify`'s last piece (a CI OIDC publish) will exercise itself on the next version bump.

### Re: Separate SQLite DBs for caches to avoid writer-lock contention? Is it even an issue with WAL? — opus-4-8

Two questions, and the second one reframes the first. Short version: **with WAL, writer-lock contention is mostly a non-issue for these caches — and the current JSON design already beats a shared cache DB for the one case where contention *would* bite (multiple processes).**

**What WAL actually fixes (and doesn't).** WAL's win is reader/writer concurrency: readers never block the writer and the writer never blocks readers. But WAL does **not** give you multiple concurrent *writers* — SQLite is still **one writer at a time per database file**, any journal mode. A second writer gets `SQLITE_BUSY` and waits out the `busy_timeout`. So WAL eliminates read-vs-write contention; it does nothing for write-vs-write.

**Is write-write contention real for tsift's caches?** Barely, because of their access pattern. The community cache (and cycle-packet, conflict-matrix, token-index) are **compute-once-per-watermark, read-many**: you write a result once when the graph watermark changes, then just read it on lookups until the next change. Writes are rare and short. So even if they all shared one WAL database, reads would be free and the occasional write would almost never collide. Within a single process, contention here is theoretical.

**Where it *would* bite — and why separation matters.** The real risk isn't the caches contending with *each other*; it's cache writes sharing a lock with something hot. Two cases:
1. **Sharing `graph.db`.** If cache writes lived in the main graph projection DB, a cache write would queue behind the indexer's write lock (and a slow cache write could stall indexing), plus you'd entangle cache churn with graph VACUUM/bloat. So: **if caches were ever in SQLite, they belong in a DB separate from `graph.db`** — exactly the pattern tsift already uses for `findings.db` (kept separate so findings survive `graph-db refresh` and don't share the projection's lock/lifecycle). Among the caches themselves, one shared `cache.db` would be fine — no need for one DB per cache.
2. **Multiple processes** (concurrent sessions, the agent-doc supervisor). This is tsift's actual contention story, and WAL only half-helps: reads are concurrent, but two processes computing/writing caches at once **serialize on the single writer lock** (`SQLITE_BUSY`, retries).

**This is the case the current JSON design wins.** Content-addressed `(graph_watermark+tagpath) → <hash>.json` files have **no writer lock at all** — each computation writes its own distinct file, so N processes can populate the cache fully in parallel with zero `SQLITE_BUSY`, zero retry, zero shared fsync. A single `cache.db` would *reintroduce* a writer lock that the JSON layout avoids. For rebuildable, content-addressed, possibly-concurrently-written data, that's a real advantage, not just simplicity.

**Bottom line.** Your instinct — "separate DBs to avoid contention" — is directionally right (isolate from `graph.db`), but JSON already takes it further: it removes the lock entirely. So:
- Keep the caches as JSON files. For their pattern (rare writes, content-addressed, multi-process) that's the lower-contention option, not a compromise.
- Reserve SQLite for what needs querying/transactions/incremental updates (`graph.db`, `findings.db`), and keep those as **separate** DBs (already done) so their write locks and refresh/VACUUM lifecycles don't cross.
- WAL is the right mode for those query DBs (free concurrent reads while the indexer writes) — but it's not a reason to fold the caches into SQLite, because WAL doesn't help the write-write/multi-process case that the JSON files sidestep completely.
<!-- agent:boundary:57f23e06 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
- Please repost answer to "Should we have separate sqlite databases for the community cache and other caches to avoid writer lock contention? Is the writer lock contention even an issue with WAL?"
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#7qky] [recommended] Reduce Codex redundant runbook reads. SHIPPED (v0.34.0 unreleased): retain-don't-reread nudge in shared SKILL Runbooks source -> regenerated all harness surfaces; contract-safe. GATED on re-measurement: re-run 3-harness redundant-read analysis after Codex sessions accumulate with nudge live, compare to baseline (Claude 11.4% / Codex 12.6% / OpenCode 5.3%). If Codex rate doesn't drop, build enforcement as a session-scoped content-dedup LEDGER in a shared LEAF crate (depends on nothing in graph; tsift + agent-doc + others depend down onto it -- NOT inside tsift or agent-doc, avoids dependency cycle). Core = register(id,hash)/should_deliver(id,hash)->Full|Stub, exemptions via hash-change + compaction; session-id passed in by harness (agent-doc cycle_state.rs adapter); thin per-consumer integration. Design tsift-runbook + agent-doc seam to call this ledger from day one. NOT a Claude-only PreToolUse Read hook.
- [/] [#trt1] [recommended] Findings graph layer: add authored Finding/Decision/Note nodes to GraphStore, anchored by stable symbol-handle/tagpath edges (not line numbers), with provenance+confidence+freshness fields gated on community_graph_watermark for staleness; expose retrieval via context-pack/search injection and a / graph-menu command; opt-in/passive capture from agent-doc session archives; md/html as on-demand projection, graph store as source of truth. Spec in src/tsift/SPEC.md.
- [/] [#0y9e] I set NPM_TOKEN="$(passage btak/NPM_TOKEN)". Publish to npm.
- [/] [#astnav-npm] Configure NPM_TOKEN + TSIFT_ENABLE_NPM_PUBLISH secrets, publish opencode-tsift to npm (medium)
- [/] [#surreal-batch-bfs] #surreal-batch-bfs-gated: Batch frontier expansion - gated (low)
- [/] [#surreal-fixture] Record SurrealDB samples in graph-db-performance-history.json fixture (medium)
- [/] [#surrealbulk-close] Close #surrealbulk review item: re-run benchmarks, update SPEC.md benchmark table, mark done
- [/] [#rankdefault] Promote ranked_neighborhood into default graph lookup/context-pack/evidence output only after quality gates pass; compare ranked vs unranked output on real backlog/code queries for useful-hit density, duplicate-name precision, handle coverage, latency, and emitted tokens.
- [/] [#surrealbulk] Batch SurrealDB projection load: replace per-row upsert loops with bulk/transactional row load plus one-pass secondary-index rebuild; measure refresh/load separately.
- [/] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
- [/] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [/] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [/] [#e2e-install] Verify end-to-end install: fresh opencode project + opencode plugin opencode-tsift + /tsift-status
<!-- /agent:review -->
###
<!--
Can we increase max-hop caps?
Could FalkorDB help with performance?
-->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/software/tsift.done.md -->

<!-- completed work archived in tasks/software/tsift.done.md -->
<!-- /agent:done -->
