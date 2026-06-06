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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260606-054319.md`*

Compacted content:
- Archived 3 response topic(s): [#trt1p4]; [#trt1p2b]; [#trt1cfg]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: Does a runbook re-read duplicate context across turns? — opus-4-8

Short answer: yes, by default. Runbooks are plain markdown files (`src/agent-doc/runbooks/*.md`) that are **not** in the always-loaded path — SKILL.md only links to them, and they enter context only when the model explicitly `Read`s one. That on-demand design is deliberate: it keeps the hot path small.

But there's no cross-turn dedup of tool results. Each `Read` of a runbook appends its full content as a fresh tool-result block to the transcript. So if the same runbook is read on turn 1 and again on turn 3, its tokens sit in the context window **twice** — the harness does not notice that the bytes are identical and collapse them.

What changes that:

- **Context compaction** — when the window fills and the harness summarizes older turns, the earlier copy gets compressed into the summary, so the duplication is transient, not permanent. It still cost tokens while it was live.
- **Prompt caching** — repeated identical prefixes can be served from cache (cheaper), but caching is about *cost*, not about *removing* the content from the window. The tokens still occupy context.
- **Retain-don't-reread discipline** — the practical mitigation: read a runbook once, keep the relevant facts in working memory, and don't re-open it on a later turn unless the content actually changed or was summarized away.

So: duplicated in the raw transcript = yes; permanently duplicated = no (compaction reclaims it); avoidable = yes (read once, retain). The SKILL/runbook split exists precisely so a turn that doesn't need a given runbook never pays for it at all.

### Re: Would "retain-don't-reread" be implemented in the skill-harness? — opus-4-8

> **Queue prompt:**
>
> Would "Retain-don't-reread discipline" be implemented in skill-harness?


Today it isn't implemented anywhere — it's pure model discipline. Nothing in the agent-doc binary, the SKILL.md, or the Claude Code harness tracks "you already read `runbooks/commit.md` this session, don't read it again." It relies entirely on the model choosing not to re-open a file it already has in context.

If you wanted to actually *implement* it, the key distinction is **advise vs. enforce**, and they live in different layers:

- **Skill layer (advisory only).** SKILL.md is prompt text. It could add a line like "do not re-read a runbook already read this session," but that's still a nudge the model can ignore — same category as the discipline itself. It raises compliance, it doesn't guarantee it. So putting it in the skill doesn't really make it "implemented," just "documented."

- **Harness layer (real enforcement).** The only place you can *enforce* it is a `PreToolUse` hook on `Read`. The hook keeps a per-session set of already-read runbook paths; on a repeat `Read` of the same path it either (a) blocks the call and returns "already read this turn — reuse prior content," or (b) returns a short pointer/stub instead of the full file body. That's deterministic — code, not vibes — and it's where this kind of rule belongs, because the harness controls tool execution and the skill can't.

- **Binary layer (mostly N/A).** `agent-doc` owns the document/commit lifecycle, not your `Read` calls, so it's not the natural home. The one adjacent thing it could do is surface in `preflight` which runbooks a turn is *likely* to need, so you fetch deliberately rather than reactively — but that's guidance, not dedup.

So: would it be in the skill-harness? The skill can only document it; if you want it enforced, it's a harness `Read` hook. Right now neither exists — it's discipline, and the cheaper backstop is that context compaction reclaims the duplicate anyway, which is why nobody has felt the need to code it.

Worth noting: a hard read-once hook has a real failure mode — if a runbook was edited mid-session, or its earlier copy was already compacted away, blocking the re-read would starve the model of content it legitimately needs. Any enforcement should key on (path + content-hash) and exempt post-compaction re-fetches, which is exactly the kind of nuance that makes the soft-discipline approach attractive enough to leave as-is.

### Re: Is the soft-discipline effective at reducing redundant runbook reads? — opus-4-8

> **Queue prompt:**
>
> Please look through Claude Code, Codex, and Opencode sessions logs to measure whether the soft-discipline approach is effective in reducing redundant runbook reads.


I measured it against real session logs. Verdict: **yes, the soft discipline is mostly effective** — redundancy sits at 5–13% of all runbook reads, and the one place the discipline can actually act (within a single live context), it's working at ~100%. The residual re-reads are almost entirely *cross-turn*, which in-context discipline can't prevent and which are often legitimate (new turn, or content already compacted away).

**Data**

| Harness | Sessions w/ ≥1 read | Total reads | Distinct | Redundant | Redundancy rate |
|---|---|---|---|---|---|
| Claude Code | 32 | 44 | 39 | 5 | **11.4%** |
| Codex | 322 | 709 | 620 | 89 | **12.6%** |
| OpenCode | 29 | 75 | 71 | 4 | **5.3%** |

"Redundant" = same runbook path read 2+ times within one session.

**The decisive cut — when do the re-reads happen?** For Claude (the only harness where I can cleanly bucket reads by turn): of the 5 redundant reads, **same-turn (true waste) = 0**, cross-turn = 5. So inside a single context window the model never re-opens a runbook it already has — exactly what the discipline asks. The re-reads only happen on later turns, where the earlier copy may already be compacted out and the re-read is legitimately needed. The discipline is effective; the leftover isn't really waste.

**Where the redundancy concentrates** (re-read counts):
- **Codex** — `harness-invocation.md` ×22, `commit.md` ×16, `pending-ops.md` ×15. These are the hot-path runbooks hit nearly every cycle.
- **Claude** — `transfer-extract.md` ×3, plus one each of `pending-ops.md`, `model-tier-gate.md`.
- **OpenCode** — `harness-invocation.md` ×3, `document-format.md` ×1.

**Why Codex is worst:** it's structural, not a discipline failure. Codex reads runbooks by shelling out (`cat`/`sed` a `runbooks/*.md`), and each cycle is a fresh process — so the hot-path runbooks get re-`cat`'d every cycle because nothing carries them across cycles the way a single Claude context does. That's precisely the case the backlog read-once item targets, and it argues the enforcement (or a Codex-side runbook cache) would pay off most on Codex, least on Claude/OpenCode where the rate is already low.

**Methodology / confidence:**
- Claude (`Read` tool, `file_path`) and OpenCode (`read` tool in `opencode.db`, `state.input.filePath`) are **clean, explicit** signals — high confidence.
- Codex is a **heuristic**: a runbook path passed to a read utility (`cat`/`bat`/`sed`/`head`/`tail`/`less`/`nl`) or `tsift source-read`; I excluded `rg`/`grep`/`ls` (searches, not reads). Medium confidence — could under/over-count.
- Scope: Claude = the agent-loop project transcripts (284 sessions); Codex = all 2026 rollout sessions (~2,866); OpenCode = full DB.

Bottom line for the discipline-vs-enforcement question from the prior turn: the soft discipline already eliminates in-context waste, so a hard read-once hook would only reclaim the cross-turn tail — and that tail is dominated by Codex's per-cycle shell re-reads, which is the targeted, high-value case rather than a broad win.
<!-- agent:boundary:dc8eb2bc -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#7qky] [recommended] Optional read-once enforcement for runbooks: PreToolUse hook on Read keyed on (path + content-hash) that returns a pointer/stub on repeat reads within a session, exempting post-compaction re-fetches and mid-session edits. Currently retain-don't-reread is model discipline only; harness hook is the only real enforcement layer.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
