# Queue response evidence and native bootstrap

Invariant: a strict response cannot silently close selected free-text work without
identifying that prompt; native library discovery parses stdout only.

Policy owner: `agent-doc-queue::queue_closeout_guard` owns missing queue answer
evidence. Write entrypoints validate the immutable candidate before capture.
CLI command dispatch owns whether `lib-path` may emit an upgrade notice;
JetBrains owns separation of the subprocess streams.

Transition table: selected free-text plus exact answer/deferral quote passes;
selected free-text plus generic answer fails before capture; unselected, struck,
id-backed, or foreign-exchange work does not trigger the new gate. Existing
completion matching still decides whether a quoted answer is terminal.
For native bootstrap, a successful existing stdout path loads even if stderr
contains notices; a missing path or failed command retains diagnostic evidence.

Evidence inputs: authoritative document, preflight application baseline, proposed
response, parsed queue item selection markers, subprocess exit code and stdout.

Reactive topology: reuse the existing authoritative document observation and
response capture boundary; accepted capture feeds the existing document-scoped
queue completion Computed and Effect. No new polling or retry owner.

Imperative extraction audit: pre-capture validation is a bounded pure transform
of an immutable candidate, with no cache or long-lived derived state. The CLI
bootstrap is an actorless one-shot subprocess; its stderr is inherited so an
unread diagnostics pipe cannot deadlock library loading.

Allowed edit surfaces: queue policy, shared write entrypoint validation, native
loader, CLI startup, regression tests, command specs, release metadata.

Verification: positive/negative queue policy tests, pre-capture rejection test,
seeded newer-version CLI cache test, JetBrains resolver regression, full
`make check`, plugin tests, release build/install and CI status inspection.

Out of scope: fuzzy completion claims, inferred completion from selection,
new controller state, editor restart automation.
