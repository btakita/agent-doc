---
agent_doc_session: tsift-v0.1
agent: codex
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

complete
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260608-212555.md`*

Compacted content:
- Archived 1 response topic(s): closeout repair
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: prompt caching — gpt-5

> **Queue prompt:**
>
> Please add analytics to the prompt cache. I want to be able to review effective it is over time.


> **Queue prompt:**
>
> Can tsift implement Prompt Caching? See https://viktor.com/blog/how-we-built-viktor-around-prompt-caching


Yes. tsift can implement prompt caching as a deterministic planning and diagnostics layer now, while leaving live provider API mutation to whichever agent runtime calls the model. I added `prompt_cache_plan` to `tsift session-cost`: it appears when cache reads/creation are observed or when uncached prompt volume is large enough to justify cache setup.

The plan captures the Viktor-style cache shape from the linked article: stable context first, append-only thread history until an intentional compaction boundary, no volatile timestamps or random ids inside the cached prefix, compaction against the live warm prefix, and provider-specific adapter requirements for Anthropic `cache_control` breakpoints, OpenAI `prompt_cache_key`, and self-hosted/edge replica affinity.

Changed paths:
- `src/tsift/SPEC.md`
- `src/tsift/specs/digests-sessions.md`
- `src/tsift/packages/tsift-agent-doc/src/session_cost.rs`
- `src/tsift/packages/tsift-cli/src/commands/digests.rs`
- `src/tsift/packages/tsift-cli/tests/exit_code.rs`

Commits:
- tsift submodule: `505506f` (`add session prompt cache plan`), pushed to `origin/main`.
- parent repo: `src/tsift` already records `505506f` at `HEAD`; no separate pointer commit was needed before this agent-doc closeout.

Verification:
- `tsift --envelope digest-runner --kind test --path . --shell-command 'cargo test -p tsift-cli --test exit_code session_cost_ -- --nocapture'`: passed, artifact `tart-02a5c5c8a8`.
- `tsift --envelope digest-runner --kind test --path . --shell-command 'make check'`: passed, artifact `tart-ffffe139d0`.
- `tsift --envelope digest-runner --kind log --path . --shell-command 'cargo install --path .'`: passed, artifact `tart-99633b041d`.
- `gh run list --workflow CI --limit 1` in `src/tsift`: latest CI was completed/success for `test semantic ast edit edge cases` at 2026-06-08T19:08:05Z.
- `gh run list --workflow CI --limit 1` in `src/tsift`: latest CI was completed/success for `add session prompt cache plan` at 2026-06-08T21:25:43Z.
- `tsift status` in `src/tsift`
- `agent-doc read /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`
- `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`
- `net_cached_input_tokens` gives a coarse effectiveness signal: cache reads minus creation tokens. Negative or small positive values are a warning sign even when cache reads are present.
- `total_cache_creation_tokens` and `cache_read_to_creation_ratio` expose rewrite churn. High creation with low reads means the runtime is repeatedly creating cache entries without getting enough warm reuse.
- `timeline` shows which turns had a ratio drop, so the next step is to inspect what changed at that boundary: prefix text, tools, cache key, compaction, or routing.
- `average_cached_input_ratio`, first/last ratios, and `cached_input_ratio_delta` show whether reuse is holding, improving, or degrading.
- Self-hosted or edge caches may be replica-local. Without routing affinity for the same cache key, a byte-identical prompt can still miss because it lands on a different replica.
- Compaction should preserve the live warm prefix and rewrite only the transcript tail. A cold standalone summarizer prompt effectively starts a new cache shape.
- Volatile values inside the cached prefix, such as timestamps, generated IDs, transient diagnostics, or one-off compaction instructions, should be treated as accidental invalidation sources.
- Thread or session key changes should invalidate OpenAI-style keyed reuse because the same byte-identical prefix is no longer associated with the same `prompt_cache_key`.
- Tool definition changes should invalidate the tool-prefix segment. For Anthropic-style explicit breakpoints, edits before a `cache_control` breakpoint invalidate that block and downstream cached material.
- A stable system/developer prefix change should invalidate the cache for that prefix because the bytes are no longer identical.
- `git diff --check`: passed.
- `tsift --envelope digest-runner --kind log --path . --shell-command 'cargo install --path .'`: passed, artifact `tart-99633b041d`.
- `tsift --envelope digest-runner --kind test --path . --shell-command 'make check'`: passed, artifact `tart-35d22ae8c2`.
- `tsift --envelope digest-runner --kind test --path . --shell-command 'cargo test -p tsift-cli --test exit_code session_cost_ -- --nocapture'`: passed, artifact `tart-02a5c5c8a8`.
- `tsift --envelope digest-runner --kind test --path . --shell-command 'cargo test -p tsift-agent-doc prompt_cache_plan_summarizes_effectiveness_over_time -- --nocapture'`: passed, artifact `tart-e2cfb2ea68`.
- parent repo: `220cbf472` (`update tsift prompt cache analytics`), pushed to `origin/main`.
- tsift submodule: `7b9da33` (`add prompt cache analytics`), pushed to `origin/main`.
- parent pointer: `src/tsift`
- `src/tsift/specs/digests-sessions.md`
- `src/tsift/packages/tsift-cli/tests/exit_code.rs`
- `src/tsift/packages/tsift-cli/src/commands/digests.rs`
- `src/tsift/packages/tsift-agent-doc/src/session_cost.rs`
- `src/tsift/VERSIONS.md`
- Added library and compiled CLI regression coverage for improving, stable, and candidate uncached prompt-cache analytics.
- Updated the session digest spec and unreleased changelog entry.
- Added compact and human text output for the analytics so cache effectiveness can be reviewed without raw JSON.
- Added bounded `prompt_cache_plan.analytics` with sample count, effective flag, trend (`single_sample`, `stable`, `improving`, `declining`, `insufficient_data`), average/first/last cached-input ratios, ratio delta, net cached-read minus cache-creation tokens, optional read/create ratio, timeline truncation flag, and a first-plus-latest per-turn cache timeline.

No follow-up backlog item was created for this cycle; the implemented `prompt_cache_plan` is the tsift-owned surface, and live provider adapter calls belong to the runtime that invokes the model.

Escalation: none.

Confidence: high.

### Re: prompt cache invalidation — gpt-5

> **Queue prompt:**
>
> Please describe how cache invalidation works for our prompt cache system.


The prompt cache system should treat invalidation as a stable-prefix contract, not as an explicit `clear cache` operation owned by tsift.

For the current `session-cost` surface, tsift observes cache effectiveness from transcript usage. The live cache itself belongs to the model provider or runtime adapter. That means a cache miss usually means the runtime sent a different cache key, different routing affinity, or different prefix bytes than the provider saw on the previous warm call.

Practical invalidation rules:
- A stable system/developer prefix change should invalidate the cache for that prefix because the bytes are no longer identical.
- Tool definition changes should invalidate the tool-prefix segment. For Anthropic-style explicit breakpoints, edits before a `cache_control` breakpoint invalidate that block and downstream cached material.
- Thread or session key changes should invalidate OpenAI-style keyed reuse because the same byte-identical prefix is no longer associated with the same `prompt_cache_key`.
- Volatile values inside the cached prefix, such as timestamps, generated IDs, transient diagnostics, or one-off compaction instructions, should be treated as accidental invalidation sources.
- Compaction should preserve the live warm prefix and rewrite only the transcript tail. A cold standalone summarizer prompt effectively starts a new cache shape.
- Self-hosted or edge caches may be replica-local. Without routing affinity for the same cache key, a byte-identical prompt can still miss because it lands on a different replica.

How the new analytics help review invalidation over time:
- `average_cached_input_ratio`, first/last ratios, and `cached_input_ratio_delta` show whether reuse is holding, improving, or degrading.
- `timeline` shows which turns had a ratio drop, so the next step is to inspect what changed at that boundary: prefix text, tools, cache key, compaction, or routing.
- `total_cache_creation_tokens` and `cache_read_to_creation_ratio` expose rewrite churn. High creation with low reads means the runtime is repeatedly creating cache entries without getting enough warm reuse.
- `net_cached_input_tokens` gives a coarse effectiveness signal: cache reads minus creation tokens. Negative or small positive values are a warning sign even when cache reads are present.

The recommended backlog item `#1zax` now tracks explicit invalidation diagnostics on top of the analytics, so tsift can eventually classify likely causes instead of only reporting symptoms.

