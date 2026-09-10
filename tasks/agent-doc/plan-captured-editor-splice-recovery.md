# Captured editor splice recovery

Invariant: a captured editor edit must either reach the current canonical replica at its proven range, or remain retained; an obsolete baseline must not create an endless registration retry.

Policy owner: the shared Rust merge layer owns bounded splice rebasing. JetBrains reports the captured edit stream, observes current controller text, and performs registration with that exact canonical hash before applying the returned splices.

Transition table: matching base forwards directly; independently changed canonical ranges translate the captured offsets; already-present whole edit batch is a no-op; overlapping changes or invalid ranges retain the batch; a canonical change during registration rejects before retiring the old replica; a later editor edit rejects projection through the existing generation fence.

Evidence inputs: captured base, ordered code-point splices and resulting cuts, controller canonical text/hash, editor projection epoch and visible cut, registration and durable publication receipts.

Reactive topology: existing editor document worker and event-driven local-edit ingress feed the existing document replica and retained remote editor apply graph. A successful rebased publication feeds the retained canonical projection and the existing generation-fenced editor apply/native-save Effect. No new state owner, timer, SQLite arbitration, or retry loop.

Imperative extraction audit: the bounded splice transform is an immutable command planning operation, not changing cached state. Registration uses the existing controller CAS authority. Remote projection/readiness stays in the existing retained editor apply graph. The current-text RPC is triggered by the captured edit recovery event and does not poll.

Allowed edit surfaces: Rust merge/FFI boundary, JetBrains replica transport and manager, regression tests, affected specs and release metadata. Response replay normalization is included only if its marker corruption reproduces independently.

Verification: independent insertion/deletion, Unicode offsets, sequential edits, already-applied batch, overlap refusal, registration CAS and native/editor projection integration; full make check and isolated tmux checks before release/install.

Out of scope: changing pending session captures, forcing disk authority, restarting active agent panes, or performing the sandbox work owned by fpe.md. The existing fpe.md pane is actively executing that work.

## Refused admission and Stop

The live refusal loop also proved that Stop ignored preflight's outcome and attempted to continue or capture a prior document cycle. The invariant is that a refused harness turn cannot acquire response/queue authority from an older binding. The policy owner is the existing Codex hook prompt envelope and Stop adapter. The preflight entrypoint writes a pending/refused receipt before executing admission and a success receipt afterward; a newer prompt clears it. Late receipts are fenced by exact session, turn ID, and prompt. Stop consumes the receipt before inspecting or mutating document work. This is a persisted boundary-event receipt across short-lived hook processes, not a second document state graph or SQLite arbitration of document transitions. No retained document intent is changed. Coverage proves no capture, no queue advance, newer-prompt reset, and late-receipt refusal.

## Implementation and verification

Source commit `b198f06d2`, release 0.35.358, JetBrains plugin 0.2.378.

The native planner validates the captured stream, translates independent changes at code-point boundaries, recognizes an already-published batch despite unrelated response changes, and refuses overlap. The plugin observes the controller cut, plans before replacing membership, CAS-registers that cut, and forwards the bounded splices. Pending captured edits defer bootstrap display even without a retained delivery. Empty peer deliveries still feed an outstanding canonical projection into the existing generation-fenced editor apply graph.

`make check tmux-ci` passed: 9,291 tests, 258 skipped, editor/native parity and formal checks, plus isolated tmux tests. The native ABI test verifies the shipped export and JSON/offset contract. Hook integration checks both successful capture and a refused second admission without modifying the prior document. Log: `/tmp/agent-doc-358-release-check.log`.

Live `fpe.md` was already actively executing work in pane `%6`; no input or restart was sent to it. The bug document still has the retained intent ending `8352e21c1e291a2156039eb40ce65288abfcbd6e4af3c5f16abd8458e7dde403`. Its editor replica was repeatedly rejecting the stale captured baseline. The running IntelliJ PID 1653132 reports plugin 0.2.376, so Kotlin activation must be checked after installation; no document reset, forced disk write, repeated finalization, or controller recycle was performed.

`make install-full` completed successfully. Installed CLI/native report 0.35.358 and the installed JetBrains package is 0.2.378. A direct call through the installed native ABI preserved both the newer response and captured queue edit with the translated offset. Install log: `/tmp/agent-doc-358-install.log`. The source commit is pushed, and tag v0.35.358 triggers release publication.

Remaining live prerequisite: IntelliJ PID 1653132 still advertises plugin 0.2.376 after installation. The updated Kotlin classes require IDE/plugin activation before the retained live editor recovery can be verified. Do not interpret the native-only hot reload as proof of updated plugin behavior, and do not bypass the retained capture to finish the document. The activation/recovery check remains open.

Subsequent recovery evidence: the controller completed the original capture automatically at 03:26:48 UTC in document commit `1b35dcb028`, followed by `captured_finalize_resume_committed` at 03:26:51. A fresh connected admission succeeded as `cycle-1789011115536`. The old closeout wedge is therefore cleared. Plugin 0.2.378 activation and a live stale-splice regression check remain outstanding; they are no longer prerequisites for resuming this document.
