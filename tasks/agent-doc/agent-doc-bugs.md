---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
Prior frontmatter bug fix remains complete. New work this cycle is planning only: added a concrete reliability plan for the “response patchback not committed / no patchback cycle ran” failure family. The current diagnosis is that two bugs can surface as the same symptom: (1) fresh Codex sessions can miss the initial route trigger entirely, so `preflight -> write -> commit` never starts; (2) a write/IPC + snapshot save can succeed, but the cycle can still die before `commit_*`, leaving a visible uncommitted assistant patchback in the working tree.
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

❯ Please make a business case for robert-ross for this model. robert-ross is inclined to have as much proprietary control as possible. He is also working on a provisional patent. I like this model and I already wanted to take agent-doc in this direction before I met robert-ross. But some Sidekick functionality is new info to me. Please make a solid business case, emphasizing ecosystem development, and giving robert-ross leverage with Dreamable over the market.
### Re: business case for Robert Ross — opus-4-6

#### The model

agent-doc (open source) publishes stable infrastructure: virtual documents, event channels, plugin API, snapshot-only mode, multi-tool export. Sidekick (proprietary) is a plugin that builds intelligence on top — session analysis, pattern detection, proposal generation, team aggregation, coaching. The infrastructure is open. The intelligence is proprietary.

#### Why this is stronger than fully proprietary

**1. Ecosystem development creates a moat Sidekick can't build alone.**

A fully proprietary stack competes on features. An open infrastructure layer competes on adoption. When agent-doc ships virtual documents and a plugin API, every developer tool company becomes a potential ecosystem participant — monitoring tools, IDE extensions, analytics platforms, CI integrations. Each one builds on agent-doc's primitives, which means each one makes the ecosystem more valuable.

Sidekick doesn't need to build integrations for every tool. The ecosystem builds them. Sidekick just needs to be the best intelligence layer on top. This is the Redis model (open protocol, proprietary enterprise features), the Kubernetes model (open orchestration, proprietary management planes), the VS Code model (open editor, proprietary extensions marketplace). The infrastructure commoditizes; the intelligence differentiates.

Rob's provisional patent covers the intelligence — the scoring algorithms, the session analysis pipeline, the discovery-lag heuristics, the counterfactual replay, the team aggregation patterns. None of that lives in agent-doc. The patent protects exactly the proprietary layer that generates revenue.

**2. Open infrastructure eliminates the adoption bottleneck.**

Sidekick's pitch identifies the ICP as "any organization with 5+ developers using Claude Code, Cursor, or GitHub Copilot." That's the same market where AGENTS.md, CLAUDE.md, .cursor/rules, and copilot-instructions.md are already fragmenting. No enterprise will adopt a fully proprietary daemon that only works with one vendor's tooling.

An open agent-doc plugin API means Sidekick works everywhere these tools run — without Dreamable having to build and maintain integrations for each one. The multi-tool export interface (AGENTS.md, copilot-instructions.md, .cursor/rules) already solves the cross-tool standardization problem from the Sidekick pitch. It solves it at the infrastructure layer, where the effort is shared, rather than the product layer, where Dreamable bears it alone.

Enterprise procurement teams evaluate vendor lock-in risk. "Sidekick uses an open-source infrastructure layer with a stable plugin API" reduces that risk to near zero. If Dreamable disappears, the customer's data and documents persist in an open format. That's a selling point in enterprise sales, not a weakness.

**3. Dreamable captures the high-margin layer.**

The Sidekick pricing model ($29/dev/month team, $50K-$500K ACV enterprise) monetizes intelligence, not infrastructure. The infrastructure is a cost center. Open-sourcing it shifts that cost to community maintenance while Dreamable retains 100% of the revenue-generating layer.

Consider the alternative: Dreamable builds and maintains its own virtual document system, its own event channel, its own daemon lifecycle manager, its own CRDT merge engine, its own snapshot system. That's 6-12 months of engineering on infrastructure that generates zero revenue. It creates a maintenance burden that scales with customer count. And it duplicates work Brian has already done in agent-doc.

With the open model, Brian's existing agent-doc work is the infrastructure. Dreamable pays Brian for the Sidekick intelligence layer on top. The infrastructure maintenance is shared with the open-source community. Dreamable's engineering investment goes entirely toward the proprietary differentiators — exactly the pieces the patent covers.

