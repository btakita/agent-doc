# lazily-interop

`lazily-interop` is the independent supervisor for cross-binding Lazily network
conformance. It imports no Lazily implementation. Peers speak the versioned
NDJSON control contract in [`docs/peer-contract.md`](docs/peer-contract.md);
state-plane frames are checked against the sibling `lazily-spec` checkout.

From this directory:

```sh
make check
cargo run -- run
make network-suite
```

The default run launches two instances of the intentionally independent
reference peer and executes `scenarios/reference-convergence.json`. Production
bindings join by adding commands to `peers.toml`; the scenario language remains
binding-neutral.

Peer groups are capability-driven. A feature scenario runs only implementations
that advertise that production feature. Unsupported implementations remain
visible in `staged_peers` but receive no pass credit. The checked-in C# entry is
staged until Lazily C# ships real IPC and CRDT surfaces. Feature-only scenarios
may leave `required_codecs` and `required_channels` empty; codec and channel
scenarios name those requirements and select the narrower supporting peer group.

`make network-suite` is the single local entrypoint for the implemented network
matrix. It runs the eight-binding semantic cohort, then the five direct-link
profiles over every ordered pair of eligible peers. A peer joins those cells
automatically after advertising the required production feature, codec, channels,
and variants; otherwise it remains visible as staged and receives no pass credit.
`network-suite-pr`,
`network-suite-merge`, and `network-suite-nightly` pin the CI seed schedules and
write machine-readable reports under `reports/`.

`cargo run -- mutation-check` must report all eleven required defect classes as
caught. This is the gate proving the harness can go red before any production
binding receives interop credit.
