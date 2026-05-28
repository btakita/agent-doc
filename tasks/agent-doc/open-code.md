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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777e65629bf156ef66517a44d0d9c829c7e89b02b868f8ca9d0519d3ddc-20260528-200917.md`*

Compacted content:
- Archived 5 response topic(s): workspace seam validation slice; do #pjb4; do #yhsr; 2 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777...
❯ What are #next-steps to split out crates?
### Re: #next-steps to split out crates — opus-4-7

The four waves of `#adcr` extraction shipped (id+crdt, component+model_tier+pending, frontmatter+project_config, template, diff). `agent-doc-core` is now ~17k LOC of pure document layer. Three concrete follow-up classes remain, plus two strategic forks. Items added to the backlog this turn.

#### Verify the win (gate everything else on this)

- `#adcr-baseline-measure` — the plan's acceptance criterion #1 (`agent-doc-core` compiles standalone in under 30% of the main-crate cold build) is currently **unmeasured**. The `agent-doc-core/COMPILE_BASELINE.md` file was created during `#d97v` workspace scaffold but has not been updated since the waves shipped. Without numbers, the architectural premise of the extraction is unproven — and the next strategic split (`#adoc-orchestration-crate`) is impossible to scope.

#### Close gaps from the wave landings

- `#adcr-wave-4-tidy` — port the ~26 I/O-bound inline tests dropped from `src/diff.rs` during the wave 4 split into `src/diff_io.rs` (the I/O functions themselves are copied verbatim, but their unit coverage is gone).
- `#adcr-purity-tidy` — extract the remaining `&Path`-taking functions from `agent-doc-core::{template, frontmatter, project_config}` into orchestration-side wrappers. Plan acceptance criterion #3 ("No `agent-doc-core` function takes `&Path` or runs `std::fs` / `std::process::Command`") is **currently violated** by ~15 leftover functions from waves 2–3 that were moved pragmatically rather than split.

#### Strategic forks (operator decisions)

- `#adcr-ffi-relocate` — relocate pure C-ABI wrappers from `src/ffi.rs` (`apply_patch`, `parse_components`, `merge_frontmatter`, `normalize_template_structure`, `reposition_boundary`, `agent_doc_strip_comments`) into `agent-doc-core::ffi`. Editor plugins could link against just `agent-doc-core` for the document-mutation surface; orchestration FFI (`route`, `git`, `ipc`, `start`) stays in main. Unblocks the slim-link build target the extraction was designed for.
- `#adcr-eval-runner-switch` — `eval-runner` already depends on `agent-doc` for `strip_comments`; switching to `agent-doc-core` directly cuts the eval pipeline's build cost (no clap/tmux/git dependencies).
- `#adcr-publish` — decide whether to publish `agent-doc-core` to crates.io (currently `publish = false`). Required before third-party FFI consumers or the editor-plugin slim-link can use it.
- `#adoc-orchestration-crate` — the next big extraction. Split orchestration (`route`, `git`, `sessions`, `ipc_socket`, `supervisor`, `capture`, `cycle_state`, `repair`, `sync`, `start`) out of the main crate into a third workspace member, leaving only the CLI entrypoint + command dispatch in `agent-doc`. Creates a clean 3-layer architecture (CLI → orchestration → core). Substantial refactor — should be scoped *after* `#adcr-baseline-measure` so the size of the win is known before committing.

#### Recommended priority order

