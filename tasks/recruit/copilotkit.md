---
agent_doc_session: 668aef4d-700f-4c9e-ae5f-710d0fa6c4fd
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Interview prep revised with annotations. Mon Mar 23, 5:00 PM ET with Jordan Ritter (CTO). Opening calibrated (prior meeting acknowledged). DX-first pitch strategy. Ready.
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

### Re: AG-UI Spec Deep Dive

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
Help me prep for the interview.
I am thinking of proposing that my agent-doc work could be a good fit for MDX + Jupyter integration...Using CopilotKit components.

How likely is it that they would consider the markdown document as an agentic interface?

This may be a good idea to explore, but I also need prep on CopilotKit's offerings as well, so I'm well rounded for the discussion.

### Re: Interview Prep — CopilotKit Offerings + MDX/Jupyter Pitch

---

#### Part 1: CopilotKit Product Knowledge (Know Their World)

**Frontend Components (React-first, expanding to Vue/Angular):**
- `CopilotChat` / `CopilotSidebar` / `CopilotPopup` — chat UIs with different layout modes: I am good with vanilla js, tiny payload javascript components, and WebComponents
- `CopilotTextarea` — drop-in AI-powered textarea replacement (autocompletion, suggestions). *Notable: there's a feature request for Markdown/Rich Text support ([#1509](https://github.com/CopilotKit/CopilotKit/issues/1509)) — directly relevant to your pitch*: It seems that it has not been a company priority, given the response. They punted it back to the community. Could this be an architectural issue?
- Generative UI — agents can render arbitrary React components in the chat stream: Bi-directional editing seems to be obvious. But company priority isn't always obvious.

**Key React Hooks (their API surface):**
- `useCoAgent` — bidirectional state sync between app and agent (the AG-UI shared state hook)
- `useCopilotReadable` — push app context to the agent (one-way: app → agent)
- `useFrontendTool` — register client-side tool handlers with optional UI rendering
- `useComponent` / `useRenderTool` — register React components as tool renderers (generative UI)
- `useHumanInTheLoop` / `useInterrupt` — pause agent for user approval/input
- `useCopilotAdditionalInstructions` — inject contextual system prompts

**Backend (Copilot Runtime):**
- Auth, middleware, routing layer sitting between frontend and agent backends
- Agent adapters: LangGraph, CrewAI, Google ADK, or raw AG-UI
- `copilotkit_emit_state()` — push state updates from agent to frontend
- MCP server connectivity for tool integration

**Business Model:**
- Open-source core (MIT) + cloud hosted runtime + enterprise features (observability, CLHF, self-hosted)
- Revenue from cloud runtime usage and enterprise licenses

---

#### Part 2: MDX + Jupyter Pitch — Honest Assessment

**How likely they'd consider markdown documents as an agentic interface: Medium-High, with caveats.**

