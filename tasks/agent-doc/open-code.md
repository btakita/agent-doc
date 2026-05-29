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
<!-- agent:boundary:a2710146 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
preset #spec-test-build-install-commit-push
- do [#adoc-sqlite-isolation]
- do [#adoc-orch-shim-cleanup]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#adoc-sqlite-seam] [recommended] The real build-isolation lever is project_controller, not archive_index. Introduce an interface seam: pass a rusqlite Connection handle (or a small storage trait) into project_controller so its SQLite-touching code can move to a lower agent-doc-sqlite crate, removing rusqlite from agent-doc-orchestration's direct dep graph. Co-locate archive_index in that crate. Prereq cycle-break: move compact::parse_topic_sections (pure text fn) down to agent-doc-core, since archive_index calls it while compact calls archive_index::index_archive — crates can't be cyclic. Only this split actually isolates the bundled-SQLite C build from orchestration recompiles.
- [ ] [#adoc-orch-shim-cleanup] Finish the migration: rewrite CLI-shell crate::X call sites to agent_doc_orchestration::X and delete the pub use shims in agent-doc/src/{lib,main}.rs. Mechanical/low-risk polish, not a true crate split. Plan's optional final step.
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/open-code.done.md -->

<!-- completed work archived in tasks/agent-doc/open-code.done.md -->
<!-- /agent:done -->