Changed paths: none in this explanation cycle.

Commands:
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`
- `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`
- `agent-doc read /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`
- `tsift status` in `src/tsift`

Verification: no code changed in this cycle, so no build/test suite was rerun.

Escalation: none.

Confidence: high.

### Re: prompt cache invalidation — gpt-5

Prompt cache invalidation is mostly a provider/runtime behavior. In tsift, the `prompt_cache_plan` is a diagnostic report emitted by `session-cost`; it observes cache reads, cache creation, ratios, and churn, but it does not own or clear the live cache.

The practical rule is: a cached prefix is reusable only while the runtime sends the same stable prefix under the same cache identity and, for replica-local caches, to compatible routing. Changing any part of that identity turns reuse into a miss.

The main invalidation cases are:
- Stable system/developer context changes: the prefix bytes differ, so the previous cached prefix no longer matches.
- Tool definitions change: the tool-prefix segment changes. With Anthropic-style explicit `cache_control` breakpoints, edits before a breakpoint invalidate that cached block and downstream material.
- Thread/session key changes: OpenAI-style reuse depends on a stable `prompt_cache_key`; a byte-identical prefix under a different key should be treated as a new cache lineage.
- Volatile values enter the cached prefix: timestamps, random IDs, generated headers, transient diagnostics, and one-off compaction instructions create accidental misses even when the semantic prompt feels unchanged.
- Compaction starts cold: compaction should reuse the live warm prefix and rewrite only the transcript tail. A standalone summarizer prompt effectively creates a new cache shape.
- Replica affinity is lost: self-hosted or edge caches can be replica-local, so byte-identical calls can still miss if routing sends the same cache key to a different replica.

The analytics are meant to reveal these invalidations over time. A declining cached-input ratio points to prefix/key/routing churn. High `total_cache_creation_tokens` with low reads means we are repeatedly writing cache entries without getting enough reuse. `net_cached_input_tokens` summarizes the rough economics by comparing reads against creation tokens. The per-turn timeline is the place to locate the boundary where reuse fell and then inspect what changed at that turn: prefix bytes, tools, compaction, key, or routing.

Changed paths: none.

Commands:
- `agent-doc preflight tasks/software/tsift.md` via MCP
- `agent-doc read tasks/software/tsift.md` via MCP
- `agent-doc plan tasks/software/tsift.md` via MCP
- `agent-doc session-check tasks/software/tsift.md` via MCP
- `tsift status` in `/home/brian/work/btakita/agent-loop`
- `tsift status` in `/home/brian/work/btakita/agent-loop/src/tsift`
- `tsift --envelope search "prompt_cache_plan" --budget normal`
- `rg -n "prompt_cache|cache_control|prompt cache|cache invalid" .`
- `tsift --envelope source-read "specs/digests-sessions.md" --path "/home/brian/work/btakita/agent-loop/src/tsift" --style window --start 150 --lines 25 --budget normal`
- `tsift --envelope source-read "packages/tsift-agent-doc/src/session_cost.rs" --path "/home/brian/work/btakita/agent-loop/src/tsift" --style window --start 528 --lines 125 --budget normal`
- `tsift --envelope source-read "packages/tsift-agent-doc/src/session_cost.rs" --path "/home/brian/work/btakita/agent-loop/src/tsift" --style window --start 650 --lines 105 --budget normal`
- `git status --short` in parent and `src/tsift`
- Latest GitHub Actions CI at check time: completed/success for `add prompt cache analytics`, run `27168741671`, timestamp `2026-06-08T21:41:20Z`.
- `make check`: passed, artifact `tart-a8fd2b6bfb`.
- `git diff --check`: passed.
- `cargo test -p tsift-cli --test exit_code session_cost_ -- --nocapture`: passed, artifact `tart-6705d54bb2`.
- `cargo test -p tsift-agent-doc prompt_cache_plan_ -- --nocapture`: passed, artifact `tart-b0ccbb4fd3`.
- `git push origin main` in `src/tsift`
- `git commit -m "add prompt cache invalidation diagnostics"` in `src/tsift`
- `gh run list --workflow CI --limit 1`
- `tsift --envelope digest-runner --kind test --path . --shell-command 'make check'`
- `tsift --envelope digest-runner --kind test --path . --shell-command 'cargo test -p tsift-cli --test exit_code session_cost_ -- --nocapture'`
- `tsift --envelope digest-runner --kind test --path . --shell-command 'cargo test -p tsift-agent-doc prompt_cache_plan_ -- --nocapture'`
- `git diff --check`
- `cargo fmt`; then restored unrelated workspace-wide rustfmt churn and kept the intended files only
- `tsift --envelope source-read ...` for `session_cost.rs`, `digests.rs`, `exit_code.rs`, `VERSIONS.md`, and `specs/digests-sessions.md`
- `tsift status` in parent and `src/tsift`
- `git restore -- tasks/software/tsift.md` to drop malformed local drift from the earlier streaming closeout; HEAD already contained the committed response
- `agent-doc repair tasks/software/tsift.md`
- `agent-doc read tasks/software/tsift.md` via MCP
- `agent-doc preflight tasks/software/tsift.md` via MCP
- parent pointer: committed with this agent-doc closeout.
- tsift submodule: `993b130` (`add prompt cache invalidation diagnostics`), pushed to `origin/main`.
- `src/tsift/packages/tsift-cli/tests/exit_code.rs`
- `src/tsift/packages/tsift-cli/src/commands/digests.rs`
- `src/tsift/packages/tsift-agent-doc/src/session_cost.rs`
- `src/tsift/specs/digests-sessions.md`
- `src/tsift/VERSIONS.md`
- `src/tsift` parent pointer
- `read_create_regression` when session-level cache reads are less than 2.0x cache-creation tokens, with guidance to stabilize prefix, key, and routing before treating the cache as effective.
- `cache_creation_spike` when cache creation is at least 20% of a turn's prompt tokens, with likely causes covering fresh prefix creation, system/developer/tool block churn, and compaction/transient instructions entering the cached prefix.
- `cached_ratio_drop` when adjacent turns lose at least 20 percentage points of cached-input reuse, with likely causes covering prefix byte changes, `prompt_cache_key` or thread/session changes, and replica-local routing loss.

Verification: no code changed, so no build or test suite was rerun. The explanation was checked against the current tsift spec and `session_cost.rs` implementation.

Escalation: none.

Confidence: high.

### Re: #1zax prompt-cache invalidation diagnostics — gpt-5

> **Queue prompt:**
>
> do [#1zax]


Implemented `#1zax` in `src/tsift`.

