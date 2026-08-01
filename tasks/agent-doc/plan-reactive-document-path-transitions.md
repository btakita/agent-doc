# Reactive document path transitions

## Decision

Treat a filesystem rename or move as a retained state transition. Lazily
projections are the source of truth; socket requests are effect attempts and
controller replies are convergence receipts. Do not model rename as a one-shot
imperative `agent-doc sync --rename` command.

The JetBrains plugin does not own durable SQLite state. It retains only its live
path-transition projection and replica frontier. The Project Controller owns
the durable state ledger and performs every SQLite migration.

## Ordered transition

1. Observe the exact old path and new path from the IDE VFS event.
2. Publish one reliable-sync liveness batch ordered as:
   new-path `Open`, new-path `Register`, old-path `Close`.
3. Ask the existing Project Controller to converge the transition.
4. In the controller, move the live CRDT hub and install an old-to-new request
   alias before migrating durable projections.
5. Merge old/new state-event lineages in stable ledger order, retire ACK cursors
   for both hashes, rekey the actor/session registry, and refresh controller
   memory.
6. Register and seed the editor replica at the new path before retiring its
   old-path forwarder.
7. Mark the retained transition converged.

Each phase is replayable. A dropped request or reply retries the same logical
transition ID and original reliable-sync OR-set tags.

## Invariants

- There is never a detached-authority gap between the old and new identities.
- The old replica remains routable until the new replica is registered.
- An in-flight old-path RPC resolves through the controller alias to the moved
  hub.
- A raced new-path preflight may append facts; path convergence merges both
  lineages instead of deadlocking on “already committed.”
- ACKs are receipts, not authority. Both path identities' cursors are retired
  when their version sequences merge.
- The registry key, entry path, pane ID, and window ID move together.
- Rename convergence never invokes tmux layout planning and therefore cannot
  rotate or rebalance the current window.
- `session-check` rejects a response heading without a body, including the
  exchange scramble where a response body appears above its stranded heading.

## Regression coverage

- deterministic retained transition retry and converged-receipt replay;
- live relay hub/identity rekey without a second canonical head;
- liveness `Open`/`Register` before old-path `Close`;
- exact liveness frame retention across enqueue/flush retry;
- split destination history merge and dual-hash ACK retirement;
- actor and registry rekey preserving pane/window binding;
- in-flight old-path controller alias resolution;
- prompt → response heading → body ordering integrity;
- plugin source contains no rename-time CLI process or tmux-layout invocation.
