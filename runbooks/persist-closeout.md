# Persist / closeout detail

Detail for SKILL.md Workflow **step 2 (Persist the response)**. The spine in
SKILL.md keeps the MANDATORY-finalize rule, the finalize command, the
`patch:exchange` marker requirement, and the no-Edit-tool rule. This runbook
carries the rest.

## Ordering: finish work before persisting

Complete requested implementation, verification, build/install, and local
inspection **before** persisting. The response-persistence command is the final
document-mutation boundary for the cycle, not an intermediate checkpoint. After
`finalize` / `write --commit`, do not start more long-running task work for that
same turn. Codex hooks in `.codex/hooks.json` / `.codex/config.toml` are a
fail-closed backstop, not a replacement for explicit closeout.

## Agent harnesses own full-suite verification

If you changed code, tests, build logic, or instruction surfaces, run the full
project verification suite explicitly after edits and before `finalize` /
`write --commit`. Do not rely on a pre-commit hook. Do not waive red suites as
"unrelated" or "flaky".

## Tmux CI review for test-bearing turns

When the cycle runs tests or changes test, build, or instruction surfaces,
inspect the latest CI tmux-test result for this repo. If the tmux leg is red
after runner startup, run `make tmux-ci` locally, fix the failure, and add or
update deterministic SimWorld coverage for the regression class when the behavior
can be modeled without live tmux. If GitHub reports an empty-step job with no logs
because the job was not started (for example billing/spending-limit exhaustion or
other runner-allocation failure), classify it as an external CI-start blocker
instead of a code/tmux regression; record the annotation and continue with local
verification evidence. Record CI and local tmux evidence in closeout.

## Session document staging rule

For ordinary repo `commit + push`, keep the session document out of that manual
git commit. Resolve the exact intended non-session path set first, stage only
that set, stop on any stage failure, verify `git diff --cached --name-only` still
matches the intended set, commit only that validated set, then let `finalize` /
`write --commit` close the session document before push.

## Finalize / session-check contract

`finalize` requires the cycle to reach `committed` and the post-commit
`session-check` guard to pass before success, including prompt-only exchange-tail
checks. `agent-doc write --commit <FILE>` shares that fail-closed boundary for
repair writes. If `finalize`, `write --commit`, or `repair` surfaces a
`session-check` interruption, continue recovery instead of reporting success.
`session-check` also enforces pending capture / `pending_done_guard`. Use
[commit.md](commit.md) and [harness-invocation.md](harness-invocation.md) for the
full closeout contract.

## Manual repair / missed patchback rule (all harnesses)

If the user's prompt is already present in the document, do **not** patch the
assistant response directly into the file. Use `agent-doc write --commit <FILE>`
so repair crosses the normal snapshot/commit boundary in one path. Do not document
or follow a manual-repair flow that stops after bare `agent-doc write`. Direct
file patching is only acceptable for inserting a missing user prompt into
`exchange` before the response exists.

Document format, frontmatter, component naming, and commit-boundary exceptions:
[document-format.md](document-format.md), [commit.md](commit.md).