`session-cost` now classifies likely prompt-cache invalidation symptoms inside `prompt_cache_plan.analytics.diagnostics`:
- `cached_ratio_drop` when adjacent turns lose at least 20 percentage points of cached-input reuse, with likely causes covering prefix byte changes, `prompt_cache_key` or thread/session changes, and replica-local routing loss.
- `cache_creation_spike` when cache creation is at least 20% of a turn's prompt tokens, with likely causes covering fresh prefix creation, system/developer/tool block churn, and compaction/transient instructions entering the cached prefix.
- `read_create_regression` when session-level cache reads are less than 2.0x cache-creation tokens, with guidance to stabilize prefix, key, and routing before treating the cache as effective.

The diagnostics are bounded, serialized in JSON, printed in compact output as `prompt-cache-diagnostic ...`, and shown in human output with likely causes and guidance. I also updated the session-cost spec and unreleased changelog entry.

Changed paths:
- `src/tsift` parent pointer
- `src/tsift/VERSIONS.md`
- `src/tsift/specs/digests-sessions.md`
- `src/tsift/packages/tsift-agent-doc/src/session_cost.rs`
- `src/tsift/packages/tsift-cli/src/commands/digests.rs`
- `src/tsift/packages/tsift-cli/tests/exit_code.rs`

