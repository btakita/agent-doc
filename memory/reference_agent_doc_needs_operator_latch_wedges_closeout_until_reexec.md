---
name: reference_agent_doc_needs_operator_latch_wedges_closeout_until_reexec
description: "A wedged agent-doc cycle showing `unmet=committed` with everything else converged is the supervisor's process-local needs_operator latch; only an install/recycle reexec clears it pre-0.35.163."
metadata: 
  node_type: memory
  type: reference
  originSessionId: 9e7ad472-d4f9-4b65-8d16-3695ecf9d7d0
  modified: 2026-08-08T04:18:47.354Z
---

**Shape** — `respond` reports the authority/disk divergence INTERRUPT and says the
controller owns the next closeout; the queue stops draining; ops.log repeats
`convergence_gate_defer ... unmet=committed committed=false editor_converged=true
inflight=0 actor_idle=true`. Everything is converged **except** the commit, and
nothing ever performs it.

**Cause** — `captured_finalize_resume` returned `NeedsOperator` (the classifier's
default arm for any error string it does not recognize). That set a
**process-local** latch in the supervisor idle-watch which suppressed *every*
trigger, including the controller's document-state edge. The only other thing that
clears it is a different operation key, which a retained capture never gets. So the
controller kept publishing `ResumeSettledDelivery` with `proof=exact_target` and the
supervisor kept ignoring it. Observed 2026-08-08 on
`src/haiven-dev/tasks/haiven-dev.md` (latched 03:51:25 UTC, ignored a fully-settled
edge at 03:55:48).

**Diagnosis is impossible from logs alone before 0.35.163** — the event carried only
`reason_bytes` + `reason_sha256`. The reason that latched that closeout 27 times was
not in ops.log, `state.db`, or the playback record. 0.35.163 adds a bounded, redacted
`reason_head`.

**How to apply:**
- **Recovery on any version:** `make install` (or `agent-doc admin recycle`) in
  `src/agent-doc`. The supervisor reexecs via `execve_preserve_child`, which resets
  the process-local latch; the retained capture then replays on its own
  (`captured_finalize_resume_started attempt=1` → `commit_success` →
  `captured_finalize_resume_committed repair_outcome=ReplayedResponse`). No
  `--force-disk`, no re-send, no hand-commit — the capture was always recoverable.
- **Distinguish from the stranded shape:** here a cycle IS open and a capture IS
  retained, so [[reference_agent_doc_recovery_index]]'s stranded remedies do not
  apply; the write is genuinely held, just by a driver that stopped driving.
- Fixed in agent-doc commit `a739c20b6` (tracked as `needsoperatorstateedge` in
  `tasks/agent-doc/agent-doc-bugs.md`): a controller document-state edge retires the
  verdict, so the latch self-heals without an install. Related:
  [[feedback_always_install_and_recycle_agent_doc]],
  [[reference_agent_doc_two_degraded_cycle_contract_shapes]].
