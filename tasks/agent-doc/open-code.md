---
agent_doc_session: 7910a62f-ef5f-4f02-9703-dade1f077521
agent: claude
claude_model: opus
opencode_model: GLM-5.1
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: true
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777e65629bf156ef66517a44d0d9c829c7e89b02b868f8ca9d0519d3ddc-20260528-224507.md`*

Compacted content:
- Archived 2 response topic(s): do #epv5; do #vb8h
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777...

### Re: do #e130 — apply_patch FFI relocated; FFI surface complete — opus-4-7

Executed `do [#e130]` (`#k9e1-ffi-apply-patch`) — **the final FFI relocation**. The three `apply_patch` variants now live in `agent-doc-core::ffi`, and with them the entire pure-FFI surface is in core.

#### What landed

Submodule `3eeecd5f` on `20260422/codex-agent-backend`, pushed. Superproject `a8ff03528` on `main`, pushed.

**Relocated to `agent-doc-core/src/ffi.rs`:** `agent_doc_apply_patch`, `agent_doc_apply_patch_with_caret`, `agent_doc_apply_patch_with_boundary`.

**The `dummy_path` hack is gone.** Each function previously called the file-based `template::apply_patches_with_overrides(doc, …, Path::new("/dev/null"), …)`. They now call `apply_patches_with_overrides_pure` directly with `summary = None` and empty component/max_lines configs — the correct values for an editor FFI call that patches in-memory content with explicit mode overrides and no backing file. `FfiPatchResult` + helpers were already in core from `#epv5`. Removed the now-unused `use crate::{component, template};` from main; `force_link_core_ffi_symbols` references all three.

**Minor behavior note:** the pre-patch boundary id no longer carries a `:null` slug (it was derived from `/dev/null`'s file stem — an accidental artifact of the dummy path). No test depended on it; verified by repo-wide search.

#### Verification

- `cargo build` / `cargo build --release`: green.
- **`nm -D libagent_doc.so`**: 39 `agent_doc_` symbols export, including all 3 `apply_patch` variants — the full editor-plugin FFI surface resolves from the main cdylib unchanged.
- `cargo test --bin agent-doc`: **2,261 pass**; `--lib`: 96 pass (both unchanged).
- `cargo clippy -- -D warnings`: clean.
- Both binaries refreshed.

#### `#k9e1` FFI relocation — complete

All 15 pure FFI functions now live in `agent-doc-core::ffi` across the four sub-tasks (`#k9e1` POC + `#epv5` + `#vb8h` + `#e130`). The editor-plugin slim-link target — linking only `agent-doc-core` (9.87s cold, 74 crates) instead of main (129s, 266 crates) — is now **unblocked** for the document-mutation surface.

#### Recommendation on the tracker item

`#yybc` (`#k9e1-ffi-mem`) tracked the relocation pattern "for future pure FFI relocations" — and those are all done. The item has no remaining purpose; I'd strike it from the queue (it sits behind the publish/orchestration items) rather than spend a cycle on a no-op. Leaving that to your call since you own the queue ordering.

Plan: tasks/agent-doc/plan-agent-doc-core-extraction.md

**Not invoking `/loop`** — the next queue item is an operator decision (publish `agent-doc-core` to crates.io — SemVer commitment, ownership readiness) that I can't execute unilaterally.

### Re: do #77v6 — agent-doc-core v0.1.0 published to crates.io — opus-4-7

Executed `do [#77v6]` (`#adcr-publish`). You answered the readiness question with "Defer," then explicitly typed **`publish`** — I took that as overriding the modal and authorizing the irreversible action, and published.

**`agent-doc-core v0.1.0` is now live on crates.io** (verified via `cargo search agent-doc-core` → `agent-doc-core = "0.1.0"`).

#### What landed

Submodule commits on `20260422/codex-agent-backend` (pushed): `53fb239a` (remove `publish = false`), `1a86397b` (VERSIONS.md). Tag `agent-doc-core-v0.1.0` pushed. Superproject `3f6769280` on `main`, pushed.

- **`agent-doc-core/Cargo.toml`**: removed `publish = false`.
- **`cargo publish --dry-run`**: clean — packaged 17 files (153.8 KiB compressed), compiled standalone from the tarball.
- **`cargo publish -p agent-doc-core`**: `Uploaded` + `Published agent-doc-core v0.1.0 at registry crates-io`.
- **VERSIONS.md**: Unreleased entry recording the publish + slim-link rationale.
- **Tag**: `agent-doc-core-v0.1.0` (the existing `v0.9.x` tags are the main `agent-doc` line; cratesub-crate gets its own tag prefix).

No GitHub Release — `agent-doc-core` is a library (no binary artifact); the GitHub-Release-with-binary convention applies to the main `agent-doc` CLI.

#### What this unblocks

Third-party FFI consumers and the editor-plugin slim-link target can now depend on the published `agent-doc-core` (~9.87s cold, 74 crates) instead of the full `agent-doc` orchestration crate (129s, 266 crates) for the document-mutation + pure-FFI surface. The 15 relocated FFI functions (`#k9e1`/`#epv5`/`#vb8h`/`#e130`) are all in this published crate.

#### Caveat — 0.1.0 SemVer

The published API is now a SemVer commitment and cannot be unpublished (only yanked). It reflects the freshly-stabilized post-extraction surface. Future breaking changes need a minor bump (alpha software — VERSIONS.md already flags this).

Plan: tasks/agent-doc/plan-agent-doc-core-extraction.md

**Not invoking `/loop`** — the queue advances to `do [#ysv9]` (`#adoc-orchestration-crate`), a large multi-cycle refactor that warrants its own scoping cycle. (`#yybc` remains a no-op tracker behind it — still recommend striking it.)

### Re: do #ysv9 — orchestration-crate extraction plan scoped — opus-4-7

Executed `do [#ysv9]` (`#adoc-orchestration-crate`). This is a **planning** task ("Plan agent-doc-orchestration extraction"), so the deliverable is the scoping document, not code. Plan written, committed, pushed; implementation captured as a follow-up backlog item.

#### What landed

Superproject `bbc418c8c` on `main`, pushed: `tasks/agent-doc/plan-agent-doc-orchestration-extraction.md`.

#### Key findings from the analysis

- **Cluster size:** the named orchestration modules total **~44,516 LOC (~29% of the 154K main crate)** — `sync` 11.4K, `git` 8.4K, `route` 8.1K, `start` 5.6K, `supervisor/` 3.5K, `repair` 3.3K, `sessions` 1.7K, `capture` 1.1K, `cycle_state` 1.0K, `ipc_socket` 0.4K.
- **Dependency shape** (drives sequencing): `sessions`/`ipc_socket`/`supervisor/` are leaves; `git ↔ cycle_state` is a near-one-way knot (git→cycle_state 35 refs, cycle_state→git **1**); `capture ↔ repair` are mutual; `route`/`sync`/`start` are top orchestrators depending on nearly everything.
- **Strategy:** because the cluster has internal cycles, it can't split into multiple crates — **move it as ONE `agent-doc-orchestration` crate** (mutual deps become intra-crate). The real work is the boundary to the CLI shell (~100 command modules + `main.rs` reference the cluster; shims absorb this during waves, like the `#adcr` wave pattern).
- **FFI:** keep a single cdylib in the CLI crate — the 11 orchestration FFI fns move to `agent_doc_orchestration::ffi` and re-export via `pub use` + the proven `force_link` pattern. No editor-plugin changes.
- **Heavy deps that leave the CLI shell:** `tokio`, `reqwest`/`ureq`/`hyper`/`rustls`, `interprocess`, `notify`, `rusqlite`, git, `zstd`, `tmux-router` — confirmed all present in main today.

#### Plan contents

Goal/3-layer target, module inventory + LOC, intra-cluster dependency graph, move-as-one-crate rationale, 5-wave migration sequencing (scaffold → leaves → state/capture → git → orchestrators → prune), CLI/orchestration boundary, single-cdylib FFI re-export, risks, acceptance criteria, and a `#adoc-orch-*` per-wave decomposition.

#### Follow-up captured

Added `#adoc-orchestration-impl` to the backlog (references the plan path). It's the multi-cycle implementation — a large refactor the operator should consciously schedule; each wave is its own green-to-green cycle.

Plan: tasks/agent-doc/plan-agent-doc-orchestration-extraction.md

**Not invoking `/loop`** — queue head advances to `do [#yybc]`, which is the obsolete no-op tracker (the FFI relocation pattern it tracked is fully done). Recommend striking it rather than spending a cycle; `#adcr` (umbrella, materially complete) sits behind it.

### Re: orchestration extraction — wave 0+1a landed — opus-4-7

You inserted `do [#bz6s]` at the queue front and chose **Start wave 0+1 now**. I scaffolded the `agent-doc-orchestration` crate and moved the one genuinely dependency-free leaf. `#bz6s` stays open — this is the first of 5+ waves.

#### What landed

Submodule `0c06479c` on `20260422/codex-agent-backend`, pushed. Superproject `144511f42` on `main`, pushed.

**Wave 0 — scaffold:** new workspace member `agent-doc-orchestration` (lib `agent_doc_orchestration`, deps `agent-doc-core`/`anyhow`/`interprocess`/`serde_json`, dev `tempfile`) + `COMPILE_BASELINE.md`.

**Wave 1a — move the leaf:** `ipc_socket.rs` (414 LOC, **zero `crate::` refs**) → orchestration. Main re-exports via `pub use agent_doc_orchestration::ipc_socket`; the ~8 lib consumers (`boundary`/`git`/`write`/`ffi`/`ops_report`/`sim_world`/`supervisor`) and `main.rs`'s `pub(crate) use agent_doc::ipc_socket` resolve unchanged.

#### Scope correction (important)

The plan called wave 1 "move `sessions`, `ipc_socket`, `supervisor/`" as leaves — but that classification was by *intra-cluster* deps. The **full `crate::` closure** tells a different story:
- `ipc_socket` — 0 `crate::` refs → true leaf ✓ (moved).
- `sessions` — pulls in `input_diag`, `project_controller`, `session_actor`, `startup_miss`.
- `supervisor/` — pulls in `env`, `harness`, `input_diag`, `test_support`.

Moving `sessions`/`supervisor` now would create a **circular `agent-doc` ↔ `agent-doc-orchestration` dependency**. I deferred them to **wave 1b**, which must first resolve that closure (move or break the references to those 7 modules). Recorded in `COMPILE_BASELINE.md` and the `#bz6s` backlog text.

#### Verification

- `cargo build` / `build --release`: green.
- `cargo test -p agent-doc-orchestration`: **11 pass** (the moved ipc_socket tests).
- main lib: **85** (was 96 — the 11 moved to orchestration, no loss); bin: **2,261** (unchanged); integration: **13/14** (the 1 failure is the pre-existing codex env test — no new IPC regressions).
- `cargo clippy -- -D warnings`: clean.
- `nm -D libagent_doc.so`: **39** `agent_doc_` symbols still export.
- Both binaries refreshed.

Plan: tasks/agent-doc/plan-agent-doc-orchestration-extraction.md

**Not invoking `/loop`** — `#bz6s` is a multi-wave effort; wave 1b needs the `sessions`/`supervisor` dependency-closure decision (move the 7 cascade modules vs. break the refs), which deserves its own scoping. `do [#bz6s]` stays queued for the next wave.

### Re: do #bz6s — wave 1b blocked; bottom-up sequencing superseded — opus-4-7

Continued `#bz6s` (orchestration extraction). I analyzed the wave-1b closure before moving anything — and the analysis says **don't move it the way the plan assumed**. No code moved this cycle; the deliverable is the corrected scoping + a halt for your decision.

#### The finding

`sessions`/`supervisor` are leaves only by *intra-cluster* deps. Their full `crate::` closure pulls in `input_diag`, `env`, `harness`, `project_controller`, `session_actor`, `startup_miss`, `test_support`, then transitively `ops_log`, `config`, `prompt`, `fs_util`, `snapshot`, `harness_prompt`. Two facts kill the bottom-up move:

1. **`snapshot → git`.** `snapshot` (a shared I/O utility) depends on `git`, a not-yet-moved cluster module. So the closure transitively reaches `git` and, through it, the rest of the cluster. There's no self-contained leaf set beyond `ipc_socket`.
2. **Shared-util breadth.** `snapshot` is used by **35** files, `ops_log` by **27**, `config` by **11** — across *both* CLI commands and orchestration. They can't move to pure `core` (they do `std::fs`), and orchestration can't depend back on `agent-doc` (circular).

So moving `sessions`/`supervisor` now would either create a circular `agent-doc ↔ agent-doc-orchestration` dependency or drag in `git` + ~15 more modules. I halted rather than force a non-compiling or architecturally-wrong move.

#### Decision needed (revised options, in the plan)

- **A — one large orchestration crate.** Move the whole entangled graph (cluster + shared I/O utils + sessions/supervisor/session_actor/project_controller/…) in essentially one migration; `agent-doc` keeps only `main.rs` + command modules touching none of it (most reach orchestration via `pub use` shims). Delivers the CLI-shell build win; large, multi-cycle, scheduled deliberately.
- **B — 4th `agent-doc-base` layer** for the shared I/O utils. Cleaner long-term but **blocked by `snapshot → git`** — `snapshot` must drop/invert its `git` dependency first.
- **C — defer.** Wave 1a already proved the crate + cross-crate shim + cdylib mechanism; the pure editor slim-link is already served by `agent-doc-core`. Hold the rest until a concrete build-time payoff justifies the large migration.

**My recommendation:** A is the only option that yields the CLI-shell win without a `snapshot`/`git` refactor, but it's a deliberate multi-cycle effort — not something to auto-queue. B is cleaner but front-loads decoupling `snapshot` from `git`. C is the low-effort hold.

Plan (REVISION section): tasks/agent-doc/plan-agent-doc-orchestration-extraction.md (superproject `6d2c4485e`).

**Not invoking `/loop`** — `#bz6s` is blocked on the A/B/C decision; auto-looping would re-hit a blocked wave. I'd suggest pausing the queue (`queue_active: false`) until you pick a direction. `#yybc`/`#adcr` behind it remain effectively closed (recommend striking).

### Re: queue blocked at #bz6s — reconciling drift — opus-4-7

No new prompt this cycle — the diff was leftover working-tree drift from the wave-1b halt: the `(HEAD)` heading marker (a boundary artifact, by design) plus the `#bz6s` backlog one-liner refinement that the live-editor IPC guard wouldn't absorb last cycle. This cycle commits that refinement and settles the working tree. No new follow-up items.

**Nothing new to execute.** `#bz6s` is blocked on the A/B/C architectural decision (one large orchestration crate / 4th `agent-doc-base` layer / defer) documented in the plan REVISION and last response. The queue head `do [#bz6s]` will keep re-surfacing a blocked wave on every invocation until you either:

- pick a direction (reply A, B, or C), or
- set `queue_active: false` to pause, and/or strike `~do [#yybc]~` / `~do [#adcr]~` (both effectively closed — the FFI pattern they reference is fully relocated, and `#adcr`'s four acceptance criteria are all met).

I'm not striking them or picking the direction for you — those are your calls. Standing by for A/B/C.

Plan: tasks/agent-doc/plan-agent-doc-orchestration-extraction.md
Direction A

### Re: Direction A — foundation leaves moved (increment 2) — opus-4-7

You picked **Direction A** (one large orchestration crate). I'm executing it incrementally, dependency-order, green-to-green. This increment moved the three zero-dependency foundation leaves.

#### What landed

Submodule `d568e6bb` on `20260422/codex-agent-backend`, pushed. Superproject `015155f69` on `main`, pushed.

- **Moved to `agent-doc-orchestration`:** `env`, `fs_util`, `secret_redact` (no `crate::` refs; deps `anyhow`/`indexmap`/`regex`). Orchestration now holds 4 modules (+ `ipc_socket`).
- Added `indexmap` + `regex` to the orchestration manifest (my earlier dep scan missed them).
- `fs_util::read_optional_text`/`read_optional_bytes`: `pub(crate)` → `pub` (now consumed across the crate boundary).
- Shims: `main.rs` `pub(crate) use agent_doc_orchestration::{env, fs_util, secret_redact}`; `lib.rs` re-exports `secret_redact` (it lived in both bin+lib trees).

#### Not movable yet

`config` looked like a foundation module but depends on `crate::project_config::{project_tmux_session,…}` — **main's file-based `project_config_io` wrappers**, not core. Moving it now would create a circular dep. It comes after `project_config_io` in the next increment.

#### Verification

- Build (debug + release) green; `clippy -- -D warnings` clean.
- Orchestration tests: **40** (11 ipc + 29 moved: env 12 + fs_util 2 + secret_redact 15).
- main lib **70** (−15), bin **2,232** (−29) — **no coverage lost** (secret_redact previously ran in *both* bin and lib trees; now once in orchestration).
- `nm -D`: **39** FFI symbols still export. Both binaries refreshed.

#### Remaining under Direction A (dependency order)

`config` + `project_config_io` → `sessions`/`supervisor` closure (`input_diag`/`harness`/`project_controller`/`session_actor`/`startup_miss`/`test_support`/`prompt`/`ops_log`/`snapshot`) → cluster (`cycle_state`/`capture`/`repair`/`git`/`route`/`sync`/`start`). Many cycles; each green-to-green.

Plan: tasks/agent-doc/plan-agent-doc-orchestration-extraction.md

**Not invoking `/loop`** — Direction A is a long multi-cycle migration; I'll advance one safe increment per invocation. Note the queue head is `do [#yybc]` (the obsolete tracker, struck-then-superseded); to continue the extraction next time, re-queue `#bz6s` or just say "continue". `#yybc`/`#adcr` remain strike candidates.

### Re: queue reconciled — #bz6s staged, queue paused — opus-4-7

Committing your queue edits and clearing the recurring snapshot/boundary drift. Managed-component edits only — no code this cycle.

#### What you changed (now committed)

- `queue_active: true` → `false` — queue paused.
- Moved the re-added `do [#bz6s]` above `do [#yybc]` — the orchestration extraction is staged as the next active item.

#### Not executing — queue is paused

`queue_active: false` is an explicit stop, so I'm not auto-running the next Direction-A increment. When you're ready to continue the extraction, set `queue_active: true` (or just say `continue`) and I'll pick up the next increment: `project_config_io` + `config`, then the `sessions`/`supervisor` closure, then the cluster (`cycle_state`/`capture`/`repair`/`git`/`route`/`sync`/`start`).

Current orchestration crate: 4 modules (`ipc_socket`, `env`, `fs_util`, `secret_redact`). `#yybc`/`#adcr` behind `#bz6s` remain strike candidates (both effectively closed).

Plan: tasks/agent-doc/plan-agent-doc-orchestration-extraction.md

### Re: continue #bz6s — increment 3: project_config_io + config — opus-4-7

You re-activated the queue (`queue_active: true`), so I picked up the next Direction-A increment. Two more modules moved to `agent-doc-orchestration`.

#### What landed

Submodule `60df7f9c` on `20260422/codex-agent-backend`, pushed. Superproject `289022bd2` on `main`, pushed.

- **`project_config_io`** — the `&Path`/`std::fs` project-config wrappers. Its only `crate::project_config` reference was a doc comment, so it depends solely on `agent_doc_core::project_config` (+ `anyhow`/`toml`). Cleanly movable.
- **`config`** — rewrote its references: `agent_doc::model_tier` → `agent_doc_core::model_tier`, `crate::frontmatter` → `agent_doc_core::frontmatter`, and the `project_tmux_session`/`update`/`clear` re-exports now come from `crate::project_config_io` (sibling in orchestration).
- Added `serde` + `toml` to the orchestration manifest. Shims in `main.rs` (bin) + `lib.rs` (`project_config_io`). Main's `project_config.rs` shim keeps re-exporting the io wrappers (now from orchestration); `#[allow(unused_imports)]` covers the tmux helpers that are consumed cross-crate via `crate::config::*`.

Orchestration now holds **6 modules**: `ipc_socket`, `env`, `fs_util`, `secret_redact`, `project_config_io`, `config`.

#### Verification

- Build (debug + release) green; `clippy -- -D warnings` clean.
- Orchestration tests: **52** (+12: config 7 + project_config_io 5).
- main lib **65** (−5), bin **2,220** (−12) — no coverage lost.
- `nm -D`: **39** FFI symbols still export. Both binaries refreshed.

#### Next increment (dependency order)

The `sessions`/`supervisor` closure: `input_diag`, `harness`, `project_controller`, `session_actor`, `startup_miss`, `test_support`, `prompt`, `ops_log`, then `snapshot` (which depends on `git`, pulling the cluster). After that the cluster itself (`cycle_state`/`capture`/`repair`/`git`/`route`/`sync`/`start`).

**Heads-up on session size:** the exchange is at ~222 lines (accretion "warn"). When you're at a stopping point, consider `/agent-doc compact tasks/agent-doc/open-code.md` to archive the older turns before continuing.

Plan: tasks/agent-doc/plan-agent-doc-orchestration-extraction.md

**Not invoking `/loop`** — advancing one safe increment per invocation. Re-activate (`queue_active: true`) or say `continue` for the next.

### Re: continue #bz6s — increment 4: ops_log + find_project_root → fs_util — opus-4-7

Picked up the next Direction-A increment. One more shared-utility module moved to `agent-doc-orchestration`, plus a small decoupling that unblocks the `ops_log`/`input_diag` chain.

#### What landed

- **`ops_log` → orchestration.** Best-effort operational logging (`log_op`, `log_cycle`, `content_hash`, `CycleEntry`) now lives in `agent-doc-orchestration`. `main.rs` re-exports it via `pub(crate) use agent_doc_orchestration::ops_log;` so every `crate::ops_log::*` call site resolves unchanged. Added `sha2 = "0.10"` to the orchestration crate — ops_log's only new external dep.
- **`find_project_root` pulled down into `fs_util`.** `ops_log` was the lone blocker still reaching back into the main crate's `snapshot` module (`snapshot::find_project_root`, 2 call sites). That helper is a pure `.agent-doc/`-ancestor path walk with no git/snapshot coupling, so it belongs in `fs_util` (already in orchestration). `snapshot::find_project_root` is now a one-line re-export shim — its 8 internal callers and the wider 47-file `find_project_root` surface compile unchanged.

Shared-util foundation now at 7 modules in orchestration (`ipc_socket` / `env` / `fs_util` / `secret_redact` / `config` / `project_config_io` / `ops_log`) before the harder `snapshot → git` keystone that gates the sessions/supervisor cluster.

#### Verification

- `make check` green (clippy + full suite) across all three crates: 2216 + 65 + the smaller suites all passed, 0 failed. The single red is the pre-existing live-tmux environmental test `codex_bare_run_inside_owning_pane_fails_before_nested_dispatch` (codex nested-dispatch routing assertion — unrelated to a logging/path-util relocation). `make check` exits 0 tolerating it, per the documented "modulo the live-tmux environmental test".
- `cargo build --release` green; `nm -D target/release/libagent_doc.so` exports the full 39-symbol `agent_doc_*` FFI surface intact (this increment touched no FFI).
- ops_log's 4 inline tests moved with it and pass under the orchestration crate.

#### Next

Backlog #bz6s updated — ops_log struck from the NEXT list. The remaining sessions/supervisor closure is gated by `snapshot → git`: `snapshot` is used by 35 files and its `git` dependency transitively reaches the whole cluster (the keystone the plan's REVISION flagged). Next increment should either invert `snapshot`'s git coupling or move a smaller still-independent module before committing to the large cluster migration.

### Re: continue #bz6s — increment 5: input_diag → orchestration — opus-4-7

Moved `input_diag` (structured tmux/supervisor input diagnostics) into `agent-doc-orchestration`. This was unblocked by increment 4: input_diag's only production dependency is `crate::ops_log`, which is now in orchestration.

#### What landed

- **`input_diag` → orchestration.** All 10 helpers (`log_key_event`, `log_text_submit`, `log_byte_events`, `log_transform_event`, `log_prompt_detection`, `format_*`, `verbose_enabled`, `KeyEventMeta`) promoted `pub(crate)` → `pub` and moved. `main.rs` re-exports via `pub(crate) use agent_doc_orchestration::input_diag;`, so its 6 consumers (`start`, `route`, `sync`, `sessions`, `queue_dispatch`, `supervisor/pty`) resolve unchanged.
- **Self-contained orchestration `test_support`.** input_diag's tests mutate env vars (`AGENT_DOC_TMUX_INPUT_DIAG`, `AGENT_DOC_DEBUG_STDIN`) under a process-global lock. Since cargo compiles each crate's tests into a *separate* test binary, a crate-local `#[cfg(test)] TEST_ENV_LOCK` is correct and sufficient — no cross-crate static sharing needed. Added a minimal `env_lock` / `ProcessGlobalLockGuard` (the depth-reentrant pattern, mirroring main's). Verified orchestration has no other env-mutating tests to race with. Main keeps its own `test_support` (still used by `prompt`, `harness_prompt`, etc.).

Orchestration is now at **8 modules** (`ipc_socket` / `env` / `fs_util` / `secret_redact` / `config` / `project_config_io` / `ops_log` / `input_diag`).

#### Verification

- `make check` green (clippy + full suite): 2213 (main, down 3 — input_diag's tests moved out) + orchestration's input_diag suite (3 passed) + the other crates, 0 failed. Only red is the documented pre-existing live-tmux test `codex_bare_run_inside_owning_pane_fails_before_nested_dispatch`; `make check` exits 0 tolerating it.
- `cargo build --release` green; `nm -D libagent_doc.so` exports the full 39-symbol `agent_doc_*` FFI surface intact (no FFI touched).

#### Next — decision point

This exhausts the clean zero/low-dependency leaf moves. What remains all hits real coupling:

- `harness_prompt` / `test_support` (shared one) → gated by **`codex_hook`** (82K).
- `prompt` → gated by **`sessions`** (cluster).
- `snapshot` → gated by **`git`** — and `git` transitively reaches the whole entangled cluster (`cycle_state` / `capture` / `repair` / `route` / `sync` / `start`).

So the next increment is no longer mechanical. Per the plan's REVISION (operator decision needed), the realistic options are: **(A)** invert `snapshot`'s `git` dependency (pass a commit/hash interface in) so `snapshot` can sit below orchestration, then the cluster can follow; or **(B)** a deliberate big-bang move of the entire entangled cluster + `sessions`/`supervisor` in one large diff. The build-time payoff (CLI shell without tokio/reqwest/git2/etc.) only arrives once the heavy-dep cluster moves — the leaf moves so far shrink main but don't yet change its dependency tree. Recommend scheduling (A) or (B) deliberately rather than auto-queuing, since both are large single cycles.
❯ Do the big bang change.

### Re: Do the big bang change — opus-4-7

Done — the full Direction-A cluster migration landed green. Submodule `cc6be806`, superproject pointer `c92df0565`.

#### What landed

- **52 modules now in orchestration.** The entire entangled cluster (`snapshot`/`git`/`cycle_state`/`capture`/`repair`/`route`/`sync`/`start`) + `sessions`/`supervisor` + their neighbors (`run`/`stream`/`watch`/`compact`/`codex_hook`/`harness`/`project_controller`/`session_actor`/`session_check`/`preflight`/`prompt*`/`queue`/`gc`/`claim`/`focus`/`agent`/`flow`/… + `frontmatter_io`/`template_io`/`security`). `agent-doc` is now a CLI shell over orchestration over core.
- **No body rewriting.** Orchestration mirrors the 5 core-backed lib shims (`component`/`crdt`/`frontmatter`/`project_config`/`template`) so every `crate::X` in the moved modules resolves intra-crate; `agent_doc::` lib-path refs were rewritten to `agent_doc_core::` (`model_tier`, `id`) / `crate::` (`debounce`, `merge`, `template`). The CLI shell keeps `pub use` shims so its ~100 call sites compile unchanged.
- **Deps + build.rs.** Orchestration gained the heavy-dep union and a `build.rs` for `AGENT_DOC_BUILD_TIMESTAMP`.
- **Cross-crate test fixtures (the tricky part).** `IsolatedTmux` re-imported from `tmux_router`; the CLI shell's `test_support` got its own crate-local `TEST_ENV_LOCK` (separate test binary); and `project_controller`'s in-process controller test short-circuits are now exposed through a new **`test-support` cargo feature** that the shell enables in `dev-dependencies` — this restores pre-extraction behavior for the one shell test (`session_actor_cmd … controller_lease …`) that depended on a moved `#[cfg(test)]` runtime shortcut. Source-inspection guard tests (`test_cli`, `debounce_gaps`, `full_content_static`) were repointed to the orchestration paths.

#### Verification

- `make check` green (clippy `-D warnings` + full suite, 600+ tests across all three crates) **modulo** the pre-existing live-tmux environmental test `codex_bare_run_inside_owning_pane_fails_before_nested_dispatch` (now in orchestration's test binary; failing identically at increments 4/5).
- `cargo build --release` green; `nm -D libagent_doc.so` exports the full **39-symbol** `agent_doc_*` FFI surface intact (`ffi.rs` stays in the CLI shell lib).
- One genuine regression found and fixed mid-cycle: the cross-crate `#[cfg(test)]` controller short-circuit (confirmed it passed at increment 5, failed after the move) → resolved with the `test-support` feature, not waived.

#### Next — Wave 5 (the actual build-time payoff)

The move itself does **not** yet change the CLI shell's dependency tree — `agent-doc` still depends on orchestration, which pulls all the heavy deps. Acceptance criteria #2–3 are delivered by Wave 5: prune the now-transitive-only direct deps from the CLI shell's `Cargo.toml`, measure the CLI-shell cold build, and record it in `COMPILE_BASELINE.md`. `#bz6s` stays open for that. Recommend running it as its own focused cycle.
<!-- agent:boundary:fa89fde9 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- ~do [#k5s7]~
- ~do [#n7dd]~
- ~do [#0ejf]~
- ~do [#ckv3]~
- ~do [#0c4e]~
- ~do [#bjrv]~
- ~do [#k9e1]~
- ~do [#5yqz]~
- ~do [#epv5]~
- ~do [#vb8h]~
- ~do [#e130]~
- ~do [#77v6]~
- ~do [#ysv9]~
- ~do [#bz6s]~
- ~do [#bz6s]~
- do [#bz6s]
- do [#yybc]
- do [#adcr]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#bz6s] #adoc-orchestration-impl agent-doc-orchestration extraction per tasks/agent-doc/plan-agent-doc-orchestration-extraction.md (DIRECTION A) — BIG-BANG COMPLETE. 52 modules now in orchestration; 3-layer architecture (CLI shell -> orchestration -> core) done. CLI shell keeps pub use shims; new test-support cargo feature exposes project_controller's in-process test short-circuits to the shell's tests. make check green (clippy + 600+ tests) modulo the pre-existing live-tmux codex_bare_run; 39-symbol FFI surface intact. NEXT (Wave 5): prune now-transitive-only heavy deps from the CLI shell Cargo.toml (acceptance #2), measure CLI-shell cold build, record COMPILE_BASELINE.md (#3) — this is what delivers the build-time payoff.
- [ ] [#yybc] #k9e1-ffi-mem POC follow-up: track the proven pattern (pub use agent_doc_core::ffi::* + force_link_core_ffi_symbols reference function in main) for future pure FFI relocations. POC moved agent_doc_free_string + agent_doc_free_state; nm verification confirmed symbols still in libagent_doc.so export table.
- [ ] [#adcr] Extract agent-doc-core crate (~17K lines) — enables fast-compiling FFI/editor-plugin target independent of orchestration layer
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/open-code.done.md -->
<!-- /agent:done -->
