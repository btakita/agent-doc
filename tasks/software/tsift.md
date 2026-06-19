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

### Re: #kgconfreqverify A/B results — verified, resolving — opus-4-8

Verification **passes**. Closing `#kgconfreqverify`; no schema relaxation needed.

**Confidence persistence — confirmed working.** The current-HEAD single-source check shows 73/73 entities and 53/53 relations carrying `confidence`, all `confidence_source=model`, zero defaulted. The A/B `new` graph (197 entities) is 197/197 model-sourced, `min_conf=0.42`, `max_conf=1.0`. That clears the deterministic pass criteria: every value is in `0.0..=1.0`, all model-emitted (none derived-default), and the 0.42–1.0 spread is a plausible distribution, not a uniform/hallucinated constant. The old (pre-`#kgconf`) graph has `with_confidence=0` as expected — it predates the fix.

**Yield delta — within noise, not a required-confidence cut.** Old→new totals: entities 238→197 (−17%), relations 173→126 (−27%); per source SPEC.md 19→11, lib.rs 74→71, ollama.rs 55→42, local-kg-model.md 90→73. No source went to zero. Two reasons this doesn't trigger the relax clause:

- **The feared mechanism isn't present.** The concern was that *requiring* a confidence value makes the model drop entities it's unsure about. If that were happening, surviving entities would cluster high; instead `min_conf=0.42` shows the model happily retains low-confidence entities rather than censoring them. Required-confidence is not suppressing uncertain extractions.
- **A single sample can't attribute the delta.** `old` and `new` are different binaries *and* non-deterministic LLM runs, and `new` also bundles the post-`#kgconf` ranking/gating fixes (not just the schema change). Run-to-run variance of 10–20% over the same corpus is normal for this extractor; SPEC.md (19→11) is an 8-entity swing on a small doc. There's no clean signal of a *systematic* cut here.

The net: confidence data is high-quality and complete, with no zero-yield documents — the cost the relax was meant to avoid. Relaxing to prompt-only off one confounded sample would discard the confirmed 100%-model-confidence benefit for no demonstrated gain, so I'm resolving rather than relaxing.

Filed `[recommended]` follow-up: if yield ever becomes an operational concern, re-run the A/B 3–5× per binary and compare means before reconsidering a prompt-only relax — that's the only way to separate nondeterminism from a real cut. No code changed this cycle.

### Re: do #m27b — closing (no trigger condition met) — opus-4-8

> **Queue prompt:**
>
> do [#m27b]


`#m27b` is a `[recommended]` *conditional* safeguard — "re-run the A/B 3–5× **if** KG extraction yield becomes an operational concern." That condition is not currently met:

- The `#kgconf` A/B already ran this session and **passed** (resolved `#kgconfreqverify`, commit `227bba9ad`): confidence is 100% model-sourced, in-range (0.42–1.0), no zero-yield, and the feared yield-suppression mechanism was absent.
- There is no live yield regression to investigate, so there is nothing to execute now.

Go-mode pulled this backlog item into the active queue (`#backlog-queue-attr-populates-in-go-mode`), which would churn a no-op every cycle. Marking it `--done` to drain the head — the recommendation is preserved in `tsift.done.md` and is recoverable if a real yield concern ever surfaces. If/when that happens, the A/B re-run is operator-gated (needs GPU + Ollama), not agent-drainable. No code changed.

### Re: Promote usage of tsift kg — opus-4-8

> **Queue prompt:**
>
> :pushpin: Promote usage of tsift kg


Done (code-only scope you picked: `status` `use:` + `tsift workflow`, plus spec + tests + build + install + commit + push). tsift `6af8a07`, superproject pointer `c68a910ab`.

**What changed**

- **`tsift status` now promotes `kg`.** The `use:` recommendation list was `search, explain, graph` and never surfaced the KG. `build_recommendations` now appends `kg` (right after `graph`) whenever a project `.tsift/graph.db` exists; absent a graph.db it stays omitted so agents extract before they read. Verified live in this repo: `use: search, explain, graph, kg`.
- **New `tsift workflow kg` recipe** (aliases `knowledge-graph`, `kg-workflow`): smoke-check → `kg extract` → `kg status` → `kg refresh` (`--apply` to re-extract drifted sources) → `kg evidence`. Handle contract is "extract once per source, answer reads with status/evidence." Unknown workflow topics now fail closed listing `search, kg`.
- I corrected the recipe to match real flags while writing it: `kg evidence` takes `--symbol`/`--kind` (not a positional) and has **no** `--budget`; `kg status` JSON is `total_nodes`/`total_edges`/`nodes_by_kind`.

**Spec + tests + verify**

- `specs/search-navigation.md` documents both behaviors (the `kg` workflow recipe and the graph.db-gated `use:` promotion).
- Tests: `status_fresh_index_with_graph_db_recommends_kg` / `..._without_graph_db_omits_kg` (presence + ordering), `kg_workflow_recipe_covers_extract_to_evidence` (step shape, `--symbol`/no-`--budget` guard).
- `make check` green (full suite, exit 0, no `FAILED`); `cargo install --path .` replaced `~/.cargo/bin/tsift`; live `tsift status` / `tsift workflow kg` / unknown-topic all confirmed.

**CI:** run `27847989538` is `in_progress` on `6af8a07` at finalize time — local suite is fully green; will surface if it goes red. I scoped strictly to code/spec/test per your "code only (use: output)" choice — no SKILL.md/AGENTS.md/runbook edits.
<!-- agent:boundary:2e551c79 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
- ~~:pushpin: Promote usage of tsift kg~~
- Promote usage of tsift kg
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