Commits:
- tsift submodule: `993b130` (`add prompt cache invalidation diagnostics`), pushed to `origin/main`.
- parent pointer: committed with this agent-doc closeout.

Commands:
- `agent-doc preflight tasks/software/tsift.md` via MCP
- `agent-doc read tasks/software/tsift.md` via MCP
- `agent-doc repair tasks/software/tsift.md`
- `git restore -- tasks/software/tsift.md` to drop malformed local drift from the earlier streaming closeout; HEAD already contained the committed response
- `tsift status` in parent and `src/tsift`
- `tsift --envelope source-read ...` for `session_cost.rs`, `digests.rs`, `exit_code.rs`, `VERSIONS.md`, and `specs/digests-sessions.md`
- `cargo fmt`; then restored unrelated workspace-wide rustfmt churn and kept the intended files only
- `git diff --check`
- `tsift --envelope digest-runner --kind test --path . --shell-command 'cargo test -p tsift-agent-doc prompt_cache_plan_ -- --nocapture'`
- `tsift --envelope digest-runner --kind test --path . --shell-command 'cargo test -p tsift-cli --test exit_code session_cost_ -- --nocapture'`
- `tsift --envelope digest-runner --kind test --path . --shell-command 'make check'`
- `gh run list --workflow CI --limit 1`
- `git commit -m "add prompt cache invalidation diagnostics"` in `src/tsift`
- `git push origin main` in `src/tsift`

