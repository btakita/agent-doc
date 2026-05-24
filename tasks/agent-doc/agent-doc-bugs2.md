---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->

Resolved #qfixture; auto queue drained.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260523-230644.md`*

Compacted content:
- Archived 1 response topic(s): ops bug cluster batch
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7...
❯ The html comment below this document's agent:exchange close tag had content that I put into it. The last agent-doc turn removed the content that I added. This should not happen.
❯ As I was typing in this prompt, the prompt was duplicated. Is this a full-document replacement issue?
❯ #spec-test-build-install-commit-push
### Re: visible scratch comment preservation — gpt-5

No, this specific comment-body deletion is not best explained by a full-document replacement. The local ops log for this document had the direct event: `post_exchange_duplicate_prompt_comment_removed ... source=route`, which points at route-time duplicate-prompt comment cleanup emptying the ordinary HTML comment body below `&lt;!-- /agent:exchange --&gt;`.

Implemented the fix in `src/agent-doc`: route and preflight now treat the current visible file as ownership proof for ordinary post-exchange HTML comment lines, so they preserve scratch comments the user typed before cleanup. Final closeout also preserves comment lines that were present in the current file before the response write. Generated duplicate comment residue can still be scrubbed only when it is absent from the baseline/snapshot and absent from the current visible file used for the mutation. Exact duplicate answered prompt tails inside `agent:exchange` still auto-clean.

Changed paths: `src/agent-doc/src/template.rs`, `src/agent-doc/src/route.rs`, `src/agent-doc/src/preflight.rs`, `src/agent-doc/src/write.rs`, `src/agent-doc/src/route/tests.rs`, `src/agent-doc/tests/finalize_integration.rs`, `src/agent-doc/specs/04-diff-computation.md`, `src/agent-doc/specs/07-closeout-commands.md`, `src/agent-doc/specs/07-session-tmux-commands.md`, `src/agent-doc/VERSIONS.md`.

Verification:
- `cargo test prompt_comment -- --nocapture`
- `cargo test finalize_preserves_current_duplicate_prompt_html_comment_body -- --nocapture`
- `cargo test current_prompt_html_comment -- --nocapture`
- `make check`
- `cargo fmt --check`
- `cargo build --locked`
- `cargo install --path . --locked`
- `agent-doc --version` -> `agent-doc 0.33.16`
- Latest GitHub CI run `26345739086` succeeded; its `Run make tmux-ci` step also succeeded.

Commit / push state:
- `src/agent-doc`: committed `45a4cd7 fix: preserve visible scratch comments` and pushed to `origin/20260422/codex-agent-backend`.
- Superproject: committed `00e149a63 chore: pin scratch comment preservation`; per the closeout ordering rule, I will push it after this finalize commit lands so the session response commit is included.

Escalation: none.

Confidence: high. The regression coverage now pins route/preflight preservation of visible scratch comments, final closeout preservation of current visible comments, and the lower-level generated-residue cleanup path.
❯ What are the #next-steps to fix the comment deletion issue?
### Re: comment deletion next steps — gpt-5

Top backlog item: `#cown`.

`#cown` is now the first pending backlog item. Its target is hardening: prove every remaining document mutation path preserves user-authored scratch comments below `&lt;!-- /agent:exchange --&gt;` while still allowing agent-doc-generated duplicate prompt residue to be removed.

For that backlog item, the concrete scope is:

1. Add a deterministic SimWorld or integration fixture that seeds a user-authored HTML comment after the exchange close tag, exercises route cleanup, preflight recovery, finalize/write direct disk, IPC/plugin handoff, repair/write --commit, compact/exchange, and generated-residue cleanup, then asserts the comment body survives.
2. Extend cleanup diagnostics around `post_exchange_duplicate_prompt_comment_removed` and skipped removals to record the ownership source, such as `baseline`, `snapshot`, `current_visible`, or `generated`, plus enough line/hash/count data to make future deletions attributable.
3. Update the relevant specs/runbooks once the coverage is in place so the invariant is explicit: ordinary post-exchange comments are user-owned unless the current cycle proves agent-doc generated them.

Changed paths for this cycle: session document/backlog only.

Commands run: `agent-doc orchestrate ... --from-exchange --plan`, `agent-doc plan ...`, `agent-doc read ...`, and `agent-doc finalize ...`.

Verification: no code suite was run because this cycle only answers the `#next-steps` prompt and records pending backlog work.

Escalation: none. Confidence: high for the recommended next step and backlog priority.
<!-- agent:boundary:b0748a78 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:queue -->
dispatch #spec-test-build-install-commit-push
- do [#1mm8]
- do [#hg0n]
- do [#te1d]
- do [#pmz2]
- do [#f4xr]
<!-- /agent:queue -->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#cown] Harden the remaining post-exchange comment ownership paths with a SimWorld/integration no-delete fixture covering route cleanup, preflight recovery, finalize/write direct disk, IPC/plugin handoff, repair/write --commit, compact/exchange, and generated duplicate-residue cleanup diagnostics.
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc-bugs2.done.md -->

<!-- completed work archived in tasks/agent-doc-bugs2.done.md -->
<!-- /agent:done -->
