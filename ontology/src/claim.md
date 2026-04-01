# Claim

## [Ontology](../../../existence-lang/ontology/src/ontology.md)

The binding between a [Document](./document.md) path and a tmux pane, establishing which pane owns a given [Session](./session.md) document. A Claim is an [Agreement](../../../existence-lang/ontology/src/agreement.md) — a registered assertion by a tmux pane that it is the active agent for a specific document.

Claims are stored as entries in `~/.local/share/agent-doc/sessions.json`. Each entry records the document path hash, the tmux target (`session:window.pane`), the process ID of the running Claude CLI, and the session UUID for resume.

## [Axiology](../../../existence-lang/ontology/src/axiology.md)

Claims matter because they enforce the one-session-per-document invariant. Without a claim registry, two tmux panes could both attempt to respond to the same document simultaneously, producing conflicting writes and corrupted history. The claim is the lock — not a filesystem flock (which guards atomic writes), but a registry-level agreement that a given pane has authority over a given document.

`agent-doc claim` establishes a new claim for the current pane. Claims are protected: if the target pane is already claimed by a different session (and alive), the claim is refused unless `--force` is passed. This prevents silent corruption when position detection falls back to the wrong pane. `agent-doc resync` validates all live claims, removing entries whose panes are dead or whose processes are no longer running. The [Route](./route.md) algorithm reads claims to dispatch commands correctly.

## [Epistemology](../../../existence-lang/ontology/src/epistemology.md)

### Pattern Expression

A Claim is the agent-doc instance of a [Pattern](../../../existence-lang/ontology/src/pattern.md) found wherever multiple agents compete for exclusive access to a shared resource: database row locks, Kubernetes leader election, DNS record ownership. The common properties — a claimed resource identity, a claiming party identity, and a validity condition — apply directly. In agent-doc, the resource is the document, the claiming party is the tmux pane, and validity is confirmed by checking that the registered PID is still running the expected process.

The `autoclaim` module implements automatic claim registration on `SessionStart` events, so that new panes claim their document without requiring explicit user action.
