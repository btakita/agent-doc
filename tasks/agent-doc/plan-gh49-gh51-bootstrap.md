# GitHub #49 / #51 bootstrap follow-up

Invariant: bootstrap preserves one document session identity, treats an absent tmux server as an expected observation without clearing prior registry evidence, and keeps native library stdout machine-readable.

Policy owner: `agent-doc-session-registry-io::tmux_server` owns server observation; the existing pure registry policy owns initialize/keep/replace. Claim owns persisting its generated identity before any provisioning branch. Existing CLI notice policy and JetBrains loader own their already-shipped #49 fix.

Transition table:
- No server / empty observation: no registry mutation; allow startup.
- First valid identity: initialize while preserving migration rows.
- Same identity: keep rows.
- Different identity: clear stale rows and store replacement.
- Malformed nonempty identity or failed query against a live server: actionable error.
- Claim without UUID: persist generated UUID before provisioning; downstream startup uses that UUID.
- Native path with newer cached release: path-only stdout, separate stderr.

Evidence inputs: tmux process output, server presence, parsed PID/start time, durable prior identity, authoritative claim content, existing native-notice regression.

Reactive topology: these changes are actorless startup/protocol boundaries before a managed session exists. Existing document-authority writes and provisioning effects remain the lifecycle ingress; no new graph, poller, retry loop, or cache. Server reconciliation is bounded cold hydration. No imperative derived-state extraction applies to this one-shot absence/parser boundary.

Allowed edit surfaces: registry observation and tests, claim initialization ordering and tests if reproduced, session command spec, source-build compatibility docs, release metadata.

Verification: isolated absent/live/malformed server observations and existing restart transition tests; all four reported CLI start tests; claim UUID persistence; native path regression; full `make check`, full release install, latest CI observation, release publication.

Out of scope: changing general tmux command semantics or introducing an independent coordinator.

## Additional observed isolation defect

The new claim regression exposed a split owner: start honored
`AGENT_DOC_TMUX_SOCKET`, while claim and route obtained default-server handles.
The shared configured tmux constructor now owns that explicit socket selection;
start delegates to it. The integration test verifies provisioning stays on the
selected socket. The two leaked test shell panes were identified by their exact
deleted fixture CWD and removed; the live editor window again has two agent panes.
