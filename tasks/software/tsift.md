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
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260619-170438.md`*

Compacted content:
- Archived 4 response topic(s): #kgeffmeasure; measured kg extraction effectiveness on a real corpus; #kgconf; 1 more
- Prior summary/context: prior compacted content: Archived 1 response topic(s): Operating effectiveness of tsift kg

### Re: #next-steps to fix tsift bugs — opus-4-8

> **Queue prompt:**
>
> What are #next-steps to fix tsift bugs?


The two queue items (`#kgconf`, `#kgrefreshdup`) are done; the backlog was empty. From the gaps those fixes exposed, here are the grounded next steps, now queued in priority order:

1. **`#kgconfrank` (high)** — `#kgconf` makes every node carry a `confidence` + `confidence_source` tag, but nothing consumes the tag yet: `node_to_candidate` in `context_pack.rs` reads only the raw `confidence`, so a *derived* `default 0.500` outranks and out-survives a *real* model `0.300` under `min_confidence`/ranking. Weight model-emitted confidence above derived defaults. This directly completes the operability `#kgconf` set up, so it leads.
2. **`#kgentitycollapse` (med)** — `#kgctxinject` dedup is naming-convergence + suppression, **not** node-identity collapse: node id = `stable_id(chunk.id, entity.id)`, so same-label entities from different chunks stay distinct nodes (two dup nodes shared one canonical `entity_id` but had different node ids — finding (c)). Add `same_as` edges keyed on the canonical `entity_id` (or a query-time merge) so retrieval sees one logical entity; preserve per-source provenance by linking rather than deleting.
3. **`#kgconfreqverify` (operator-verify)** — `#kgconf` made `confidence` a *required* schema field to elicit real scores. Verify on the live Qwen3-32B model that this doesn't cut extraction yield or induce hallucinated scores; relax to prompt-only + default fallback if it does. Needs GPU + Ollama, so it's operator-gated.

**Top of backlog: `#kgconfrank`.**

(Heads-up: the exchange is at `session_accretion: warn` — 80 lines / 4 response sections. Say the word and I'll `agent-doc compact tasks/software/tsift.md --commit`.)
<!-- agent:boundary:ff8316cd -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- do [#kgentitycollapse]
- do [#kgconfrank]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#kgconfreqverify] [operator-verify] Verify the confidence-required schema change (#kgconf, payload_schema in packages/tsift-kg/src/ollama.rs) does not reduce extraction yield on the live local model (Qwen3-32B-Q4) via an A/B over a real corpus; if required-confidence cuts entities/relations or induces hallucinated scores, relax to prompt-only + the derived-default fallback. Needs GPU + Ollama (operator-gated).
- [ ] [#kgentitycollapse] tsift KG does not collapse entities across chunks by canonical identity: node id = stable_id(chunk.id, entity.id), so same-label entities from different chunks become distinct nodes — duplicate canonical entities even within one extraction (two dup nodes shared canonical entity_id but differed in node id, finding (c)). Add same_as edges keyed on the canonical entity_id property (or a query-time identity-merge) so retrieval sees one logical entity. Preserve per-source provenance — prefer linking over hard delete.
- [ ] [#kgconfrank] tsift KG context-pack ranking/gating ignores the new confidence_source tag (#kgconf): node_to_candidate in packages/tsift-kg/src/context_pack.rs reads only the raw confidence, so a derived default 0.500 outranks and out-survives a real model 0.300. Thread confidence_source into ranking (model-tier above default-tier) and/or min_confidence gating so derived defaults are distinguishable from model scores — completes the operability #kgconf set up.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#vmax] [operator-verify] Enable crates.io OIDC publishing for tsift (advances review #1td4; code already shipped in fe4144b/f29f7a2, release.yml crates job uses rust-lang/crates-io-auth-action@v1). Operator-gated steps (need crates.io account + browser 2FA — agent cannot perform): (1) On crates.io register a Trusted Publisher for EACH of the 23 tsift crates — GitHub repo btakita/tsift, workflow release.yml, no environment: tsift-core, tsift -md-ast, tsift-graph, tsift-sqlite, tsift-algorithms, tsift-resolution, tsift-cache, tsift-tokensave, tsift-libsql, tsift-index, tsift-summarize, tsift-quality, tsift-agent-doc, tsift-digest, tsift-search, tsift-status, tsift-session, tsift-memory, tsift-memgraphrag, tsift-surrealdb, tsift-cli, tsift, tsift-sim-world. (2) Enable the gate var: gh variable set TSIFT_ENABLE_CRATES_PUBLISH --body true. (3) Force-move the v0.1.70 tag to HEAD and push to re-trigger; confirm the publish-crates.io job authenticates via OIDC (no CARGO_REGISTRY_TOKEN) and publishes.
- [/] [#1td4] Release infra: crates.io publish job in .github/workflows/release.yml is MIGRATED to OIDC trusted publishing (commit fe4144b, on origin/main f29f7a2; uses rust-lang/crates-io-auth-action@v1, id-token: write, no CARGO_REGISTRY_TOKEN). Code complete. Remaining is operator-gated and tracked as actionable backlog #vmax: register the Trusted Publisher per crate on crates.io (23 crates), set repo var TSIFT_ENABLE_CRATES_PUBLISH=true, then re-tag v0.1.70 to verify OIDC publish. Stays gated until operator completes crates.io registration.
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
