# Plan: roll out one interop peer to every Lazily binding

Parent: `tasks/software/plan-lazily-interop-network-suite.md`
Contract: `src/lazily-interop/docs/peer-contract.md`

## Rule: adapters exercise the library

An interop peer is a thin executable over the binding's public implementation.
It MAY translate NDJSON control DTOs into library calls. It MUST NOT contain a
second CRDT runtime, hand-written wire mirror, alternate stamp ordering, or
fixture-specific expected values. If the binding lacks a required surface, the
peer reports it unsupported and the binding work remains open.

Every peer must:

- flush one response for every control request;
- use the assigned `PeerId` and logical time;
- encode and decode with the binding's production codec;
- expose a canonical, stable-order snapshot without mutating state;
- own link resources by link id and close them deterministically;
- write diagnostics only to stderr;
- exit 0 only after `bye`;
- support a `--self-check` that runs the peer-contract handshake without the
  network orchestrator.

## Shared acceptance sequence

Each binding lands through the same gates:

1. control-protocol schema tests;
2. local semantic transcript against canonical fixtures;
3. cross-language JSON send/receive against two already-green bindings;
4. cross-language msgpack send/receive;
5. duplicate, reverse-order, partition/heal, and resync scenarios;
6. every applicable channel profile;
7. one mutation proving the harness names that binding as the culprit;
8. full binding `make check`, peer self-check, and orchestrator smoke.

No binding is marked shipped from a self-round-trip.

## Entrypoints and binding work

| Binding | Planned entrypoint | Production surfaces the peer wraps | First gate |
|---|---|---|---|
| Rust | `src/bin/lazily-interop-peer.rs` | `CrdtPlaneRuntime`, IPC codecs, sync driver, FFI/arena/channel adapters | reference pair, never oracle |
| Zig | `src/interop_peer.zig` + `zig build interop-peer` | distributed runtime, canonical JSON codec, FFI, arena, DataChannel | pair with Rust; retain the frontier/key mutations |
| Python | `python -m lazily.interop_peer` | `crdt_plane`, `ipc`, FFI/arena and transport modules | pair with Rust and Zig |
| Kotlin | Gradle `interopPeer` application | `CrdtPlane`, `Ipc`, `LazilyFfiBoundary`, `ShmBlobArena`, channel adapters | pair with Rust and Zig |
| JavaScript | `bin/interop-peer.mjs` | `CrdtPlane`, `IpcMessage`, browser/Node channel adapters | Node loopback first, browser real-stack profile second |
| Dart | `dart run bin/interop_peer.dart` | `CrdtPlane`, IPC streaming codec, FFI/arena/channel adapters | VM profile first, browser carve-outs explicit |
| Go | `cmd/lazily-interop-peer` | distributed plane, `IpcMessage`, FFI/arena/channel adapters | race-enabled peer tests |
| C++ | `tools/interop_peer.cpp` | `CrdtPlaneRuntime`, keyed/positional codec, FFI/arena/channel adapters | ASan/UBSan ownership profile |
| C# | `src/Lazily.InteropPeer` console project | only real Lazily IPC/CRDT/sync code after `#lzcsipc`/`#lzcssync` | blocked until those surfaces ship |

Entrypoint names are part of the plan, not wire authority; a binding may choose
the idiomatic package location as long as `peers.toml` is stable.

## Rollout batches

### Batch A — Rust and Zig

Use the two most mature distributed implementations to harden the contract.
They must both run as sender and receiver for canonical JSON and msgpack.
Preserve mutations for:

- frontier encoded as an object instead of `[peer, stamp]`;
- absent `CrdtOp.key` instead of explicit null;
- explicit null rejected;
- welcome roster unsorted;
- duplicate op counted as applied.

Deliverable: a green two-peer suite plus mutation tests that fail for the right
peer and direction.

### Batch B — Python, Kotlin, JavaScript, Dart

These bindings already expose structured wire codecs and channel abstractions.
Land one peer per repo; do not share generated source between them. The
orchestrator contract is the shared artifact.

Browser-capable runtimes must declare separate VM/Node and browser profiles so a
browser FFI carve-out cannot silently hide a native-runtime omission.

Deliverable: six bindings green in a pairwise star against both Batch A peers,
then one six-way convergence scenario.

### Batch C — Go and C++

Run Go with the race detector and C++ with sanitizers in addition to the common
suite. C++ must test both its schema-shaped keyed msgpack codec and positional
codec as distinct codec capabilities; positional success never substitutes for
canonical msgpack.

Deliverable: all eight currently complete bindings green, plus sanitizer/race
artifacts attached to failures.

### Batch D — C#

Dependency order:

1. `#lzcsipc`: real Snapshot/Delta/CrdtSync types and schema-validated codecs;
2. `#lzcssync`: CRDT runtime, anti-entropy, outbox, and sync driver;
3. peer adapter;
4. channel adapters.

The C# peer is not allowed to deserialize into test-only DTOs and claim
conformance while the Lazily library lacks the same types.

Deliverable: nine-way convergence and C# as both sender and receiver on every
applicable channel.

## `peers.toml`

The central registry records commands, cwd, build command, startup timeout, and
profiles. It contains no expected capabilities; the peer advertises those live,
and the suite compares them with the required profile.

Example:

```toml
[peers.lazily-go]
command = ["./build/lazily-interop-peer"]
cwd = "../lazily-go"
build = ["go", "build", "-race", "-o", "build/lazily-interop-peer", "./cmd/lazily-interop-peer"]
profiles = ["native"]
```

CI builds each peer once per job and reuses the executable across scenarios.
The orchestrator records the binding commit SHA and executable hash in results.

## Per-binding completion checklist

- Peer implementation and contract tests live in the binding repo.
- `peers.toml` registration lives in `lazily-interop`.
- The binding's `make check` invokes peer self-check but not the whole matrix.
- The central suite invokes real pairwise tests.
- README documents the executable as test infrastructure, not a production
  daemon.
- CI uploads peer stderr, transcript, minimized replay, and matrix row.
- Binding commit is pushed before the central registry points at it.
