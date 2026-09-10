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
same turn. Codex hooks in user-level `$CODEX_HOME/hooks.json` plus project-local `.codex/config.toml` are a
fail-closed backstop, not a replacement for explicit closeout.

## Agent harnesses own full-suite verification

If you changed code, tests, build logic, or instruction surfaces, run the full
project verification suite explicitly after edits and before `finalize` /
`write --commit`. Do not rely on a pre-commit hook. Do not waive red suites as
"unrelated" or "flaky".

## Minimal-blocker closeout

Close the turn once the requested work is implemented, local verification is
sufficient for the risk, and the required persistence/session-check boundary has
passed. Do not keep the turn open for queued/in-progress CI, manual review, live
editor/pane confirmation, or another external async signal unless that signal is
required proof for this turn or the user explicitly asks you to wait. Record
pending or external status in closeout instead of turning it into a blocker.

## Tmux CI review for test-bearing turns

When the cycle runs tests or changes test, build, or instruction surfaces,
inspect the latest CI tmux-test result for this repo with
`gh run list --workflow CI --limit 1`. If the tmux leg is already red after
runner startup and the failure belongs to this change, run `make tmux-ci`
locally, fix the failure, and add or update deterministic SimWorld coverage for
the regression class when the behavior can be modeled without live tmux. If the
latest run is queued, in progress, unrelated red, or externally blocked, record
that status and continue from local verification evidence instead of waiting for
CI to finish; do not use `gh run watch` as a closeout gate unless the user
explicitly asks. If GitHub reports an empty-step job with no logs because the job
was not started (for example billing/spending-limit exhaustion or other
runner-allocation failure), classify it as an external CI-start blocker instead
of a code/tmux regression; record the annotation and continue with local
verification evidence. Record CI and local tmux evidence in closeout.

## Session document staging rule

For ordinary repo `commit + push`, keep the session document out of that manual
git commit. Resolve the exact intended non-session path set first, stage only
that set, stop on any stage failure, verify `git diff --cached --name-only` still
matches the intended set, commit only that validated set, then let `finalize` /
`write --commit` close the session document before push.

## Finalize / session-check contract

Strict template closeout accepts only explicit exchange patch blocks. A response
missing either `<!-- patch:exchange mode="append" -->` or
`<!-- /patch:exchange -->` fails before capture or mutation; add the markers and
retry the same closeout rather than converting it into a plain append implicitly.

`finalize` requires the cycle to reach `committed` and the post-commit
`session-check` guard to pass before success, including prompt-only exchange-tail
checks. `agent-doc write --commit <FILE>` shares that fail-closed boundary for
repair writes. If `finalize`, `write --commit`, or `repair` surfaces a
`session-check` interruption, continue recovery instead of reporting success.
`session-check` also enforces pending capture / `pending_done_guard`. Use
[commit.md](commit.md) and [harness-invocation.md](harness-invocation.md) for the
full closeout contract.

Terminal success also proves current canonical CRDT authority equals the disk
projection byte-for-byte. A zero-replica or authority/disk mismatch schedules
automatic supervisor recovery of the same durable `(cycle, capture, response)`
operation. Once the response is captured, the binary owns reconcile, replay,
dedupe, ACK proof, and commit retry; the agent must not recapture the response or
rerun finalize. For a non-committed closeout, `session-check` reports this
retained intent directly instead of misclassifying it as a manual patchback; a
committed cycle still follows ordinary terminal validation. The Stop hook is a status gate for that operation, not another
write-loop driver. Recovery always stays on the editor/CRDT authority path: it
does not kill the controller or elect `--force-disk`. Only a typed
`needs_operator` result for competing user-authored intent pauses automatic
recovery.

A canonical editor advance after delivery proof is another recoverable delivery
race, not evidence that the capture failed. The document actor re-enters the CRDT
replace/ACK barrier with the original projection base and target, preserving the
same capture identity and exact-once response and backlog mutations. If the
foreground rebase budget is exhausted, the retained operation continues through
the supervisor and `session-check`; the agent still must not recapture, rerun
`finalize` or `write --commit`, or force a disk projection.

A replacement editor replica may bootstrap from that retained canonical target
with an empty delivery queue. In that state no later ACK exists to retire the
historical deferred slot. `session-check` recognizes exact
canonical/target/disk equality plus captured-response materialization, clears
only that retained lineage, refreshes the response snapshot, advances the same
capture to `write_applied`, and commits it. Retry only `session-check`; another
finalize/write payload would create closeout churn rather than recovery.

Rule-based ambiguity resolution requires positive evidence. A duplicate semantic
operation is deduped; a causally newer editor/replica epoch wins over its stale
projection; compatible concurrent CRDT histories merge. Missing causal lineage or
two incompatible semantic replacement intents retain both candidates and produce
`needs_operator` without mutation. CRDT byte convergence alone is not proof of
semantic intent.

An external disk change while any editor is open is not an ambiguity for
`finalize` to merge through. The binary retains it in a Lazily slot independent
of the captured response and waits for editor evidence. Exact accept/reload plus
replica propagation clears it; a later editor edit clears it; an editor save
whose exact bytes reach disk clears it and makes that save cut authoritative;
the final editor close clears it and falls back to disk. Until one of those
events, finalize remains mutation-free for the disk candidate and may continue
retrying only the independent response lineage.

## Manual repair / missed patchback rule (all harnesses)

If the user's prompt is already present in the document, do **not** patch the
assistant response directly into the file. Use `agent-doc write --commit <FILE>`
so repair crosses the normal snapshot/commit boundary in one path. Do not document
or follow a manual-repair flow that stops after bare `agent-doc write`. Direct
file patching is only acceptable for inserting a missing user prompt into
`exchange` before the response exists.

Document format, frontmatter, component naming, and commit-boundary exceptions:
[document-format.md](document-format.md), [commit.md](commit.md).
