---
agent_doc_session: tsift-v0.1
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
Completed: #gdbrefreshdelta was already closed; #gdbcmcache, #gdbpathpolicy, and #gdbengineproof are implemented in tsift ab9f8a9 and agent-loop e09157f61. Local verification passed (`make check`, `cargo build`, `cargo install --path .`); latest tsift CI run 26372477579 is green.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260525-055028.md`*

Compacted content:
- Archived 10 response topic(s): #gdbfullreuse + #gdbscanproof + #gdbperfgate landed; review item #gdbcacheprove closed; #gdbgatecold packet in flight; 7 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
### Re: do [#cachelookupshift] — opus-4-7

Dispatched as a focused packet. Investigates why `preparation_cache_lookup` (~250 ms) never hits across consecutive `tsift graph-db backend-eval` runs on agent-loop — runs a pair, captures the watermark hash inputs, identifies the drifting input (likely a path slipping past `traversal_relative_path_is_generated_artifact` or a directory mtime entering the hash per the `#gdbgatecold` history), applies the minimal stable fix or stops with a "no fix — by design" verdict, then verifies cache-hit on a post-fix consecutive pair. Bumps to v0.1.51 if a fix lands. Hard regression-safety: `search_timeout_reports_reindex_when_index_turns_stale_during_worker_run` must stay green.

Awaiting the packet's proof block before `make check` + `cargo install --path .` + commit + push + finalize with `--done cachelookupshift`.
### Re: do [#gdbvacconvex] — closed via self-hosted Convex Docker — opus-4-7

Auth wall bypassed. Workflow end-to-end verified. Demo schema has a real snapshot scale gap (captured as new backlog items `#convexsnapshotscale` + `#convexscopedeval`) that keeps the Convex half of `#gdbvacproof` review-gated.

#### Bypass: self-hosted Convex backend (no convex.dev account)

```
docker run -d --name convex-tsift --rm -p 3210:3210 -p 3211:3211 \
  -v convex-tsift-data:/convex/data \
  ghcr.io/get-convex/convex-backend:latest
docker exec convex-tsift bash -c "cd /convex && ./generate_admin_key.sh"
```

Schema/functions deployed in ~220 ms via `bunx convex dev --once` with `CONVEX_SELF_HOSTED_URL` + admin key in `examples/convex-graph-app/.env.local` (gitignored). No browser auth, no convex.dev account, no `npx convex dev` device-name prompt.

#### Sync proven (tsift scope)

```
TSIFT_CONVEX_GRAPH_URL="http://localhost:3211/tsift/graph" \
  tsift convex-sync . --scope tsift --remote-snapshot --apply --chunk-size 50 --json
```

| Metric | Value |
| --- | ---: |
| node_upserts planned | 4,767 |
| edge_upserts planned | 28,427 |
| chunks dispatched | 665 |
| chunk receipts (HTTP 200) | **665 / 665** |
| transport diagnostic | "live Convex transport completed all planned chunks" |
| mutation order asserted | nodes-before-edges (upsert) / edges-before-nodes (tombstone) |

End-to-end proof of the chunk pipeline, the schema-side `upsertNodes`/`upsertEdges` mutations, the foreign-key check inside `upsertEdges`, and the chunk ordering contract against a real Convex backend.

#### Scale gap (demo schema, not a self-host limitation)

`examples/convex-graph/graph.ts:snapshot` calls `.collect()` on both populated tables in one isolate request. Even at 33k rows it hits Convex's `maximum total syscall duration (15s)` budget — same budget on the cloud product. Result: `--remote-snapshot` freshness verification returns `snapshot_hash: None`, which blocks `--confirmed-convex-reconciled` for the agent-loop scale graph (357k nodes / 605k edges). Full superproject sync also hits the isolate's 99 MiB carry-over limit at chunk-size 100 on `upsertEdges`; smaller chunks (25/50) clear that, but `snapshot` is the binding gate.

The Convex tombstone-prune leg of the other review item cannot run until `#convexsnapshotscale` lands.

#### Commits + cleanup
- tsift `3e8a211` — `docs(graph-db): #gdbvacconvex evidence — self-hosted backend bypasses auth; demo schema has snapshot scale gap`
- superproject `b4a23f705` — submodule pointer bump
- Container `convex-tsift` stopped + `convex-tsift-data` volume removed. `examples/convex-graph-app/.env.local` deleted (admin key was instance-local). `convex/_generated/` left in place (gitignored).
<!-- agent:boundary:72c6ea63 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
-->

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#cachelookupshift]~
- ~do [#gdbvacconvex]~
- complete [#gdbvacproof]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#convexsnapshotscale] Replace examples/convex-graph/graph.ts:snapshot's ctx.db.query('nodes'/'edges').collect() with a cursor-based paginated query (or a content-hash digest variant) so tsift convex-sync --remote-snapshot can verify reconciliation on tables larger than ~5k rows. Today the demo schema hits Convex's 15s syscall budget on snapshot once the backend has ~33k rows, which blocks --confirmed-convex-reconciled for the full agent-loop graph and therefore blocks the Convex half of #gdbvacproof.
- [ ] [#convexscopedeval] Decide whether tsift convex-sync should ship a scope-bounded default that promotes per-submodule reconciliation as the supported large-graph workflow, or whether the full-graph workflow waits on #convexsnapshotscale. Per-scope sync against the self-hosted backend (chunk-size 50) was end-to-end verified at the tsift scope (4767 nodes + 28427 edges, 665/665 receipts); full superproject sync hit the 99 MiB isolate carry-over limit at chunk-size 100.
- [/] [#orchdog] [recommended] Dogfood the completed graph-backed agent orchestration stack on a real agent-doc queue run: run refresh -> evidence -> conflict-matrix -> dispatch-trace -> jobs create/collect, compare worker_prompt_packet and worker_result closure fields, and capture any tsift-side fixture gaps. **Dogfood performed 2026-05-21 on `session-share/tasks/root.md` items #x63e + #v4v0; findings recorded in #gdbevid, #cmgr, #gdbidx — see `session-share/tasks/root.md` exchange "graph-backed orchestration dogfood" entry for the full pipeline trace.** **Second dogfood 2026-05-21 against `tasks/software/tsift.md` (preset `#dogfood-tsift`) surfaced 2 new gaps: #cmgrwres and #gdblock.**
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#gdbvacproof] [recommended] Run the real Convex reconciliation plus graph-db compact workflow and capture before/after file size, freelist, tombstone counts, status/doctor timings, and scan-cost deltas so compaction policy is backed by operational evidence.
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