Verification:
- `cargo test -p tsift-agent-doc prompt_cache_plan_ -- --nocapture`: passed, artifact `tart-b0ccbb4fd3`.
- `cargo test -p tsift-cli --test exit_code session_cost_ -- --nocapture`: passed, artifact `tart-6705d54bb2`.
- `git diff --check`: passed.
- `make check`: passed, artifact `tart-a8fd2b6bfb`.
- Latest GitHub Actions CI at check time: completed/success for `add prompt cache analytics`, run `27168741671`, timestamp `2026-06-08T21:41:20Z`.

Escalation: none.

Confidence: high.

### Re: prompt cache and MemGraphRag — gpt-5

> **Queue prompt:**
>
> Can the prompt cache system be integrated into the MemGraphRag?


Yes, but the integration should be telemetry/projection, not ownership transfer of the provider cache.

The prompt-cache system currently lives in `session-cost` as diagnostics over model usage: cache reads, cache creation, effectiveness analytics, and invalidation symptoms. MemGraphRag is already present in tsift as a shared graph substrate path: `tsift memory project-graph` projects memory events into `.tsift/graph.db`, and `tsift memory ontology-graph` derives the ontology layer over node/edge kinds. That gives us a good integration point.

The right shape is to project prompt-cache observations into the memory/code graph as first-class session telemetry:
- `prompt_cache_observation` or `model_call` nodes linked to the owning `memory_session`, `memory_event`, agent-doc cycle, or transcript turn.
- Properties for provider, model, prompt token totals, cached-read tokens, cache-creation tokens, read/create ratio, trend, and diagnostic kinds such as `cached_ratio_drop`, `cache_creation_spike`, and `read_create_regression`.
- Edges from the observation to likely affected context: tool-set/version nodes, compaction boundary nodes, session/thread key lineage, and source or worker-context handles when available.
- Ontology derivation can then expose these as queryable types and relations, so graph retrieval can answer questions like "which sessions had repeated cache rewrites after tool changes?" or "which workflows lose cache affinity after compaction?"

What should not move into MemGraphRag: live cache mutation, provider `cache_control`, OpenAI `prompt_cache_key` selection, and replica-affinity routing. Those still belong to the runtime/model adapter that sends the request. MemGraphRag should store hashed or normalized cache-lineage identifiers and diagnostics, not raw cached prompt prefixes or sensitive provider cache keys.

