# TLA+ / PlusCal model checks

`AgentDocCloseout.tla` is authored as a concurrent PlusCal algorithm and
translated in a temporary directory on every test run. TLC then checks the
generated TLA+ state machine for:

- exact retained-response safety across replay and bounded ACK failures;
- preservation of steering added after the original response capture;
- separation of native-library, installed-package, and live-editor generations;
- adoption of a plugin generation registered after the turn's preflight; and
- eventual package convergence, editor publication, and response commit under
  the PlusCal process fairness assumptions.

Run `make tla`. Set `TLA_TOOLS_JAR=/path/to/tla2tools.jar` to use an existing
TLA+ tools installation. Otherwise the runner downloads the pinned upstream
artifact into `target/tla/` and verifies its SHA-256 digest.
