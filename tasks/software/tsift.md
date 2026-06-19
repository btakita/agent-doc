---
agent_doc_session: tsift-v0.1
agent: codex
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

❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260619-191329.md`*

Compacted content:
❯ - Archived 3 response topic(s): release + publish; release + publish (duplicate head); Follow crates.io/docs/trusted-publishing to setup trusted publishing
❯ - Prior summary/context: ❯ *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54...

### Re: How to verify #kgconfreqverify — gpt-5

`#kgconfreqverify` is still operator-gated because the real proof requires loading Qwen3-32B through Ollama and mutating a scratch KG graph. Do not use the current `.tsift/graph.db` as proof: it has 1 `kg_source` and 26 `semantic_entity` rows, but my read-only SQLite check found no persisted `confidence` / `confidence_source` on those existing entities, so it predates the confidence persistence fix.

The quick sanity check is:

```bash
cd /home/brian/work/btakita/agent-loop/src/tsift
tsift kg smoke --profile qwen3-32b-q4-ollama --unload --json
GRAPH=/tmp/tsift-kgconf-current.db
rm -f "$GRAPH"
tsift kg extract --profile qwen3-32b-q4-ollama \
  --input specs/local-kg-model.md \
  --source-ref specs/local-kg-model.md \
  --graph-db "$GRAPH" \
  --no-context \
  --json
sqlite3 "$GRAPH" "
select 'entities', count(*),
       sum(json_extract(properties_json,'$.confidence') is not null) as with_conf,
       sum(json_extract(properties_json,'$.confidence_source')='model') as model_conf,
       sum(json_extract(properties_json,'$.confidence_source')='default') as default_conf
from graph_nodes where kind='semantic_entity';
select 'relations', count(*),
       sum(json_extract(properties_json,'$.confidence') is not null) as with_conf,
       sum(json_extract(properties_json,'$.confidence_source')='model') as model_conf,
       sum(json_extract(properties_json,'$.confidence_source')='default') as default_conf
from graph_edges where kind='semantic_relation';
"
```

That verifies the current implementation path: `payload_schema()` requires `confidence`, `entity_node()` persists `confidence` plus `confidence_source`, and context-pack gating excludes explicit defaults under a positive confidence gate.

For the actual review closeout, run an A/B against the pre-`#kgconf` commit and current HEAD on the same real corpus. Use `b465979` as the baseline commit; `ac423b1` is the `#kgconf` change and current `v0.1.71` includes the follow-up ranking/gating fixes.

```bash
cd /home/brian/work/btakita/agent-loop/src/tsift
git worktree add /tmp/tsift-pre-kgconf b465979
cargo build --release --bin tsift
(cd /tmp/tsift-pre-kgconf && cargo build --release --bin tsift)

OLD=/tmp/tsift-pre-kgconf/target/release/tsift
NEW=/home/brian/work/btakita/agent-loop/src/tsift/target/release/tsift
OLD_GRAPH=/tmp/tsift-kgconf-old.db
NEW_GRAPH=/tmp/tsift-kgconf-new.db
rm -f "$OLD_GRAPH" "$NEW_GRAPH"

for src in SPEC.md specs/local-kg-model.md packages/tsift-kg/src/lib.rs packages/tsift-kg/src/ollama.rs; do
  "$OLD" kg extract --profile qwen3-32b-q4-ollama --input "$src" --source-ref "$src" --graph-db "$OLD_GRAPH" --no-context --json > "/tmp/old-${src//\//_}.json"
  "$NEW" kg extract --profile qwen3-32b-q4-ollama --input "$src" --source-ref "$src" --graph-db "$NEW_GRAPH" --no-context --json > "/tmp/new-${src//\//_}.json"
done

for graph in "$OLD_GRAPH" "$NEW_GRAPH"; do
  echo "== $graph =="
  sqlite3 "$graph" "
  select json_extract(properties_json,'$.source_ref') as source_ref,
         count(*) as semantic_entities
  from graph_nodes
  where kind='semantic_entity'
  group by source_ref
  order by source_ref;
  select count(*) as semantic_relations
  from graph_edges
  where kind='semantic_relation';
  select count(*) as entities,
         sum(json_extract(properties_json,'$.confidence') is not null) as with_confidence,
         sum(json_extract(properties_json,'$.confidence_source')='model') as model_confidence,
         sum(json_extract(properties_json,'$.confidence_source')='default') as default_confidence,
         min(cast(json_extract(properties_json,'$.confidence') as real)) as min_conf,
         max(cast(json_extract(properties_json,'$.confidence') as real)) as max_conf
  from graph_nodes
  where kind='semantic_entity';
  "
done
```

Pass criteria I would use: current extraction still returns nonzero entities for every source that produced entities in the baseline; aggregate entity/relation counts do not show a large systematic drop; every new `semantic_entity` and `semantic_relation` has `confidence` in `0.0..=1.0`; most or all new confidence values are `confidence_source=model`; and a spot-check of `/tmp/new-*.json` shows the scores are plausible rather than uniform or hallucinated. If requiring confidence causes zero-yield docs, a material count drop, or suspicious scoring, keep the review open and relax toward prompt-only confidence plus the derived-default fallback.

Commands I ran this cycle were read-only: `tsift status`, `tsift --version`, `tsift kg --help`, `tsift kg extract/status/evidence --help`, `tsift kg status/evidence --json`, SQLite schema/count queries, and `git log` to identify the A/B boundary. No code changed; `src/tsift` is clean.
<!-- agent:boundary:0fb5a71c -->
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
- [/] [#kgconfreqverify] [operator-verify] Verify the confidence-required schema change (#kgconf, payload_schema in packages/tsift-kg/src/ollama.rs) does not reduce extraction yield on the live local model (Qwen3-32B-Q4) via an A/B over a real corpus; if required-confidence cuts entities/relations or induces hallucinated scores, relax to prompt-only + the derived-default fallback. Needs GPU + Ollama (operator-gated).
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
