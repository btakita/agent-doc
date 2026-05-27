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
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->

Consumed the OpenCode next-steps queue prompt; #gopencode remains the top backlog item.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260527-173928.md`*

Compacted content:
- Archived 7 response topic(s): #gfullhot; closeout repair; #gsqlstage; 4 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Please evaluate the docs + code on https://github.com/aovestdipaperino/tokensave
❯ Is there anything that tokensave more effectively than tsift? Anything that tokensave covers that tsift does not?
### Re: tokensave vs tsift — gpt-5

I inspected upstream `tokensave` at commit `e03f89cddcd4904d9230f95a25106b943531858d` and compared it against the local `src/tsift` spec/code. Short version: yes, `tokensave` covers several areas that `tsift` does not, and it is probably more effective for broad, general-purpose MCP code intelligence in arbitrary repositories. It is not clearly stronger than `tsift` for this project's core use case: bounded agent-doc/session evidence, graph-backed dispatch planning, transcript/test/log digests, and operator-grade graph proof.

Sources inspected: [`tokensave` README](https://github.com/aovestdipaperino/tokensave/blob/e03f89cddcd4904d9230f95a25106b943531858d/README.md), [`tokensave` tool definitions](https://github.com/aovestdipaperino/tokensave/blob/e03f89cddcd4904d9230f95a25106b943531858d/src/mcp/tools/definitions.rs), [`tokensave` language registry](https://github.com/aovestdipaperino/tokensave/blob/e03f89cddcd4904d9230f95a25106b943531858d/src/extraction/mod.rs), [`tokensave` extraction worker](https://github.com/aovestdipaperino/tokensave/blob/e03f89cddcd4904d9230f95a25106b943531858d/src/extraction_worker.rs), local `src/tsift/SPEC.md`, `src/tsift/src/main.rs`, `src/tsift/src/lang/mod.rs`, and `src/tsift/src/index.rs`.

Where `tokensave` is more effective:

- Native MCP product surface. `tokensave serve` exposes a real MCP server, resources, tool annotations, always-loaded core tools, and a dynamic `tokensave tool ...` CLI wrapper over the same MCP handlers. `tsift` is mainly a CLI/hook/rewrite system; I did not find an equivalent native MCP server implementation in `src/tsift`.
- Agent integration breadth. `tokensave install --agent ...` covers Claude, Codex, OpenCode, Cursor, Gemini, Copilot, Cline/Roo, Kiro, Zed, Antigravity, Kilo, Kimi, and Vibe, with tests that preserve existing configs. `tsift init --codex` and Claude hook guidance are useful, but much narrower.
- Language coverage. `tokensave` defaults to a full tier with many extractors: the registry covers Rust, Go, Java, Scala, TypeScript/JavaScript, Python, C/C++, C#, Kotlin, Swift, Dart, PHP, Ruby, Bash, Proto, PowerShell, Nix, Lua, Zig, ObjC, Perl, Batch, Fortran, COBOL, Basic variants, Dockerfile, shader languages, Markdown, R, SQL, Julia, Haskell, OCaml, Clojure, Erlang, Elixir, F#, Lean, and TOML. `tsift` currently covers Rust, Python, TypeScript/TSX, JavaScript/JSX, Kotlin, Zig, Bash, and Markdown.
- Richer code-intelligence tool catalog. `tokensave` has first-class tools for type hierarchy, implementations, derives, constructors, field read/write sites, signature search, unsafe-pattern scanning, diagnostics, affected-test execution, source-to-test mapping, porting order/status, branch search/diff/list, and many code-health metrics. `tsift` has some overlap through `graph`, `impact`, `communities`, `graph-db`, and digests, but not this broad API catalog.
- MCP edit primitives. `tokensave_str_replace`, `tokensave_multi_str_replace`, `tokensave_insert_at`, `tokensave_replace_symbol`, `tokensave_insert_at_symbol`, and optional `ast-grep` rewrite are exposed as tool calls and re-index the touched file afterward. `tsift edit` has staged multi-file search/replace behavior, but not this MCP-native, symbol-addressed edit surface.
- Multi-branch indexing. `tokensave` can maintain per-branch databases, query another tracked branch, diff symbol graphs between branches, and fall back to nearest ancestor branch DBs. `tsift` has stronger submodule/workspace scoping, but not comparable branch-aware graph DBs.
- Crash isolation for broad grammar support. `tokensave` runs extraction in authenticated worker subprocesses so a native tree-sitter grammar abort/segfault skips one file instead of killing sync. `tsift` keeps a smaller grammar set and indexes in-process. If `tsift` grows into the same long-tail language set, this isolation pattern is worth copying.
- Product observability. `tokensave` has an MCP savings ledger, `gain`, `cost`, `monitor`, session start/end health snapshots, and a global/local token counter story. `tsift` has better bounded transcript cost/session review for Claude/Codex/agent-doc logs, but not a live MCP usage monitor.

Where `tokensave` covers things `tsift` does not:

- True MCP resources: `tokensave://status`, `tokensave://files`, `tokensave://overview`, and `tokensave://branches`.
- Broad language extractors and language-tier packaging (`lite`, `medium`, `full`).
- Cross-branch symbol search/diff/list.
- Code-health analytics: composite health score, Gini, dependency depth, DSM, redundancy, test-risk.
- Porting tools for source/target symbol parity and topological port order.
- Persistent cross-session memory via `record_decision`, `record_code_area`, and `session_recall`.
- Symbol-aware write tools as MCP calls.
- Runtime telemetry for the running MCP server.

Where `tsift` remains stronger or more specialized:

- Agent-doc/session workflow. `tsift context-pack`, `session-review`, `session-digest`, `session-cost`, `diff-digest`, `test-digest`, `log-digest`, and `rewrite --run` are directly tuned for avoiding raw transcript, diff, test, and build log replay. I did not see an equivalent `tokensave` surface for agent-doc closeout evidence, prompt-tail recovery, or harness transcript compaction.
- Graph orchestration/proof. `tsift graph-db evidence`, `related`, `doctor`, `drift`, `compact`, `convex-sync`, `backend-eval`, `conflict-matrix`, `dispatch-trace`, and `dependency-dag` form a much deeper operator/replay substrate than `tokensave`'s code graph. `tokensave` is broad code intelligence; `tsift` is a graph-backed workflow control plane.
- Provider-neutral graph promotion discipline. `tsift` has explicit SQLite/Convex snapshot parity, backend promotion gates, full-projection evidence, lock behavior checks, and performance history fixtures. `tokensave` uses local libSQL and has benchmarks, but I did not see equivalent backend parity/gating machinery.
- Submodule/workspace isolation. `tsift`'s per-submodule indexes, scope ambiguity failure, federation controls, and tagpath alias normalization are more aligned with this `agent-loop` monorepo than `tokensave`'s project/branch model.
- Real semantic-row retrieval. `tokensave` markets semantic code intelligence, but the inspected search/context path is mostly FTS/exact-name plus graph expansion; there is a `vectors` table but I did not find implemented embedding generation/search. `tsift` has cached LLM summary/concept/entity rows and `graph-db related` built around those semantic rows.

What I would consider stealing for `tsift`, if we decide to expand scope:

- A native MCP server wrapper around the existing `tsift` envelope commands and `graph-db` surfaces.
- More agent installers, especially Codex/OpenCode/Cursor config writers that preserve existing config.
- Subprocess-isolated extraction before adding many more tree-sitter grammars.
- Branch-aware index snapshots, if cross-branch code review becomes a recurring workflow.
- Symbol-addressed edit primitives, but only if they preserve `tsift edit`'s current multi-file atomicity expectations.

Caveats: I did not build or run `tokensave`; this was a docs-and-code inspection. No code changed, so I did not run the project verification suite. Worktree note: existing modified paths were present before response persistence (`src/tsift` submodule pointer state, `tasks/agent-doc-bugs2.done.md`, `tasks/software/tsift.done.md`, and this session document); I did not stage or change non-session files.
❯ What are #next-steps to integrate opencode?
### Re: OpenCode integration next steps — gpt-5

Top backlog item: #gopencode.

The next step is to make OpenCode a first-class `tsift` consumer instead of treating it as a one-off harness edge case. I added #gopencode ahead of #gbackendproof because OpenCode integration affects daily agent workflow directly; optional graph backend promotion can stay behind it.

Priority order:

1. Define the OpenCode integration contract for `tsift`: which commands should be surfaced to OpenCode, which hooks/config files are installed, and what evidence proves the integration works.
2. Add or document an OpenCode setup path, likely mirroring the existing Codex/Claude hook guidance: install config, preserve user config, and expose `tsift` envelope commands for session review, context packs, diff digests, test digests, and log digests.
3. Verify transcript and permission-prompt capture under OpenCode. The related `tasks/agent-doc/open-code.md` notes point at HITL prompt input issues, so `tsift` integration should explicitly cover prompt/log capture and avoid depending on raw terminal replay.
4. Add deterministic tests for generated OpenCode config and supported rewrite/envelope command routing.
5. Build/install locally and run one end-to-end OpenCode smoke session through `agent-doc`, then close with `agent-doc session-check` evidence.

No repo code changed in this response; this is backlog capture and prioritization only.
### Re: OpenCode integration next steps — gpt-5


Top backlog item: #gopencode.

No additional backlog item is needed for this queue pass. The OpenCode integration work is already captured as #gopencode and is correctly ordered ahead of #gbackendproof because it unblocks daily harness use before optional graph backend experiments.

The next actionable sequence remains:

1. Define the OpenCode integration contract for `tsift`: config files, installed hooks, command shortcuts, and proof criteria.
2. Add or document an OpenCode setup path that preserves existing user config and exposes `tsift` envelope workflows for session review, context packs, diff digests, test digests, and log digests.
3. Verify OpenCode transcript capture and permission-prompt diagnostics, especially around HITL prompt navigation.
4. Add deterministic tests for generated OpenCode config and command routing.
5. Build/install locally, run an end-to-end `agent-doc start opencode ...` smoke session, and close with `agent-doc session-check` evidence.
### Re: OpenCode integration next steps — gpt-5


Top backlog item: #gopencode.

The queue prompt is now fully answered by the existing #gopencode backlog item. I am not adding another item because that would duplicate the current top priority.

Priority order remains:

1. #gopencode — make OpenCode a first-class `tsift` integration, including installer/config docs, digest shortcuts, transcript and permission-prompt diagnostics, deterministic tests, local build/install, and an end-to-end OpenCode smoke session.
2. #gbackendproof — evaluate optional graph backends only after OpenCode integration is not blocking daily workflow.
<!-- agent:boundary:3f001905 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to make the graph faster?
---
What are #next-steps to make the graph + neighborhoods faster + more effective?
-->

<!-- agent:queue auto -->
- What are #next-steps to integrate opencode?
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#gopencode] [tag: opencode] [tag: integration] Make OpenCode a first-class tsift integration: add installer/config docs, envelope shortcuts for session/log/test/diff digest use, verify OpenCode transcript capture and permission-prompt diagnostics, add deterministic tests, then build/install and run an end-to-end agent-doc OpenCode smoke session.
- [ ] [#gbackendproof] [tag: graph] [tag: performance] Evaluate FalkorDB/Kuzu only as native optional GraphStore adapters: require provider-neutral projection writes/load, freshness/parity, lock behavior, install portability, and faster real/full-projection evidence/conflict/dispatch/path metrics than SQLite before promotion.
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
