# Plan: evaluate axocoatl for agent-doc + an axocoatl-like admin dashboard UI

**Source:** agent-doc-bugs2.md — "Evaluate https://axocoatl.ai/ +
https://github.com/axocoatl/axocoatl. Is there anything agent-doc can use to
improve its development process or enhance its capabilities? Can we have an
axocoatl-like UI into the admin dashboard? Create a comprehensive plan."

## What axocoatl is

A self-hosted, local-first **Rust agentic runtime** for persistent multi-agent
systems (zero telemetry, provider-independent). Key traits (from axocoatl.ai; the
GitHub repo returned 403 to automated fetch — re-fetch via `gh`/browser for the
source-level pass):

- **Declarative multi-agent DAGs** — agents declare `depends_on`; work cascades
  through the dependency chain with no explicit scheduler.
- **Stigmergic coordination** — agents publish `TaskCompleted` events; an
  `EventLattice` accumulates "pheromone" signals and activates downstream agents
  when thresholds cross (event-driven, not centrally orchestrated).
- **Actor model** — `ractor` actors, low memory, fast cold start.
- **Persistence/resilience** — checkpointing; an agent restarts from its last
  checkpoint, not from zero.
- **4-tier persistent memory** with per-agent token budgets.
- **Multi-provider** (Ollama/OpenAI/Anthropic/Mistral/Gemini/OpenRouter), **MCP**
  (tools) + **A2A** (agent interop).
- **CLI:** `onboard`, `dev|serve`, `workflow list|run` (the DAGs), `chat -a <agent>`,
  `tokens report`.

## What agent-doc can actually use (mapped to existing surfaces)

| axocoatl idea | agent-doc today | Opportunity |
|---|---|---|
| Declarative `depends_on` DAG | `auto-dag` lane-grouping only (no real edges) | **High.** Add an optional `depends_on: [#id,…]` annotation on backlog/review items; `auto-dag` infers true edges → a real dependency DAG, not just lanes. Directly completes the `#auto-dag-first-class` follow-up. |
| Event-driven actor coordination (`ractor`/EventLattice) | synchronous, single-doc-lock pipeline; `hooks.rs` cross-session events | **High, validates the redesign.** The IPC/CRDT plan's *per-document work queues + non-blocking fire-and-reconcile* is the actor model. axocoatl is proof the actor approach fits; consider `ractor` (or a lean channel-per-doc) for `plan-ipc-crdt-pipeline-redesign.md` Phase 4. |
| Checkpoint/restart resilience | `capture.rs` + `snapshot.rs` + `cycle_state.rs` recovery | Low/medium. agent-doc already checkpoints responses + cycle state; axocoatl's framing could tighten the recovery story but no missing primitive. |
| Per-agent token budgets | `model_tier.rs` + FlowCore token budget gate | Low. Concept already present. |
| `tokens report` CLI | claude-mem / tsift token analytics | Low. Already covered externally. |
| Web dashboard | `agent-doc admin` (CLI fleet actor control plane) | **High — the user's explicit ask.** See below. |

**Not worth adopting:** axocoatl's provider abstraction (agent-doc delegates to
the harness — Claude/Codex/OpenCode — by design), 4-tier memory (out of scope),
A2A (agent-doc is single-agent-per-doc by design).

## Axocoatl-like admin dashboard UI (the explicit ask)

agent-doc has a real control plane already — `agent-doc admin` (`#ipc-admin-api`,
fleet-wide actor admin: `admin list`, `admin detect`, etc.) — but it is CLI-only.
Plan a **local web UI** over it:

- **Backend:** a read-mostly HTTP/SSE endpoint exposing the existing admin data
  (actors, owned panes, per-document queues, session logs, cycle/pipeline state)
  — reuse `project_controller` + `sessions` + `admin` queries; no new source of
  truth. Mutations (clear / interrupt-clear / restart-supervisor / route) go
  through the existing operator-command paths.
- **Views:**
  1. **Fleet** — every document, its authoritative actor/pane, busy/idle, last
     cycle, open-cycle/startup-miss warnings (what `admin detect` surfaces).
  2. **Per-document queue** — live `agent:queue` with heads + auto-loop state.
  3. **Auto-DAG** — render `agent-doc auto-dag <FILE>` (the Mermaid work-graph) in
     the browser; click an item → its plan file / status. This is where the
     axocoatl "workflow DAG" UX maps directly.
  4. **Turn status** — the `.agent-doc/turn-active.json` markers (live "turn in
     progress" per pane).
- **Tech:** keep it lean + local-first (matches axocoatl's ethos and the
  livestream-safe constraint) — a small Rust server (axum) + a static frontend,
  no telemetry.

## Phasing
1. `depends_on` edges for `auto-dag` (small, high value, completes the auto-dag).
2. Admin HTTP/SSE read API over existing `admin`/`project_controller` data.
3. Web UI: Fleet + Per-doc queue views.
4. Web UI: Auto-DAG render + Turn status.
5. (Redesign cross-link) evaluate `ractor` vs channel-per-doc for the IPC/CRDT
   per-document work queues (`plan-ipc-crdt-pipeline-redesign.md` Phase 4).

## Follow-up
- Re-fetch the GitHub repo via `gh`/browser for a source-level pass (license,
  actual `ractor` usage, dashboard code to borrow) — automated fetch hit 403.
