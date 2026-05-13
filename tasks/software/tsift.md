---
agent_doc_session: tsift-v0.1
resume: 019dd526-7958-77e3-9a41-92b8944fc550
agent: codex
claude_model: claude-opus-4-6
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
---

## Status

<!-- agent:status patch=replace -->
Project created. Repo: https://github.com/btakita/tsift. Submodule: src/tsift. Spec extracted to src/tsift/SPEC.md.
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260513-043627.md`*

Compacted content:
- Archived 1 response topic(s): #snav
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Review the logs for tsift effectiveness + issues. #next-steps
### Re: tsift log review — gpt-5

Reviewed the tsift-related logs with `log-digest`, `session-digest`, `session-review`, `session-cost`, `context-pack`, and `locks`.

Effectiveness: `log-digest` is now useful on `.agent-doc/logs/tsift-v0.1.log`: it found 1,512 non-empty lines, 3 file-ref groups, 143 symbol-ref groups, and no warning/error signal lines. That is a concrete improvement over the earlier file-anchor issue. `session-digest` on the same log also stayed cleaner than the aggregate review: 0 prompt targets, concrete restart/timeout failures, 43 runtime-event groups, and closeout/restart churn summaries.

Main issues: `session-review` and `context-pack --next-context` still overreach. The aggregate review matched 12 sessions, but then surfaced stale historical prompts, prompt-preset text, examples, unrelated file paths, and `/!` as context. Its failure extraction is also too broad: instruction text like the finalize rule, source snippets like `_ => panic!(...)`, generic exit strings, and old test failures are mixed together as active failures. That makes next-context noisy even though the lower-level `session-digest` for the current log had 0 prompt targets.

Operational signals: `session-cost` found restart/no-op churn in the tsift session log: 28 fresh restarts, 8 ctrl-D restart-loop events, 5 auto-trigger timeouts, and 123 `commit_already_current` events. The wider review also reported `commit_already_current` 4306 times. `tsift locks` reported no writer lock for the root index but did find live WAL/SHM sidecars and recommended inspecting for a wedged writer before reindexing.

I added five backlog items in priority order. Top backlog item: `#sctx`.
<!-- agent:boundary:648d8966 -->
<!-- /agent:exchange -->
###
<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#sctx] Fix tsift session-review / context-pack --next-context active-context scoping so prompt targets, files, and failures come from the current unresolved tail instead of historical assistant text, presets, examples, or unrelated matched sessions; regression should reject stale do-hash prompts, Re summaries, prompt-preset text, and bogus paths like /! from tasks/software/tsift.md review output.
- [ ] [#sfail] Tighten tsift session-review failure extraction so it reports command/test/log failures with command and session anchors, not instruction text or source snippets like After finalize, panic snippets, or generic command exited with code 1; preserve real failing assertions and cargo exits separately.
- [ ] [#stok] Audit Codex/Claude token accounting in tsift session-review and session-cost for cumulative-usage double counting and cross-session duplication; the 12-session tasks/software/tsift.md review reported about 1.4B prompt tokens and a 1.4M-token largest turn, which needs a deterministic fixture.
- [ ] [#slock] Add deterministic lock-contention coverage for tsift index/search/status paths and recovery guidance; historical review surfaced 34 database file is locked failures while tsift locks can report no writer lock with live WAL/SHM sidecars, so snapshot fallback and diagnostics need regression coverage.
- [ ] [#sfile] Filter empty or non-file file references from log-digest / session-digest; .agent-doc/logs/tsift-v0.1.log produced an empty path file group with 132 occurrences, degrading anchors and downstream next-context file lists.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

## Completed / Reaped
k
<!-- agent:done -->
- 2026-05-12 [#686r] Add a DCI-style tsift benchmark: compare exact/chained rg, lexical BM25, and hybrid search on multi-hop code/repo tasks, tracking localization, tool calls, latency, and token budget.
- 2026-05-12 [#s1k1] Add a composable search workflow surface or documented recipes that preserve result handles across search --exact, search, explain, summarize, and digest expansion steps.
- 2026-05-12 [#v6y3] Add breadth/scale guards to search envelopes: high-hit warnings, corpus-size/tool-budget signals, and concrete next-narrowing commands before agents fan out too broadly.
- 2026-05-12 [#sflt] Fix session-review/session-digest failure classification so successful test summaries and generic instruction text like `failures:` are not reported as unresolved failures; preserve real failing assertions/exits with command and session anchors.
- 2026-05-12 [#sprm] Tighten `tsift session-review --next-context` prompt-target extraction so frontmatter prompt presets, examples, archived/resolved prompts, and repeated historical directives do not appear as active prompt targets.
- 2026-05-13 [#t275] [recommended] Prototype a tsift-local deterministic SimWorld for session/rewrite/status state-machine coverage before extracting any shared simulation crate.
- 2026-05-13 [#sops] Teach `tsift log-digest` to parse structured agent-doc `file=...`, event, pane, and session fields as anchors; the 99,637-line ops log currently reports `file_ref_groups=0` and `no file anchors detected` despite path-bearing records.
- 2026-05-13 [#snav] Add a status/release regression proving `tsift status --fix` refreshes stale Code Navigation instructions after a binary version bump; current local status reports instructions at 0.1.41 while the installed binary is 0.1.42.
<!-- /agent:done -->

## Completed / Reaped

<!-- agent:pending-done -->
- 2026-05-09 [#ctxp] Add context-pack to the token-savings fixture/regression, including diff/test/log/next-context inputs and a fail-under threshold for its envelope preview.
- 2026-05-10 [#real] Add a real-session token-savings benchmark from recent tsift/agent-doc transcripts, covering session-review and context-pack paths that dominate actual prompt volume.
- 2026-05-10 [#budg] Tune the response-budget presets and auto caps against measured token-savings data so envelope previews stay compact without hiding required follow-up commands.
<!-- /agent:pending-done -->

<!--
-->
