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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260513-052929.md`*

Compacted content:
- Archived 2 response topic(s): tsift log review; #sctx
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ do [#sfail]. spec-test-build-install-commit-push
### Re: #sfail — gpt-5

Implemented `#sfail` in `src/tsift` and marked it done.

What changed:
- `session-digest` failure rows now carry an optional parsed `command` anchor, and Codex/Claude tool output failure lines inherit the command that produced them.
- `session-review` failure rows now carry optional `command` and `session_path` anchors, including budget/envelope previews and human output.
- Failure extraction now filters active prompt directives, instruction/meta prose such as `After finalize...`, source snippets like `panic!(...)`, and unknown-command generic exits, while preserving real panic/assertion lines and named command exits such as `cargo test exited with code 1`.
- Updated `SPEC.md`, `VERSIONS.md`, and regression coverage for both digest-level filtering and review-level command/session anchors.

Verification:
- `cargo test session_digest::tests::codex_jsonl_digest_anchors_command_failures_and_filters_instruction_snippets` passed.
- `cargo test session_review::tests::session_review_failure_rows_keep_command_and_session_anchors` passed.
- `make check` passed after all edits: clippy, 567 unit tests, and 95 compiled CLI tests.
- `cargo build --release` passed.
- `cargo install --path .` replaced the local binary, and `tsift --version` reports `tsift 0.1.42`.
- Latest GitHub Actions `CI` for `src/tsift` was green, and root `Agent Doc` run `25780225104` had `tmux-ci` and `check` green.

Commits pushed:
- `btakita/tsift@48d18c3` (`fix: anchor session failure extraction`)
- `btakita/agent-loop@20441a6e1` (`chore: update tsift submodule`)
<!-- agent:boundary:f9a22476 -->
<!-- /agent:exchange -->
###
<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
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
- 2026-05-13 [#sctx] Fix tsift session-review / context-pack --next-context active-context scoping so prompt targets, files, and failures come from the current unresolved tail instead of historical assistant text, presets, examples, or unrelated matched sessions; regression should reject stale do-hash prompts, Re summaries, prompt-preset text, and bogus paths like /! from tasks/software/tsift.md review output.
- 2026-05-13 [#sfail] Tighten tsift session-review failure extraction so it reports command/test/log failures with command and session anchors, not instruction text or source snippets like After finalize, panic snippets, or generic command exited with code 1; preserve real failing assertions and cargo exits separately.
<!-- /agent:done -->

## Completed / Reaped

<!-- agent:pending-done -->
- 2026-05-09 [#ctxp] Add context-pack to the token-savings fixture/regression, including diff/test/log/next-context inputs and a fail-under threshold for its envelope preview.
- 2026-05-10 [#real] Add a real-session token-savings benchmark from recent tsift/agent-doc transcripts, covering session-review and context-pack paths that dominate actual prompt volume.
- 2026-05-10 [#budg] Tune the response-budget presets and auto caps against measured token-savings data so envelope previews stay compact without hiding required follow-up commands.
<!-- /agent:pending-done -->

<!--
-->