A minimal implementation path would be:
1. Extend `tsift-memory` event capture with a prompt-cache observation payload derived from `session-cost` output.
2. Extend `project_memory_events` so those observations become graph nodes/edges in the existing `GraphProjection` path.
3. Refresh `memory status`/readiness so it can report whether prompt-cache telemetry is present in graph retrieval.
4. Add graph-db related/evidence fixtures for cache invalidation queries.
5. Keep the provider adapter boundary explicit: graph retrieval explains observed cache behavior, while the runtime remains responsible for preventing misses.

Changed paths: none in this answer cycle.

Commands:
- `agent-doc preflight tasks/software/tsift.md`
- `agent-doc plan tasks/software/tsift.md`
- `agent-doc session-check tasks/software/tsift.md`
- `rg -n "MemGraphRag|MemGraph|memgraph|RAG|rag" .` in `src/tsift`
- `sed -n` source reads for `packages/tsift-cli/src/cli.rs`, `packages/tsift-cli/src/commands/memory.rs`, and `packages/tsift-sqlite/src/lib.rs`
- `git status --short` in the parent repo
- Official provider docs lookup for OpenAI, Anthropic, Z.AI, and OpenCode
- `rg -n "prompt cache|prompt_cache|cache" ...` in `src/tsift` and the fetched Codex manual
- `node /home/brian/.codex/skills/.system/openai-docs/scripts/fetch-codex-manual.mjs`
- `agent-doc read /home/brian/work/btakita/agent-loop/tasks/software/tsift.md` via MCP
- `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/software/tsift.md` via MCP
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/software/tsift.md` via MCP
- `agent-doc --version`
- `tsift status` in `/home/brian/work/btakita/agent-loop`
- Codex manual fetched via the OpenAI docs skill: `/tmp/openai-docs-cache/codex-manual.md`
- OpenCode provider configuration: https://opencode.ai/docs/providers/
- Z.AI Context Caching: https://docs.z.ai/guides/capabilities/cache
- Anthropic Prompt Caching: https://platform.claude.com/docs/en/build-with-claude/prompt-caching
- OpenAI Prompt Caching: https://developers.openai.com/api/docs/guides/prompt-caching
- `provider_cache_line_id`: hash of the provider, model, endpoint family, org/account boundary, retention/cache-key policy, tokenization-sensitive prefix bytes, and tool schema hash. This is not portable; it is the per-provider cache target.
- `shared_prefix_artifact_id`: hash of the canonical prompt blocks after redaction/normalization. This is portable across harnesses.
- Optional warmup scheduling: warm each provider/model cache line separately, using the same stable prefix manifest, without pretending the provider caches are the same storage object.
- Shared analytics: normalize `cached_input_tokens`, cache-creation/write tokens, read/create ratio, cached ratio trend, invalidation diagnostics, and the turn boundary where reuse dropped.
- Adapter hints: Anthropic gets `cache_control` placement, OpenAI gets a stable `prompt_cache_key` and retention setting when the runtime can pass them, Z.AI/GLM gets stable repeated system/context placement and cached-token observation, and OpenCode passes through only what its chosen provider adapter supports.
- A provider/model cache-line identity: `{provider, endpoint, model, account scope, retention policy, stable_prefix_hash, tool_schema_hash, provider_cache_key_or_breakpoint_hash}`.
- Stable prefix artifacts: canonical ordered blocks for repo instructions, tool schemas, runbooks, document context, and durable examples, each with content hashes.

Verification: no code changed for this answer cycle, so no additional build/test suite was run. The answer was checked against the existing `#memgraphrag2` projection and `#memgraphrag-ont` ontology code paths.

Escalation: none.

Confidence: high.

### Re: cross-provider prompt cache — gpt-5

> **Queue prompt:**
>
> Can we share the prompt cache across models and providers (Claude Code, Codex, Opencode/GLM)?


Not as one shared server-side prompt cache. Yes as a shared cache-control and telemetry layer.

