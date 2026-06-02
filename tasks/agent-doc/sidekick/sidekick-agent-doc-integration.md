---
agent_doc_session: sidekick-agent-doc-integration
agent_doc_format: template
agent_doc_write: crdt
agent: claude
claude_model: opus
---

## Status

<!-- agent:status patch=replace -->
Planning: Sidekick integration with agent-doc's daemon extension layer.
<!-- /agent:status -->

## Context

Sidekick uses agent-doc's vendor-independent daemon extensions (virtual documents, event channels, plugin API) as its backend infrastructure. This document captures the Sidekick-specific integration layer — how Sidekick's features map to agent-doc's generic capabilities.

**Separation of concerns:**
- agent-doc (open source) provides: the supervisor/runtime, virtual documents, snapshot-only mode, event channels, plugin API, multi-tool export, and a locally inspectable/editable document surface
- Sidekick (proprietary) provides: session analysis intelligence, pattern detection heuristics, proposal generation, team aggregation, coaching

Agent-doc plan: [agent-doc-platform.md](agent-doc-platform.md)

## Integration Architecture

Sidekick registers as an agent-doc plugin. It:
1. Subscribes to supervisor events (tool calls, token usage, session lifecycle)
2. Runs its intelligence layer (health monitoring, pattern detection, analysis pipeline)
3. Writes results back to virtual document components via the plugin API
4. Consumers (VS Code extension, CLI, admin console) read from those components

```
┌──────────────────────────────────────────────────┐
│                  Sidekick Plugin                   │
├──────────────┬───────────────┬───────────────────┤
│ Health       │ Analysis      │ Proposal           │
│ Monitor      │ Pipeline      │ Generator          │
├──────────────┴───────────────┴───────────────────┤
│          agent-doc Plugin API                     │
│  register_listener | write_component | export     │
├──────────────────────────────────────────────────┤
│          agent-doc Supervisor                     │
│  event channel | virtual docs | snapshot-only     │
└──────────────────────────────────────────────────┘
```

## Feature Mapping

| Sidekick Feature | agent-doc Capability Used | Component |
|------------------|--------------------------|-----------|
| Health Monitor | Event listener + rule engine | `sidekick:health` |
| Budget Guardian | Event listener (token events) | `sidekick:budget` |
| Context Surgery | Virtual doc state + CRDT write | `sidekick:rewind` |
| Pre-Session Briefing | Export interface | AGENTS.md / context files |
| Session Analyzer | Plugin API (post-session hook) | `sidekick:analysis` |
| AGENTS.md Proposals | Export interface + write_component | `sidekick:proposals` |
| Discovery-Lag Metric | tsift plugin integration | `sidekick:metrics` |
| Prompt Coach | Event listener (user prompts) | `sidekick:coaching` |
| Knowledge Graph | tsift community data | `sidekick:knowledge` |

## Why the virtual document surface matters

The virtual document is useful even if the practitioner never opens it. It gives Sidekick a stable, vendor-independent state surface that is still easy to inspect when trust, debugging, or adoption requires it, while the supervisor provides the always-on runtime that keeps that surface synchronized and observable.

- **Novel in combination:** the pieces have prior art, but the combination is unusual: a daemon-managed markdown materialized view of agent state that remains locally readable, writable, and traceable
- **Local-first trust surface:** when a team wants to inspect or correct Sidekick output, they can open the document directly instead of depending on a proprietary UI
- **Traceable operations:** snapshot history provides a machine audit trail, and teams that want stronger human review can opt into git-backed exports or commits for selected artifacts
- **Better product story:** "Sidekick writes a real working document on your machine" is easier to demo and easier for enterprise buyers to trust than "Sidekick stores state in a hidden service"
- **Higher operator productivity:** the same document surface can be consumed by plugins, IDE extensions, CLI tools, and humans without separate adapters per consumer

## Virtual document vs supervisor

For Sidekick, the clean comparison is:

- **Supervisor:** the always-on process that receives events, manages lifecycle, coordinates plugins, and keeps state moving
- **Virtual document:** the local markdown state surface that the supervisor and plugins keep up to date
- **Supervisor answers "how does the system run?"** while **the virtual document answers "what state is the system in?"**
- **Supervisor is operational infrastructure** while **the virtual document is inspectable infrastructure**

This distinction matters in the business case. The supervisor is what makes Sidekick continuous and tool-agnostic. The virtual document is what makes it legible, reviewable, and portable.

## Sidekick-Specific Requirements

### Event types Sidekick subscribes to
- `tool_call` — kind, target file, duration, tokens consumed
- `token_spend` — running total, per-call cost
- `correction_event` — user corrects agent output
- `session_start` / `session_end` — lifecycle boundaries
- `file_modification` — which files the agent touches

### Analysis pipeline (proprietary)
- 6-stage pipeline (parse → segment → annotate → extract → propose → summarize)
- Runs post-session via Stop hook
- Results written to `sidekick:proposals` and `sidekick:analysis` components
- Counterfactual replay scoring (proprietary algorithm)

### Team aggregation (enterprise)
- Each developer's virtual Sidekick document → aggregation daemon
- Privacy stripping before any cross-developer data sharing
- Team analytics written to a separate virtual document per team

### Traceability modes
- Default: snapshot-backed virtual documents with no commit noise
- Optional: git-backed commits or exports for teams that want durable review history, compliance evidence, or handoff artifacts

## IP Boundary

The virtual document infrastructure (supervisor/runtime, CRDT merge, snapshot persistence, daemon lifecycle, plugin API) is part of agent-doc's open-source layer. Each component has extensive prior art (Plan 9 synthetic files, Yjs/Automerge CRDTs, VS Code virtual documents, CQRS materialized views). The novelty is in the composition and workflow, not in owning the underlying primitives. Sidekick's patent scope should focus on the intelligence that runs *on top of* virtual documents — the analysis pipeline, scoring algorithms, and team aggregation — not the document infrastructure itself. Keeping the infrastructure open and unencumbered is what enables the ecosystem adoption that makes Sidekick's position defensible. See [business-case-open-platform.md](business-case-open-platform.md) for the full analysis.

## Open Questions

1. Plugin binary distribution: ship Sidekick as a standalone binary that registers with agent-doc, or as a library linked into agent-doc?
2. Licensing boundary: does the plugin API itself need to be MIT/Apache to avoid copyleft concerns?
3. Should Sidekick's virtual document schema be publicly documented (for interop) or proprietary?

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