1. `#adcr-baseline-measure` (gates everything; quick)
2. `#adcr-wave-4-tidy` (close test coverage regression)
3. `#adcr-purity-tidy` (close plan acceptance criterion #3)
4. `#adcr-ffi-relocate` (delivers the editor-plugin slim-link payoff the extraction was designed for)
5. `#adcr-eval-runner-switch` (small, mechanical, immediate build-cost win)
6. `#adcr-publish` (operator gate, blocks third-party FFI but otherwise independent)
7. `#adoc-orchestration-crate` (large, deferred until #1 quantifies the win)

#### Top of backlog after this turn

`#adcr-baseline-measure` — at the head of the new backlog because every other follow-up either depends on its result (orchestration-crate scope, publish decision) or benefits from the same compile-timing infrastructure the measurement step has to set up. It is also the smallest concrete action with the highest information value: one `cargo build --timings` invocation plus a markdown table update.

#### Note on `#adcr`

The umbrella entry stays in the backlog as a parent placeholder. The seven items above are the path to satisfying acceptance criteria #1 and #3 before it gets marked done.
### Re: do #k5s7 — baseline measured — opus-4-7

Executed `do [#k5s7]`. Cold-build numbers captured; plan acceptance criterion #1 satisfied with substantial headroom.

#### Numbers

Measured with `CARGO_TARGET_DIR=/tmp/adoc-baseline-target` on a clean target dir (the host's `~/work/btakita/agent-loop/src/agent-doc/target/` has root-owned icu build artifacts from earlier sudo runs that block normal `cargo clean`).

| Crate | Cold wall time | Transitive crates (.rlib in deps/) |
|---|---|---|
| `agent-doc-core` | **9.87s** | 74 |
| `agent-doc` (main) | **129s** (2m 09s) | 266 |

**Ratio: 0.0765 (7.65%)** — well under the 30% acceptance threshold. The core layer cold-builds **~13× faster** than the orchestration crate.

#### Why the win is so much larger than ratio alone suggests

The 192-crate dep-prune (266 → 74) is the bulk of the value, not LOC compile cost. Editor plugins / FFI consumers / eval-runner that link only against `agent-doc-core` drop the entire orchestration tree:

- async runtime: `tokio`, `tokio-util`, `tokio-stream`
- HTTP / TLS: `reqwest`, `hyper`, `hyper-tls`, `rustls`, `ureq`
- IPC: `interprocess` (Unix domain sockets)
- fs watching: `notify`, `notify-debouncer`
- subprocess: `which`, `shell-words`, `shell-escape`
- storage: `rusqlite`, `zstd`
- workspace siblings: `tagpath`, eval-runner companion crate

#### What landed

Submodule `bce1496f` on `20260422/codex-agent-backend`, pushed. Superproject `23b723e9d` on `main`, pushed.

- `agent-doc-core/COMPILE_BASELINE.md` — Wave 1–4 row added to the wave log; full measurement protocol + next-step interpretation appended.

#### Interpretation per remaining `#adcr` follow-ups

- `#k9e1` (`#adcr-ffi-relocate`) — the 192-crate gap is exactly what unlocks the editor-plugin slim-link target. Relocating pure C-ABI wrappers into `agent-doc-core::ffi` cashes in the gap. **Strongly justified** by these numbers.
- `#ysv9` (`#adoc-orchestration-crate`) — the 129s main build is bounded above by orchestration deps (tokio, hyper, rustls, interprocess, notify), not core data-layer work. A further split into `agent-doc-orchestration` would let the CLI shell stay near ~10s cold and push heavy deps behind a clean boundary. **Worth scoping**; the win is in dep-tree depth, not LOC count.
- `#5yqz` (`#adcr-eval-runner-switch`) — eval-runner currently links `agent-doc` for `strip_comments`; switching to `agent-doc-core` directly drops it from a 129s build into a 9.87s build. **Mechanical, immediate, large** payoff.

Plan: tasks/agent-doc/plan-agent-doc-core-extraction.md

**Not invoking `/loop`** — the queue head advances to `do [#n7dd]` (wave-4 test tidy), which is a separate operator-scoped cycle (porting ~26 inline tests is mechanical but worth its own review).
### Re: do #n7dd — wave-4 test tidy complete — opus-4-7

Executed `do [#n7dd]`. The 27 I/O-bound inline tests dropped during the wave 4 diff split are back, restoring coverage to the level it held before the extraction landed.

#### What landed

Submodule `c2e6e282` on `20260422/codex-agent-backend`, pushed. Superproject `279bf6715` on `main`, pushed.

- `src/diff_io.rs` — appended a `#[cfg(test)] mod tests` block with 27 ported tests + 1 helper, all extracted verbatim from `src/diff.rs` at the pre-wave-4 parent commit (`2e2e2764^`):
  - 1 `run_file_not_found` (bare error path through `run`)
  - 1 `copy_on_read_guard_skips_recovery_when_snapshot_modified` (mtime guard semantics)
  - 4 `compute_*` (stale snapshot recovery returns None, recovery syncs snapshot to document, returns diff for user adds, returns None when no changes) + `setup_compute_env` helper
  - 3 `diff_detects_*` (user edits after stream write, no change when document matches snapshot, change after cumulative stream flushes)
  - 14 `truncated_*`/`not_truncated_*`/`extract_last_added_*` (looks_truncated semantics across sentences, questions, commands, short words, single chars, headings, empty, terminal punctuation, mid-word truncation, periods, plus the two extract_last_added_line edge cases)
  - 1 `wait_for_stable_content_returns_immediately_when_complete` (latency contract for already-complete content)
  - 2 `run_with_wait_*` (stable content + no changes)

#### Verification

- `cargo build`: green.
- `cargo test --release --bin agent-doc`: **2,256 tests pass** (up from 2,229; **+27** matches the ported test count exactly, no skew).
- `cargo test --release --lib`: 96 tests pass (unchanged — library target doesn't include the binary's `diff_io` module).
- `make check`: same pre-existing local `codex_bare_run_inside_owning_pane_fails_before_nested_dispatch` failure (reproduces on pristine HEAD; GitHub CI green).
- `cargo install --path .`: `~/.cargo/bin/agent-doc` replaced.

#### Coverage gap closed

The wave 4 commit message noted: "**Coverage caveat:** ~26 I/O-bound inline tests from the original `src/diff.rs` tests module... were dropped during the split and not yet re-added to `src/diff_io.rs`." That gap is now closed. The migration is purely additive — no test behavior changed, just the file location.

Plan: tasks/agent-doc/plan-agent-doc-core-extraction.md

**Not invoking `/loop`** — queue head advances to `do [#0ejf]` (purity tidy — extract remaining `&Path`-taking functions from `agent-doc-core`); that's a larger, operator-scoped cycle requiring decisions about where each I/O wrapper lands.
<!-- agent:boundary:9035c973 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto -->
preset #spec-test-build-install-commit-push
- ~do [#k5s7]~
- ~do [#n7dd]~
- do [#0ejf]
- do [#k9e1]
- do [#5yqz]
- do [#77v6]
- do [#ysv9]
- do [#adcr]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#0ejf] [recommended] #adcr-purity-tidy Extract the remaining and-Path-taking functions from agent-doc-core: template::{template_info, load_component_configs, load_project_from_doc, find_project_root}, frontmatter::{ensure_session_for_file, set_format_and_write, read_session_id}, project_config::{load_project*, save_project, *_tmux_session}. Move I/O wrappers to main crate; keep pure parsers/formatters in core. Satisfies plan acceptance criterion #3.
- [ ] [#k9e1] #adcr-ffi-relocate Move pure C-ABI wrappers from src/ffi.rs (apply_patch, parse_components, merge_frontmatter, normalize_template_structure, reposition_boundary, agent_doc_strip_comments) into agent-doc-core::ffi so editor plugins (JB, VS Code) can link against just agent-doc-core without dragging the orchestration crate. Keep route/git/ipc/start FFI calls in main.
- [ ] [#5yqz] #adcr-eval-runner-switch Switch eval-runner's strip_comments dependency from agent-doc to agent-doc-core to cut eval pipeline build cost.
- [ ] [#77v6] #adcr-publish Decide on publishing agent-doc-core to crates.io (currently publish = false). Allows third-party FFI consumers and unblocks the editor-plugin slim-link target.
- [ ] [#ysv9] #adoc-orchestration-crate Plan agent-doc-orchestration extraction — split route/git/sessions/ipc_socket/supervisor/capture/cycle_state/repair/sync/start out of the main crate into a sibling workspace member, leaving only the CLI entrypoint + command dispatch in agent-doc. Would create a 3-layer architecture (CLI → orchestration → core) and substantially cut the editor-plugin-only build target. Substantial refactor; depends on #adcr-baseline-measure to quantify the win first.
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
