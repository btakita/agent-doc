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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260608-021521.md`*

Compacted content:
- Archived 1 response topic(s): release + publish
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: source-read windows vs graph — gpt-5

The `--start` / `--lines` usages I found are generated expansion windows, not evidence that the agent is skipping the semantic graph.

In the `agent-doc-bugs2.md` session, the compacted markdown body does not contain older hand-written `source-read --start --lines` calls. The current `tsift context-pack` for that session does two things side by side:

- `graph_orchestration` reports the semantic graph projection as ready/current and records relationship evidence, for example the session context mentioning symbols such as `project_controller`, `queue_command`, `queue_dispatch`, `codex_hook`, and `state_store`.
- `exploration.source_windows` then emits bounded follow-up commands such as `tsift --envelope source-read "tasks/agent-doc/agent-doc-bugs2.md" --style window --start 1 --lines 96 --budget normal` for the concrete touched files.

So the intended flow is: graph/search/session-review find and rank the relevant nodes, then `source-read --start/--lines` gives a small, line-numbered text window when the agent needs literal source or markdown context. That is still better than `cat`, `sed`, or full-file reads because it is bounded and carries AST/markdown refs when available.

The code path matches that interpretation. `build_context_pack_exploration_packet` builds `relationship_map` first, then adds `source_windows` for changed/touched files. `context_exploration_source_window` currently starts those touched-file windows at line 1 and uses the normal budget line count. The formatter for those expand commands is `source_read_window_command`, which deliberately includes `--style window --start <n> --lines <n>`.

The real rough edge is not that `--start` / `--lines` exists. The rough edge is that for broad markdown/session touched-file entries, context-pack only has a file-level reason, so it emits a line-1 preview instead of a more semantic section/block/symbol target. For code symbols, the better path is still `symbol-read` / graph expansion first; for markdown sessions, tsift needs stronger section/block anchors if we want those expansion commands to stop looking like line slicing.

Changed paths: none.

Commands: `tsift status`, `tsift status --fix`, `agent-doc preflight tasks/software/tsift.md`, `tsift --envelope session-review tasks/agent-doc/agent-doc-bugs2.md --next-context --budget normal`, `tsift --envelope context-pack tasks/agent-doc/agent-doc-bugs2.md --budget normal`, `rg` for `source-read` / `--start` / `--lines`, `tsift --envelope search`, `tsift --envelope symbol-read`, `tsift graph`, `agent-doc plan tasks/software/tsift.md`.

Verification: no code changed. The explanation is verified against the live `context-pack` output and the tsift implementation paths that generate the `source_windows` expansion commands.

Confidence: high.

Escalation: none.
Would it be better or worse to edit source files using the AST?

### Re: AST edits — gpt-5

Usually better as a targeted internal primitive, worse as the default editing surface.

For tsift, AST is strongest when it can identify intent and boundaries: “replace this function body,” “insert this method next to this symbol,” “rename this binding,” “add an import only if missing,” or “preserve the enclosing module while changing one node.” That would be materially better than line-window editing because it can avoid off-by-one edits, preserve nearby unrelated text, and produce clearer provenance: symbol handle -> AST node -> rewrite.

It gets worse if AST editing becomes the only path. Real source files contain comments, formatting style, macros, generated sections, mixed-language blocks, syntax errors during work-in-progress, and build-tool-specific trivia that many AST libraries either drop or normalize. A lot of safe agent edits are textual patches that deliberately preserve everything outside a tiny region. If an AST rewriter reformats too much or cannot represent partially invalid code, it raises blast radius.

So the practical answer is hybrid:

- Use AST for selection, validation, and structural guardrails.
- Prefer concrete patch output for the actual file mutation unless the parser has a lossless concrete syntax tree for that language.
- Promote AST-native rewrites only for operations with a narrow, proven contract, such as import insertion, symbol rename, moving declarations, function-body replacement, or structured markdown block updates.
- Always show the resulting diff and run normal verification; AST success is not proof the edit is semantically correct.

For tsift specifically, I would treat AST editing as an eventual capability layered after source-read/symbol-read/search, not a replacement for them. The best path is probably: graph/search identifies the symbol, source-read gives bounded literal context, an AST/CST edit primitive proposes a small patch, and the final patch is applied through the same diff-aware workflow agents already understand.

Changed paths: none.

Commands: `tsift status`, `agent-doc --version`, `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`, `agent-doc read /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`, `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`, `agent-doc reset --from-current /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`, `agent-doc write --template --stream --force-disk --origin skill --commit /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`.

Verification: no code changed; this is a design tradeoff response only.

Confidence: high.

Escalation: none.
❯ Add backlog items to complete the strategy you discussed.

### Re: AST edit backlog — gpt-5

Added backlog items in priority order to carry the AST/CST editing strategy from design into implementation.

Top backlog item: `#tsiftasteditspec`.

Changed paths: `tasks/software/tsift.md`.

Commands: `tsift status`, `agent-doc --version`, `agent-doc preflight /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`, `agent-doc session-check /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`, `agent-doc plan /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`, `agent-doc read --component backlog /home/brian/work/btakita/agent-loop/tasks/software/tsift.md`, `agent-doc finalize /home/brian/work/btakita/agent-loop/tasks/software/tsift.md --baseline-file /home/brian/work/btakita/agent-loop/.agent-doc/baselines/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c.md --origin skill --stream --force-disk`.

Verification: no code changed; verified the backlog was empty before adding the ordered items. `tsift status` still reports workspace index staleness from unrelated workspace scope state, but this turn did not rely on tsift search/explain/graph results.

Confidence: high.

Escalation: none.
<!-- agent:boundary:83511cf4 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue preset="#spec-test-build-install-commit-push" -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog priority -->
- [ ] [#tsiftasteditspec] Update `src/tsift/SPEC.md` to define the hybrid AST/CST edit strategy: AST for selection/validation, minimal textual patch output for mutation, refusal modes, diff visibility, and verification requirements.
- [ ] [#tsiftastselect] Prototype AST/CST edit-target selection that maps `search` / `symbol-read` / graph handles to concrete node spans without mutating files.
- [ ] [#tsiftastpatch] Implement a patch-proposal interface for AST/CST operations that emits minimal diffs, preserves comments/formatting/trivia where supported, and refuses unsupported parser states.
- [ ] [#tsiftastops] Add the first narrow AST/CST-backed edit operations, starting with import insertion and function-body replacement before broader rename/move operations.
- [ ] [#tsiftasttests] Add fixtures and tests for the AST/CST edit path covering comments, formatting preservation, macros or generated sections, mixed-language blocks, syntax-error work-in-progress files, and verification failures.
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#tsiftinitboost] Rerun plain `tsift init` in `src/boost-client` and `src/boost-client/src/monsterrodholders-dev` after the active monsterrodholders workflow stops rebasing/resetting those worktrees; commit their `AGENTS.md` updates and any resulting pointers.
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
