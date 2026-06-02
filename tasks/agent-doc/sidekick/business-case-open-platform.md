# Business Case: Open Infrastructure + Proprietary Intelligence

**For:** Robert Ross / Dreamable
**Subject:** Why agent-doc as open-source infrastructure strengthens Sidekick's market position

## The model

agent-doc (open source) publishes stable infrastructure: the supervisor/runtime, virtual documents, event channels, plugin API, snapshot-only mode, and multi-tool export. Sidekick (proprietary) is a plugin that builds intelligence on top — session analysis, pattern detection, proposal generation, team aggregation, coaching. The infrastructure is open. The intelligence is proprietary.

## Why this is stronger than fully proprietary

### 1. Ecosystem development creates a moat Sidekick can't build alone

A fully proprietary stack competes on features. An open infrastructure layer competes on adoption. When agent-doc ships virtual documents and a plugin API, every developer tool company becomes a potential ecosystem participant — monitoring tools, IDE extensions, analytics platforms, CI integrations. Each one builds on agent-doc's primitives, which means each one makes the ecosystem more valuable.

Sidekick doesn't need to build integrations for every tool. The ecosystem builds them. Sidekick just needs to be the best intelligence layer on top. This is the Redis model (open protocol, proprietary enterprise features), the Kubernetes model (open orchestration, proprietary management planes), the VS Code model (open editor, proprietary extensions marketplace). The infrastructure commoditizes; the intelligence differentiates.

Rob's provisional patent covers the intelligence — the scoring algorithms, the session analysis pipeline, the discovery-lag heuristics, the counterfactual replay, the team aggregation patterns. None of that lives in agent-doc. The patent protects exactly the proprietary layer that generates revenue.

### 2. Open infrastructure eliminates the adoption bottleneck

Sidekick's pitch identifies the ICP as "any organization with 5+ developers using Claude Code, Cursor, or GitHub Copilot." That's the same market where AGENTS.md, CLAUDE.md, .cursor/rules, and copilot-instructions.md are already fragmenting. No enterprise will adopt a fully proprietary daemon that only works with one vendor's tooling.

An open agent-doc plugin API means Sidekick works everywhere these tools run — without Dreamable having to build and maintain integrations for each one. The supervisor is what makes that practical: one open runtime handles lifecycle, event delivery, and state coordination across tools, while the multi-tool export interface (AGENTS.md, copilot-instructions.md, .cursor/rules) solves the cross-tool standardization problem from the Sidekick pitch. It solves it at the infrastructure layer, where the effort is shared, rather than the product layer, where Dreamable bears it alone.

Enterprise procurement teams evaluate vendor lock-in risk. "Sidekick uses an open-source infrastructure layer with a stable plugin API" reduces that risk to near zero. If Dreamable disappears, the customer's data and documents persist in an open format. That's a selling point in enterprise sales, not a weakness.

### 3. Dreamable captures the high-margin layer

The Sidekick pricing model ($29/dev/month team, $50K-$500K ACV enterprise) monetizes intelligence, not infrastructure. The infrastructure is a cost center. Open-sourcing it shifts that cost to community maintenance while Dreamable retains 100% of the revenue-generating layer.

Consider the alternative: Dreamable builds and maintains its own supervisor/runtime, its own virtual document system, its own event channel, its own daemon lifecycle manager, its own CRDT merge engine, its own snapshot system. That's 6-12 months of engineering on infrastructure that generates zero revenue. It creates a maintenance burden that scales with customer count. And it duplicates work Brian has already done in agent-doc.

With the open model, Brian's existing agent-doc work is the infrastructure. Dreamable pays Brian for the Sidekick intelligence layer on top. The infrastructure maintenance is shared with the open-source community. Dreamable's engineering investment goes entirely toward the proprietary differentiators — exactly the pieces the patent covers.

### 4. This gives Dreamable leverage over the market, not just a product

The Sidekick pitch says "Sidekick is NOT another coding agent. It is the compiler and coach that turns agent traces into institutional knowledge." If the infrastructure under that compiler is proprietary, Sidekick is just another developer tool competing for shelf space. If the infrastructure is an open standard, Sidekick becomes the reference implementation of how intelligent session management works.

This is the leverage play: Dreamable doesn't just sell a product. Dreamable defines the category. When competitors build their own session intelligence tools, they build on agent-doc's plugin API — the same API Sidekick already dominates. The competition validates the ecosystem while competing at the intelligence layer, where Sidekick's patent, data flywheel (more sessions observed ... better pattern detection ... better proposals ... higher acceptance rate ... more sessions observed), and first-mover advantage are defensible.

Compare: if Dreamable builds a fully proprietary stack, competitors build their own incompatible stacks. Now Dreamable is in a features war with no ecosystem advantage. Enterprise customers pick the tool with the most integrations, and Dreamable's 6-person team can't out-integrate a well-funded competitor.

### 5. Brian's alignment strengthens execution

Brian was building agent-doc toward daemon extensions, virtual documents, and real-time event streams before meeting Rob. That trajectory now converges with Sidekick's requirements. The open-source model means Brian's personal investment in agent-doc and his work for Dreamable are additive — agent-doc features he builds for his own users benefit Sidekick, and Sidekick requirements push agent-doc in directions the open-source community also wants.

