---
agent_doc_session: 668aef4d-700f-4c9e-ae5f-710d0fa6c4fd
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Interview prep complete + AG-UI spec deep dive. Mon Mar 23, 5:00 PM ET with Jordan Ritter (CTO).
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Recruiter is kevin-volk.
CTO is Jordan Ritter https://www.linkedin.com/in/jordanritter/
See tasks/recruit/kevin-volk.md
See mail/contacts/kevin-volk

https://www.copilotkit.ai/
https://docs.copilotkit.ai/

I have an interview tomorrow at 17:00 Eastern
Please research.
Give me a summary of the company.
Technical alignment with what I have been doing.
### Re: CopilotKit Research & Interview Prep

**Interview:** Mon Mar 23, 5:00 PM ET / 2:00 PM PT — with Jordan Ritter (CTO)

---

#### Company Summary

CopilotKit (Tawkit, Inc.) — open-source framework for building AI copilots and agent-native applications. Founded 2023 by Atai Barkai (CEO) and Uli Barkai (CMO), Seattle. Jordan Ritter (CTO) — Napster co-founder, CloudCrowd CTO, 25+ years building distributed systems.

- **GitHub:** 28k+ stars (CopilotKit), 9k+ stars (AG-UI protocol)
- **Traction:** 120k+ weekly npm installs, 2M+ agent-user interactions/week, 10%+ Fortune 500 (Cisco, Deloitte, DocuSign, TripAdvisor)
- **Stage:** Pre-Series A. ~$660K revenue (Sep 2025), ~6-person team expanding to ~13
- **Funding:** Seed from SignalFire, Techstars Seattle, Abstraction Capital

**Core Product:**
1. **CopilotKit SDK** — React/Vue/Angular components for in-app AI copilots (chat, sidebar, popups, generative UI, headless)
2. **AG-UI Protocol** — Open event-based protocol (HTTP+SSE) for agent↔frontend. 16 event types. Adopted by Google, LangChain, AWS, Microsoft, Mastra, PydanticAI
3. **Copilot Runtime** — Backend: auth, middleware, routing, agent connections
4. **CoAgents** — Bidirectional state sync between app UI and agent reasoning (LangGraph, CrewAI, ADK)
5. **Enterprise Intelligence** — Observability, CLHF, self-hosted option

**Protocol stack:** MCP (agent↔tool) + A2A (agent↔agent) + AG-UI (agent↔user) — CopilotKit owns the user-facing layer.

---

#### Technical Alignment

| CopilotKit Need | Your Direct Match |
|-----------------|-------------------|
| AG-UI protocol (agent↔frontend events, SSE) | agent-doc — CRDT-based agent-user interaction with IPC patches, boundary markers, snapshot diffing |
| TypeScript + Python SDKs | ctx-core, rmemo, relysjs (TS), lazily (TS+Rust+Python), corky (Python/Rust) |
| React infrastructure | rmemo, RelementJS, hyopjs (61-byte hydration) — React-level infra |
| Generative UI / dynamic rendering | agent-doc template mode: named components with patch blocks, streaming checkpoints |
| State synchronization | 15-year reactive lineage: backbone-signal → ctx-core → rmemo → relysjs. CRDT merge. |
| Agent runtimes / CoAgents | agent-doc IS an agent runtime with human-in-the-loop, bidirectional state, persistence |
| Real-time streaming | Presence AI: multimodal pipeline on H100 GPUs via WebRTC |
| Open source at scale | RSpec core, 15+ packages, SvelteJS/Nano Stores contributor, 40k+ commits |

**Strongest differentiator:** You didn't just *use* agent frameworks — you *built* one. agent-doc's architecture (CRDT streaming, boundary markers, template patching) solves the same problem as AG-UI: keeping agents and humans synchronized in real-time. Document metaphor vs UI component metaphor — deeply complementary mental models.

---

#### Key Talking Points for Jordan

