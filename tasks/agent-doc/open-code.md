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
queue_active: false
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
<!-- agent:boundary:bbd38963 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto -->
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
- do [#bz6s]
- do [#yybc]
- do [#adcr]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#bz6s] #adoc-orchestration-impl Implement agent-doc-orchestration extraction per tasks/agent-doc/plan-agent-doc-orchestration-extraction.md. DIRECTION A chosen (one large orchestration crate). DONE: wave 0 scaffold + ipc_socket + env/fs_util/secret_redact (4 foundation modules in orchestration). NEXT: config + project_config_io closure, then sessions/supervisor closure, then cluster (cycle_state/capture/repair/git/route/sync/start). Each green-to-green with make check + nm -D gate.
- [ ] [#yybc] #k9e1-ffi-mem POC follow-up: track the proven pattern (pub use agent_doc_core::ffi::* + force_link_core_ffi_symbols reference function in main) for future pure FFI relocations. POC moved agent_doc_free_string + agent_doc_free_state; nm verification confirmed symbols still in libagent_doc.so export table.
- [ ] [#adcr] Extract agent-doc-core crate (~17K lines) — enables fast-compiling FFI/editor-plugin target independent of orchestration layer
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/open-code.done.md -->
- 2026-05-27 [#fr8q] [recommended] Compact this session (116 exchange lines, block-level accretion) before next implementation turn
- 2026-05-27 [#n89s] #agent-doc-bug Fix `dispatch_only_dispatch_start_proof_required` to return `false` for opencode harness (routed_reopen.rs:319) — interim fix until tracker is built
- 2026-05-27 [#xd6k] #agent-doc-bug Add model name validation in `canonical_model_name` or probe builder for opencode harness — reject bare names without provider prefix (model_tier.rs:256, codex.rs:534)
- 2026-05-27 [#adcr] Extract agent-doc-core crate (~17K lines: template, diff, component, frontmatter, crdt, pending, model_tier + leaf modules) — enables fast-compiling FFI/editor-plugin target independent of orchestration layer
- 2026-05-28 [#stqc] #adcr-dep-audit Survey inbound deps from orchestration code (preflight/plan/cycle_state/dispatch/route/ipc/git/harness) into {template, diff, component, frontmatter, crdt, pending, model_tier}; identify leaf vs coupled modules — precondition for #adcr
- 2026-05-28 [#5x1r] #adcr-api-surface Define agent-doc-core export surface (pure types: DocumentTemplate, Component, Frontmatter, Diff patches, Crdt ops, Pending model, ModelTier resolution) — keep orchestration types out
- 2026-05-28 [#rtx6] [recommended] #adcr-snapshot-seam Decide diff.rs ↔ snapshot.rs seam — either extract Snapshot data type into agent-doc-core and keep load/save/resolve/path_for in orchestration, or inject snapshot through a trait. Required before extracting diff.
- 2026-05-28 [#d97v] #adcr-workspace Convert src/agent-doc into Cargo workspace root; add agent-doc-core stub crate; record cargo build --timings baselines per tasks/agent-doc/plan-agent-doc-core-extraction.md
<!-- /agent:done -->
