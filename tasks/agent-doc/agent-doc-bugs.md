---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
agent: claude
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
✅ Fixed the frontmatter body-slice bug in `src/agent-doc/src/frontmatter.rs` so repeated frontmatter rewrites no longer accumulate blank lines before the first body heading. Added regression coverage for exact body preservation across repeated parse/write cycles, then verified the crate with `cargo test`, `cargo build --release`, and `cargo install --path .`. The code change is committed and pushed in `src/agent-doc` as `f4b9f59` on `20260422/codex-agent-backend`.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
## Exchange Summary (compacted from ~960 lines)

### Features implemented

1. **Queue orchestration (#queue, Phases 1-3)** — QueueComponent parser + data model, preflight integration + trigger resolution, consumption + dispatch + halt detection. Added `agent:queue` to template scaffold. Prompt preset expansion in orchestrate.
2. **Orphaned socket GC (#gcsock)** — `start.rs` GCs stale supervisor sockets before binding, preventing CRDT corruption from orphaned sessions.
3. **Snapshot auto-migration on rename (#mvsnap)** — state files (snapshots, baselines, captures, CRDT) follow when document path changes.
4. **Snapshot integrity validation (#startval)** — validates snapshot before IPC listener start.
5. **Per-harness model override (#hmodel)** — `claude_model` / `codex_model` frontmatter fields for harness-specific model selection.
6. **Rename pending → backlog (#hbacklog)** — `agent:pending` → `agent:backlog` (alias preserved). `patch=replace` deprecated on backlog tags.
7. **Icebox component (#hicebox)** — `agent:icebox` added to template scaffold.
8. **Migrate command** — `agent-doc migrate` for deprecated component name/attribute migrations.
9. **Pane eviction guard (#pane1)** — `route.rs` skips tmux pane eviction when agent process active.
10. **Route trigger path resolution (#route1)** — trigger paths resolved to absolute, preventing submodule CWD misrouting.
11. **Pending-capture heuristic fix (#4wa1)** — detects unconditional follow-up patterns that false-positive-triggered recommendation batch guard.
12. **Strip guard markers (#npc1)** — `strip_guard_markers` removes `` and `` from within content lines in committed blobs.
13. **Compact --message - stdin support** — `--message -` reads from stdin (standard Unix convention).
14. **Supervisor model injection (v0.33.15)** — `start.rs` injects `--model` from `claude_model`/`codex_model`/`model` frontmatter when not already in agent args.
15. **Post-preflight planning command** — `agent-doc plan <FILE>` emits structured planning record.

### Bugs diagnosed

- **CRDT corruption after file rename** — orphaned sessions continued writing via stale IPC sockets. Root cause: no socket GC + no snapshot migration. Fixed by #gcsock + #mvsnap.
- **Patchback routing failures** — triggers resolved relative to CWD not document root. Fixed by #route1. Pane loss from eviction during active agent. Fixed by #pane1.
- **Compact --message -** — treated `-` as literal string instead of stdin marker. Fixed.

### Releases

- **v0.33.14** — published to crates.io + GitHub Releases (tag on `20260422/codex-agent-backend`)
- **v0.33.15** — version bumped, binaries built locally, waiting on manual testing gate before publish

### Open / gated

- v0.33.15 publish gated on manual testing confirmation

Please use agent-doc to migrate the current document
### Re: migrate + pre-commit pending capture gate — opus-4-6

Ran `agent-doc migrate` — document updated to canonical format.

**Phase 1 implemented:** pre-commit pending capture gate in `finalize`. When `pending_capture_guard: strict`, `finalize` now scans the response for uncaptured recommendations *before* committing. If recommendation-like items are detected without `--pending-add` flags, finalize exits non-zero before the commit step — the agent can retry with `--pending-add` without needing a follow-up commit.

Changes:
- `src/session_check.rs` — made `resolve_pending_capture_guard_mode` and `response_text_for_guards` `pub(crate)` for reuse
- `src/write.rs` — added `precommit_pending_capture_check()` between write and commit phases, 5 unit tests
- `tests/finalize_integration.rs` — updated integration test for new pre-commit gate behavior (response written to disk but not committed)
- `specs/07-commands.md` — documented pre-commit gate contract under `finalize`

Tests: 5/5 new pass, full suite clean (autoclaim cascade is pre-existing). Clippy clean. Release built + installed. Committed + pushed.

Both phases added to backlog. Phase 2 (plan emits `pending_mutations`) is gated on Phase 1 validation.

❯ Did you retain `agent:pending` being an alias to `agent:backlog`?
### Re: agent:pending alias retained — opus-4-6

Yes. `agent:pending` is a fully functional alias for `agent:backlog`. The aliasing is handled at two levels:

1. **Component parser** (`component.rs:71-82`): `BACKLOG_ALIAS = "pending"` and `is_backlog_component()` accepts both `"backlog"` and `"pending"`. All pending-related operations (add, done, edit, gate, etc.) use `is_backlog_component()` to find the component, so either tag name works.

2. **Migration** (`migrate.rs`): `agent-doc migrate` rewrites `&lt;!-- agent:pending --&gt;` → `&lt;!-- agent:backlog --&gt;` in the document text for canonicalization, but documents that still use the old tag continue to work without migration.

The `migrate` run on this document renamed the tags to canonical `agent:backlog`, but if you had a document still using `agent:pending` tags, all binary operations would work identically.

❯ Please add both phase 1 and 2 from tasks/agent-doc/plan-pending-capture-precommit.md to backlog
❯ do phase 1. #spec-test-build-install-commit-push
### Re: add plan phases to backlog + do phase 1 — opus-4-6

Phase 1 (pre-commit pending capture gate) is already implemented, spec'd, tested, built, installed, committed, and pushed:

- **Spec**: `specs/07-commands.md` line 439 — documents the pre-commit gate contract under `finalize`
- **Tests**: 5/5 pass in `src/write.rs` (`precommit_pending_capture_tests` module) — covers strict blocks, pending-add passthrough, warn mode, default mode, suppression marker
- **Build**: `cargo build --release` clean
- **Install**: `cargo install --path .` → `~/.cargo/bin/agent-doc` v0.33.15
- **Commit**: `b7e2563 feat: pre-commit pending capture gate in finalize (strict mode)`
- **Push**: `origin/20260422/codex-agent-backend` up to date

Added Phase 1 (`#pcg1`) to backlog and marked done. Phase 2 (`#pcg2`) was already in backlog as gated.

❯ Is phase 2 done?
### Re: phase 2 status — opus-4-6

Partially. The infrastructure is in place but the core heuristic is missing.

**What's done:**
- `pending_mutations` field exists in the `PlanRecord` struct (`plan.rs:47`)
- `PendingMutationPlan` struct with `kind`, `id`, `text` fields
- `ResolveExisting` kind — detects `do #id` in repo actions and matches against open backlog items
- Test: `build_plan_includes_finalize_placeholder_for_template_docs` covers the `do #id` → `ResolveExisting` path

**What's missing (the actual Phase 2 from the plan):**
- `ExpectAdd` variant in `PendingMutationKind` — signals "this prompt will likely generate recommendations"
- Heuristic in `pending_mutations_for_doc()` to detect recommendation-generating prompts: user asks for "tasks", "todo", "backlog", "what's next", "recommendations"
- Tests from the plan: `test_plan_detects_backlog_request`, `test_plan_detects_recommendation_request`, `test_plan_no_false_positive_on_questions`

The `ResolveExisting` kind that exists now is an adjacent feature (resolve `do #id` against pending) rather than the Phase 2 `expect_add` heuristic. `#pcg2` should stay gated.

❯ Please complete #pcg2. spec-test-build-install-commit-push
### Re: complete #pcg2 — opus-4-6

Done. Phase 2 implemented, tested, built, installed, committed, and pushed.

**What was added:**

- `ExpectAdd` variant in `PendingMutationKind` — signals that the upcoming response will likely generate recommendations
- `prompt_requests_backlog_work()` heuristic in `pending_mutations_for_doc()` — detects when prompt targets contain backlog/recommendation signals ("tasks", "todo", "backlog", "what's next", "recommendations", "next steps", "action items", etc.)
- When detected, plan emits `{"kind": "expect_add", "id": "", "text": "user requested backlog/recommendations"}` in `pending_mutations`

**Tests (3 new, all pass):**

- `test_plan_detects_backlog_request` — "add to backlog" prompt → `ExpectAdd`
- `test_plan_detects_recommendation_request` — "What should we do next? Any recommendations?" → `ExpectAdd`
- `test_plan_no_false_positive_on_questions` — "How does the CRDT merge work?" → no `ExpectAdd`

**Spec:** updated `specs/07-commands.md` plan section — documents both `resolve_existing` and `expect_add` mutation kinds.

**Verification:** clippy clean, 5/5 plan tests pass, full suite clean (1 pre-existing failure in codex hook test). Release built + installed. Commit `f67049e`, pushed to `origin/20260422/codex-agent-backend`.

❯ release + publish
### Re: release + publish — opus-4-6

v0.33.15 released and published.

- **VERSIONS.md** updated with all 0.33.15 changes (supervisor model injection, pre-commit pending capture gate, ExpectAdd pending mutations, post-preflight planning command)
- **crates.io**: published
- **GitHub Release**: https://github.com/btakita/agent-doc/releases/tag/v0.33.15
- **Tag**: `v0.33.15` on `20260422/codex-agent-backend`
- **PyPI**: publishes automatically via CI on the GitHub release event

❯ do #cxdd. spec-test-build-install-commit-push
### Re: do #cxdd — opus-4-6

Fixed Codex submodule handoff. Root cause: `codex exec resume` does not accept `--add-dir`, but `append_resume_args` was passing it through. The resumed session inherits writable roots from the original `exec`, so stripping is correct behavior.

**Changes:**

- `src/agent/codex.rs` — `append_resume_args` now strips `--add-dir <DIR>` and `--add-dir=<DIR>` forms (same pattern as `-s/--sandbox` translation)
- `specs/06-config.md`, `specs/07-commands.md`, `specs/09-git-integration.md` — document backend-specific `--add-dir` handling: both Claude and Codex `exec` accept it, but `codex exec resume` does not

**Tests (4 new, all pass):**

- `build_command_exec_preserves_add_dir` — fresh exec keeps `--add-dir` in args
- `build_command_resume_strips_add_dir` — resume strips `--add-dir <DIR>`
- `build_command_resume_strips_add_dir_equals_form` — resume strips `--add-dir=<DIR>`
- `build_command_resume_strips_multiple_add_dirs` — resume strips multiple `--add-dir` entries

**Verification:** clippy clean, 7/7 build_command tests pass, full suite clean (pre-existing route + codex hook failures only). Release built + installed. Commit `ffdc1b6`, pushed to `origin/20260422/codex-agent-backend`.

**Note:** manual verification with a live submodule session is the remaining gate before release.

❯ release + publish
### Re: release + publish — opus-4-6

v0.33.16 released and published.

- **VERSIONS.md** updated with the Codex submodule handoff fix
- **crates.io**: published
- **GitHub Release**: https://github.com/btakita/agent-doc/releases/tag/v0.33.16
- **Tag**: `v0.33.16` on `20260422/codex-agent-backend`
- **PyPI**: publishes automatically via CI on the GitHub release event

❯ Please read:
❯ [sidekick-use-cases.md](../../src/session-share/tasks/sidekick/sidekick-use-cases.md)
❯ [sidekick-pitch.md](../../src/session-share/tasks/sidekick/sidekick-pitch.md)

❯ Can agent-doc, with the supervisor and "virtual" documents for session tracking...be a good backend for Sidekick. Even if agent-doc were not used by the practitioner?
### Re: agent-doc as Sidekick backend — opus-4-6

Short answer: agent-doc is a strong integration layer and lifecycle host, but it shouldn't be the whole backend. The fit is partial and real — the question is which parts to reuse vs build fresh.

#### Where agent-doc maps well

**Supervisor as daemon.** Sidekick needs a persistent per-project process monitoring sessions. Agent-doc's supervisor already does this — it manages IPC, tracks session state, handles socket GC, and survives across agent restarts. The lifecycle machinery (start/stop, orphan detection, health checks) is directly reusable. You wouldn't have to rebuild process management.

**Virtual documents as session tracking surfaces.** This is the most interesting fit. A "virtual" agent-doc document that the practitioner never opens could model a Sidekick session record: components for observations, proposals, metrics, and pending follow-ups. The CRDT merge engine means the daemon can write observations while git operations happen concurrently. The template/component system (`agent:status`, `agent:exchange`, `agent:backlog`) maps onto Sidekick's data surfaces — a `sidekick:proposals` component, a `sidekick:metrics` component, a `sidekick:session-log` component.

**AGENTS.md write-back.** Sidekick's core output is AGENTS.md proposals. Agent-doc already has a safe write pipeline with CRDT merging, snapshot baselines, and commit boundaries. When Sidekick proposes a change to `src/auth/AGENTS.md`, piping it through agent-doc's `finalize` gives you concurrent-edit safety and git integration for free.

**Multi-harness abstraction.** Agent-doc already dispatches across Claude Code, Codex, and Cursor. Sidekick needs exactly this cross-tool reach. The harness detection, model resolution, and hook integration are transferable.

#### Where it doesn't fit

**Session transcript analysis is stream processing, not document editing.** The 6-stage Session Analyzer Pipeline (parse JSONL → segment → annotate → extract → propose → summarize) is fundamentally a data pipeline. Agent-doc's document model adds friction here — you'd be forcing transcript analysis into markdown components when it wants to be structured data flowing through transforms. This should be a purpose-built pipeline.

**Metrics and analytics need a proper store.** Discovery-lag tracking, token cost baselines, correction event rates, per-task-type averages — this is time-series data. SQLite (as Sidekick's pitch specifies) is the right choice. Trying to encode this in document components would be awkward and slow.

**Knowledge graph and semantic search are their own domain.** The codebase knowledge graph, vector embeddings for session search, and cross-developer pattern mining have no analog in agent-doc. These need a vector store and graph data structures.

**Real-time in-session interventions (health monitor, budget guardian, context surgery) are event-driven.** They need to process tool-call events as they stream in. Agent-doc's cycle model (preflight → respond → finalize) is turn-based, not event-stream-based.

#### The pragmatic architecture

Agent-doc as **one layer** in a Sidekick stack:

| Layer | Owns | Backed by |
|-------|------|-----------|
| **Process lifecycle** | Daemon start/stop, socket management, health checks | agent-doc supervisor (reuse) |
| **Session analysis** | JSONL parsing, segmentation, pattern detection, metrics | Purpose-built pipeline → SQLite |
| **Knowledge** | Semantic search, knowledge graph, cross-session intelligence | Vector store + graph structures |
| **Document output** | AGENTS.md proposals, session summaries, virtual tracking docs | agent-doc write pipeline (reuse) |
| **Harness integration** | Hook dispatch, multi-tool model resolution | agent-doc harness layer (reuse) |

The key insight: agent-doc doesn't need to be the practitioner's editing tool for this to work. The virtual document concept lets agent-doc manage Sidekick's internal state documents (session records, proposal queues, tracking files) without requiring the user to ever open a markdown file. The user interacts through the VS Code extension, CLI, or admin console — agent-doc handles the durable-state plumbing underneath.

**Even if the practitioner never runs `/agent-doc`**, the Sidekick daemon could use agent-doc internally to:
1. Maintain a virtual session document per project that accumulates observations
2. Write AGENTS.md proposals through the safe merge pipeline
3. Track proposal lifecycle (pending → reviewed → merged/rejected) via the backlog component
4. Commit changes atomically through the existing git integration

The pieces that should NOT go through agent-doc: JSONL transcript analysis, real-time monitoring, metrics aggregation, semantic search, and the knowledge graph. Those need their own fit-for-purpose infrastructure.

So: good backend for the write-side and lifecycle-management parts of Sidekick. Not the right choice for the read-side analytics. The two together make a complete system.

I do want to steer agent-dor toward real-time streams. Even though the lifecycle is currently turn-based. I envision agent-doc to be an interactive dashboard document. Would this change any of the analysis? We can build toward being a "virtual" document for Sidekick. 
### Re: real-time streams and the dashboard document vision — opus-4-6

Yes, this materially changes the analysis. The biggest gap I identified — "real-time in-session interventions are event-driven, agent-doc's cycle model is turn-based" — collapses if the supervisor learns to ingest event streams and push live updates to document components.

#### What shifts

**The document becomes a materialized view, not just a transcript.** Today agent-doc's document is a conversation record: user writes, agent responds, cycle commits. A dashboard document is fundamentally different — it's a live projection of system state. Components update because the underlying data changed, not because someone wrote a prompt. This is a bigger conceptual shift than it sounds: the trigger for document mutation moves from "user edit detected" to "event received."

**The supervisor already has the right shape.** It's a long-running daemon with IPC sockets, CRDT merge capability, and concurrent-write safety. The missing piece is an event ingestion path. If Claude Code hooks pipe tool-call events into the supervisor socket (PostToolUse, token counts, file modifications), the supervisor can:

1. Maintain running state (tool call history, token spend, loop counters)
2. Evaluate rules against that state (search loop detection, budget thresholds)
3. Push component updates to the document (live metrics, health status, alerts)
4. Trigger interventions (context injection, rewind suggestions)

That's Sidekick's Health Monitor and Budget Guardian running inside agent-doc's existing process model.

**Virtual dashboard components map directly to Sidekick surfaces:**

| Component | Updates on | Content |
|-----------|-----------|---------|
| `sidekick:health` | Every N tool calls | Loop count, correction cascade status, current episode classification |
| `sidekick:budget` | Token threshold crossings | Spend vs baseline, projected total, alert level |
| `sidekick:proposals` | Post-session analysis | Queued AGENTS.md diffs with confidence scores |
| `sidekick:metrics` | Session end | Discovery-lag, correction rate, golden path delta |
| `sidekick:session-log` | Continuous | Structured episode timeline with annotations |

The practitioner never opens this document. The VS Code extension or CLI reads from it. The document format gives you structured persistence, git history, and diff-ability for free.

#### What still needs its own infrastructure

**The analysis pipeline is still a pipeline.** JSONL transcript → segment → annotate → extract → propose is a batch data-processing job. It could feed *results* into document components, but the transform stages themselves don't belong in a document model. Think of it like: the pipeline writes to agent-doc, not through agent-doc.

**Vector store and knowledge graph.** Semantic session search and the codebase knowledge graph are query-optimized data structures. They need a proper store (SQLite + embedded vectors). But agent-doc could be the write-back surface — when the knowledge graph detects a new pattern, it materializes as a component update in the virtual document.

❯ Could tsift help here?

**Cross-developer aggregation.** Team-level pattern mining aggregates across multiple developers' session data. This is a server-side concern (or a local aggregation daemon in local-only mode). Agent-doc operates per-document. But: each developer's virtual Sidekick document could be the data source that feeds the aggregator.

#### The revised architecture

| Layer | Owns | Implementation |
|-------|------|----------------|
| **Event ingestion** | Hook events → supervisor state machine | agent-doc supervisor + new event channel |
| **Live dashboard** | Real-time component updates, health/budget surfaces | agent-doc virtual document + CRDT |
| **Analysis pipeline** | JSONL processing, pattern extraction, proposal generation | Purpose-built (writes results to agent-doc components) |
| **Knowledge store** | Semantic search, knowledge graph, metrics time-series | SQLite + vector store (reads/writes via agent-doc API) |
| **Harness integration** | Multi-tool hooks, model resolution, write-back | agent-doc (already exists) |

The key difference from my first analysis: agent-doc moves from "integration layer and lifecycle host" to "the central nervous system." The analysis pipeline and knowledge store become plugins that feed into and read from agent-doc, rather than independent systems that happen to use agent-doc for output.

#### Architecture decisions to resolve

1. **Event throughput.** A busy session might fire 50+ tool calls per minute. CRDT merge on every event is expensive. Debouncing strategy: batch component updates at ~1s intervals, but evaluate alert rules immediately.

2. **Component schema.** Dashboard components need structured data (JSON-like metrics), not just markdown prose. This might mean a new component content type — structured data rendered as markdown for human readability but parseable for machine consumption.

3. **Push vs pull for consumers.** VS Code extension reading from the document: does it watch the file, subscribe to the supervisor's IPC, or poll? The supervisor IPC path is lower latency and doesn't require git operations.

Building toward this makes agent-doc a credible full backend for Sidekick — not just the write side, but the state management and real-time observation layer too. The only pieces that stay external are the heavyweight analytics (batch pipeline, vector store) that any system would separate.

❯ Please create a running plan document.
❯ One requirement of Sidekick is to not pollute the git repo with commits. agent-doc is currently has many commits and the lifecycle uses commits. Can we have an option to use snapshots only?
### Re: tsift for knowledge layer + plan + snapshot-only mode — opus-4-6

#### tsift as knowledge component

Yes — tsift is a natural fit for Sidekick's knowledge layer. It already has the two hardest pieces built:

1. **Hybrid BM25 + vector search** → powers "How did we implement rate limiting?" style queries over session history. If transcripts are indexed as a new content type alongside code, tsift handles semantic session search directly.

2. **Community graph** → the AST-derived community clusters already show which files/symbols cluster together. This IS the "codebase knowledge graph" that Sidekick defines as a Phase 4 feature. The difference: tsift builds it from code structure, while Sidekick's version builds it from behavioral observation (which files get accessed together in sessions). The ideal is both signals merged — structural communities weighted by behavioral access patterns.

3. **Discovery-lag scoring** — tsift's search relevance naturally encodes "how easy is this fact to find." A fact with high BM25 relevance (appears in filenames, common terms) has low discovery-lag potential. A fact that only appears deep in an implementation file with non-obvious naming → high discovery-lag candidate. This could feed Sidekick's prioritization of which facts belong in AGENTS.md.

The integration path: tsift indexes session transcripts (or pre-processed segments) as a parallel corpus alongside code. The virtual Sidekick document's `sidekick:knowledge` component materializes from tsift graph queries.

#### Plan document created

Created [plan-sidekick-backend.md](plan-sidekick-backend.md) with the full architecture, phased implementation, and requirements. The plan covers 5 phases: snapshot-only mode → event channel → tsift integration → analysis pipeline → multi-tool export.

#### Snapshot-only mode

This is the right requirement. Here's how it works:

**The insight:** agent-doc's diffing already works against snapshots, not git commits. The commit step in `finalize` is a separate phase that happens *after* the document write + snapshot update. Removing it is architecturally clean.

**Implementation:**

A `commit: false` frontmatter setting (or global config for virtual documents) would:
- `finalize` writes the document and updates the snapshot → stops there (no `git add`, no `git commit`)
- Preflight diffs against the snapshot file (already does this)
- Session-check verifies snapshot consistency instead of requiring HEAD match
- The document stays as an uncommitted working-tree file, or better: lives in `.agent-doc/virtual/` which is gitignored entirely

**For virtual Sidekick documents specifically:** they'd live in `.agent-doc/virtual/{project-hash}/sidekick.md` — completely outside the git working tree. No commits, no staging, no repo pollution. The snapshot system provides the same diff/state-tracking guarantees that git normally provides, just without the VCS overhead.

**What you lose:** git history of the document's evolution, and the ability to recover state from git if the snapshot is lost. For a daemon's internal state document, this is an acceptable tradeoff — the source of truth is the live session data, not the historical document state.

❯ Sidekick is proprietary and confidential. agent-doc is open source. We need a vendor independent layer for elements. Make a separate Sidekick integration document.
### Re: vendor-independent separation — opus-4-6

Done. Restructured into two documents with clean boundaries:

**1. [plan-sidekick-backend.md](plan-sidekick-backend.md)** — now vendor-independent. Renamed focus to "agent-doc daemon extensions." Covers generic capabilities any integration can use:
- Snapshot-only mode (no commit pollution)
- Event channel on supervisor
- Virtual documents (daemon-managed, no user interaction)
- Plugin API (register listeners, read/write components, subscribe to changes)
- Multi-tool export interface (AGENTS.md, copilot-instructions, .cursor/rules)
- tsift integration as a knowledge plugin

No Sidekick-specific terminology. Any monitoring tool, analytics platform, or IDE extension could build on these primitives.

**2. [sidekick-agent-doc-integration.md](../../src/session-share/tasks/sidekick/sidekick-agent-doc-integration.md)** — Sidekick-specific. Lives in the session-share/sidekick directory alongside the pitch and use-cases docs. Captures:
- How Sidekick registers as an agent-doc plugin
- Feature-to-component mapping (health monitor → event listener, proposals → export interface)
- Event types Sidekick subscribes to
- Proprietary analysis pipeline details
- Team aggregation architecture
- Open questions about plugin distribution and licensing boundaries

The separation is: agent-doc publishes stable, documented extension points. Sidekick is one consumer that builds proprietary intelligence on top. Other consumers (open-source monitoring tools, enterprise observability platforms) could use the same plugin API without any Sidekick dependency.

<!-- agent:boundary:306de0f1 -->
<!-- /agent:exchange -->

<!--
I want sidekick to have the option to use git commits as well.
-->

## Pending / Not Built

<!-- agent:backlog -->
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:backlog -->

<!--
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
-->