**4. This gives Dreamable leverage over the market, not just a product.**

The Sidekick pitch says "Sidekick is NOT another coding agent. It is the compiler and coach that turns agent traces into institutional knowledge." If the infrastructure under that compiler is proprietary, Sidekick is just another developer tool competing for shelf space. If the infrastructure is an open standard, Sidekick becomes the reference implementation of how intelligent session management works.

This is the leverage play: Dreamable doesn't just sell a product. Dreamable defines the category. When competitors build their own session intelligence tools, they build on agent-doc's plugin API — the same API Sidekick already dominates. The competition validates the ecosystem while competing at the intelligence layer, where Sidekick's patent, data flywheel (more sessions observed → better pattern detection → better proposals → higher acceptance rate → more sessions observed), and first-mover advantage are defensible.

Compare: if Dreamable builds a fully proprietary stack, competitors build their own incompatible stacks. Now Dreamable is in a features war with no ecosystem advantage. Enterprise customers pick the tool with the most integrations, and Dreamable's 6-person team can't out-integrate a well-funded competitor.

**5. Brian's alignment strengthens execution.**

Brian was building agent-doc toward daemon extensions, virtual documents, and real-time event streams before meeting Rob. That trajectory now converges with Sidekick's requirements. The open-source model means Brian's personal investment in agent-doc and his work for Dreamable are additive — agent-doc features he builds for his own users benefit Sidekick, and Sidekick requirements push agent-doc in directions the open-source community also wants.

A fully proprietary model creates tension: Brian either stops developing agent-doc (losing the open-source community and his own tool) or maintains two diverging codebases. The open model eliminates that tension. One codebase. One trajectory. Dreamable gets the infrastructure for free by aligning with it.

#### Addressing the patent concern directly

The provisional patent protects algorithms and methods, not infrastructure. The separation is clean:

| **Protected by patent (proprietary)** | **Open infrastructure (agent-doc)** |
|---|---|
| Session analysis pipeline (6-stage) | Virtual document lifecycle |
| Discovery-lag scoring algorithm | Event channel on supervisor |
| Counterfactual replay scoring | Plugin API (register, read, write, subscribe) |
| Team aggregation + privacy stripping | CRDT merge engine |
| Prompt coaching heuristics | Multi-tool export interface |
| Expertise-weighted voting | Snapshot-only mode |
| Cross-session pattern mining | Daemon process management |

The patent covers the intelligence that reads and writes through the infrastructure. Open-sourcing the infrastructure doesn't weaken the patent — it strengthens the market position of the patented technology by giving it a larger addressable surface.

#### The bottom line for Rob

Fully proprietary means Dreamable builds everything, maintains everything, and competes on features with a 6-person team against well-funded competitors. Open infrastructure + proprietary intelligence means Dreamable defines the category, shares maintenance costs, eliminates vendor lock-in objections in enterprise sales, and focuses 100% of engineering on the revenue-generating layer that the patent protects. The ecosystem does the distribution. Sidekick does the thinking.

❯ Please write the analysis into a directory with sidekick-agent-doc-integration.md and plan-sidekick-backend.md. Rename plan-sidekick-backend.md to agent-doc-realtime.md or another more appropriate name.
### Re: reorganize analysis documents — opus-4-6

Done. Created `tasks/agent-doc/sidekick/` with all three documents co-located:

| File | Content |
|------|---------|
| [agent-doc-platform.md](sidekick/agent-doc-platform.md) | Vendor-independent daemon extensions plan (renamed from `plan-sidekick-backend.md`) |
| [sidekick-agent-doc-integration.md](sidekick/sidekick-agent-doc-integration.md) | How Sidekick registers as an agent-doc plugin, feature-to-component mapping |
| [business-case-open-platform.md](sidekick/business-case-open-platform.md) | Business case for Robert Ross — open infrastructure + proprietary intelligence model |

The business case is a standalone document extracted from the exchange response, with a `Related documents` section linking to its two companion files. Cross-references between the three are updated.

Naming choice: `agent-doc-platform.md` rather than `agent-doc-realtime.md` because the plan covers more than real-time — virtual documents, plugin API, multi-tool export, tsift integration. "Platform" captures agent-doc evolving from a CLI tool into an extensible platform with a plugin ecosystem.

