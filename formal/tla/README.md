# TLA+ / PlusCal model checks

The models are authored as concurrent PlusCal algorithms and translated in a
temporary directory on every test run.

`AgentDocCloseout.tla` checks:

- exact retained-response safety across replay and bounded ACK failures;
- preservation of steering added after the original response capture;
- separation of native-library, installed-package, and live-editor generations;
- adoption of a plugin generation registered after the turn's preflight; and
- eventual package convergence, editor publication, and response commit under
  the PlusCal process fairness assumptions.

`PassiveTmuxSync.tla` checks the exact-visible editor-tab sync boundary:

- authoritative actor lookup is available only inside the owning Project
  Controller and never from a standalone safe-passive request;
- a controller-local proof permits the target actor to atomically swap with the
  stale visible actor while preserving a unique visible/stashed partition;
- neither request autostarts an actor; and
- fair execution eventually applies the controller-local request and blocks the
  external request.

Run `make tla`. Set `TLA_TOOLS_JAR=/path/to/tla2tools.jar` to use an existing
TLA+ tools installation. Otherwise the runner downloads the pinned upstream
artifact into `target/tla/` and verifies its SHA-256 digest.
