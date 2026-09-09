# Live queue edits during a turn

Invariant: a newer operator edit to a queue prompt survives stale maintenance and completion; changed prompt text enters the existing turn steering aggregate verbatim.

Policy owners: the component CRDT reconciler owns merge precedence; the realtime baseline comparison owns the steering aggregate. Queue parsing owns prompt boundaries and lifecycle decoration.

Transition table:
- Unchanged prompt plus agent selection/completion decoration: ordinary lifecycle join; no steering.
- Operator changes a live prompt while the agent strikes the old revision: preserve the edited live prompt.
- Operator deletes a prompt: preserve deletion; never resurrect the old revision.
- Replaced live queue prose: include the complete prompt in the aggregate, including multiline content. Independent additions remain queued work and do not preempt the active turn.
- Reorder or marker-only change: preserve placement, without generating a new instruction.
- Editor repeats the same observation: keep one semantic steering identity.

Evidence inputs: immutable turn baseline, canonical current document, parsed queue prompts and completed entries, per-item merge base and both current sides.

Reactive topology: existing document-scoped replica observations publish canonical text, the controller publishes the cycle-fenced steering observation into the existing document projection, and turn/closeout consumers read its aggregate. Extend this existing ingress, without a new polling loop or competing document authority.

Imperative extraction audit: the comparison and leaf merge are pure transforms of immutable input cuts. Reuse the existing reactive projection and event ingress; do not introduce an independent refresh cache. Harness consumption remains through the existing turn projection and safe response boundary, without injecting keystrokes into an active composer.

Allowed edit surfaces: primary cell merge and fallback component merge reconciliation; realtime baseline comparison and controller projection regressions; shared done-ID revision guard and both queue consumption adapters; public specification, version and release surfaces.

Verification: reproduce changed-ID prompt vs stale strike and queue-only steering gaps before fixing; check multiline edits, marker-only changes, completion, reordering, deletion, repeated observations and simultaneous exchange edits; run make check, inspect CI, install and verify release artifacts.

Out of scope: changing the user's project document, replaying its application work, or introducing a new harness transport.

## Implementation evidence

- Both initial regressions failed before their fixes: a stale strike removed the operator's edited queue text, and queue-only edits produced an empty steering set.
- The primary cell merger and fallback keyed merger share the complete instruction normalization. New operator wording wins over agent-only lifecycle decoration; actual disjoint content edits retain the existing merge/conflict policy.
- Done-ID marking and consumption use the same revision eligibility policy. A changed instruction cannot be completed by a receipt for its prior wording.
- The controller's existing cycle-fenced replica event carries the complete queue correction, and a retraction publishes an authoritative empty set with the current content hash. Existing editor subscribers receive the update; harness consumption continues through the existing response boundary.
- The first full check found that independent queue additions were being misclassified as active-turn steering. The comparison now requires replacement of an existing live revision, and a dedicated test retains independent additions as ordinary queued work.
- The done-ID guard likewise distinguishes replacement from an independently added same-ID copy. Its existing duplicate synchronization regression remains green.

## Verification

- Targeted package verification: 269 realtime tests, 299 merge tests, and 123 queue-I/O tests passed (691 total).
- Final workspace test run: 9,275 passed, 256 skipped. The simulator corpus, version synchronization, installed-instruction audit, JetBrains tests, 179 VS Code tests, and real native cross-editor peer simulation passed.
- The final `make check` invocation stopped only at the missing 0.35.354 editor-parity ledger row. After adding the row, `python3 scripts/check_editor_parity.py && make lean tla` passed, completing every remaining gate without repeating the already-passing workspace suite.
- Latest completed CI for the preceding release passed. The existing PyPI upload failure was verified as the project's storage quota (HTTP 400), not a code failure.
- Public private-name scan: no matches. Release installation and final publication evidence follow below.

## Installation and source commit

`make install-full` completed successfully. The installed executable and native ABI both report 0.35.354. The install also synchronized JetBrains 0.2.376 and VS Code 0.2.70. Its normal cleanup removed repository build artifacts, so post-install verification used the installed files and native version export rather than comparing against removed build outputs.

- Installed executable SHA-256: `13abebc1f0a33a7ece690dafaa8ce626ae7bf13824f68a191f8ab791d617486e`.
- Installed native library SHA-256: `0a13ca35959b7b3f2c5ef1083935717f6f512e1cb3af2ddd45696ac380d5f7d7`.
- Workspace controller generation 1188 and affected project controller generation 1111 completed handoff, report `stable`, and prove their running inode equals the installed 0.35.354 executable.
- Source commit: `53b90b4ae` (`fix: preserve live queue instruction revisions`).

Main and release-tag publication follow the session-document closeout. External CI is observed once after push; the existing PyPI quota review remains open.
