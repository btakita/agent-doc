---
name: reference_lazily_release_targets
description: Release/publish targets + ownership for the 6 lazily ports (crates.io/PyPI/npm/GitHub)
metadata: 
  node_type: memory
  type: reference
  originSessionId: 4f0c363c-493a-415d-a9d0-a2c8dea9d6b5
---

Release/publish map for the lazily ports (verified 2026-06-24, session lazily-rs.md):

- **lazily-rs** → crates.io crate `lazily` (OWNED by `btakita`; remote git@github.com:lazily-hub/lazily-rs.git) + GitHub release. Live **v0.19.0** (published 2026-07-02: StateChart gained a typed Rust `ChartBuilder`/`StateBuilder`/`TransitionBuilder` (definition in Rust *or* JSON via shared `ChartDef::from_states`) + `ThreadSafeStateChart` (feature `thread-safe`) + `AsyncStateChart` (feature `async`); Harel transition algorithm extracted context-free into `engine_send`/`engine_compute_exit_enter`/`engine_restore_via_history` — semantics byte-identical, 7/7 conformance + 104 lib tests green). **NOTE:** the `statechart` module (Harel `StateChart`/`ChartDef`) landed ~v0.16; it is NOT in older published `lazily` (0.13.x only has flat `state_machine.rs`). In lazily 0.18+, `ThreadSafeContext`/`ThreadSafeStateMachine` are gated behind the `thread-safe` feature (were default in 0.13). Has `ci.yml` + `mdbook.yml` but **no release workflow** — publish is manual `cargo publish` (crates.io token in `~/.cargo/credentials.toml` + `CARGO_REGISTRY_TOKEN`, non-interactive, no 2FA). Flow that worked 2026-07-02: bump Cargo.toml → `cargo build` (refresh lock) → commit/push → `cargo publish --dry-run` → `cargo publish` → `git tag vX.Y.Z` + push tag. (Older v0.13.0 flow also ran `update-benchmark-results.py --no-run`; skip if BENCHMARKS version test not touched.)
- **lazily-spec** → mdBook → GitHub Pages (`mdbook.yml` deploys on push to main). No registry. "publish" = push (auto-deploys site).
- **lazily-zig** → GitHub release only (Zig has no central registry; consumed via `zig fetch`). Released **v0.2.0** (minor: Signal/StateMachine/ShmBlobArena/allocator-modes/before_change + breaking `.direct`→`.literal`).
- **lazily-py** → PyPI `lazily` (OURS — author Brian Takita). Live **0.11.0** (published; #kb9b done — tag v0.11.0 + GitHub release + PyPI all live, verified 2026-06-25). Manual twine + passkey 2FA (no PyPI publish workflow; `.github/workflows/` only has `precommit.yml`).
- **lazily-zig** → GitHub release only. Live **v0.2.0** (tag + release, 2026-06-25).
- **lazily-kt** → **no package-registry infra** (no Maven/Sonatype) → the GitHub release IS the publish. Released **v0.1.0** 2026-06-25 (tag + GitHub release at lazily-hub/lazily-kt; `./gradlew test` green). Remote: git@github.com:lazily-hub/lazily-kt.git.
- **lazily-js** → npm `@lazily/js` (scoped). v0.1.0 GitHub release done 2026-06-25 (tag + release; `npm test` 28/28); **npm publish PENDING operator 2FA** (#lzjsnpmpub): npm is E401 (not logged in), `@lazily/js` unpublished (first scoped publish) → `! npm login` then `cd src/lazily-js && npm publish --access public`. No CI/OIDC publish workflow.

PyPI/npm publishes are operator-driven (interactive browser/passkey 2FA). zig/kt "publish" = GitHub release (agent-doable, no 2FA). See [[reference_efs_d1_token_working]] for "ours vs theirs" registry checks.

**"Publish all" drain (2026-06-25, lazily-rs.md):** operator authorized publishing all remaining bindings. py+zig were already live; kt+js GitHub-released this cycle; only js npm publish left (operator 2FA, #lzjsnpmpub). After draining all 5 `parity. release + publish` queue heads (all struck `~~`), preflight kept reporting `queue_drainable_head_count: 4` with `no_changes: true` — **stale struck-head residue, NOT real work**; do not loop (#qchurn). The `go` marker + `queue: start` keep re-arming; operator sets `queue: stop` / drops `go` to halt.
