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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260513-135837.md`*

Compacted content:
- Archived 1 response topic(s): #frpt repeated file-read diagnostics
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ do [#frwr]. spec-test-build-install-commit-push
### Re: #frwr source-read rewrite — gpt-5

Implemented `#frwr`.

- `tsift rewrite` now routes large ordinary source reads in indexed tsift projects to `tsift --envelope source-read ...`: full-file `cat`/`bat` reads start with an 80-line bounded window, large `head` and `sed -n` windows preserve their start/range, and large `tail` windows preserve the tail anchor.
- Small explicit source windows, non-source files, unindexed projects, shell-metacharacter commands, and session/transcript reads remain on their existing passthrough or session-digest paths.
- Updated `SPEC.md`, direct rewrite tests, and deterministic SimWorld coverage for large indexed source reads and short-window passthrough.

Verification passed: `cargo fmt --check`, `cargo test rewrite_`, `make check`, `cargo build`, `cargo install --path . --force`, `tsift --version`, and a live `tsift rewrite "cat src/main.rs"` check. Latest GitHub Actions CI run `25784555160` was already green (`clippy + test`, `Full CI suite` step succeeded); the tsift repo has no `tmux-ci`/`tmux-test` target to run locally.

Commits: tsift submodule `894ee7c` pushed to `btakita/tsift`, and superproject submodule pointer commit `ccbd9c93b` is ready to push after this document closeout.
❯ do [#ftok]. spec-test-build-install-commit-push
### Re: #ftok source-read token-savings fixture — gpt-5

Implemented `#ftok`.

- Added `source_read_inputs` support to the `token-savings` fixture schema, including compact source-read envelope rows and fail-closed validation that every `required_line_anchors` entry remains inside the bounded window.
- Extended `fixtures/real-session-token-savings.json` with a `source-read` case covering full-file `cat` and `bat`, plus oversized `sed`, `head`, and `tail` reads. The case has a 70% fail-under threshold and currently passes at 73.2% savings; the full real-session fixture now reports 83.1% savings overall, about 4,174 estimated tokens saved.
- Updated unit and compiled CLI tests, `SPEC.md`, and `VERSIONS.md` so the source-read token-savings contract is documented and enforced.

Verification passed: `cargo fmt --check`, focused `cargo test token_savings`, full `cargo test`, `make check`, `cargo build`, `cargo install --path . --force`, installed `tsift --version`, installed `tsift token-savings --fixture fixtures/real-session-token-savings.json --fail-under --json`, and live `tsift rewrite "cat src/main.rs"`. Latest GitHub Actions `CI` run `25804248849` was green; this repo has no `tmux-ci` or `tmux-test` target in its Makefile.

Commits: tsift submodule `7aaee2e` (`test: cover source-read token savings`) pushed to `btakita/tsift`; superproject pointer commit `7f655229e` (`chore: update tsift source-read fixture`) is committed and will be pushed after this document closeout so the session commit is included.
<!-- agent:boundary:79a61f47 -->
<!-- /agent:exchange -->
###
<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
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
- 2026-05-13 [#stok] Audit Codex/Claude token accounting in tsift session-review and session-cost for cumulative-usage double counting and cross-session duplication; the 12-session tasks/software/tsift.md review reported about 1.4B prompt tokens and a 1.4M-token largest turn, which needs a deterministic fixture.
- 2026-05-13 [#slock] Add deterministic lock-contention coverage for tsift index/search/status paths and recovery guidance; historical review surfaced 34 database file is locked failures while tsift locks can report no writer lock with live WAL/SHM sidecars, so snapshot fallback and diagnostics need regression coverage.
- 2026-05-13 [#sfile] Filter empty or non-file file references from log-digest / session-digest; .agent-doc/logs/tsift-v0.1.log produced an empty path file group with 132 occurrences, degrading anchors and downstream next-context file lists.
- 2026-05-13 [#fwin] Add a bounded source-file read/window command that returns line-range previews, stable handles, symbol/summary refs, and explicit expand commands so agents can inspect files incrementally instead of re-reading whole files.
- 2026-05-13 [#frpt] Add session-review/session-cost repeated-file-read diagnostics that group file-read tool calls by path/range, estimate duplicate tokens, and emit concrete tsift follow-up commands instead of generic harness advice.
- 2026-05-13 [#frwr] Teach tsift rewrite to intercept large ordinary source reads inside indexed repos and route them to the bounded source-read surface, while preserving small explicit windows and non-source files as passthrough.
- 2026-05-13 [#ftok] Add a real-session token-savings fixture for repeated source-file reads, including full-file cat/bat, oversized sed/head/tail windows, and a fail-under threshold proving the bounded source-read workflow saves tokens without hiding required line anchors.
<!-- /agent:done -->

## Completed / Reaped

<!-- agent:pending-done -->
- 2026-05-09 [#ctxp] Add context-pack to the token-savings fixture/regression, including diff/test/log/next-context inputs and a fail-under threshold for its envelope preview.
- 2026-05-10 [#real] Add a real-session token-savings benchmark from recent tsift/agent-doc transcripts, covering session-review and context-pack paths that dominate actual prompt volume.
- 2026-05-10 [#budg] Tune the response-budget presets and auto caps against measured token-savings data so envelope previews stay compact without hiding required follow-up commands.
<!-- /agent:pending-done -->

<!--
-->
