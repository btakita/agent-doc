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
queue_active: false
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

### Re: Implement #7qky for Codex only, not Claude/OpenCode? — opus-4-8

> **Queue prompt:**
>
> Should we implement [#7qky] for Codex based on the last response? But not on Claude/OpenCode?


The asymmetry you're pointing at is real and correct: **scope any intervention to Codex, leave Claude/OpenCode alone.** But I'd push back on implementing *#7qky as written* (the hard read-once `Read` hook) even for Codex — pick the cheaper, lower-risk Codex fix first.

**Why not Claude/OpenCode** — the data settles it:
- Claude's 11.4% redundancy is *entirely cross-turn* (same-turn waste = 0). A read-once block there would fire exactly on the post-compaction re-reads that are legitimately needed → it starves the model. Net-negative.
- OpenCode is 5.3% and 3 of its 4 re-reads are `harness-invocation.md` — too little to justify any mechanism.

**Why Codex is the right target** — 12.6%, 89 re-reads concentrated in `harness-invocation.md` ×22, `commit.md` ×16, `pending-ops.md` ×15. Those are the closeout/hot-path runbooks re-`cat`'d nearly every cycle.

**But #7qky (the hook) is the wrong first move, even for Codex:**
1. **Mechanism mismatch.** #7qky was written as a Claude-style `PreToolUse` hook on the `Read` tool. Codex doesn't read runbooks through a `Read` tool — it shells out (`cat`/`sed` a `runbooks/*.md`). Enforcing read-once would mean intercepting arbitrary shell exec and pattern-matching commands — fragile, and easy to evade with `head`/`awk`/redirects.
2. **A hard block can still starve Codex.** Within a long Codex session the earlier `cat` output can be compacted out too; blocking the re-read then removes content it genuinely lost.
3. **Cheaper fix likely closes most of the gap.** The Codex re-reads cluster on 3 hot-path runbooks. Two lower-risk options dominate the hook on ROI:
   - **(a) Inline the 3 hot-path runbooks** (`harness-invocation`, `commit`, `pending-ops`) into Codex's always-loaded `AGENTS.md` closeout section, so there's nothing to re-read — turns the re-read into a zero-cost cache hit.
   - **(b) Strengthen the retain-don't-reread nudge** in Codex's AGENTS.md / Stop-hook guidance, since Codex's in-context discipline is measurably weaker than Claude's (which already runs at ~0 same-turn waste).
4. **The measurement is medium-confidence** (Codex shell heuristic). I'd want to re-measure after a cheap fix before committing to hook machinery.

**Recommendation:** Yes to Codex-only scope; no to building the hook yet. Do (a) inline the 3 hot-path runbooks for Codex first, re-measure, and only build an enforcement mechanism if the 12.6% doesn't drop. I've re-scoped #7qky to reflect this (Codex-targeted, cheap-fix-first, hook as fallback) so it doesn't get picked up as a blanket all-harness hook.

If you'd rather I just implement (a) now — inline the three runbooks into the Codex AGENTS.md closeout surface — say the word and I'll do it as its own cycle.

### Re: Implement #7qky in tsift or skill-harness? — opus-4-8

I implemented the **nudge** in the **skill-harness** this turn (per your "nudge-only, re-measure first" pick), and that's the right home for *that* piece. But your tsift-vs-skill-harness question is the sharper one for the *enforcement* fallback — and there my answer is **tsift**, not a per-harness hook. Both parts below.

**What shipped (skill-harness) — done + verified:**
- Added a retain-don't-reread paragraph to the shared `## Runbooks` section of `src/agent-doc/SKILL.md` (the one source that renders to every harness via `skill.rs` `include_str!`). Contract-safe, no per-harness divergence.
- Rebuilt release binary, regenerated all four installed surfaces (`agent-doc skill install --all` → Claude / Codex / OpenCode / Cursor), each +2 lines, nudge confirmed present in all.
- `make check`: **3756 passed, 0 failed, exit 0**; `audit-docs`: clean (2 non-blocking mtime advisories).
- Commits: submodule `a68864f5` (SKILL.md + VERSIONS.md only — staged precisely to avoid the unrelated in-flight `write.rs`/`id.rs`/`template.rs`/lazily-`Cargo.lock` changes), superproject `8c4e4a99` (regenerated surfaces + submodule pointer).
- **Caveat (honest):** `cargo install --path .` (which refreshes `~/.cargo/bin`, used by tmux-spawned sessions) currently **fails** — but on a *pre-existing* `agent-doc-orchestration/src/write.rs` E0061 from another task's in-flight work, surfaced only under `cargo install`'s fresh dep resolution (the workspace cargo-patch hides it for `cargo build`/`make check`). My change is markdown-only and cannot cause it. The nudge is live regardless, because it lives in the **static installed surfaces**, not the binary — future sessions read it no matter which binary version runs. `~/.cargo/bin` can be refreshed once that `write.rs` work lands.

**Where the enforcement fallback belongs — tsift, not skill-harness:**
- The **skill-harness can only advise** (prose the model may ignore) — that's the nudge, already shipped. It cannot *enforce* read-once.
- A **per-harness hook** enforces but is fragmented and fragile: a Claude `PreToolUse` Read hook covers Claude only; Codex reads via shell `cat` (no Read tool), so you'd have to intercept arbitrary shell exec — easy to evade. Three different mechanisms for three harnesses.
- **tsift is the one mechanism that covers all three.** If runbook fetches route through a dedup-aware `tsift runbook <name>` (returning full content on first fetch this session, a "already delivered — reuse" stub on repeat), the read-once guarantee becomes *deterministic and tool-owned*, identical across Claude/Codex/OpenCode. It sidesteps the shell-vs-Read-tool mismatch entirely, and tsift already owns bounded, session-aware content delivery (envelopes, context-pack, source-read). The cost is adoption: the SKILL instructions must point runbook reads at tsift instead of raw `Read`/`cat`.
- So: **nudge in skill-harness (done); enforcement, if re-measurement proves it's needed, in tsift** — fitting, since this is the tsift backlog. I'd spec it as a `tsift runbook` fetch with per-session dedup rather than scatter hooks across harnesses.

**#7qky status:** moved to Review, **gated** on re-measurement — re-run the 3-harness redundant-read analysis after Codex sessions accumulate with the nudge live, compare to baseline (Claude 11.4% / Codex 12.6% / OpenCode 5.3%). Build the tsift enforcement only if Codex's rate doesn't drop.

### Re: Should skill-harness own the enforcement library (shared)? — opus-4-8

> **Queue prompt:**
>
> Should skill-harness contain the library for the enforcement fallback? agent-doc and other dependencies would likely need similar behavior.


Yes — your DRY instinct is right. The dedup mechanism should be a **shared library**, not buried inside tsift. But "should skill-harness contain it" resolves on one constraint: the **dependency direction**. Let me separate the three layers, because only one of them is actually shareable.

**1. The generic core (the shareable part) — a leaf library.**
The reusable thing is a *session-scoped content ledger*: keyed by `(logical-id + content-hash)`, with an API like `register(id, hash)` and `should_deliver(id, hash) → Full | Stub{reason}`. Exemptions fall out of the key: a changed runbook has a new hash → delivered again; a compacted-away copy is re-requested → delivered again. This part is pure and depends on nothing.

**2. Session identity — a harness-specific input, not owned by the library.**
The ledger needs a session key to scope "this session." That key is harness-specific (Claude session id, Codex rollout id, OpenCode session id). The library should **take it as a parameter**, not detect it. Keep harness-awareness out of the leaf.

**3. Consumer integration — thin, per tool.**
tsift decides "this fetch is a runbook, emit a stub with a re-fetch hint"; agent-doc decides the same for its own runbook references. Thin adapters over the shared core.

**So: should skill-harness contain it?** Yes *if* "skill-harness" means a **leaf crate that nothing else in the graph depends back into** — then tsift, agent-doc, and future consumers all depend on it. The thing to avoid is putting it inside either tsift *or* agent-doc:
- Inside **tsift** → agent-doc would have to depend on tsift just to dedup its own runbooks. Wrong direction.
- Inside **agent-doc** → tsift would depend on agent-doc. Also wrong (and agent-doc is a heavy dep to pull into a search tool).
- A **shared leaf crate** → both depend *down* onto it; no cycle. This is exactly agent-doc's own documented **Shared Foundation pattern** ("shared logic at the broadest scope; consumers are thin"). Your "agent-doc and other dependencies would need similar behavior" is the textbook trigger for it.

One nuance worth keeping: agent-doc already owns session/cycle state (`cycle_state.rs`), so it's the natural place to *provide the session-identity adapter* (layer 2) — but the ledger itself (layer 1) still belongs in the leaf crate that agent-doc depends on, not inside agent-doc.

**Caveat (unchanged):** don't build any of this until re-measurement proves the nudge didn't already close the gap — #7qky stays gated. But designing the **seam** now is free and worth it: have `tsift runbook <name>` and agent-doc's runbook references both call the same ledger interface from day one, so the enforcement can drop in later without reworking either consumer. I've recorded this placement decision on the gated #7qky item.

### Re: Is the nudge in effect in monsterrodholders, and how successful? — opus-4-8

> **Queue prompt:**
>
> Please look at the monsterrodholders.md session. Is the nudge in effect? If yes, how successful was it?


No — the nudge is **not yet in effect** for monsterrodholders, and even if it were, that session is the wrong place to see its impact. Two independent reasons, both from the logs:

**1. No monsterrodholders session has run since the nudge went live.**
- Nudge committed/installed into the surfaces at **2026-06-06 02:33 EDT** (06:33 UTC).
- The real monsterrodholders agent-doc sessions (`7a627c84`, `d39155c7`, `d0da43fc`, `e1015ac9`) all last ran **2026-06-05 or earlier** — newest activity `e1015ac9` at 2026-06-05T04:39Z. All pre-nudge.
- The one "today" hit my search surfaced (`32e4e1a3`, 06:59Z) is **this very session** (tsift.md) — it only matches "monsterrodholders" because the string lives in my memory/CLAUDE.local context, not because it's a monsterrodholders session.
- A Claude Code session only picks up the new SKILL.md at skill-invocation time, so the nudge can't apply retroactively to a session that ended yesterday. It takes effect the next time `/agent-doc .../monsterrodholders.md` is freshly invoked.

**2. monsterrodholders barely reads runbooks at all — there's almost nothing for the nudge to act on.**
Pre-nudge baseline across all four real sessions: **1 runbook read total, 0 redundant.**
| Session | reads | distinct | redundant |
|---|---|---|---|
| 7a627c84 | 0 | 0 | 0 |
| d39155c7 | 0 | 0 | 0 |
| d0da43fc | 0 | 0 | 0 |
| e1015ac9 | 1 | 1 | 0 |

That's consistent with my earlier finding: Claude Code already had the lowest, cleanest redundancy (all cross-turn, ~0 same-turn waste). monsterrodholders is a Claude research session, so it was never the target. **The nudge targets Codex's per-cycle shell re-reads** (`.codex/AGENTS.md`, which now carries the nudge), where the 12.6% redundancy and the `harness-invocation`/`commit`/`pending-ops` hot-path re-`cat`ing actually live.

**So, to actually measure success:** look at **Codex** sessions that run *after* 2026-06-06 06:33 UTC and re-measure the redundant-read rate against the 12.6% baseline — not Claude research sessions like monsterrodholders. That's exactly the re-measurement #7qky is gated on; it has no data yet because no qualifying Codex session has run since install. Bottom line: nudge is live in the surfaces, but its first measurable test is a fresh post-install Codex cycle, not monsterrodholders.

(Housekeeping: preflight is now flagging this exchange as large — `block`-level accretion, 6 response sections. If you want, I can run `agent-doc compact tasks/software/tsift.md --commit` to archive the resolved Q&A tail. Not doing it without your go-ahead.)

### Re: Promote `tsift __digest-runner` to a public `tsift digest-runner` command — opus-4-8

> **Queue prompt:**
>
> Promote `tsift __digest-runner` to be a public CLI command. Remove the `__` prefix...`tsift digest-runner`


> **Queue prompt** (`#spec-test-build-install-commit-push`):
>
> Promote `tsift __digest-runner` to be a public CLI command. Remove the `__` prefix...`tsift digest-runner`

Done — shipped, built, installed, committed, and pushed.

**What changed.** `__digest-runner` was a `hide = true` internal helper. It's now a first-class public command:

- `cli.rs`: `#[command(name = "digest-runner", alias = "__digest-runner")]` with a `///` doc comment, and `hide = true` dropped. So `tsift --help` now lists it:
  > `digest-runner    Run a shell command and emit a bounded, artifact-backed test/log digest envelope`
- **Backward compatible.** `__digest-runner` stays as a *hidden* clap alias, so everything already emitting the old name keeps resolving: rewrite-generated commands, the `.codex`/`.opencode` hooks, and instruction files installed in other projects. I verified both names execute and both return `"tool":"digest-runner"`.
- **New emissions use the public name.** `rewrite.rs`'s `build_digest_runner_command` and the `tsift init` instruction templates (managed Code Navigation block + OpenCode `tsift-test-digest`/`tsift-log-digest` commands) now emit `tsift --envelope digest-runner ...`.
- **Docs/surfaces updated:** `specs/{search-navigation,output-formats,release-integration}.md`, `AGENTS.md`/`CLAUDE.md`, the `opencode-tsift` package commands + README, and a `VERSIONS.md` Unreleased entry (`#digest-runner-public`).

**Spec + tests.** Integration tests in `exit_code.rs` now invoke the public `digest-runner`; added `digest_runner_legacy_underscore_alias_still_resolves` to lock the alias; `rewrite`/`sim_world`/`init` unit tests now assert the emitted form is `digest-runner` and *not* the underscore form.

**Verification.** `make check` green — `RAW_EXIT=0`, zero `FAILED`/`error` lines (RTK-digested log; exit captured explicitly per the masking caveat). Targeted runs: digest-runner integration 5 passed, rewrite 6 passed, sim_world 3 passed, init 41 passed. Release built; `cargo install --path .` refreshed `~/.cargo/bin/tsift`, which confirms the public command in `--help`.

**Commits.** tsift submodule `3a69aed` → pushed to `btakita/tsift main`; superproject pointer bump `acf6cf87f`. tsift CI for `3a69aed` is **in progress** (prior commit green); local full suite is green.

**Note on naming.** I kept the legacy `__digest-runner` working rather than hard-removing it, since hooks and installed instruction files in other projects still call it — a clean break would break those mid-flight. Queued **#rx4x** to regenerate the agent-loop superproject's own instruction surfaces (CLAUDE.md, SKILL.md, runbooks) onto the public name; they still work via the alias, so it's non-breaking cleanup, not urgent.

### Re: [#rx4x] — sync agent-loop instruction surfaces to public `digest-runner` — opus-4-8

> **Queue prompt:**
>
> [#rx4x]


Done — `#rx4x` executed, committed, and pushed.

**What changed.** Swept every agent-loop superproject instruction surface that still named the old hidden `__digest-runner` onto the public `tsift digest-runner`:

- `AGENTS.md` (→ `CLAUDE.md` symlink) Code Navigation block + the `.opencode` `tsift-test-digest`/`tsift-log-digest` shortcuts — refreshed via `tsift init --opencode` (which also added newer `tsift-explain`/`tsift-symbol-read`/`tsift-graph` shortcuts; left untracked, out of this commit's scope).
- `CLAUDE.local.md`, `runbooks/code-navigation.md`, and the tsift skill's `runbooks/hook-integration.md` — these aren't `tsift init` targets, so updated in place.

A full sweep (incl. hidden `.claude`/`.codex`) confirms zero `__digest-runner` references remain in the superproject. Non-breaking regardless: the old name still resolves via the hidden clap alias shipped last cycle.

**Note — preset scope.** The queue carried `#spec-test-build-install-commit-push`, but `#rx4x` is pure instruction-file content sync — no tsift source/spec/test/binary change — so "update spec + tests / build + install" didn't apply this cycle (those landed with the promotion itself). Only commit + push were relevant.

**Push hurdle (resolved).** The superproject push was initially rejected (remote +9). Those 9 commits touched **only** `src/boost-client` (a submodule pointer a parallel monsterrodholders session advanced), and all three differing submodules (`agent-doc`, `boost-client`, `tsift`) were verified local-ahead — remote pointers are ancestors of local. Reconciled with a merge, resolved the lone `boost-client` gitlink conflict to the local committed pointer (`97aac84d`, a descendant of remote `292c4dcb` — loses nothing), preserved the unrelated in-flight working-tree changes via stash, and pushed. Confirmed my doc commit `2993b8448` is reachable from `origin/main` and all four files are clean on HEAD.

**Commit.** `2993b8448` (+ merge `964194c72`) → pushed to `btakita/agent-loop main`.

### Re: Queue stopped — opus-4-8

Acknowledged — the queue is now inactive (`queue: stop`, `queue_active: false`, `go` removed from the queue block) with no items pending, so there's nothing to drain and the auto-loop stays off. No repo work this cycle.

The backlog/review still hold the 13 gated items (e.g. `#7qky`, `#trt1*`, the SurrealDB substrate threads) if you want to pick one up next — just add it to the queue or say the word.
<!-- agent:boundary:e83680bc -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
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