1. **agent-doc as prior art for AG-UI** — "Same abstraction, different angle. agent-doc streams agent responses into documents with CRDT merge. AG-UI streams events into UI components. Both solve bidirectional state sync between human and agent."
2. **Cross-language SDK design** — lazily ships identical reactive signal semantics across TS, Rust, Python. Directly relevant to CopilotKit's multi-language SDK strategy.
3. **"Pilots not mechanics"** — Your workflow is AI-native. You build WITH agents (Claude Code + agent-doc), not just tools for them. You live the product.
4. **Protocol thinking** — AG-UI is an event protocol. agent-doc has its own (IPC patches, boundary markers, snapshot diffs). You think at the protocol level.
5. **CloudCrowd history** — Jordan was CTO when you got the offer in 2010. Warm conversation.

Tell me more about the ag-ui spec.
### Re: AG-UI Spec Deep Dive (HEAD)

**Transport:** Transport-agnostic, primarily HTTP+SSE. Agents implement `run(input: RunAgentInput) -> Observable<BaseEvent>`. Standard `HttpAgent` client sends POST with `RunAgentInput`, receives stream of typed `BaseEvent` objects. Also supports HTTP Binary Protocol (production perf) and WebSockets.

**Event Types (16+ standardized):**

| Category | Events | Key Fields |
|----------|--------|------------|
| **Lifecycle** | `RUN_STARTED` → `STEP_STARTED`/`STEP_FINISHED` → `RUN_FINISHED` \| `RUN_ERROR` | threadId, runId, parentRunId?, stepName |
| **Text Messages** | `TEXT_MESSAGE_START` → `TEXT_MESSAGE_CONTENT` → `TEXT_MESSAGE_END` | messageId, role (developer/system/assistant/user/tool), delta |
| **Tool Calls** | `TOOL_CALL_START` → `TOOL_CALL_ARGS` → `TOOL_CALL_END` + `TOOL_CALL_RESULT` | toolCallId, toolCallName, delta (streamed JSON args), content |
| **State** | `STATE_SNAPSHOT`, `STATE_DELTA`, `MESSAGES_SNAPSHOT` | snapshot (full state), delta (RFC 6902 JSON Patch ops) |
| **Reasoning** | `REASONING_START` → `REASONING_MESSAGE_*` → `REASONING_END` | delta (chain-of-thought chunks), encryptedValue |
| **Activity** | `ACTIVITY_SNAPSHOT`, `ACTIVITY_DELTA` | activityType ("PLAN"/"SEARCH"/etc), RFC 6902 patches |
| **Special** | `RAW`, `CUSTOM`, `META_EVENT` (draft) | Passthrough, custom name+value, annotations |

**State Management (core innovation):**
- `STATE_SNAPSHOT` — complete state object at init or recovery
- `STATE_DELTA` — RFC 6902 JSON Patch operations: `add`, `remove`, `replace`, `move`, `copy`, `test` with JSON Pointer (RFC 6901) paths
- Uses `fast-json-patch` for atomic immutable application
- Bidirectional: agents read app state, emit deltas; frontend observes and reacts
- Human-in-the-loop via state modification (approve/reject/modify agent proposals)

**Direct parallels to agent-doc:**

| AG-UI | agent-doc | Pattern |
|-------|-----------|---------|
| `STATE_DELTA` (JSON Patch) | `patch:component` blocks | Named region patching |
| `STATE_SNAPSHOT` | Snapshot files | Full state baseline |
| `RUN_STARTED`/`RUN_FINISHED` | Boundary markers | Session lifecycle |
| SSE streaming | `--stream` CRDT merge | Real-time event delivery |
| Human-in-the-loop (state mod) | Concurrent user editing | Bidirectional sync |
| `TOOL_CALL_*` lifecycle | IPC patch system | Agent↔host communication |

The mental model overlap is deep — AG-UI patches JSON state via RFC 6902, agent-doc patches document regions via named components. Both stream events, both handle concurrent human edits, both maintain snapshots for recovery. The difference is the medium (JSON state vs markdown document) and the transport (HTTP+SSE vs filesystem+IPC).
<!-- agent:boundary:4c19e980 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- Review and refine talking points before interview
- Check for "Interview Prep + Confirmation" email from Cham Espanola (may have prep instructions)
- Confirm Kevin Volk that interview is locked in
- Post-interview: debrief and next steps
<!-- /agent:pending -->
