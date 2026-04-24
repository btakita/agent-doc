---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent: codex
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
---



## Status

<!-- agent:status patch=replace -->

✅ Verified and landed the bare-path compatibility alias plus committed-historical snapshot self-heal in `src/agent-doc`. `agent-doc <FILE>` now routes to `run`, and `commit` / `session-check` repair already-committed historical `### Re:` drift when `HEAD` proves the response is no longer out-of-band. The latest `src/agent-doc` change is built, installed locally, committed, and pushed.
Remaining open work in this bug doc is still only the multi-user pending auto-create policy follow-up (`#ha3h`), but it remains gated: the linked issue explicitly says not to ship it before multi-user workspace support, and the current `src/agent-doc` tree still documents the product as single-user only.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Compacted exchange summary: shipped agent-doc 0.33.13 plus follow-up fixes for fail-closed unrecoverable preflight_started cycles, bare-path compatibility, committed historical snapshot-drift repair, clean post-commit boundary churn cleanup, manual escaped-tail cleanup respect, mode-aware bare-path and run entrypoints, and durable post-write run closeout that records write_applied before commit and lets preflight finish interrupted runs without duplicate patchback. Confirmed and closed work in this exchange includes #cde3, #cdp4, #bndy, #veqs, and #n5hs. #ha3h remains explicitly gated on future multi-user workspace support. Remaining pending items outside the compacted exchange are #3sd5 for audit-docs root selection, #ndyy for src/agent-doc normalization before rebasing, and gated #ha3h.
❯ do #3sd5. update spec + tests. build + install for local testing. commit + push
### Re: `#3sd5` audit-docs root selection — gpt-5

Implemented and verified in `src/agent-doc`.

What changed:

- Tightened `src/audit_docs.rs` so `agent-doc audit-docs` now prefers the running `src/agent-doc` crate root when the binary is being launched from an outer repo that contains that nested crate checkout, instead of blindly auditing the outer repo.
- Kept the override narrow: explicit `--root` still wins, normal installed-binary behavior is unchanged, and unrelated working directories do not get redirected.
- Added regression coverage for the nested-crate override shape and updated `specs/07-commands.md` so the `cargo run --manifest-path src/agent-doc/Cargo.toml -- audit-docs` behavior is documented.

Verification:

- `cargo test --manifest-path src/agent-doc/Cargo.toml audit_docs::tests:: -- --nocapture`
- `cargo test --manifest-path src/agent-doc/Cargo.toml test_cli_audit_docs_ -- --nocapture`
- `cargo fmt --manifest-path src/agent-doc/Cargo.toml`
- `make -C src/agent-doc check`
- `cargo build --manifest-path src/agent-doc/Cargo.toml --release`
- `cargo install --path src/agent-doc`

Commit / push:

- `src/agent-doc`: `e4bb6df` — `fix: scope audit-docs to crate root`
- pushed to `origin 20260422/codex-agent-backend`

Outcome:

- `#3sd5` is done.
- `agent-doc audit-docs` no longer audits the outer monorepo root when it is launched from that root via `cargo run --manifest-path src/agent-doc/Cargo.toml -- audit-docs`; it scopes discovery to the `src/agent-doc` crate workspace instead.

❯ Why id `agent-doc repair` not commit the changes?
❯ • You ran agent-doc repair tasks/claudescore-3.md
❯ └ [recover] Response already present in document — skipping apply, cleaning up pending file
❯ [recover] repaired escaped conversation tail in tasks/claudescore-3.md
❯ [recover] No pending response found for tasks/claudescore-3.md
### Re: `agent-doc repair` commit behavior — gpt-5

Short answer: because `agent-doc repair` is a recovery command, not the commit-boundary command.

Your output hit the dedupe path: the response was already present in `tasks/claudescore-3.md`, so `repair` cleaned up stale pending state and repaired the escaped exchange tail, but it did not run `git::commit()`. The CLI wiring for `repair` just calls `recover::run()` and returns.

The documented commands that are supposed to cross the commit boundary are:

- normal response path: `agent-doc finalize <FILE>`
- missed-patchback repair when the prompt is already in the doc: `agent-doc write --commit <FILE>`

So if you run `agent-doc repair` by itself, a commit is not guaranteed. In your case the later `agent-doc preflight tasks/agent-doc/agent-doc-bugs.md` did the commit during its recover/commit sweep, which is why this session started with a fresh preflight commit (`8ba82014`).

If you want, I can turn that mismatch into a follow-up bug/doc task so `repair` vs `write --commit` is harder to confuse.

❯ `agent-doc repair` should commit. Because if I run repair there is uncommitted content in the document. Which can confuse the next prompt.
### Re: `agent-doc repair` should commit — gpt-5

I think that is a real product bug, not just a docs mismatch.

The current implementation treats `repair` as a low-level recovery primitive, so it can leave the file ahead of the snapshot and rely on a later `preflight` / `commit` sweep to catch up. But that is a bad user-facing contract for exactly the reason you called out: after `repair`, the document can contain recovered assistant content that is still uncommitted, and the next turn sees a dirty, slightly ambiguous state.

This turn's preflight showed the symptom directly:

- `file is 140 bytes larger than snapshot`
- `staged snapshot already matches HEAD`

So the recovered document state and the recorded snapshot/commit boundary were temporarily out of sync.

Given the current product contract that appended responses should cross a commit boundary, I think the better behavior is:

- for git-backed docs, `agent-doc repair` should attempt the same closeout path and not stop after recovery alone
- if that commit cannot complete, it should fail closed instead of silently leaving recovered response state hanging around for the next prompt

I added this as pending work in the doc backlog.
<!-- agent:boundary:759cbc69 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] [#f3nj] Make git-backed agent-doc repair cross the commit boundary after successful recovery so repaired response content does not remain uncommitted into the next prompt.
- [ ] [#ndyy] Normalize src/agent-doc before rebasing: remove tracked .agent-doc runtime files from the index, remove the typo .gitgnore file if accidental, keep the real ignore rule, then rebase 20260422/codex-agent-backend onto origin/main.
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->
