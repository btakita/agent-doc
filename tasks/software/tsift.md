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
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260608-002838.md`*

Compacted content:
- Archived 1 response topic(s): column-based JSONL token efficiency
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...

### Re: source-read envelope token efficiency — gpt-5

> **Queue prompt:**
>
> Please evaluate the output format of the tsift --envelope source-read. I see much repetition with the keys in metrics. Please look at real-world examples, such as with the agent-doc-bugs2.md session history. Make the output more token efficient. Is JSON the best format? Should we use a column-based strategy for these sort of lists?


I evaluated `source-read` against the existing envelope contracts and a real session-review example from `tasks/agent-doc/agent-doc-bugs2.md`. JSON is still the right primary machine format for agent-facing output, but repeated homogeneous lists should use the existing schema-then-values form instead of repeating object keys. That keeps the output parseable as ordinary JSON while making dense lists like `summary.metrics`, `report.preview`, `report.symbols`, and markdown outlines materially smaller.

Implemented in `src/tsift`:

- `tsift --envelope source-read ...` now defaults to the existing `_c`/`_r` column layout.
- Non-envelope `source-read --json` remains expanded object JSON unless callers explicitly pass `--schema`.
- The semantic-edit verification path now accepts either expanded arrays or `_c`/`_r` tables when reading nested source-read envelopes.
- Regression tests and `specs/output-formats.md` document the default and decode both shapes.

A manual sample confirmed `summary.metrics` now emits as a column table, e.g. `_c: ["label", "value"]` with `_r` rows. This is the right compromise: do not replace JSON with a separate text/table protocol, but do use column encoding inside JSON where repeated list keys dominate token cost.

Validation completed:

- `cargo test -p tsift-cli --test exit_code source_read_json -- --nocapture`
- targeted semantic-edit verification regressions
- `make check`
- `cargo install --path .`
- `gh run list --workflow CI --limit 1` showed the latest CI run completed successfully

Committed and pushed tsift: `a39d6e5 feat: columnize source-read envelopes`. Superproject pointer committed: `bc60866a9 chore: update tsift submodule`.

### Re: AGENTS install runbook reference — gpt-5

Yes, but not as a runbook-only install. The practical Codex/OpenCode reuse pattern is that `AGENTS.md` must stay self-contained because those harnesses can reuse the prompt-visible block directly without loading extra files. The better shape is hybrid: keep the generated Code Navigation block compact and complete enough to run, and point repositories that ship current deeper docs back to `.claude/skills/tsift/SKILL.md` or `runbooks/code-navigation.md` for command detail.

Implemented that hybrid:

- `tsift init` now injects a self-contained Codex/OpenCode line that references current deeper tsift runbooks when present.
- `src/tsift/AGENTS.md` and `specs/search-navigation.md` were updated to match the generated contract.
- `VERSIONS.md` records the behavior.
- The parent `runbooks/code-navigation.md` was refreshed from `0.1.62` to the current `0.1.64` guidance, including `source-read`, `symbol-read`, the current rewrite fallback wording, and the simplified CI check instruction.

Commits:

- `src/tsift`: `0f0aae3 docs: reference current tsift runbooks in init`
- parent repo: `b9152265 docs: refresh tsift code navigation runbook`

Verification:

- `cargo test -p tsift-index init_creates_agents_md_when_none_exists -- --nocapture`
- `git diff --check` for the tsift submodule and parent runbook
- `make check` passed end to end on the final tree
- `cargo install --path .` installed the final binary
- `gh run list --workflow CI --limit 1` showed the latest CI run completed successfully
- Latest `Agent Doc` run `27110521399` failed before any job started because GitHub reported billing/spending-limit exhaustion, so there were no failed test logs to inspect.
- `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`; `gh workflow list` shows only `Agent Doc`.
- Marker scan confirms the committed marker-bearing files are at `tsift:code-navigation v=0.1.64`.
- `git diff --check` passed after the final stable commit.
- `make check` passed.
- Latest `Agent Doc` run `27110521399` failed before any job started because GitHub reported billing/spending-limit exhaustion, so there were no failed test logs to inspect.
- `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`; `gh workflow list` shows only `Agent Doc`.
- Marker scan confirms the committed marker-bearing files are at `tsift:code-navigation v=0.1.64`.
- `git diff --check` passed after the final stable commit.
- `make check` passed.

No escalation was needed. Confidence is high: the install now restores the runbook reference where it helps, without breaking standalone checkouts or forcing Codex/OpenCode to load a separate document before using tsift.
❯ Run the init on all projects in agent-loop that use tsift?

### Re: tsift init across agent-loop projects — gpt-5

I ran plain `tsift init` across the marker-bearing projects in `agent-loop`. I intentionally did not pass `--codex` or `--opencode`, so the run refreshed the managed Code Navigation sections and did not install new Codex hooks or OpenCode shortcuts.

Committed stable updates in parent commit `b459624b9 docs: refresh tsift init instructions across projects`. That records root `AGENTS.md` plus submodule pointer updates for `src/agent-doc`, `src/agent-doc-js`, `src/agent-resume`, `src/chromium-bridge`, `src/corky`, `src/ctx-core-dev`, `src/equityfundingsource-dev`, `src/existence-lang/{existence,ontology,spec}`, `src/lazily-rs`, `src/presence`, `src/session-share`, `src/tagpath`, and `src/webmaster`. `src/tsift` was already current and needed no commit.

`src/boost-client` and `src/boost-client/src/monsterrodholders-dev` are the exception. I applied the init there twice, but an active monsterrodholders workflow rebased/reset those worktrees during this cycle and removed the AGENTS updates. I stopped rather than keep racing that active agent. I added a backlog item to rerun init for that pair after the active workflow settles.

Verification:

- `make check` passed.
- `git diff --check` passed after the final stable commit.
- Marker scan confirms the committed marker-bearing files are at `tsift:code-navigation v=0.1.64`.
- `gh run list --workflow CI --limit 1` could not run because this repo has no workflow named `CI`; `gh workflow list` shows only `Agent Doc`.
- Latest `Agent Doc` run `27110521399` failed before any job started because GitHub reported billing/spending-limit exhaustion, so there were no failed test logs to inspect.

No escalation was needed. Remaining root dirt is pre-existing or concurrent work: `.codex/config.toml`, `src/boost-client`, `src/instruction-files`, `src/webmaster`, `tasks/agent-doc-bugs2.done.md`, plus the session document until this write commit lands.
<!-- agent:boundary:7e614b59 -->
<!-- agent:boundary:e575bad2 -->
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
- [ ] [#tsiftinitboost] Rerun plain `tsift init` in `src/boost-client` and `src/boost-client/src/monsterrodholders-dev` after the active monsterrodholders workflow stops rebasing/resetting those worktrees; commit their `AGENTS.md` updates and any resulting pointers.
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