❯ Does the virtual document concept have prior art? Is it cover under a patent? Is there open source prior art?
### Re: virtual document prior art — opus-4-6

#### Short answer

Every individual building block has extensive prior art. The specific combination — markdown as a CRDT-backed, daemon-managed, plugin-extensible materialized view of AI agent state — appears novel.

#### Open-source prior art by component

| Component | Prior art | Year |
|-----------|-----------|------|
| Daemon-managed files (no user editing) | Plan 9 synthetic files, `/proc`, FUSE, VS Code `TextDocumentContentProvider` | 1992+ |
| Programmatic IPC creation/update | Jupyter kernel protocol (ZeroMQ), MCP (Anthropic → Linux Foundation), Plan 9 ctl files | 2001+ |
| Structured components in documents | Org-mode (headings, properties drawers, babel blocks), Obsidian Dataview (YAML frontmatter + inline fields) | 2003+ |
| State outside git working tree | `.git/` itself, LSP virtual documents, AgentFS (Turso, SQLite-backed) | ongoing |
| Snapshot-based state tracking | Automerge (Ink & Switch), AgentFS, Jupyter checkpoints | 2017+ |
| CRDT merge for concurrent writes | Yjs, Automerge, Loro (Rust), Peritext (rich text CRDTs), Jupyter Real-Time Collaboration (pycrdt) | 2015+ |
| Plugin API with event subscriptions | VS Code Extension API, Obsidian Plugin API, Yjs providers (Y-Sweet) | ongoing |
| Materialized view of system state | CQRS + Event Sourcing, Riffle (MIT/Ink & Switch, UIST 2023), ksqlDB materialized views | well-established |

All of these are well-documented, open-source, and unencumbered.

#### Patent landscape

| Patent | Year | Status | What it covers |
|--------|------|--------|----------------|
| US5787449A (IBM) | 1994 | **Expired** | Separating document architecture from content; multiple views from same components |
| EP1642208A2 (Edgar Online) | 2004 | Active (some jurisdictions) | "Virtual document architecture" — reusable paragraph-level components, version control, collaborative editing for financial/legal docs |
| US20160048486A1 | 2014 | Application | Multi-user stream collaborative editing with per-user revision streams |

EP1642208 is the closest patent using the term "virtual document," but it targets human-collaborative publishing (financial/legal documents). It has no CRDT, no daemon lifecycle, no AI agent context. Its "virtual document" means "document composed from reusable component fragments" — different from agent-doc's meaning of "document managed by a daemon without direct user editing."

No patents were found combining CRDT merge + daemon management + AI agent session state + structured components + plugin API.

#### What's actually novel

The individual pieces are all prior art. The novelty is in the composition applied to AI agent session management:

1. **Markdown-as-materialized-view of AI agent state.** No existing system uses a structured markdown document as a live materialized view that a daemon keeps synchronized with system state. AgentFS uses SQLite. Devin uses sandbox state. Codex uses JSON memories. The "document IS the state surface" idiom applied to daemon-managed (not user-managed) documents is new.

2. **CRDT merge on a non-interactive document.** All existing CRDT collaborative editing work targets human-to-human or human-to-system collaboration where users actively edit. Using CRDTs for daemon-to-daemon concurrent writes on a document no human directly edits is an unusual application — the CRDT serves plugin conflict resolution, not user collaboration.

3. **Plugin API over structured markdown components via IPC.** The ability for external tools to `read_component(doc_id, "sidekick:health")` and `subscribe_changes(doc_id, "sidekick:metrics")` where the storage is a markdown document with named `&lt;!-- agent:name --&gt;` component markers has no analog. Obsidian Dataview reads structured markdown but is query-only, not writable. VS Code virtual documents are programmatic but unstructured.

4. **The closest analog is Plan 9 synthetic files** — daemon-managed structured state exposed as files with a protocol for reading/writing — but applied to a markdown document format with CRDT merge semantics and AI agent session management as the domain.

#### Implications for the patent

