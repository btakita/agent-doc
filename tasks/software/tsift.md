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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260513-183202.md`*

Compacted content:
- Archived 1 response topic(s): rwno
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ Evaluate the logs for tsift effectiveness and bugs. #next-steps
### Re: logs review — gpt-5

I evaluated the current tsift-related logs with `tsift 0.1.42` using `session-review`, `session-review --next-context`, `log-digest`, `session-digest`, and `session-cost` against `tasks/software/tsift.md`, `.agent-doc/logs/tsift-v0.1.log`, `.agent-doc/logs/ops.log`, and the latest matching Codex session.

The useful part: tsift is finding real operational signals. `session-digest` on `tsift-v0.1.log` grouped `claude_exit`, `auto_trigger_timeout`, `fresh_restart`, `ctrl_d_restart_loop`, and closeout churn. `ops.log` surfaced repeated `ipc_socket_sidecar_timeout` fallback paths with file anchors, including `tasks/software/tsift.md`. The current session cost view is also much less absurd than the older completed findings: the latest matching review reported `latest_total:162.5K`, and the current session-cost pass rose to `256.2K` after this evaluation work, with cache ratio visible instead of being hidden.

The remaining bugs are still important. `session-review --next-context` missed the live prompt in this document: preflight saw `Evaluate the logs for tsift effectiveness and bugs. #next-steps`, but next-context reported `prompts:0`. It also pulled 12 files and 12 failures from historical related sessions, including unrelated task docs, so active-tail scoping is still not reliable after compact/template boundaries. Failure extraction is also still too eager: some groups are assistant prose or prior closeout status, such as a commit sentence, a JetBrains-guard explanation, and `make check is still running`, rather than actual failing commands or log events.

`log-digest` is weaker than `session-digest` for these logs. On `tsift-v0.1.log`, it reported `signals:0` even though the same file contains the failures and churn listed above. For `ops.log`, it found route/start-miss errors and many file anchors, but most value came through repeated-line and symbol counts rather than event-classified diagnostics. That makes `log-digest` less effective as the first stop for agent-doc operational logs.

I added prioritized follow-ups to the backlog. Top backlog item: `#xctx`.

No repo code changed, so I did not run the tsift test suite.
<!-- agent:boundary:b2c4edcd -->
<!-- /agent:exchange -->
###
<!--
-->

## Backlog

<!-- agent:backlog -->
- [ ] [#xlog] Teach tsift log-digest to classify agent-doc runtime/churn/failure events as signals; current tsift-v0.1.log digest reports signals:0 even though session-digest finds claude_exit, auto_trigger_timeout, fresh_restart, ctrl_d_restart_loop, and closeout churn.
- [ ] [#xcst] Fix session-cost/log guardrail wording for agent-doc logs so restart_loop warnings are only emitted when restart churn is actually present; ops.log currently reports fresh_restart=0 auto_trigger_timeout=0 ctrl_d_restart_loop=0 while still warning restart churn detected.
- [ ] [#xctx] Fix session-review --next-context active-tail scoping after compaction/template boundaries: current tasks/software/tsift.md has a live user prompt, but next-context reports prompts:0 and carries historical files/failures from unrelated sessions.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#9jc8] graphify integration: LLM-based entity extraction for semantic layer beyond AST
- [ ] [#zv4m] graphify integration: HTML visualization of symbol/dependency graphs
- [ ] [#rd2f] graphify integration: semantic similarity between concepts (embedding-based)
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-05-09 [#ctxp] Add context-pack to the token-savings fixture/regression, including diff/test/log/next-context inputs and a fail-under threshold for its envelope preview.
- 2026-05-10 [#real] Add a real-session token-savings benchmark from recent tsift/agent-doc transcripts, covering session-review and context-pack paths that dominate actual prompt volume.
- 2026-05-10 [#budg] Tune the response-budget presets and auto caps against measured token-savings data so envelope previews stay compact without hiding required follow-up commands.
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
- 2026-05-13 [#rgls] Fix tsift rewrite handling for file-listing commands such as rg --files and find: preserve listing semantics, multiple roots, and shell safety, or pass through with an explanatory no-rewrite result instead of rewriting rg --files src/tsift to exact text search.
- 2026-05-13 [#sfpx] Add a deterministic session-digest/session-review regression for failure false positives from assistant progress or CI-status prose; the current rollout classified my assessment sentence and prior status updates mentioning failure/red/check as failure groups even though --next-context correctly reported zero unresolved failures.
- 2026-05-13 [#fref] Tighten codex-jsonl file-reference extraction so conversational fragments and shell syntax are not reported as touched files; the current session-digest surfaced false paths like 2>/dev/null, agent-doc/tsift, digest/session, progress/CI-status, and version/preflight.
- 2026-05-13 [#scst] Calibrate session-review/session-cost aggregate token reporting so cached multi-session totals are separated from per-session cost; current review reports 57,615,403 total tokens and largest turn 186,897 across 12 matched sessions, while the latest single-session cost is 518,391 total with largest turn 67,644.
- 2026-05-13 [#rwno] Make tsift rewrite unsupported-command outcomes explicit: commands with shell redirection or find predicates currently exit 1 with no explanation, so rewrite should either pass through safely or emit a bounded no-rewrite reason for agents.
<!-- /agent:done -->