The live cache entries are provider/model-local. OpenAI routes requests by a prompt-prefix hash plus optional `prompt_cache_key`, and its docs say prompt caches are not shared between organizations. Anthropic's API cache is driven by `cache_control` breakpoints over the `tools`, `system`, and `messages` prefix, with provider-owned 5-minute/1-hour TTL behavior. Z.AI/GLM describes automatic context caching with cached-token accounting. Those mechanisms are not portable KV tensors: model weights, tokenization, endpoint routing, retention policy, account/org scope, and provider privacy boundaries all differ.

What tsift can share across Claude Code, Codex, OpenCode/GLM, and future runtimes is the provider-neutral cache lineage:
- Stable prefix artifacts: canonical ordered blocks for repo instructions, tool schemas, runbooks, document context, and durable examples, each with content hashes.
- A provider/model cache-line identity: `{provider, endpoint, model, account scope, retention policy, stable_prefix_hash, tool_schema_hash, provider_cache_key_or_breakpoint_hash}`.
- Adapter hints: Anthropic gets `cache_control` placement, OpenAI gets a stable `prompt_cache_key` and retention setting when the runtime can pass them, Z.AI/GLM gets stable repeated system/context placement and cached-token observation, and OpenCode passes through only what its chosen provider adapter supports.
- Shared analytics: normalize `cached_input_tokens`, cache-creation/write tokens, read/create ratio, cached ratio trend, invalidation diagnostics, and the turn boundary where reuse dropped.
- Optional warmup scheduling: warm each provider/model cache line separately, using the same stable prefix manifest, without pretending the provider caches are the same storage object.

For Codex specifically, the public Codex manual shows model/provider selection and usage records with `cached_input_tokens`, but I did not find a public Codex CLI/App control surface for directly setting OpenAI API prompt-cache parameters. So Codex should be treated as observable first: preserve stable prompt prefixes, record cached-input usage, and only emit provider controls where the runtime adapter exposes them.

The implementation shape I would use in tsift is two IDs:
- `shared_prefix_artifact_id`: hash of the canonical prompt blocks after redaction/normalization. This is portable across harnesses.
- `provider_cache_line_id`: hash of the provider, model, endpoint family, org/account boundary, retention/cache-key policy, tokenization-sensitive prefix bytes, and tool schema hash. This is not portable; it is the per-provider cache target.

That gives us cross-runtime coordination without violating provider boundaries. A Claude Code, Codex, and OpenCode/GLM run can all agree that they are using the same stable prefix artifact, while analytics still report separate cache lines for Anthropic, OpenAI, and Z.AI. A miss then becomes diagnosable: the shared prefix changed, the tool schema changed, the model/provider changed, the cache key/retention changed, or routing/replica affinity was lost.

Sources checked:
- OpenAI Prompt Caching: https://developers.openai.com/api/docs/guides/prompt-caching
- Anthropic Prompt Caching: https://platform.claude.com/docs/en/build-with-claude/prompt-caching
- Z.AI Context Caching: https://docs.z.ai/guides/capabilities/cache
- OpenCode provider configuration: https://opencode.ai/docs/providers/
- Codex manual fetched via the OpenAI docs skill: `/tmp/openai-docs-cache/codex-manual.md`

Changed paths: none outside this session document.

Commands:
- `tsift status` in `/home/brian/work/btakita/agent-loop`
- `agent-doc --version`
- `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/software/tsift.md` via MCP
- `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/software/tsift.md` via MCP
- `agent-doc read /home/brian/work/btakita/agent-loop/tasks/software/tsift.md` via MCP
- `node /home/brian/.codex/skills/.system/openai-docs/scripts/fetch-codex-manual.mjs`
- `rg -n "prompt cache|prompt_cache|cache" ...` in `src/tsift` and the fetched Codex manual
- Official provider docs lookup for OpenAI, Anthropic, Z.AI, and OpenCode

Verification: no code changed, so no build/test suite was rerun. The answer was checked against the current provider docs and the existing tsift prompt-cache diagnostics context in the session.

Escalation: none.

Confidence: high.
<!-- agent:boundary:808e2ddb -->
<!-- agent:boundary:b84388e3 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" go -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
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
