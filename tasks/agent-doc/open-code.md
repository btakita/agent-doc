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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777e65629bf156ef66517a44d0d9c829c7e89b02b868f8ca9d0519d3ddc-20260529-023721.md`*

Compacted content:
- Archived 3 response topic(s): #next-steps to split out modules; do [#adoc-pure-to-core]; do [#adoc-pure-to-core]
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777...
### Re: do [#adoc-sqlite-isolation] — opus-4-7

Ran the feasibility spike. Verdict: **the archive_index-only split is clean and feasible, but it does not achieve the stated goal.** Marking the spike done and redirecting the work via `[#adoc-sqlite-seam]` (new, intentionally open follow-up).

#### What I found

**1. `archive_index` is well-isolated.** It owns its own DB (`.agent-doc/archive-index.db`) and schema, has no reference to `project_controller`, and `project_controller` has no reference to it. Its only intra-crate couplings are two pure helpers: `compact::parse_topic_sections` and `frontmatter::parse`.

**2. There is a cycle to break before extraction.** `compact.rs:837` calls `archive_index::index_archive`, while `archive_index` calls `compact::parse_topic_sections`. Crates can't be mutually dependent, so extraction requires moving `parse_topic_sections` (a pure `&str → (String, Vec<String>)` text fn, no rusqlite) down into `agent-doc-core`. `frontmatter::parse` already exists in core, so that side is free.

**3. Consumers to repoint** (all mechanical): `compact.rs` (`index_archive`), `response_toc.rs` (`list_recent_turns`, `fetch_turn_window`), and the CLI shell `main.rs` (`run_index`, `run_search`).

**4. The blocker is real and decisive.** `rusqlite` (bundled) is a *direct* dependency of `agent-doc-orchestration` through **two** independent consumers: `archive_index` **and** `project_controller` (4471 LOC, cluster-central, its own state DB). Confirmed rusqlite appears nowhere else — not in `agent-doc-core`, not in the CLI shell. So extracting only `archive_index` leaves `project_controller` still pulling `rusqlite`, which means orchestration still links `libsqlite3-sys` and the bundled SQLite C build stays coupled to orchestration. The C amalgamation is already cached at the `libsqlite3-sys` level — it does not rebuild on orchestration Rust edits — so the only real costs are the clean-build critical path and relink, both of which remain while `project_controller` keeps rusqlite.

#### Recommendation

Do **not** ship the `archive_index`-only carve as originally scoped — it adds a crate boundary for ~zero build-isolation payoff. The lever that actually isolates the C build is `project_controller`: add an interface seam (pass a `Connection` handle / storage trait in) so its SQLite code drops to a lower `agent-doc-sqlite` crate, and co-locate `archive_index` there. Captured as `[#adoc-sqlite-seam]`.

No code changed this cycle — spike only, so no spec/test/build/install was required.
### Re: do [#adoc-sqlite-seam] — opus-4-7

Implemented the **foundation** of the seam and landed it green. Being precise about what's done vs. what isn't: **build isolation is not yet achieved** — that needs Phase C, which is a larger, regression-sensitive refactor of cluster-central code. `#adoc-sqlite-seam` stays **open**.

#### Done this cycle — Phase A+B (submodule `0bae9a72`, superproject `0e77d0e6f`, pushed)

- **Phase A (cycle break):** moved `parse_topic_sections` / `parse_topic_sections_with_tail` / `TopicSections` (pure text parsing) out of orchestration's `compact.rs` into a new `agent_doc_core::topic` module. This is the prerequisite that lets `archive_index` leave orchestration without a `compact ↔ archive_index` crate cycle.
- **Phase B (new crate):** created `agent-doc-sqlite` (depends only on `agent-doc-core` + `rusqlite`/bundled), moved `archive_index.rs` into it, repointed its `frontmatter::parse` to core. Orchestration now does `pub use agent_doc_sqlite::archive_index`, so every existing call site (`compact`, `response_toc`, CLI `main`) is unchanged. Added to workspace members.

#### Why this alone doesn't meet the goal

`rusqlite` was a direct dep of orchestration through **two** consumers. I moved one (`archive_index`); `project_controller` is the other and still pulls `rusqlite` into orchestration's graph, so the bundled-SQLite C build is **not** yet isolated from orchestration recompiles.

#### Phase C — the remaining (substantive) work

Investigation showed `session_actor` delegates **all** persistence to `project_controller`'s DB functions, but those handlers interleave SQL with `ops_log`, `session_actor`, and projection logic. So Phase C isn't a wholesale move — it's surgically extracting ~15 SQL functions into a `StateDb` seam in `agent-doc-sqlite`, moving the storage types down, repointing `session_actor` + `handle_*`, and dropping `rusqlite` from orchestration's normal deps (dev-dep for the ~10 DB tests). Refined into the backlog item. I scoped it precisely rather than rush a risky refactor of the session/dispatch lifecycle into a broken state on a dog-fooded tool.

#### Verification

`cargo build` full workspace green; core+sqlite tests **652 passed**; `cargo build --release` + `cargo install --path .` both succeeded (binaries current). `make check` is clean **except** one pre-existing failure — `codex_bare_run_inside_owning_pane_fails_before_nested_dispatch` (`run_integration.rs`), a codex/tmux-dispatch test that touches none of the relocated code. I confirmed it fails on clean HEAD with my changes stashed, so it is **not** a regression from this work.
### Re: do [#adoc-sqlite-seam] — Phase C complete — opus-4-7

Phase C done — the seam is complete and the build-isolation goal is met. `#adoc-sqlite-seam` is marked done.

#### What landed (submodule `81fb4bc4`, superproject `9ba120105`, pushed)

- Moved **all** of `project_controller`'s SQLite into `agent_doc_sqlite::state_store`: `open_state_db` + schema, row mapping, actor/transition/lease/dispatch/projection reads, `insert_actor_transition`, `upsert_actor_document`, lease/dispatch writes, layout state, and the CAS + legacy-migration transaction bodies.
- Moved the storage types (`ActorState`/`ActorRecord`/`ActorLastTransition`) and the 5 status types into `agent-doc-sqlite`, re-exported from `session_actor`/`project_controller` so **all call sites are unchanged**.
- Orchestration glue (`ops_log`, `sessions.json` projection, drift checks, `handle_*`, `serve`) stays in `project_controller` and calls `state_store::*`. Small tendrils lifted to params: `upsert_actor_document` takes `launch_mode` + `controller_epoch`; legacy-projection load stays in orchestration and feeds `state_store` transaction fns.
- `rusqlite` is now **dev-only** for `agent-doc-orchestration` — editing orchestration source no longer compiles/links `libsqlite3-sys`. `cargo tree -e normal --depth 1` shows only `agent-doc-sqlite`.

#### Test-infra fix (needed to validate)

`agent-doc-orchestration`'s test target was **uncompilable on HEAD** (pre-existing: missing `filetime` dev-dep used by `gc.rs`; `ScopedCurrentDir` missing from `test_support`). Fixed both — this unhid **1844** orchestration tests, including the `project_controller`/`session_actor` persistence tests that validate this seam.

#### Verification

- `cargo build` full workspace clean; `cargo test -p agent-doc-core -p agent-doc-sqlite` → **652 passed**.
- `cargo test -p agent-doc-orchestration` → **1844 passed / 0 failed** (incl. all `project_controller` + `session_actor` tests).
- `make check` green **except** the pre-existing `codex_bare_run_inside_owning_pane_fails_before_nested_dispatch` (proven to fail on HEAD without these changes).
- `cargo build --release` + `cargo install --path .` succeeded; both binaries current.

Follow-up: `make check` does not run orchestration's isolated suite (separate pre-existing gap) — captured in backlog.

### Re: fix the queue-drift bug — opus-4-7

Diagnosed the recurring drift and filed it for a deliberate fix rather than a speculative patch on critical session code.

**Root cause:** a background `agent-doc start --route-owned tasks/agent-doc/open-code.md` process (PID 138671) + controller hold an **IPC working buffer** for this doc. When the auto-queue halted (`item_modified`) with a live item still queued, the buffer's queue state (re-activated + an extra mirrored item) diverged from the committed snapshot/HEAD and never reconciled — so every preflight re-surfaced a 1-line queue diff + recurring `stuck_captured_cycle` + `inactive_queue_residue`, with no user edit. `agent-doc commit` and raw `git commit` both regenerated it; only `git checkout` (discarding the stale buffer) broke the loop. Tree is now stable (`preflight` → `no_changes`).

**Filed (per the `#agent-doc-bug` flow):**
- Bug `#adoc-queue-ipc-drift` → `tasks/agent-doc/agent-doc-bugs2.md` backlog (commit `a4df1f2e0`).
- Fix plan → `tasks/agent-doc/plan-queue-ipc-drift.md` (commit, with root-cause analysis + a required deterministic repro harness + fix direction: reconcile the IPC buffer to the committed queue shape after maintenance/commit; don't strand live residue when a queue halts; decouple `stuck_captured_cycle` from queue-component churn).

**Why no code change this turn:** the bug spans IPC-buffer ↔ snapshot ↔ queue-halt ↔ capture-ledger reconciliation. It is not deterministically reproducible from a clean state (it needed a cross-turn stale IPC buffer), so a blind patch on this critical path risks worse regressions. The plan builds the repro first. The immediate symptom is already resolved.
<!-- agent:boundary:10174e0a -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- ~do [#adoc-sqlite-isolation]~
- ~do [#adoc-sqlite-seam]~
- do [#adoc-orch-shim-cleanup]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#adoc-make-check-orch-tests] [recommended] make check (cargo nextest --all-targets) does not run agent-doc-orchestration's isolated test suite (1844 tests incl. project_controller/session_actor persistence) — they only run via explicit cargo test -p agent-doc-orchestration. Wire the orchestration test target into the make check gate. (filetime dev-dep + test_support::ScopedCurrentDir were added in the sqlite-seam phase C to make that target compile again.)
- [ ] [#adoc-orch-shim-cleanup] Finish the migration: rewrite CLI-shell crate::X call sites to agent_doc_orchestration::X and delete the pub use shims in agent-doc/src/{lib,main}.rs. Mechanical/low-risk polish, not a true crate split. Plan's optional final step.
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/open-code.done.md -->

<!-- completed work archived in tasks/agent-doc/open-code.done.md -->
<!-- /agent:done -->