**Why it could land well:**
1. **CopilotTextarea already exists** — they've already invested in "AI-enhanced text surfaces." A document/notebook is the natural next step from a textarea.
2. **Feature request [#1509]** — Markdown/Rich Text support for CopilotTextarea is an open community ask. Your pitch connects to real demand.: This has not been a company priority yet. But given that I have experience in this area and they are expanding engineering capacity. And I have a working environment with markdown. Which can expand to MDX and Jupyter.
3. **MDX = React + Markdown** — MDX components ARE React components. CopilotKit's entire stack is React. An MDX document with CopilotKit components embedded is technically coherent.
4. **Jupyter notebooks are agentic documents** — cells execute code, render output, maintain state. An AG-UI-powered notebook where agents can stream into cells, modify state, render generative UI — that's a compelling demo.
5. **Differentiation** — No one else in the AG-UI/CopilotKit ecosystem is doing document-native agent interaction. It's a whitespace opportunity.

**Why to be careful:**
1. **CopilotKit is UI-component-first, not document-first.** Their mental model is "embed AI in your React app." Proposing documents as the primary surface could feel orthogonal to their core thesis.: Should I pitch this as a Developer Experience improvement first? Then pitch this as a tool for openclaw style personal + enterprise knowledge work?
2. **Jordan is a systems engineer** (Napster, distributed systems). He'll appreciate the architectural elegance but will ask "what's the market?" Frame it as an expansion surface, not a pivot.: Enterprise knowledge work?
3. **Don't oversell agent-doc as a competitor.** Position it as "I built a prototype of what a CopilotKit-powered document could look like" — your prior art validates the pattern, CopilotKit has the distribution.: I wouldn't want to oversell as a competitor. This is an opportunity for CopilotKit or whoever hires me specifically.

**Recommended pitch framing:**
> "CopilotKit has chat, sidebar, popup, textarea — what about documents? MDX notebooks where CopilotKit components are first-class citizens. Think: Jupyter meets AG-UI. Agents stream into named document regions, users edit inline, state syncs bidirectionally. I built a prototype of this pattern with agent-doc — CRDT merge, template patching, concurrent editing. The agent-doc experience proves the pattern works. CopilotKit has the ecosystem to scale it."

---

#### Part 3: Interview Prep Cheat Sheet

**Opening (2 min):**
- "Jordan, great to connect. I understand you were at CloudCrowd around 2010 — I actually received an offer from CloudCrowd that year, so we almost crossed paths.": We did actually meet. Jordan has interviewed me before. I actually wanted them to work with Tom Taira because I didn't want to leave Honk. It was kindof awkward though.
- Brief: 20+ years, full-stack systems, last 2 years deep in AI tooling

**Their Questions → Your Answers:**

| They'll Ask | Your Answer |
|-------------|-------------|
| "Tell me about your recent work" | agent-doc (CRDT streaming, template patching, IPC), Presence AI (WebRTC multimodal on H100s), lazily (cross-language reactive signals) |
| "Experience with React?" | rmemo, RelementJS, hyopjs (61-byte hydration). Deep reactive systems, not just React apps — you build the primitives |
| "How do you think about state management?" | 15-year lineage: backbone-signal → ctx-core → rmemo → relysjs. CRDT merge for concurrent edits. Signal-based reactivity. |
| "Agent frameworks?" | agent-doc IS one. Human-in-the-loop, bidirectional state, persistence, streaming. Directly parallel to CoAgents |
| "Why CopilotKit?" | AG-UI is the right abstraction layer — agent↔user protocol. Combined with MCP (agent↔tool) and A2A (agent↔agent), this is the full stack. You want to build at this layer. |
| "Where do you see AG-UI going?" | Beyond chat — into documents, notebooks, collaborative editing. Agent-native surfaces that aren't just chatbots. |

**Your Questions for Jordan:**

1. "AG-UI has 16 event types today. What's the philosophy on adding new ones vs. keeping the protocol minimal?"
2. "CoAgents sync state via snapshots and deltas. Have you explored CRDT-style conflict resolution for cases where both agent and user mutate state concurrently?"
3. "CopilotTextarea seems like a bridge toward richer text surfaces. Is there a roadmap toward document-level or notebook-level agent interaction?"
4. "What does the engineering team structure look like? How much is protocol work vs. SDK work vs. enterprise features?"
5. "What's the biggest technical challenge you're facing right now that you'd want a new engineer to tackle?"

**Things to avoid:**
- Don't position agent-doc as something CopilotKit should adopt wholesale — position it as validation of patterns they could build on
- Don't go too deep into Rust/systems unless Jordan steers there (he probably will given his background)
- Don't criticize their current architecture — show you understand it before suggesting extensions
With my annotation prompts, please regeneratne the prep document.

### Re: Revised Interview Prep (with annotations incorporated)

---

#### Part 1: CopilotKit Product Knowledge — With Your Edge

**Frontend Components:**
- `CopilotChat` / `CopilotSidebar` / `CopilotPopup` — React chat UIs. Your angle: you work at a lower level — vanilla JS, tiny-payload components, WebComponents. You can build these kinds of components from scratch, not just consume them.
- `CopilotTextarea` — AI textarea replacement. The Markdown/Rich Text feature request ([#1509](https://github.com/CopilotKit/CopilotKit/issues/1509)) was punted back to the community. This suggests it may not just be a priority question — there could be an architectural gap. Worth probing: "Is the textarea architecture extensible enough for rich text, or would it need a rethink?" This is where your experience building editor-level primitives (agent-doc's CRDT merge, template patching) becomes directly relevant.
- Generative UI — agents render React components in chat. Bi-directional editing is the obvious next step, but CopilotKit hasn't prioritized it. You've already built it.

**Key Hooks (their API surface you should know):**
- `useCoAgent` — bidirectional state sync (AG-UI shared state)
- `useCopilotReadable` — push app context to agent (one-way)
- `useFrontendTool` — register client-side tools with optional UI
- `useComponent` / `useRenderTool` — generative UI component registration
- `useHumanInTheLoop` / `useInterrupt` — pause agent for user input

**Backend:** Copilot Runtime (auth, middleware, routing) + agent adapters (LangGraph, CrewAI, ADK, raw AG-UI) + MCP server connectivity.

**Business:** Open-source core (MIT) + cloud runtime + enterprise (observability, CLHF, self-hosted). Pre-Series A, ~$660K revenue, scaling from ~6 to ~13.

---

#### Part 2: MDX/Jupyter Pitch — Revised Strategy

**Pitch in two layers:**

**Layer 1 — Developer Experience (lead with this):**
> "Markdown documents are already how developers interact with AI agents — Claude Code, Cursor, GitHub Copilot all use text-based interaction. CopilotKit could own the developer-facing document experience: an MDX notebook where CopilotKit components are first-class. Think: agent-doc meets CopilotKit. CRDT streaming, named component patches, concurrent editing — I've built this. It's a DX tool that your own team and community would use."

**Layer 2 — Enterprise knowledge work (if Layer 1 lands):**
> "Beyond DX, this pattern extends to knowledge work — legal documents, research notebooks, compliance workflows. Think OpenClaw-style personal + enterprise knowledge management where AI agents are embedded in the document itself. The document IS the agentic interface. CopilotKit already has the components and protocol — the document surface is the missing piece."

**Why #1509 matters despite being deprioritized:**
The fact that CopilotKit punted Markdown/Rich Text back to the community could mean:
1. It's architecturally hard with the current textarea design (your opportunity to propose the right architecture)
2. It wasn't high enough priority vs. chat/sidebar/popup (but the team is growing)
3. No one on the team has deep editor/document experience (you do)

Don't lean on #1509 as proof of demand. Instead, frame it as: "I noticed this was shelved. I think I know why it's hard and how to make it work."

---

#### Part 3: Interview Cheat Sheet (Revised)

**Opening (2 min):**
- "Jordan, good to see you again. We actually met back in the CloudCrowd days — you interviewed me around 2010. I ended up staying at Honk at the time, but I've always respected what you were building."
- Brief: 20+ years, full-stack systems, last 2 years deep in AI tooling — building agent runtimes, not just using them.

**Their Questions → Your Answers:**

| They'll Ask | Your Answer |
|-------------|-------------|
| "Tell me about your recent work" | agent-doc (CRDT streaming, template patching, IPC), Presence AI (WebRTC multimodal on H100s), lazily (cross-language reactive signals) |
| "Frontend experience?" | Deeper than React — you build the primitives. rmemo, RelementJS, hyopjs (61-byte hydration). Vanilla JS, WebComponents, tiny payloads. Can build React-level infra or work within it. |
| "State management?" | 15-year lineage: backbone-signal → ctx-core → rmemo → relysjs. CRDT merge for concurrent edits. Signal-based reactivity. This is what CoAgents needs. |
| "Agent frameworks?" | agent-doc IS one. Human-in-the-loop, bidirectional state, persistence, streaming. Same problem as CoAgents, different medium. |
| "Why CopilotKit?" | AG-UI is the right abstraction layer. The protocol stack (MCP + A2A + AG-UI) is the full picture. You want to build at the agent↔user layer. |
| "Where should CopilotKit go next?" | Beyond chat into documents and notebooks. DX first (devs already work in markdown), enterprise knowledge work second. |

**Your Questions for Jordan:**

1. "CopilotTextarea's Markdown/Rich Text support was shelved. Is that an architecture limitation or a priority call? I have ideas on how to approach it."
2. "CoAgents sync state via snapshots and deltas. Have you explored CRDT-style conflict resolution for concurrent agent+user mutations?"
3. "AG-UI has 16 event types. What's the philosophy — keep it minimal or expand for new interaction patterns like document-level events?"
4. "What does the team structure look like? How much is protocol work vs. SDK vs. enterprise?"
5. "What's the biggest technical challenge you'd want a new engineer to tackle first?"

**Tone calibration:**
- You and Jordan have history — keep it warm but don't dwell on the CloudCrowd awkwardness. Acknowledge the meeting briefly and move forward.
- Position agent-doc as YOUR prototype, not as something CopilotKit should adopt. It validates the pattern; CopilotKit has the distribution to scale it.
- Lead with DX, follow with enterprise. Don't pitch the whole vision at once.
- Let Jordan steer into systems/Rust territory if he wants — that's where you'll both be most comfortable.
<!-- agent:boundary:1ca0eedd -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- **NOW:** Rehearse opening + key talking points (interview at 5:00 PM ET today)
- Check for interview confirmation email from Cham Espanola
- Confirm Kevin Volk that interview is locked in
- Post-interview: debrief and next steps
<!-- /agent:pending -->
