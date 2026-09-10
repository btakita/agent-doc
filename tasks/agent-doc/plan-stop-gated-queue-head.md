# Stop hook eligibility for pending queue work

## Contract (2026-09-10)

Invariant: a committed document with only deferred review work cannot acquire
writeback debt merely because a raw queue mirror still names that work.
The Stop hook must agree with the existing queue eligibility policy, while still
preserving genuine exchange debt and ordinary manual queue prompts.

Evidence: v0.35.360 finalized cycle-1789014602820 successfully, with no queue
continuation. The Stop hook then demanded `do [#harnessswitchverify]`, despite
that item being in review under `[operator-verify]`; explicit session-check again
reported committed/no_drainable_work. `first_active_queue_prompt_in_content`
selects the first raw prompt without the queue policy's deferral predicate.

Owner: agent-doc-queue's existing content-derived eligibility policy. The hook
is a one-shot adapter over the current authoritative document observation.
Use its shared predicate for pending manual work and automatic drain work;
do not create a hook-specific gated-id parser, new state, database arbitration,
or retry loop. The live queue graph remains the lifecycle owner.

Transitions:
- Deferred review-only head after committed closeout: allow Stop, no capture.
- Deferred head followed by executable work: select that executable work.
- Real unsaved/exchange prompt: preserve existing writeback guard.
- Manual queue without auto activation: preserve pending-work behavior.
- Explicit stop or a head fence: do not cross the operator's boundary.
- Inline operator verdict: reuse existing eligibility semantics.

Verification: reproduce through the production Stop hook with a committed
review-only fixture and no response capture; test manual and mixed queues,
inline verdicts and stopped queues. Run full make check/tmux-ci, install and
release the correction, then finalize this document cycle. Do not mark the real
unsaved harness-switch proof complete without observing it.

## Completion proof

Implemented v0.35.361 in source commit `89d0f027a`: the Stop adapter calls the
shared queue policy's pending-head projection. Pending manual work and automatic
continuation share the existing eligibility predicate; manual inspection does
not activate a queue. Existing real exchange-debt guards remain in place.

Two regressions failed before the correction, including the production hook
attempting a new capture for a committed review-only fixture. They pass after the
fix, including the git-backed fixture, with no extra response capture. Coverage
also verifies mixed gated/ready queues, ordinary manual prompts, inline operator
verdicts and stop fences.

Full `make check tmux-ci` passed: 9,304 workspace tests (259 skipped), 28 explicit
tmux tests, editor integration, native cross-editor simulation, Lean and TLA.
Privacy and diff checks passed. An older standalone test process outlived a
recompile of its executable, so its self-relaunch timeout test saw ENOENT; the
fresh full suite and a separate fresh timeout-test run both passed. This was a
replaced test executable, not a waived source failure.

`make install-full` succeeded; CLI/native 0.35.361 and the matching existing
editor packages are installed. Source main and tag v0.35.361 are pushed. A normal
controller recycle succeeded without force; live panes remain [%177,%6] with
%6 active. No real unsaved editor harness-switch verdict was manufactured.

Logs: `/home/brian/.cache/agent-doc-test-tmp/stop-gated-{before,focused,check,timeout-proof,install}.log`.
One post-push observation found CI and PyPI in progress, Release queued:
- https://github.com/btakita/agent-doc/actions/runs/34440308473
- https://github.com/btakita/agent-doc/actions/runs/34440310281
- https://github.com/btakita/agent-doc/actions/runs/34440310434

The remaining #harnessswitchverify review needs a disposable live IntelliJ
document and an observed unsaved codex → claude → codex edit with supervisor
receipts. Disk/CLI edits do not prove that editor transition.