A fully proprietary model creates tension: Brian either stops developing agent-doc (losing the open-source community and his own tool) or maintains two diverging codebases. The open model eliminates that tension. One codebase. One trajectory. Dreamable gets the infrastructure for free by aligning with it.

## Addressing the patent concern directly

The provisional patent protects algorithms and methods, not infrastructure. The separation is clean:

| **Protected by patent (proprietary)** | **Open infrastructure (agent-doc)** |
|---|---|
| Session analysis pipeline (6-stage) | Supervisor/runtime and daemon lifecycle |
| Discovery-lag scoring algorithm | Virtual document lifecycle |
| Counterfactual replay scoring | Event channel on supervisor |
| Team aggregation + privacy stripping | Plugin API (register, read, write, subscribe) |
| Prompt coaching heuristics | CRDT merge engine |
| Expertise-weighted voting | Multi-tool export interface |
| Cross-session pattern mining | Snapshot-only mode, daemon process management |

The patent covers the intelligence that reads and writes through the infrastructure. Open-sourcing the infrastructure doesn't weaken the patent — it strengthens the market position of the patented technology by giving it a larger addressable surface.

### Why virtual documents specifically should stay out of the patent

The virtual document concept — daemon-managed structured documents with CRDT merge and snapshot persistence — is built from well-established open-source primitives, each with decades of prior art:

- **Daemon-managed files:** Plan 9 synthetic files (1992), Linux `/proc`, FUSE, VS Code `TextDocumentContentProvider` (2016), AgentFS (Turso, 2024)
- **CRDT merge for concurrent writes:** Yjs (2015), Automerge (Ink & Switch, 2017), Loro (2023), Peritext (2022)
- **Structured components in documents:** Org-mode (2003), Obsidian Dataview (2020)
- **Materialized views of system state:** CQRS/Event Sourcing patterns, Riffle (MIT/Ink & Switch, UIST 2023), ksqlDB
- **Snapshot-based state tracking:** Automerge, Jupyter checkpoints, AgentFS
- **Plugin APIs with event subscriptions:** VS Code Extension API, Obsidian Plugin API, Yjs providers

There is also existing patent art in the space: EP1642208A2 (2004, Edgar Online) covers a "virtual document architecture" with reusable components and version control for collaborative publishing — active in some jurisdictions. US5787449A (1994, IBM, expired) covers separating document architecture from content.

Including virtual documents in the patent scope would invite prior art challenges on every component. More importantly, it would work against ecosystem adoption — the whole point of the open infrastructure model. If the infrastructure is patented, third-party developers can't build on it freely, which kills the ecosystem flywheel before it starts.

The stronger patent strategy: let the infrastructure be open and unencumbered, focus the patent claims on the intelligence layer that *uses* that infrastructure. The scoring algorithms, the 6-stage analysis pipeline, the counterfactual replay, the discovery-lag heuristics, the team aggregation with privacy stripping — these have no meaningful prior art in the AI agent session management space. That's where the defensible IP lives.

### Why virtual documents are still strategically valuable

Keeping virtual documents out of the patent does **not** make them unimportant. It makes them a stronger product and market wedge.

- **The novelty is in the combination and packaging.** A local markdown document that is daemon-managed, CRDT-safe, snapshot-traceable, and usable as an agent state surface is a distinctive product shape even if the primitives have prior art. It is useful to users without needing to be exclusionary IP.
- **Local editability increases trust.** Teams can open the artifact on their own machine, inspect what the system believes, and correct it without asking Sidekick for permission. That reduces the "black box" objection that slows enterprise adoption.
- **Traceability has operational value.** Snapshot history gives a machine-owned audit trail; optional git-backed commits or exports give teams a human review trail when they want compliance evidence, handoff records, or retrospective analysis.
- **The document itself is a marketing asset.** "The agent leaves behind a real working document" is tangible. It demos well, screenshots well, and is easier to understand than an invisible background service. The artifact helps buyers immediately see what they are getting.
- **The document surface improves productivity.** A single artifact can serve as the daemon's state, the plugin integration surface, the debugging surface for engineers, and the review surface for managers. That collapses coordination overhead that would otherwise be spread across separate UIs and hidden stores.

In other words: the right move is not to hide virtual documents behind proprietary IP. The right move is to make them legible, useful, and widely adoptable so Sidekick becomes the best intelligence layer on top of a format people already trust.

## The bottom line

Fully proprietary means Dreamable builds everything, maintains everything, and competes on features with a 6-person team against well-funded competitors. Open infrastructure + proprietary intelligence means Dreamable defines the category, shares maintenance costs, eliminates vendor lock-in objections in enterprise sales, and focuses 100% of engineering on the revenue-generating layer that the patent protects. The ecosystem does the distribution. Sidekick does the thinking.

## Related documents

- [agent-doc-platform.md](agent-doc-platform.md) — vendor-independent daemon extensions plan (virtual docs, event channels, plugin API, multi-tool export)
- [sidekick-agent-doc-integration.md](sidekick-agent-doc-integration.md) — how Sidekick registers as an agent-doc plugin and maps features to capabilities
