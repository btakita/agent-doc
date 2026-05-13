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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260513-055736.md`*

Compacted content:
- Archived 1 response topic(s): #stok
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ do [#slock]. spec-test-build-install-commit-push
### Re: #slock — gpt-5

Implemented `#slock`.

Added deterministic WAL-without-`index.lock` regressions for the direct `tsift index`, `tsift search`, and `tsift status` paths. The tests now lock the case where SQLite WAL/SHM sidecars are live but the tsift-owned writer sidecar reports no active holder, preserving WAL-aware snapshot fallback and recovery guidance instead of raw `database is locked` failures or rollback-journal-only diagnostics.

Updated `SPEC.md` and `VERSIONS.md` with the recovery invariant, extended the search post-precheck test hook so it can inject either rollback-journal or WAL contention, and installed the rebuilt local binary.

Verification: targeted new regressions passed; `cargo fmt --check`; `make check`; `cargo build`; `cargo install --path .`; `tsift --version` -> `tsift 0.1.42`; latest GitHub Actions CI run `25781168424` was green.

Pushed commits: `src/tsift` `38e16b3` (`test: cover WAL lock recovery paths`) and superproject `c7e9e079e` (`chore: update tsift submodule`).
❯ do [#sfile]. spec-test-build-install-commit-push
### Re: #sfile — gpt-5

Implemented `#sfile`.

`log-digest` and `session-digest` now reject structured runtime path fields when they normalize to an empty display path or point at an existing directory. That removes `cwd_resolved path=<project-root> source=project_root` from file anchors and next-context file lists while preserving real `file=...` / `path=...` document anchors.

Added regressions for both digest paths, updated `SPEC.md` and `VERSIONS.md`, and verified the real `.agent-doc/logs/tsift-v0.1.log` case: both `log-digest` and `session-digest --source agent-doc-log` now report only `tasks/software/tsift.md` and `tasks/tsift.md`, with no empty file group.

Verification: targeted regressions passed; `cargo fmt --check`; `make check`; `cargo build`; `cargo install --path .`; `tsift --version` -> `tsift 0.1.42`; tsift GitHub Actions CI run `25782061824` was green. Superproject `Agent Doc` run `25782075410` had green `tmux-ci`; its separate `check` job remained red on `codex_hook::tests::stop_blocks_when_parent_submodule_pointer_closeout_fails` in `src/agent-doc`, outside the tsift change surface.

Pushed commits: `src/tsift` `9f97992` (`fix: filter non-file digest anchors`) and superproject `1071f58c6` (`chore: update tsift submodule`).
<!-- agent:boundary:4fe0fb83 -->
<!-- /agent:exchange -->
###
<!--
```
Suggestions
ℹ File reads using 15.9k tokens (8%) → save ~4.8k
  If you are re-reading files, consider referencing earlier reads. Use offset/limit for large files.
```
spec-test-build-install-commit-push
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
<!-- /agent:done -->

## Completed / Reaped

<!-- agent:pending-done -->
- 2026-05-09 [#ctxp] Add context-pack to the token-savings fixture/regression, including diff/test/log/next-context inputs and a fail-under threshold for its envelope preview.
- 2026-05-10 [#real] Add a real-session token-savings benchmark from recent tsift/agent-doc transcripts, covering session-review and context-pack paths that dominate actual prompt volume.
- 2026-05-10 [#budg] Tune the response-budget presets and auto caps against measured token-savings data so envelope previews stay compact without hiding required follow-up commands.
<!-- /agent:pending-done -->

<!--
-->