The virtual document *infrastructure* is a composition of well-established open-source patterns. Attempting to patent the infrastructure concept would face prior art challenges on every individual component. This reinforces the business case: the infrastructure should be open (it's built on open primitives), while the patent should cover the intelligence layer — the scoring algorithms, analysis pipeline, discovery-lag heuristics, and team aggregation that *use* virtual documents but aren't *about* virtual documents.

❯ Please update the documents. I want to steer robert-ross from patenting the Virtual Document concept, as this is part of the open source platform. Don't steer too hard but make a case for keeping it open.
### Re: update documents re virtual document patent scope — opus-4-6

Updated both documents:

**business-case-open-platform.md** — added a new subsection "Why virtual documents specifically should stay out of the patent" under the existing patent section. Lists the prior art by component (Plan 9, Yjs/Automerge, Org-mode, CQRS, etc.), references EP1642208A2 and US5787449A as existing patent art, and frames the argument pragmatically: including virtual documents invites prior art challenges and kills the ecosystem flywheel. The stronger patent strategy is to let infrastructure be open and focus claims on the intelligence layer where there's no meaningful prior art.

**sidekick-agent-doc-integration.md** — added an "IP Boundary" section above Open Questions that establishes the line: virtual document infrastructure is open-source, patent scope covers intelligence running on top. Cross-links to the business case.

The tone is factual, not adversarial — it presents the prior art reality and the strategic logic without framing it as "you can't patent this." The emphasis is on where the patent is *strongest* (intelligence layer) rather than where it's *weakest* (infrastructure).

❯ Explore the novelty and utility of the virtual document. Explore how the virtual document is also editable on a local machine and is traceable. Explore how making the virtual document and agent-doc editing itself is useful from a marketing and productivity perspective.
### Re: virtual document novelty, utility, and traceability — gpt-5

Updated the sidekick docs to make this case directly, not just imply it.

**business-case-open-platform.md** now has a new section, Why

❯ Include the agent-doc supervisor in the analysis and business case. List the supervisor as part of the Open infrastructure.
### Re: include the supervisor in the open infrastructure case — gpt-5

Updated the analysis and business-case framing to name the `agent-doc` supervisor explicitly as part of the open infrastructure.

**business-case-open-platform.md** now includes the supervisor/runtime in the top-level model, in the adoption argument, and in the patent-boundary table. The business case is clearer now: the open layer is not just virtual documents and a plugin API, but also the always-on runtime that handles lifecycle, event delivery, and state coordination across tools.

**sidekick-agent-doc-integration.md** now includes the supervisor/runtime in the separation-of-concerns section and in the IP boundary. I also tightened the utility section so it states that the virtual document is the visible state surface, while the supervisor is the open runtime that keeps that surface synchronized and observable.

That makes the open/proprietary split cleaner: the supervisor, daemon lifecycle, event channel, virtual documents, CRDT merge, snapshots, plugin API, and export layer stay open; Sidekick's proprietary value remains the analysis pipeline, scoring, aggregation, and coaching that run on top.

❯ Please describe how the virtual document compares with the daemon.
### Re: virtual document compared with the daemon — gpt-5

Added the comparison directly to the Sidekick analysis docs.

**agent-doc-platform.md** now has a `Virtual document vs daemon` section that makes the distinction explicit: the daemon/supervisor is the live runtime that receives events, coordinates plugins, manages lifecycle, and decides when state changes; the virtual document is the durable markdown artifact that materializes that state so humans and tools can inspect, diff, export, and sometimes edit it. The short version is: the daemon is the engine, the document is the visible interface.

**sidekick-agent-doc-integration.md** now has a matching `Virtual document vs supervisor` section framed in Sidekick terms. It says the supervisor answers "how does the system run?" while the virtual document answers "what state is the system in?" That also sharpens the business case: the supervisor is what makes Sidekick continuous and tool-agnostic, while the virtual document is what makes it legible, reviewable, and portable.

So the clean comparison is not one replacing the other. The daemon is the active coordinator. The virtual document is the inspectable state surface the daemon keeps current.

❯ Why is the response patchback not committed?

❯ Please create a plan for agent-doc to fix the issue and backlog item in agent-loop/tasks/agent-doc/agent-doc-bugs.md
❯ ### Re: fix plan for uncommitted / missing response patchback — codex

Yes. The right plan is to treat this as one reliability bug family with **two concrete entry points** that need separate guards:

1. **Fresh-start trigger miss**  
   The Codex session starts, but `route.rs` never injects the first `agent-doc <file>` trigger, so there is no `preflight_diff_start`, `write_origin`, or `commit_*` at all.

2. **Post-write / pre-commit interruption**  
   The response is already visible in the document and the snapshot save has happened, but the process dies before `commit_staging` / `commit_success`, leaving an uncommitted patchback in the working tree.

#### Proposed implementation plan

**Phase 1 — make fresh Codex start fail closed instead of silently idle**

- Audit the fresh-start path across `start.rs` and `route.rs`, specifically where `auto_trigger = false` hands responsibility to prompt-readiness detection.
- Add an explicit **first-trigger watchdog** for fresh Codex launches:
  - if `codex_start mode=fresh` occurs,
  - and no `preflight_diff_start` / trigger dispatch follows within a bounded window,
  - record a hard failure in cycle state / ops log instead of silently leaving the document idle.
- Add a deterministic fallback trigger path:
  - either a second injection attempt once readiness stabilizes,
  - or a recoverable “route never triggered” state that `preflight` / `repair` can surface immediately on the next run.
- Regression coverage:
  - fresh Codex start where readiness prompt is delayed / noisy,
  - fresh Codex start where first detection fails once then succeeds,
  - fresh Codex start where no readiness ever appears and the run fails closed with a clear reason.

**Phase 2 — close the gap between successful write/IPC and commit completion**

- Treat `write_origin` + `ipc_write_consumed` / `snapshot_saved_file_ipc` without later `commit_*` as a first-class interrupted state, not just generic open-cycle drift.
- Extend `preflight` / `repair` to recognize this state and prefer **resume-commit** over generic recovery when:
  - the captured response body exists,
  - the baseline hash still matches,
  - the working file already contains the intended assistant patchback.
- Tighten `codex_hook.rs` Stop-hook recovery so that when `last_assistant_message` is empty or unusable but the binary can prove the write already landed, it attempts to finish only the **commit boundary** rather than reclassifying the whole turn as “missing response”.
- Regression coverage:
  - IPC write succeeds, commit never starts,
  - commit starts but does not finish,
  - next preflight auto-recovers by finishing commit,
  - recovery refuses to commit if the file drifted after the interrupted write.

**Phase 3 — improve operator visibility**

- Add explicit ops-log markers for:
  - `fresh_route_trigger_missing`
  - `write_completed_commit_missing`
  - `resume_commit_attempt`
  - `resume_commit_success`
  - `resume_commit_blocked_drift`
- Update `session-check` messaging so the two failure classes are distinguishable:
  - “no patchback cycle ever started”
  - “patchback landed but commit boundary was interrupted”
- Update `SKILL.md`, `runbooks/commit.md`, and direct-exec/Codex instructions so the user-facing contract matches the binary recovery behavior.

#### Why this shape

The important thing is not to treat all uncommitted patchbacks as the same bug. A **route-trigger miss** is a startup/routing failure. A **visible uncommitted response** is a commit-boundary interruption. They need different invariants, different logs, and different repair paths.

#### Backlog item to add

I’d track this as one umbrella item with phase wording so the fix stays coherent:

- `[#pbc1] Commit-boundary reliability for Codex/IPC sessions: fail closed when fresh-start route trigger never fires, and auto-resume commit when write/snapshot succeeded but commit boundary was interrupted`

❯ Does #pbc1 still need to be done?
### Re: #pbc1 status — gpt-5 (HEAD)

No. `#pbc1` is already done in `src/agent-doc`.

The implementation landed in commit `92b8df7` (`fix: harden codex commit boundary recovery`), and the current `src/agent-doc` HEAD is later than that on `origin/20260422/codex-agent-backend`, so the fix is already present in this checkout. The code and docs match the plan: `src/preflight.rs` handles the write-completed / commit-missing recovery path, `src/route.rs` fails closed on missed fresh-start triggers, `src/session_check.rs` distinguishes the interrupted commit-boundary state, and the behavior is documented in `specs/07-commands.md` and `specs/codex-support.md`.

I’m marking `#pbc1` done in backlog in this cycle so the document matches the repo state.
<!-- agent:boundary:770a1526 -->
<!-- /agent:exchange -->

<!--
I want sidekick to have the option to use git commits as well.
-->

## Pending / Not Built

<!-- agent:backlog -->
- [x] [#pbc1] Commit-boundary reliability for Codex/IPC sessions: fail closed when fresh-start route trigger never fires, and auto-resume commit when write/snapshot succeeded but commit boundary was interrupted. Plan: `tasks/agent-doc/plans/commit-boundary-reliability-codex-ipc.md`
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->
