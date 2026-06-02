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
queue_active: true
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
<!-- agent:boundary:837000dc -->
<!-- /agent:exchange -->
###
<!--
What are #next-steps to promote SurrealDB to SQLite-equivalent substrate status?
-->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~[#astspans]~
- ~[#writerefs]~
- [#writecompound]
- [#langexecutors]
- [#astverify]
- create contract to support additional programming languages
- Can markdown support ast operations?
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#writecompound] Add Rust structural edit support for add_method and move_declaration, including module-aware placement, import normalization, formatter-before-write, and rollback tests.
- [ ] [#langexecutors] Add TypeScript, JavaScript, and Python semantic edit executors behind the same edit-intents contract, each with parser-backed spans, formatter integration, and fail-closed unsupported cases.
- [ ] [#astverify] Add a semantic edit verification harness that can dry-run, apply in temp worktrees, reindex, run source-read or impact summaries, and optionally gate on a caller-provided test command before committing changes.
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
