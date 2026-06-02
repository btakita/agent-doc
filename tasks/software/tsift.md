---
agent_doc_session: tsift-v0.1
resume: 019e85d7-f9ee-79f3-b597-4ed352495bbd
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
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Spec

See [src/tsift/SPEC.md](../../src/tsift/SPEC.md) for the AST-Aware Code RAG spec.

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc65443c45f7b49f5e6ace54e3f6bafe4077ade57924422b5eb3baf4dbd23c-20260602-030752.md`*

Compacted content:
- Archived 4 response topic(s): tsift as Read/Write substrate; next steps for tsift Read/Write replacement; queued tsift read/write replacement work; 1 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/53cc6544...
❯ What are #next-steps for read/write ast support?

### Re: next steps for read/write AST support — gpt-5

Top backlog item: `#astspans`.

The next useful layer is to make tsift's read/write surfaces share one AST span model instead of relying on symbol line ranges plus text operations. After that, the write executor can expand from the current Rust `rename_symbol`, `replace_function_body`, and `insert_import` support into true reference/call-site rewrites and larger structural edits.

Priority order added to backlog:

1. `#astspans` — AST-backed spans and stable span handles for `symbol-read` and `edit-intents`.
2. `#writerefs` — Rust `rewrite_call_sites` and `update_call_signature` support with AST validation.
3. `#writecompound` — Rust `add_method` and `move_declaration` structural edits.
4. `#langexecutors` — TypeScript, JavaScript, and Python executor adapters under the same contract.
5. `#astverify` — temp-worktree verification harness for dry-run/apply/reindex/read-impact/test gates.

That order keeps the substrate honest: first improve the AST read/write address space, then add harder Rust mutations, then generalize across languages, then wrap the whole flow in stronger apply-time verification.

### Re: #astspans — gpt-5

