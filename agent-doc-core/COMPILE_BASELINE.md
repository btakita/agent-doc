# agent-doc-core compile baseline

Tracks cold-build cost over time as the `#adcr` extraction waves land.
See `tasks/agent-doc/plan-agent-doc-core-extraction.md` for context.

## Measurement protocol

From `src/agent-doc/`:

```bash
# Cold rebuild of just the core crate (includes its dep tree):
cargo clean
cargo build -p agent-doc-core --release --timings
# Note the wall time + transitive dep count.

# Cold rebuild of the full main crate from the same clean state:
cargo clean
cargo build --release --timings
# Note the wall time.
```

The timing-report HTML lands in `target/cargo-timings/`. Record the
total wall time + transitive dep count below.

The acceptance criterion from the plan: `agent-doc-core` cold-builds in
**under 30%** of the main-crate cold build time.

## 2026-05-28 — Wave 0 (stub only)

`agent-doc-core` body is empty. This row establishes the dep-graph
floor: how long it takes to compile the seven pure deps
(`anyhow`, `serde`, `serde_yaml`, `uuid`, `pulldown-cmark`, `yrs`,
`similar`) before any extracted module exists.

| Crate | Wall time | Transitive crates | Notes |
|---|---|---|---|
| `agent-doc-core` (first build, deps included) | 5.70s | 48 | `cargo build -p agent-doc-core --release` from a clean target dir; build includes dep-tree compile |
| `agent-doc-core` (warm, only the stub) | 0.05s | 1 | `cargo build -p agent-doc-core --release` after `cargo clean -p agent-doc-core --release` (deps stay warm) |
| `agent-doc` main crate (incremental on top of core build) | 112s (1m 52s) | 2 new crates after the agent-doc-core build | `cargo build --release` from the state where agent-doc-core is already compiled |

Cold-from-scratch baselines for both crates have not yet been captured
on the same `cargo clean` boundary — they should be the first thing
collected at the start of `#adcr` Wave 1. The acceptance threshold
(30%) is anchored against those clean numbers.

## Wave log

Append a row per wave landing. Include:

- Date
- Wave + extracted module(s)
- agent-doc-core cold wall time
- agent-doc main-crate cold wall time
- agent-doc-core LOC after extraction
- Ratio (agent-doc-core / agent-doc) — must be ≤ 0.30 at Wave 4

| Date | Wave | Modules | core wall time | main wall time | core LOC | ratio |
|---|---|---|---|---|---|---|
| 2026-05-28 | 0 (stub) | (none) | tbd cold | tbd cold | 0 | n/a |
| 2026-05-28 | 1–4 (all shipped) | id, crdt, component, model_tier, pending, frontmatter, project_config, template, diff (pure half) | **9.87s** (74 transitive crates) | **129s / 2m 09s** (266 transitive crates) | 19,907 | **0.0765 (7.65%)** ✓ ≪ 30% |

### 2026-05-28 — Cold baseline measurement (Wave 4 landing, `#k5s7`)

Both numbers measured with `CARGO_TARGET_DIR=/tmp/adoc-baseline-target`
on a clean target dir so the host's
`~/work/btakita/agent-loop/src/agent-doc/target/` (with root-owned
icu build artifacts from earlier sudo builds) was bypassed. Identical
hardware, sequential runs.

```bash
# agent-doc-core cold
rm -rf /tmp/adoc-baseline-target
CARGO_TARGET_DIR=/tmp/adoc-baseline-target time cargo build -p agent-doc-core --release --timings
# real    0m9.889s  (Finished release profile in 9.87s)
# transitive: 74 .rlib in deps/

# agent-doc main cold (separately, fresh target dir)
rm -rf /tmp/adoc-baseline-target
CARGO_TARGET_DIR=/tmp/adoc-baseline-target time cargo build --release --timings -p agent-doc
# real    2m9.084s  (Finished release profile in 2m 09s)
# transitive: 266 .rlib in deps/
```

Acceptance criterion #1 from `plan-agent-doc-core-extraction.md`
satisfied with substantial headroom: **7.65%** is well under the **30%**
threshold. The editor-plugin slim-link / FFI consumer / eval-runner
slim-dep targets that the extraction was designed to unblock are
quantitatively justified — the core layer cold-builds **~13× faster**
than dragging the orchestration crate.

Dep-count delta (266 → 74 = **192 crates pruned** for core-only
consumers) is the bulk of the win, not LOC compile cost. The pruned
deps include the heavyweight orchestration tree:
`tokio`/`tokio-util`/`tokio-stream`, `reqwest`, `hyper`/`hyper-tls`,
`rustls`, `interprocess` (Unix socket IPC),
`notify`/`notify-debouncer`, `which`/`shell-words`/`shell-escape`,
`rusqlite`/`zstd`/`ureq`/`tagpath`, plus the eval-runner companion
crate. Editor plugins and CI/eval consumers carry none of these.

### Next-step interpretation

- `#k9e1` (`#adcr-ffi-relocate`) — the 192-crate gap is what unlocks
  the editor-plugin slim-link target. Relocating pure C-ABI wrappers
  into `agent-doc-core::ffi` cashes in the gap.
- `#ysv9` (`#adoc-orchestration-crate`) — the 129s main-crate build is
  bounded above by the orchestration deps (tokio, hyper, rustls,
  interprocess, notify, etc.), not core data-layer work. A further
  split into `agent-doc-orchestration` would let the CLI shell stay
  ~10s cold and push the heavy deps behind a clean boundary.
- `#5yqz` (`#adcr-eval-runner-switch`) — eval-runner currently links
  `agent-doc` for `strip_comments`; switching to `agent-doc-core`
  directly drops it from a 129s build into a 9.87s build for the
  pipeline that needs only document parsing.
