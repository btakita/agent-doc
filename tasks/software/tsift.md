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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260618-054354.md`*

Compacted content:
- Archived 4 response topic(s): do [#kgwiring]; do [#kgoperatorrun]; tsift kg multi-session reference lifetime; 1 more
- Prior summary/context: prior compacted content: Archived 8 response topic(s): #lmlazy; active local-model KG usage; #kgmrs; 5 more

### Re: Is tsift kg now in active usage? — partial; read seam live, lifecycle/extraction not yet consumed — opus-4-8

> **Queue prompt:**
>
> Is tsift kg now in active usage? If not, what are #next-steps needed?

**Partially — the read/evidence layer is live and has data, but the lease lifecycle and extraction are not yet consumed by any autonomous path.** I checked code wiring + on-disk state rather than assuming:

1. **Evidence read seam — WIRED, data present (active on real digests).** `graph_evidence` is wired into `session_digest::compute` (→ `session-review`, `context-pack`) with passing tests (`markdown_digest_surfaces_graph_evidence_when_graph_db_present`), and this workspace has `.tsift/graph.db` — 2.4 GB / 418k-node superproject graph and a 135 MB / 10,412-node submodule graph that **includes the seeded KG content** (`semantic_entity: 26`, `kg_chunk: 2`, plus `kg_source` from `#kgoperatorrun`). So a real planning digest with a touched-symbol scope surfaces KG evidence — totals-only for the oversized superproject graph (it exceeds the 50k bounded-scan cap), full for the submodule. (I confirmed the wiring in code + the data on disk; I did not reproduce a live emission via a bare-path CLI call — the digest derives its scope from session-transcript context, not a path argument.)
2. **Lease lifecycle (`#kgreflease`) — BUILT but NOT consumed.** `~/.tsift/gpu-lease.json` is empty (`"leases": {}`), and nothing calls `acquire`/`renew`/`release`/`reap` outside the CLI and tests. The reference-counted lifetime I shipped is ready infrastructure that no extraction or session actually acquires.
3. **Extraction — manual / operator-gated, stale snapshot.** `kg extract` runs only via explicit CLI (GPU + Ollama); nothing schedules or refreshes it. The graph.db is a Jun-14 snapshot.

**So: not in autonomous active usage.** The read seam fires when digests run over existing data, but extraction and lease management are CLI-only.

**#next-steps (priority order, added to backlog):**
- **`#kgleasewire`** (top) — wire `acquire_lease` before model load + `release_lease --unload-on-last-release` after in `cmd_kg_extract` / the extract pipeline, so the `#kgreflease` lifecycle is actually consumed (concurrent extracts serialize on the GPU; VRAM auto-frees). This closes the loop on the work just shipped.
- **`#kgleasereap`** — invoke `reap` + `renew` automatically: wire `tsift local-model lease reap --unload-empty` into the agent-doc supervisor idle-watch (and renew long-lived holders) so crashed-session leases are reclaimed without manual CLI.
- **`#kgextractrefresh`** — define an extraction refresh trigger (on-demand re-extract of changed files, or scheduled) so `graph.db` isn't a stale manual snapshot.
- **`#kggraphscope`** — the 418k-node superproject graph exceeds the 50k bounded-scan cap (evidence is totals-only there); decide per-submodule KG graphs vs. raising/segmenting the cap so superproject evidence is useful.

**Top of backlog: `#kgleasewire`.** (`#vmax` — crates.io OIDC registration — remains below these as operator-gated.)
<!-- agent:boundary:bd2638cf -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- do [#kgleasewire]
- do [#kgleasereap]
- do [#kgextractrefresh]
- do [#kggraphscope]
<!-- /agent:queue -->
<!--
https://github.com/chopratejas/headroom
-->

## Backlog

<!-- agent:backlog priority queue -->
- [ ] [#kgleasewire] Wire the #kgreflease lease lifecycle into kg extract: acquire_lease (profile, holder pid) before model load in cmd_kg_extract/extract pipeline, release_lease --unload-on-last-release after, so concurrent extracts serialize on the GPU and VRAM auto-frees. Closes the loop on #kgreflease (registry currently has zero callers outside CLI/tests).
- [ ] [#kgleasereap] Invoke lease reap + renew automatically: wire `tsift local-model lease reap --unload-empty` into the agent-doc supervisor idle-watch (and renew long-lived holders) so crashed-session leases are reclaimed and unreferenced models unloaded without manual CLI.
- [ ] [#kgextractrefresh] Define a kg extract refresh trigger (on-demand re-extract of changed files, or scheduled) so .tsift/graph.db is not a stale manual snapshot; current graph.db is a Jun-14 one-shot.
- [ ] [#kggraphscope] Superproject graph.db is 418k nodes > 50k bounded-scan cap, so session-digest graph_evidence is totals-only there. Decide: per-submodule KG graphs vs raising/segmenting DEFAULT_EVIDENCE_MAX_SCAN_NODES so superproject evidence is useful.
- [ ] [#vmax] [operator-verify] Enable crates.io OIDC publishing for tsift (advances review #1td4; code already shipped in fe4144b/f29f7a2, release.yml crates job uses rust-lang/crates-io-auth-action@v1). Operator-gated steps (need crates.io account + browser 2FA — agent cannot perform): (1) On crates.io register a Trusted Publisher for EACH of the 23 tsift crates — GitHub repo btakita/tsift, workflow release.yml, no environment: tsift-core, tsift -md-ast, tsift-graph, tsift-sqlite, tsift-algorithms, tsift-resolution, tsift-cache, tsift-tokensave, tsift-libsql, tsift-index, tsift-summarize, tsift-quality, tsift-agent-doc, tsift-digest, tsift-search, tsift-status, tsift-session, tsift-memory, tsift-memgraphrag, tsift-surrealdb, tsift-cli, tsift, tsift-sim-world. (2) Enable the gate var: gh variable set TSIFT_ENABLE_CRATES_PUBLISH --body true. (3) Force-move the v0.1.70 tag to HEAD and push to re-trigger; confirm the publish-crates.io job authenticates via OIDC (no CARGO_REGISTRY_TOKEN) and publishes.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