> **Queue prompt:**
>
> [#astspans]


Done.

Implemented AST-backed symbol spans for `tsift`: tree-sitter extraction now persists declaration node kind, start/end byte offsets, and body byte offsets; the index migrates those columns additively and keeps read-only fallback behavior for older DBs. `source-read`, `symbol-read`, and `edit-intents` now expose stable `span-*` metadata with byte ranges, line projections, body ranges, and parent/child span refs where available. I also updated the spec/docs/version note and added regression assertions for the three user-facing packet surfaces.

Verification completed:
- `cargo check -p tsift-cli`
- `cargo test -p tsift-graph resolve_edges -- --nocapture`
- `cargo test -p tsift-cli --test exit_code read_json_reports -- --nocapture`
- `cargo test -p tsift-cli --test exit_code edit_intents_json_validates_semantic_write_plan_without_mutating -- --nocapture`
- `make check`
- `cargo build`
- `cargo install --path . --force`; installed binary reports `tsift 0.1.62`
- GitHub CI run `26796469963` passed: `clippy + test` and `live Convex graph acceptance`. No tmux-specific CI job is configured in the `tsift` workflow.

Commits:
- `src/tsift`: `d5f7eba feat: add AST symbol spans` (pushed to `btakita/tsift` `main`)
- parent repo gitlink: `e46e0129e chore: update tsift submodule`

Queue note: `#astspans` is complete; the remaining queue items are still pending.

### Re: #writerefs — gpt-5

> **Queue prompt:**
>
> [#writerefs]


Implemented `#writerefs`.

`edit-intents --apply` now supports Rust `rewrite_call_sites` and `update_call_signature` using indexed same-file call refs plus AST validation. Plans expose `call_refs`; cross-file call refs fail closed for now. `update_call_signature` rewrites the function signature and requires `call_replacement` when indexed same-file calls must change. The executor still routes every mutation through formatter-before-write staging and the existing atomic backup/rollback write path.

Updated the docs/spec/version notes to include the new call-site rewrite surface and `call_replacement` contract. Added fixture coverage for successful call-site apply, successful signature-and-call-site apply, and no-write refusal when a signature update is missing `call_replacement`.

Commits:

- `src/tsift`: `a160e70` (`feat: add Rust call-site edit intents`), pushed to `origin/main`.
- parent repo: `89398d7f5` (`chore: update tsift submodule`), ready to push after this document closeout commit.

Verification:

- `cargo check -p tsift-cli` passed.
- `cargo test -p tsift-cli --test exit_code edit_intents_apply_` passed: 6 tests.
- `make check` passed: workspace clippy, workspace tests/doc-tests, OpenCode npm tests, and npm dry-run publish check.
- `cargo build` passed.
- `cargo install --path . --force` passed.
- Installed binary reports `tsift 0.1.62`.
- GitHub CI for `src/tsift` run `26797215803` passed: `clippy + test` and `live Convex graph acceptance` green. Existing CI annotations are the Node 20 actions deprecation and the optional live Convex deployment configuration note.
- No tmux CI workflow or `make tmux-ci` target exists in this repo; deterministic SimWorld tests ran through `make check` and passed.

### Re: #writecompound — gpt-5

> **Queue prompt:**
>
> [#writecompound]


Completed `#writecompound`.

Implemented Rust structural edit support for `add_method` and `move_declaration` in `packages/tsift-cli/src/lib.rs`. `add_method` now targets indexed Rust structs/enums, validates the replacement as one method, inserts into an existing inherent `impl` when present, or creates a new inherent `impl` near the type. `move_declaration` now treats `file` as the destination, finds the indexed source declaration, supports same-directory Rust destination files, inserts after the destination prelude, normalizes the source module with `mod <destination>;` and `use <destination>::<symbol>;`, and flows through the atomic formatter-before-write rollback path.

Added exit-code coverage for adding a method, moving a declaration between module files, and refusing an invalid move without mutating files. Updated `SPEC.md`, `AGENTS.md`, `CLAUDE.md`, and `VERSIONS.md` for the new structural intent contract.

Commits:

- `e9445fe` in `src/tsift`: `feat: add Rust structural edit intents`
- `4cd531a0e` in the parent repo: `chore: update tsift submodule`

Verification:

- `cargo check -p tsift-cli`
- `cargo test -p tsift-cli --test exit_code edit_intents_apply_`
- `cargo clippy -p tsift-cli --all-targets -- -D warnings`
- `make check`
- `cargo build`
- `cargo install --path . --force`
- `tsift --version` -> `tsift 0.1.62`
- `git diff --check`
- GitHub Actions CI `26797936405` passed: `clippy + test` and `live Convex graph acceptance`

CI only reported the existing Node 20 actions deprecation and optional Convex deployment annotation. I did not find a tmux CI target/workflow in this repo; SimWorld coverage ran as part of `make check`. An extra package-wide `cargo fmt --check --package tsift-cli` still reports existing formatter drift in unrelated command modules, so I left that churn out of this patch.

### Re: #langexecutors — gpt-5

Implemented `#langexecutors` in `src/tsift`.

The semantic edit executor now routes `.ts`, `.tsx`, `.js`, `.jsx`, `.py`, and `.pyi` through language-specific script executors behind the same edit-intents contract as Rust. TypeScript/JavaScript/Python support `rename_symbol`, `replace_function_body`, and `insert_import` with parser-backed validation. Unsupported script-language intents such as call-site rewrites, signature updates, method insertion, and declaration moves fail closed without mutating files.

Formatter handling is fail-closed for Rust and validation-first for the script executors: Rust still uses `rustfmt --edition 2024`, TypeScript/JavaScript optionally use local `prettier`, and Python optionally uses `ruff format` or `black` when available. If a script formatter is absent, the executor keeps the tree-sitter validated output and reports no formatter rather than inventing one.

Added exit-code coverage for the new script executors: TypeScript body/import edits, JavaScript rename edits, Python body/import edits, and a TypeScript unsupported call rewrite refusal that verifies the source file is unchanged.

Commits:

- `eac47cf` in `src/tsift`: `feat: add script semantic edit executors` (pushed to `btakita/tsift main`)
- `aab3223f9` in the parent repo: `chore: update tsift submodule` (local pointer commit; isolated root push follows this document closeout)

Verification completed:

- `cargo check -p tsift-cli`
- `cargo clippy -p tsift-cli --all-targets -- -D warnings`
- `cargo test -p tsift-cli --test exit_code edit_intents_apply_ -- --nocapture`
- `make check`
- `cargo build`
- `cargo install --path . --force`
- `tsift --version` -> `tsift 0.1.62`
- `git diff --check`
- `tsift status`
- GitHub Actions CI `26798728907` passed. The only annotations were the existing Node 20 actions deprecation and the optional live Convex deployment configuration note.

No tmux CI workflow/target was present for this repo; the SimWorld coverage ran through `make check`.

### Re: #langexecutors closeout repair — gpt-5

Recorded a binary closeout repair for `#langexecutors`: the implementation response, done-state, and queue reap were already present in the committed document after the IPC visible-buffer recovery, but the recovery cycle lacked a captured response body. This write adds the missing agent-doc capture/commit boundary only; no tsift code, tests, or parent pointer changed in this repair step.

### Re: #astverify — gpt-5

> **Queue prompt:**
>
> [#astverify]


Implemented the semantic edit verification harness for `tsift edit-intents`.

The new `--verify` path creates a detached temporary git worktree, reindexes it before planning, applies supported semantic edit intents there, reindexes again, runs bounded `source-read` windows and an impact summary, and optionally runs a caller-provided `--verify-command`. `--verify` by itself reports verification without mutating the original tree; `--verify --apply` runs the full verification gate before applying to the real tree.

Added integration coverage for verify-only no mutation, verify-plus-apply command gating, and failed verify command blocking real mutation. Documentation was updated in `AGENTS.md`, `CLAUDE.md`, `SPEC.md`, and `VERSIONS.md`.

Commits:
- `src/tsift`: `9515b68 feat: verify semantic edit intents`
- parent repo pointer: `d6e6d14d0 chore: update tsift submodule`

Verification passed:
- `cargo check -p tsift-cli`
- `cargo clippy -p tsift-cli --all-targets -- -D warnings`
- `cargo test -p tsift-cli --test exit_code edit_intents_verify_ -- --nocapture`
- `cargo test -p tsift-cli --test exit_code edit_intents_ -- --nocapture`
- `git diff --check`
- `make check`
- `cargo build`
- `cargo install --path . --force`
- `tsift --version` -> `tsift 0.1.62`
- `tsift status` -> fresh index, instructions current
- GitHub Actions CI `26799671257` passed both live Convex acceptance and `clippy + test`

No project `tmux-ci` target or tmux workflow exists in `src/tsift`; deterministic SimWorld coverage ran through `make check`. The remote CI annotations were existing workflow notes about Node 20 action deprecation and optional live Convex configuration, not test failures.

### Re: create contract to support additional programming languages — gpt-5

> **Queue prompt:**
>
> Can markdown support ast operations?


Created the semantic edit language-extension contract.

`edit-intents` language support now flows through a `SemanticEditLanguageContract` table that declares each executor's canonical language id, display name, `graph::Lang` parser binding, formatter staging suffix, aliases, file extensions, supported intent kinds, script-family behavior, and formatter policy. The existing Rust, TypeScript/TSX, JavaScript/JSX, and Python behavior is preserved, but language/file resolution, supported-intent checks, and formatter selection now come from the contract instead of scattered matches.

I also added a contract unit test that checks aliases, extensions, formatter policy, and the exact supported intent set for every current executor, plus spec/version documentation describing the contract future language additions must satisfy.

Commits:
- `src/tsift`: `30d1910 feat: add semantic edit language contract`
- parent repo pointer: `c160d4232 chore: update tsift submodule`

Verification passed:
- `cargo test -p tsift-cli semantic_edit_language_contracts_resolve_current_executor_surface -- --nocapture`
- `cargo check -p tsift-cli`
- `cargo clippy -p tsift-cli --all-targets -- -D warnings`
- `cargo test -p tsift-cli --test exit_code edit_intents_ -- --nocapture`
- `git diff --check`
- `tsift diff-digest`
- `make check`
- `cargo build`
- `cargo install --path . --force`
- `tsift --version` -> `tsift 0.1.62`
- `tsift status` -> fresh index, instructions current
- GitHub Actions CI `26800161158` passed both live Convex acceptance and `clippy + test`

No project `tmux-ci` target or tmux workflow exists in `src/tsift`; deterministic SimWorld coverage ran through `make check`. Remote CI annotations were the existing Node 20 action deprecation and optional live Convex configuration notes, not failures.
<!-- agent:boundary:efbecd27 -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#surrealbulk] Batch SurrealDB projection load: replace per-row upsert loops with bulk/transactional row load plus one-pass secondary-index rebuild; measure refresh/load separately.
- [/] [#full-e2e-verify] In fresh project: opencode plugin opencode-tsift && opencode then /tsift-status. On success mark #e2e-install done.
- [/] [#npm-publish-verify] After auth configured, tag release and verify publish-opencode-plugin CI job succeeds and opencode-tsift appears on npmjs.org
- [/] [#npm-auth-setup] Configure NPM_TOKEN secret + TSIFT_ENABLE_NPM_PUBLISH=true variable in GitHub repo settings
- [/] [#e2e-install] Verify end-to-end install: fresh opencode project + opencode plugin opencode-tsift + /tsift-status
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
