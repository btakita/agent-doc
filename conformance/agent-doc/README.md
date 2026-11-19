# Agent-doc state conformance fixtures

Agent-doc owns the [state vocabulary](../../schemas/agent-doc-state.json)
and the canonical snapshot and delta in this directory. The fixtures use
lazily-spec's generic graph envelope with agent-doc-specific tags and payloads.

The snapshot at epoch 3 followed by the delta from epoch 3 to 6 advances the
closeout cycle from `preflight_started` to `committed`, completes the selected
queue head, and adds an applied transport patch.

The Rust producer tests read these files directly. The JetBrains and VS Code
tests read byte-identical copies from their test resources. When changing the
fixtures, update both editor copies and rerun the Rust and editor tests.
