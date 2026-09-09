# Recycle dispatch and Codex thread identity

Invariant: document control prompts never become harness conversation identity;
dispatch pauses only across the supervisor replacement boundary, with a client
deadline that permits the controller's bounded settlement wait to complete.

Policy owners: Codex hook ledger identity provenance and supervisor recycle
statechart. Route uses their existing projections.

Transition table: actual hook → resumable thread; external control prompt →
prompt/cooldown evidence only; legacy external clear without a turn → not resume
authority; parked genuine hook → still resumable. Compile → existing supervisor
remains dispatchable; actual reexec → InFlight; fresh watch loop → Settled;
bounded wait deadline → explicit failure without injecting.

Evidence inputs: hook versus external-prompt producer, exact hook session ID,
turn ID, legacy clear prompt shape; recycle lifecycle events and wait budget.

Reactive topology: existing ProcessScope recycle Source → Computed phase →
dispatch wait/notification; existing document hook ledger → child lifecycle
lineage observation. Compilation does not publish an unsafe-boundary fact. The
controller condition-variable wait consumes its retained reactive phase instead
of re-folding the durable projection in the wait loop.

Imperative extraction audit: remove redundant recycle graph publication from
the waiter. Hook provenance is serialized at ingress and filtered by one ledger
selector; no new polling, synthetic conversation IDs, or latest-global resume.

Allowed edit surfaces: hook state/producers/tests; supervisor auto-install
boundary; controller settlement transport/wait tests; specs and release metadata.
The full native-peer gate exposed duplicate Koffi struct names on reload; extend
the existing VS Code binding adapter and native harness to use anonymous ABI
descriptors and verify repeated reload followed by retained-state reconnect.

Verification: external/legacy control prompts cannot mask the real thread;
parked and turnless genuine hook identities remain valid; controller settlement
after the ordinary RPC deadline still reaches the caller; full `make check` and
`make tmux-ci`, build/install, inspect live target identity and layout.

Out of scope: unsafe pane injection, killing the active task, rewriting the
document directly, widening arbitrary RPC timeouts or waiting through builds.
