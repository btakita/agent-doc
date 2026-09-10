# Grok Build integration

Invariant: selecting `agent: grok` runs Grok Build with document-bound session identity and publishes only a successful final response; it never silently launches Claude or treats a Grok notification hook as blocking admission.

Policy owner: `agent-doc-harness::HarnessConfig` owns interactive capabilities and pane evidence; `agent-doc-agent-io::agent::grok` owns the headless protocol. The existing supervisor and document/turn scopes continue to own dispatch, lineage, queue and closeout.

Transition table:

| Input | Decision |
| --- | --- |
| Fresh session | Launch grok; assign a fresh UUID where supported |
| Document-bound resume ID | `--resume ID`; never choose another document's latest session |
| Fork with an exact ID | Resume that ID plus `--fork-session`; the generic first-run fork flag without an ID starts fresh |
| Successful JSON final | Publish `text` and `sessionId` |
| Empty, malformed, interrupted, failed or incomplete final | Error; preserve the document |
| Proven empty idle composer | Existing computed dispatch readiness may submit |
| Busy, drafted, permission, authentication or unknown pane | Preserve pending input; do not inject |
| Committed closeout with actionable queue | Continue through normal binary queue continuation |

Evidence inputs: installed Grok Build 1.0.24 help and a successful disposable headless probe; official https://docs.x.ai/build/cli/reference and https://docs.x.ai/build/features/hooks. Live JSON uses `text`, `stopReason`, `sessionId`; it is not Claude's `result` schema. User authorized CLI tests; Heavy plan is unavailable until a later upgrade, so do not require it or select a paid tier implicitly.

Reactive topology: existing DocumentScope/TurnScope observations (document agent, lineage, pane output, turn receipts) → computed authority/readiness/continuation → supervisor launch/dispatch/closeout effects → generation-fenced receipts. Grok only adds boundary facts and protocol adapters. Headless `run` and skill installation are actorless one-shot boundaries and retain the shared bounded subprocess executor. No new retry loop, polling owner, SQLite arbiter, or per-call Lazily context.

Imperative extraction audit: reuse the existing computed pane/dispatch projections; add Grok-specific parsing solely at the harness boundary. JSON validation and immutable launch argument construction are pure one-shot functions. Skill rendering reuses the shared template. Grok passive Stop hook stdout cannot be used for queue steering; explicit binary admit/finalize remains the contract.

Allowed edit surfaces: backend resolver and new Grok adapter; harness configuration/readiness; launch/model identity and generic skill installation surfaces where needed; focused tests, documentation and release metadata.

Verification: fake-process protocol tests for success/resume/fork/error/env/large prompt, pure harness transitions and negative pane evidence, disposable authorized live headless/interactive smoke, full `make check` and tmux CI, installed CLI verification, one external CI observation.

Out of scope: Heavy-plan entitlement changes, credential modifications, and a second ACP orchestrator. Any unverified live capability must remain explicit rather than guessed.

## Implementation evidence

The adapter now has a separate `resume.grok` slot, including scalar migration,
alias normalization, claims and clearing without changing Claude/Codex history.
Native JSON fixtures exercise the real `agent-doc run` commit boundary without
`--force-disk`; the response and Grok resume ID reach HEAD, while thoughts do not.
Parent `GROK_SESSION_ID` is removed at child launch so another harness cannot
inherit the parent's identity. Quota JSON surfaces its message without exposing
other output fields.

The project installer merges Grok MCP configuration and preserves a pre-existing
server entry, including an intentional disable. It renders the shared skill and
bundles the same runbooks/OKF; Grok uses connected admission and closeout because
its notification hooks cannot inject those contracts.

Live Grok 1.0.24 checks verified final JSON, tool session environment, cold/idle
and busy composer shapes, and double Ctrl+Q exit on an isolated tmux server.
Both disposable tmux servers exited; no operator pane was moved for the probes.
Two early headless probes returned unstructured responses that did not satisfy
the document capture proof; they were left on the retained-intent path after
one diagnostic check each. The correctly structured local fixture committed.

The authenticated MCP probe reached the free usage limit before a complete
closeout. Full live MCP admission-to-finalize/queue verification remains blocked
on renewed quota or the planned subscription. Do not spend more quota or claim
that verification passed. Heavy models are not a prerequisite of the adapter.

The preceding source CI run (b198f06d2) was successful. Full local release checks
and final install/push evidence are recorded below when complete.

## Release proof

Source commit `714382522` and tag `v0.35.359` are pushed. Final `make check tmux-ci` exited 0: 9,302 tests passed, 258 skipped, plus editor/native parity,
SimWorld, Lean/TLA and 21 isolated tmux cases. The private-name audit and staged
diff checks passed. Logs: `/home/brian/.cache/agent-doc-test-tmp/grok-final-proof.log`.

The release CLI/native builds and `make install-full` exited 0. Installed CLI
reports 0.35.359; Grok skill check passed. Project controller recycle returned
`recycled: true` without interrupting the active child. IntelliJ PID 1035669
already runs plugin 0.2.378, so its earlier activation gate is satisfied; the
live unsaved/captured-splice verification remains separate.

One post-push observation showed CI, Release and PyPI in progress, and Book
successful. External CI was not awaited. The Grok authenticated MCP quota gate
remains the only integration-specific live verification remainder.
