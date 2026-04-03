---
agent_doc_session: 668aef4d-700f-4c9e-ae5f-710d0fa6c4fd
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Follow-up email pushed to Gmail drafts. Review and send from Gmail.
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

| They'll Ask | Your Answer                                                                                                                                                                           |
|-------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| "Tell me about your recent work" | agent-doc (CRDT streaming, template patching, IPC), Presence AI (WebRTC multimodal on H100s), lazily (cross-language reactive signals)                                                |
| "Frontend experience?" | Deeper than React — you build the primitives. rmemo, RelementJS, hyopjs (61-byte hydration). Vanilla JS, WebComponents, tiny payloads. Can build React-level infra or work within it. |
| "State management?" | 15-year lineage: backbone-signal → ctx-core → rmemo → rappstack. CRDT merge for concurrent edits. Signal-based reactivity. This is what CoAgents needs.                               |
| "Agent frameworks?" | agent-doc IS one. Human-in-the-loop, bidirectional state, persistence, streaming. Same problem as CoAgents, different medium.                                                         |
| "Why CopilotKit?" | AG-UI is the right abstraction layer. The protocol stack (MCP + A2A + AG-UI) is the full picture. You want to build at the agent↔user layer. human-agent alignment.                   |
| "Where should CopilotKit go next?" | Beyond chat into documents and notebooks. DX first (devs already work in markdown), enterprise knowledge work second.                                                                 |

The interview is only scheduled for 30 minutes. I suspect there won't be many questions for me. So I will need to drive this in an open-ended format. Please help me with key points to discuss.

**Your Questions for Jordan:**

1. "CopilotTextarea's Markdown/Rich Text support was shelved. Is that an architecture limitation or a priority call? I have ideas on how to approach it.": I want to be careful not to be too pushy in case their priorities are different. This is a question of whether or not our interests align as well.
2. "CoAgents sync state via snapshots and deltas. Have you explored CRDT-style conflict resolution for concurrent agent+user mutations?"
3. "AG-UI has 16 event types. What's the philosophy — keep it minimal or expand for new interaction patterns like document-level events?"
4. "What does the team structure look like? How much is protocol work vs. SDK vs. enterprise?"
5. "What's the biggest technical challenge you'd want a new engineer to tackle first?"

**Tone calibration:**
- You and Jordan have history — keep it warm but don't dwell on the CloudCrowd awkwardness. Acknowledge the meeting briefly and move forward.
- Position agent-doc as YOUR prototype, not as something CopilotKit should adopt. It validates the pattern; CopilotKit has the distribution to scale it.
- Lead with DX, follow with enterprise. Don't pitch the whole vision at once.
- Let Jordan steer into systems/Rust territory if he wants — that's where you'll both be most comfortable.
Iterate on the prep docs with my annotations.

### Re: 30-Minute Interview Flow — Key Points to Drive

With only 30 minutes and likely an open-ended format, here's a structured flow you can drive:

---

**Minutes 1–3: Warm reconnect**
"Jordan, good to see you again — we met back in the CloudCrowd days, you interviewed me around 2010. I ended up staying at Honk at the time. A lot has happened since then."

> I ended up staying at Honk at the time. A lot has happened since then.

Let's rephrase this a bit.

Keep it brief. Don't dwell on the awkwardness — just acknowledge and move forward.

I agree.

---

**Minutes 3–8: What you've been building (your pitch, not their questions)**

Lead with agent-doc as a story, not a resume bullet:

> "For the last two years I've been deep in AI tooling. The thing I'm most excited about is agent-doc — it's a system I built for human-agent collaboration through documents. The agent streams responses into named document regions using CRDT merge, the user edits concurrently, and state syncs bidirectionally. It's essentially the same problem AG-UI solves — keeping agents and humans synchronized in real-time — but through a document metaphor instead of web UI components."

Then bridge to CopilotKit:

> "When I saw AG-UI, the pattern recognition was immediate. Same architecture: snapshots, deltas, streaming events, concurrent editing. Different medium. That's what drew me to CopilotKit — you're building the protocol layer for human-agent alignment, which is exactly where I want to work."

---

**Minutes 8–15: Technical depth (let Jordan steer)**

Have these ready but let the conversation flow naturally:

- **State sync lineage**: "I've been building reactive state systems for 15 years — backbone-signal through ctx-core, rmemo, rappstack. CRDT merge for concurrent edits. Signal-based reactivity. This maps directly to what CoAgents does with `useCoAgent` and state deltas."

Include lazily...which I used in production Python. But it's also cross platform with Zig and Rust.

- **Cross-language work**: "lazily ships the same reactive signal semantics across TypeScript, Rust, and Python. Same concept as CopilotKit maintaining SDKs across languages."

- **Presence AI**: "Most recently built a multimodal pipeline on H100 GPUs via WebRTC — real-time streaming at infrastructure scale."

- **Vanilla JS / WebComponents depth**: "I work at the primitive level — 61-byte hydration runtime, WebComponents, tiny payloads. I can build React-level infrastructure, not just consume it."

I can also build reactive libraries that are Front End library agnostic. 

---

**Minutes 15–22: Explore mutual alignment (not a sales pitch)**

This is where you explore fit rather than push. Softer framing:

> "I'm curious about where CopilotKit sees the interaction surface expanding. Chat, sidebar, popup, textarea — is there interest in richer surfaces like documents or notebooks? That's where my experience naturally leads, and I want to make sure our interests align."

If Jordan engages with the document idea:
> "I've been thinking about MDX notebooks where CopilotKit components are first-class — agents streaming into named regions, users editing inline. It's a DX play first — developers already work in markdown. But it extends to enterprise knowledge work too."

If Jordan steers elsewhere, follow his lead. The goal is to surface alignment, not force a pitch.

Sounds good.

---

**Minutes 22–28: Their side**

Questions that show you understand CopilotKit deeply:

1. "What's the biggest technical challenge the team is facing right now?"
2. "How do you think about keeping AG-UI minimal vs. expanding for new interaction patterns?"
3. "What does the engineering team structure look like — how much is protocol vs. SDK vs. enterprise?"

---

**Minutes 28–30: Close**

> "This has been great. I'm genuinely excited about AG-UI as a protocol layer — it maps to patterns I've independently built. I'd love to continue the conversation."

---

**Key mindset: You're evaluating them too.** This is 30 minutes to determine if CopilotKit's direction aligns with where you want to build. Don't pitch too hard. Let the technical overlap speak for itself. Jordan is a systems engineer — he'll recognize the depth without you having to sell it.
Iterate with annotations.

### Re: Revised 30-Minute Flow (with annotations)

---

**Minutes 1–3: Warm reconnect (rephrased)**

"Jordan, good to connect again — you interviewed me back in the CloudCrowd days around 2010. It's been a while. I've been building distributed systems and reactive tooling since, and the last couple years have been all AI infrastructure."

*Keeps it warm, doesn't linger on the Honk/CloudCrowd dynamics, pivots straight to what you're doing now.*

---

**Minutes 3–8: What you've been building**

> "The thing I'm most excited about right now is agent-doc — a system I built for human-agent collaboration through documents. The agent streams responses into named document regions using CRDT merge, the user edits concurrently, and state syncs bidirectionally. It's the same problem AG-UI solves — keeping agents and humans synchronized in real-time — but through a document metaphor instead of web UI components.
>
> I also built lazily — a cross-platform reactive signals library that runs in production Python, and also compiles to Zig and Rust. Same signal semantics across languages. That cross-language SDK thinking maps directly to what CopilotKit does with its TypeScript and Python SDKs.
>
> More broadly, I build reactive libraries that are frontend-framework-agnostic — they work with React, Svelte, vanilla JS, WebComponents, whatever. The primitives are the same regardless of the rendering layer. That's how I think about CopilotKit's component surface too — the protocol and state management are the real product, the UI framework is just the rendering target."

Then bridge:

> "When I saw AG-UI, the pattern recognition was immediate. Snapshots, deltas, streaming events, concurrent editing — same architecture I built independently. That's what drew me to CopilotKit. You're building the protocol layer for human-agent alignment."

---

**Minutes 8–15: Technical depth (let Jordan steer)**

Ready to go deeper on any of these:

- **Reactive lineage**: "15 years of reactive state — backbone-signal → ctx-core → rmemo → rappstack. Signal-based, framework-agnostic. The same patterns power agent-doc's CRDT merge and CopilotKit's CoAgents state sync."

- **lazily in production**: "lazily runs in production Python at Presence AI. Cross-compiles to Zig and Rust. Same signal semantics everywhere — subscribe, derive, batch. Directly relevant to CopilotKit's multi-language SDK challenge."

- **Framework-agnostic approach**: "I don't build for React or Svelte — I build the reactive layer underneath. rmemo + relementjs work with any renderer. That means I can contribute to CopilotKit's React SDK today and help extend to Vue, Angular, or WebComponents tomorrow."

- **Presence AI**: "Multimodal pipeline on H100 GPUs via WebRTC — real-time streaming at infrastructure scale."

---

**Minutes 15–22: Explore alignment (soft)**

> "I'm curious about where CopilotKit sees the interaction surface expanding. Chat, sidebar, popup, textarea — is there interest in richer surfaces like documents or notebooks? That's where my experience naturally leads, and I want to make sure our interests align."

If he engages → DX first, enterprise knowledge work second. If not → follow his lead.

---

**Minutes 22–28: Questions**

1. "What's the biggest technical challenge the team is facing right now?"
2. "How do you think about keeping AG-UI minimal vs. expanding for new interaction patterns?"
3. "What does the engineering team structure look like — protocol vs. SDK vs. enterprise?"

---

**Minutes 28–30: Close**

> "This has been great. I'm genuinely excited about AG-UI as a protocol layer — it maps to patterns I've independently built. I'd love to continue the conversation."
Should I mention my recent LinkedIn posts and videos to demonstrate what I'm working on and how I work?

### Re: Mentioning LinkedIn Posts & Videos

**Yes, but strategically — don't lead with it.** Here's when and how:

**Don't proactively offer it in the first 15 minutes.** The opening and pitch should stand on their own — agent-doc, lazily, reactive lineage. You want Jordan to be interested in what you built, not what you posted about.

**Natural moment to drop it (minutes 15–22, alignment exploration):**

If Jordan seems engaged and you're discussing the document/notebook direction or your workflow:

> "I've actually been posting about this on LinkedIn — demos of agent-doc in action, the CRDT streaming, how I work with Claude Code day-to-day. I can share the links after if you're curious. It shows the workflow better than I can describe it in a call."

**Why this works:**
- Shows you're actively building in public, not just interviewing
- Demonstrates confidence — you're not hiding your work
- Gives Jordan something concrete to review after the call (extends your 30 minutes)
- "I can share after" keeps the conversation flowing rather than derailing into "let me pull up a video"

**If he asks what you've been working on and seems to want more detail:**
> "I've been livestreaming my development sessions — building with Claude Code and agent-doc in real-time. It's all on LinkedIn and YouTube. Happy to send links."

He will probably ask early what I'm working on. But you recommend I mention this in minute 22+.
So should I mention this earlier or later?

**Don't:**
- Pull up videos during the call (wastes the 30 minutes)
- List specific post titles or view counts
- Frame it as "content creation" — frame it as "building in public"
Iterate on the prep doc.

### Re: When to Mention LinkedIn Posts/Videos

You're right — Jordan will almost certainly ask "what have you been working on?" in the first 5–10 minutes. That's the natural moment for the LinkedIn/video mention, not minute 22.

**Revised timing:** Weave it into your pitch (minutes 3–8) as a closing beat after describing agent-doc and lazily:

> "...I've been building in public — posting demos on LinkedIn, livestreaming dev sessions. If you want to see agent-doc in action rather than just hear me describe it, I can share links after the call."

**Why this works better than saving it for minute 22:**
- It's a natural continuation of "what are you working on" — not a tangent
- Shows you're actively producing, not just interviewing
- "I can share after" plants a seed without derailing the conversation
- Jordan has something to review post-call (extends your 30 minutes)

**Updated flow:**

| Minutes | What | Key Points |
|---------|------|------------|
| 1–3 | Reconnect | CloudCrowd history, brief |
| 3–8 | Your pitch | agent-doc → lazily → framework-agnostic → "building in public, happy to share demos" |
| 8–15 | Technical depth | Let Jordan steer. State sync lineage, Presence AI, rmemo + relementjs |
| 15–22 | Alignment | "Where does CopilotKit see the interaction surface expanding?" Soft document/notebook pitch |
| 22–28 | Questions | Biggest challenge, AG-UI philosophy, team structure |
| 28–30 | Close | "Genuinely excited, love to continue" |

The LinkedIn mention moved from minute 22 to the end of your pitch block (minute 7–8). Everything else stays the same.
Please render the detail document for the call. Not just the summary.
Should I name drop rmemo + relementjs? He probably has not heard of it. I havn't had anyone ask more about it.

### Interview Script — CopilotKit CTO Call, Mon Mar 23, 5:00 PM ET

**Jordan Ritter, CTO — 30 minutes, open-ended format**

---

#### Minutes 1–3: Reconnect

"Jordan, good to connect again — you interviewed me back in the CloudCrowd days around 2010. It's been a while. I've been building distributed systems and reactive tooling since, and the last couple years have been all AI infrastructure."

---

#### Minutes 3–8: Your Pitch — What You've Been Building

**agent-doc (lead):**

"Right now, I'm excited about agent-doc — a system I built for human-agent collaboration through markdown documents. The agent streams responses into named document regions using CRDT merge, the user edits concurrently, and state syncs bidirectionally. It's the same problem AG-UI solves — keeping agents and humans synchronized in real-time — but through a document metaphor instead of web UI components."

**lazily (cross-language signals):**

"I also built lazily — a cross-platform reactive signals library that runs in production Python, and also compiles to Zig and Rust. Same signal semantics across languages. The Zig and Rust implementations use FFI for cross platform development. That cross-language SDK thinking maps directly to what CopilotKit does with its TypeScript and Python SDKs."

agent-doc's plugin architecture also uses cross a platform FFI shared library for in-process support with multiple editors.

**Framework-agnostic reactive libraries:**

"More broadly, I build reactive libraries that are frontend-framework-agnostic — rmemo and relementjs work with React, Svelte, vanilla JS, WebComponents, whatever. The primitives are the same regardless of the rendering layer. That's how I think about CopilotKit's component surface too — the protocol and state management are the real product, the UI framework is just the rendering target."

**Bridge to CopilotKit:**

"When I saw AG-UI, the pattern recognition was immediate. Snapshots, deltas, streaming events, concurrent editing — same architecture I built independently. That's what drew me to CopilotKit. You're building the protocol layer for human-agent alignment."

**Building in public (closing beat):**

"I've been posting demos on LinkedIn and livestreaming dev sessions — dogfooding agent-doc with Claude Code in real-time. Happy to share links after the call if you want to see the workflow rather than just hear me describe it."

---

#### Minutes 8–15: Technical Depth — Let Jordan Steer

Have these ready. Don't dump them all — let the conversation pull what's relevant.

**If he asks about state management:**

"I've been building reactive state systems for 15 years — backbone-signal through ctx-core, rmemo, rappstack. Signal-based, framework-agnostic. CRDT merge for concurrent edits. This maps directly to what CoAgents does with `useCoAgent` and state deltas. The pattern is the same: snapshots for baselines, deltas for incremental updates, atomic application."

**If he asks about agent-doc's architecture:**

"agent-doc uses named document components — `&lt;!-- agent:exchange --&gt;`, `&lt;!-- agent:status --&gt;` — similar to AG-UI's component model. The agent emits patch blocks targeting specific components, applied via CRDT merge so the user can edit concurrently without conflicts. Snapshots track state between sessions. IPC patches let the editor (JetBrains plugin) react to agent writes in real-time. On the wire it's filesystem + IPC instead of HTTP + SSE, but the event architecture is structurally identical to AG-UI."

**If he asks about Presence AI:**

"Most recently I built a multimodal pipeline on H100 GPUs via WebRTC — real-time audio/video/screen streaming at infrastructure scale. Production Python with lazily for reactive state management."

**If he asks about frontend depth:**

"I work at the primitive level — hyopjs is a 61-byte hydration runtime. WebComponents, vanilla JS, tiny payloads. I can build React-level infrastructure, not just consume it. rmemo and relementjs are framework-agnostic — they work with any renderer. That means I can contribute to CopilotKit's React SDK today and help extend to Vue, Angular, or WebComponents tomorrow."

**If he asks about open source:**

"RSpec core contributor in the early days — helped build the testing framework Ruby developers still use. SvelteJS contributor, Nano Stores contributor. 40k+ commits. 15+ published packages across npm, PyPI, crates.io."

---

#### Minutes 15–22: Explore Alignment — Soft, Not a Sales Pitch

**Opening question:**

"I'm curious about where CopilotKit sees the interaction surface expanding. Chat, sidebar, popup, textarea — is there interest in richer surfaces like documents or notebooks? That's where my experience naturally leads, and I want to make sure our interests align."

**If Jordan engages with the document idea:**

"I've been thinking about MDX notebooks where CopilotKit components are first-class citizens — agents streaming into named regions, users editing inline. It's a DX play first — developers already work in markdown. But it extends to enterprise knowledge work too — legal documents, research notebooks, compliance workflows. The document IS the agentic interface."

"I noticed CopilotTextarea's Markdown/Rich Text support was shelved. I'm curious whether that's an architecture question or a priority question — I have thoughts on how to approach it if it's ever on the table."

**If Jordan steers elsewhere — follow his lead.** The goal is to surface alignment, not force a pitch.

**If he asks "what's the market?":**

"Developer experience first — your own community and team would use it. Then enterprise knowledge work — think OpenClaw-style personal and enterprise knowledge management where AI agents are embedded in the document itself."

---

#### Minutes 22–28: Your Questions for Jordan

1. "What's the biggest technical challenge the team is facing right now?"
2. "How do you think about keeping AG-UI minimal vs. expanding for new interaction patterns?"
3. "What does the engineering team structure look like — how much is protocol work vs. SDK vs. enterprise features?"
4. *(If time)* "CoAgents sync state via snapshots and deltas. Have you explored CRDT-style conflict resolution for cases where both agent and user mutate state concurrently?"

---

#### Minutes 28–30: Close

"This has been great. I'm genuinely excited about AG-UI as a protocol layer — it maps to patterns I've independently built. I'd love to continue the conversation."

---

#### Quick Reference Card

| Topic | Key Phrase |
|-------|-----------|
| agent-doc | "CRDT streaming, named document regions, bidirectional state sync — same problem as AG-UI" |
| lazily | "Cross-platform reactive signals — production Python, compiles to Zig and Rust" |
| Framework-agnostic | "I build the reactive layer underneath — rmemo + relementjs work with any renderer" |
| AG-UI connection | "Pattern recognition was immediate — snapshots, deltas, streaming, concurrent editing" |
| Document pitch | "MDX notebooks with CopilotKit components — DX first, enterprise knowledge work second" |
| Building in public | "Livestreaming dev sessions, demos on LinkedIn — happy to share links" |
| Why CopilotKit | "Protocol layer for human-agent alignment — where I want to build" |

### Re: Name-dropping rmemo + relementjs

**Skip the names. Describe the capability.**

Jordan hasn't heard of them and won't ask. Dropping unfamiliar library names in a 30-minute call creates cognitive load without payoff — he has to decide whether to ask about them or let them pass, and either way it breaks flow.

**Instead, say the same thing without the names:**

Before (name-heavy):
> "rmemo and relementjs work with React, Svelte, vanilla JS, WebComponents"

After (capability-focused):
> "I build reactive libraries that are framework-agnostic — they work with React, Svelte, vanilla JS, WebComponents. Same primitives regardless of the rendering layer."

Same signal, zero friction. If Jordan is genuinely interested and follows up, you can name them then. The names are for follow-up links, not the call itself.

**Same applies to lazily** — you can say "a cross-platform reactive signals library I built" rather than leading with the name. The name matters in a README, not in a pitch.

**Exception: agent-doc.** Keep naming agent-doc — it's your headline project and you'll reference it multiple times. One named project is memorable. Three is noise.

**Updated quick reference:**

| Topic | Say This |
|-------|----------|
| Framework-agnostic | "I build reactive libraries that are framework-agnostic — they work with any renderer" |
| Cross-language | "I built a cross-platform reactive signals library — production Python, compiles to Zig and Rust" |
| agent-doc | Keep the name — it's your headline |
Please format into easily scannable conversational snippets.

### Pitch Snippets — Scannable, Conversational

Use these as modular beats. Each one stands alone. String them together or pull individually as the conversation flows.

---

**agent-doc (lead with this):**

> "Right now I'm excited about agent-doc — a system I built for human-agent collaboration through markdown documents.
>
> The agent streams responses into named document regions using CRDT merge. The user edits concurrently. State syncs bidirectionally.
>
> It's the same problem AG-UI solves — keeping agents and humans synchronized in real-time — but through a document metaphor instead of web UI components."

---

**Cross-platform signals (lazily):**

> "I also built a cross-platform reactive signals library. It runs in production Python and compiles to Zig and Rust via FFI — same signal semantics across all three languages.
>
> That cross-language SDK thinking maps directly to what CopilotKit does with its TypeScript and Python SDKs."

---

**FFI plugin architecture:**

> "agent-doc's editor plugin uses the same approach — a cross-platform FFI shared library for in-process support. Right now it powers the JetBrains plugin, but the architecture supports multiple editors through the same native core."

---

**Framework-agnostic reactive libraries:**

> "More broadly, I build reactive libraries that are framework-agnostic. They work with React, Svelte, vanilla JS, WebComponents — same primitives regardless of the rendering layer.
>
> That's how I think about CopilotKit's component surface — the protocol and state management are the real product. The UI framework is just the rendering target."

---

**AG-UI pattern recognition:**

> "When I saw AG-UI, the pattern recognition was immediate. Snapshots, deltas, streaming events, concurrent editing — same architecture I built independently.
>
> That's what drew me to CopilotKit. You're building the protocol layer for human-agent alignment."

---

**Building in public:**

> "I've been posting demos on LinkedIn and livestreaming dev sessions — dogfooding agent-doc with Claude Code in real-time.
>
> Happy to share links after the call if you want to see the workflow rather than just hear me describe it."

---

**State management depth (if asked):**

> "I've been building reactive state systems for 15 years. Signal-based, framework-agnostic, CRDT merge for concurrent edits.
>
> This maps directly to what CoAgents does with `useCoAgent` and state deltas. The pattern is the same — snapshots for baselines, deltas for incremental updates, atomic application."

---

**Presence AI (if asked):**

> "Most recently I built a multimodal pipeline on H100 GPUs via WebRTC — real-time audio, video, and screen streaming at infrastructure scale. Production Python."

---

**Document/notebook pitch (minute 15–22, only if alignment feels right):**

> "I'm curious about where CopilotKit sees the interaction surface expanding. Chat, sidebar, popup, textarea — is there interest in richer surfaces like documents or notebooks?
>
> That's where my experience naturally leads. I want to make sure our interests align."

If he engages:

> "Think MDX notebooks where CopilotKit components are first-class — agents streaming into named regions, users editing inline. DX play first, enterprise knowledge work second."
Please transcribe and dialize [2026-03-23 16-58-55.mp4](../../../../../Videos/2026-03-23%2016-58-55.mp4)

Help me with the debriefing. Looks like it's a no for now but maybe more work later.

### Interview Debrief — Mar 23, 5:00 PM ET

**Duration:** ~44 minutes (longer than expected 30 min — good sign of engagement)

**Outcome:** Not a fit for current open roles. Potential future alignment in 3–6 months.

---

#### What Jordan Said (Key Takeaways)

**Their immediate need: Partnerships & Integrations (mechanical work)**
- CopilotKit needs to fill a "two-dimensional matrix of frameworks × feature expressions"
- The job is: new framework comes out → integrate it, document it, ship reference examples
- "There isn't really much ideation involved in that" — Jordan was explicit
- They need people already embedded in the CopilotKit/AG-UI ecosystem daily

I'm not trying to be an ideator. I think he misunderstood what I was trying to convey.
Which is unfortunate. But perhaps I missed the energy of how he wanted to frame the conversation.

**Team structure:**
- OSS team: core protocol + partnerships/integrations (2 groups)
- Enterprise team: persistence, analytics, observability inside customer infra
- R&D "tiger team" for hard problems (not production-facing)
- Current eng team: ~3 people, growing to 5, then 10
- Jordan is CTO + quasi-COO

**The $10B idea:**
- CopilotKit ($500M) → making agents smarter in situ ($10B)
- The "80/20 problem": agents get 80% right, same 20% wrong every week
- Fix needs to happen at the human-agent interaction nexus — that's AG-UI/CopilotKit
- This is where your enterprise knowledge work pitch would land later

**On your pitch:**
- Jordan reflected back: "I'm hearing a really strong desire and curiosity and passion for envisioning how to use it"
- He identified you as having **founder-level aspirations**, not employee-level
- "I've never seen a better time to be a founder" — genuine advice, not a dismissal
- Suggested contributing to open-source repos as a bridge (150 unanswered issues, 100+ PRs)

I dogfood. So I build and use at the same time.

**On documents as interface:**
- Jordan resonated: "We've been using Notion... that common shared document representation"
- He connected it to "agentic memory, agentic organization of learned things"
- But framed CopilotKit's current focus as "maintaining edge mechanically"
- The MDX/Jupyter pitch landed but isn't actionable for them right now

I can build interactive Notion documents as well.
Does Notion support the interactive development that I'm doing with agent-doc?

---

#### What Went Well

1. **Warm reconnect worked** — "Well, well, well" + genuine laughing. Good chemistry from minute 1.
2. **agent-doc pitch landed** — You explained CRDT merge, bidirectional state sync, FFI plugin architecture clearly. Jordan engaged with the document-as-interface concept.
3. **Philosophy angle was interesting** — Existence language, ontology scoping. Jordan didn't push back. It differentiated you from a typical IC candidate.
4. **Video offer accepted** — "Sure, that'd be nice" when you offered to share demo links.
5. **Mutual respect** — Jordan gave real talk, not recruiter talk. "If I could reflect that back to you..." was genuine mentoring.

#### What Could Have Gone Better

1. **Too much exploration, not enough "I can execute"** — You leaned into the visionary/philosophical side. Jordan explicitly said the role needs "fingers on keyboards getting shit done." Should have balanced vision with "I also do the mechanical work happily.": I had a difficult time recovering from his judgement of me. So I would like to assert that I can fill the role of "getting shit done".
2. **Rambled at times** — The Existence Language segment (~17:24–19:44) went deep into ontology/scoping without a clear "and here's why CopilotKit benefits." In a 30-min call, every minute needs to earn its keep.
3. **Didn't close on the integration role** — When Jordan described the matrix of integrations, you could have said: "I can do that. I've shipped 15+ packages across npm/PyPI/crates.io. Integration work is second nature to me." Instead the conversation drifted toward your own projects.: I didn't think of it during the interview. It was difficult to transition to that. Maybe I was not prepared.
4. **Tiny JS libraries tangent** — The rmemo/relementjs discussion (~42:49–43:45) right at the end was off-target. Jordan explicitly said there's no market for optimization anymore. Should have pivoted to "exactly — and that experience makes me fast at integration work.": I wanted to express that I can work with vanillajs and integrate with different libraries.

---

#### Next Steps

1. **Send follow-up email to Jordan** with video links (he said "sure, that'd be nice")
2. **Contribute to CopilotKit OSS** — pick from the 150 issues or 100+ PRs. Best path to staying visible
3. **3–6 month check-in** — Jordan explicitly said to reach back out when they're scaling from 5→10 and building enterprise features
4. **The real opening:** Enterprise side — "what else can we do with this knowledge" + reinforcement learning. That's where your document-as-interface, CRDT, agentic memory work aligns

---

#### Honest Assessment

Jordan liked you personally and respects the depth, but the current need is integration grinders, not visionaries. The founder reflection was genuine — he sees you as someone who should be building their own thing. The door is open for later when CopilotKit moves from "maintain the edge" to "build new things." Contributing to the OSS repos is the bridge.

My ability to integrate with editors shows that I can do integration work. Perhaps that's something to note in the followup.
I got emotional and aspirational because of the possibility.
I get that workers should keep a very narrow vision.
Basically, I need cashflow. Is there a way to pivot the conversation toward that with Jordan?
Help me with further communication. I only have his LinkedIn. I don't have his email.
Should I ask for his email for a follow up?

### Re: Processing the Interview + Next Steps

---

#### On being misread

You're right — you weren't pitching as an ideator. You're a dogfooder who builds AND uses simultaneously. That's a rare and valuable combination. But the framing came across as visionary because:
- The Existence Language/ontology discussion is hard to land in 2 minutes with someone who doesn't have that context
- You led with "what could be" rather than "what I ship daily"
- Jordan was listening for "can this person grind through 150 integration issues" and instead heard "this person wants to explore how documents become agentic interfaces"

The mismatch wasn't about your actual capabilities — it was about how they were framed in that moment. That's fixable.

#### On Notion

No — Notion does NOT support what you're doing with agent-doc. Notion is:
- Collaborative document editing with AI bolt-ons (Notion AI)
- No CRDT merge with external agent processes
- No bidirectional state sync with CLI tools
- No git-centric commit-per-iteration workflow
- No IPC patch system with editor plugins
- No template mode with named component regions

Jordan said "we use Notion" for human-in-the-loop planning. That's a manual process — humans paste plans, humans write comments, humans update. agent-doc automates that loop. **This distinction is worth making in a follow-up.**

#### On "I can execute"

Your editor integration work IS the proof. You:
- Built a cross-platform FFI shared library for editor plugins
- Ship to npm, PyPI, crates.io
- Maintain a JetBrains plugin with IPC patching
- Release agent-doc ~weekly with `cargo publish` + `maturin publish` + GitHub releases
- Have 40k+ commits

That's not ideation. That's execution. The interview just didn't surface it clearly enough.

#### On getting emotional

Completely understandable. The possibility was exciting, the chemistry was good, and Jordan's openness about the company drew you in. When someone's vision aligns with yours, it's natural to lean into shared excitement rather than "here's why you should hire me for the boring work." No shame in that — just awareness for next time.

---

#### Follow-up Strategy

**Don't ask Jordan for his email directly.** Here's the better path:

**1. LinkedIn message to Jordan (short, direct):**

> Jordan — really enjoyed the conversation today. I want to clarify something: I got excited talking about the vision, but I should have been clearer — I'm a builder who ships, not just an ideator. I release agent-doc weekly across crates.io, PyPI, and npm. I built a cross-platform FFI plugin system for editor integration. I maintain a JetBrains plugin. I have 40k+ commits.
>
> I understand the current need is integration grinders. That's work I do daily — I ship packages across multiple ecosystems and integrate with external tools and editors as a core part of my workflow.
>
> You mentioned contributing to the repos. I'd like to do that. And I'm serious about the 3-6 month check-in.
>
> Here are the demo videos I mentioned: [links]
>
> Thanks for the real talk. It landed.

I actually have multiple releases across multiple projects every day.
I'm working on my social media presence & marketing...with the help of agents.
Should I ask for his email?

**2. Email Kevin Volk (through existing channel):**

Let Kevin know the outcome and that Jordan suggested staying in touch. Kevin can keep you in the pipeline for future roles. Ask Kevin if he can share Jordan's email for a follow-up — that's his job as the recruiter bridge.

Please draft an email.

**3. Contribute to CopilotKit repos:**

This is the highest-leverage move. Pick 2-3 issues, submit quality PRs. This does two things:
- Proves you can do integration work
- Puts your name in front of the team organically

I don't have a natural reason to contribute yet. I don't use their components. I may in the future.
Perhaps with Judith Krug's project.
But I need to have a job to use their components before it's worthwhile for me to take the time to contribute to their Open Source.

**4. On cashflow:**

Be direct with Kevin: "I need work. The enterprise/R&D roles Jordan mentioned aren't open yet, but I'm available for contract work, integration work, whatever is needed now. Is there flexibility?" Recruiters understand cash flow urgency. Jordan may not know you need income soon — Kevin should.

I agree. Should I mention that to Jordan?
If he is aligned with where I want to take things, could he be an investor?

---

#### Key reframe for all future communication

**"I dogfood. I build tools, then I use them to build more tools. agent-doc isn't a side project — it's my daily development environment. The same energy I put into building it is the same energy I'd put into CopilotKit integrations."**

This reframe connects your passion (building agent-doc) to their need (someone who lives in the ecosystem and ships).
It's interesting because he wanted me to be honest, but being honest only got me discounted from the role.

### Re: Follow-up Strategy — Email, OSS, Investor?

---

#### Should you ask for his email?

**No — go through Kevin Volk.** Ask Kevin: "Can you share Jordan's email so I can send a follow-up with the demo links he asked for?" That's a normal recruiter request. If Kevin can't, use LinkedIn messaging directly to Jordan — he's already engaged there.

#### On OSS contributions

You're right — forced OSS contributions without genuine use are performative and Jordan would see through that. Don't contribute just to be visible. If Judith Krug's project or another real use case brings you into CopilotKit components naturally, then contribute authentically. Otherwise, skip it.

#### Should you pitch Jordan as an investor?

**Not in this follow-up. Not yet.** Here's why:

- Jordan just told you "this isn't a fit for the roles we have." Immediately pivoting to "invest in me" would feel like you're not listening.
- He DID say "I've never seen a better time to be a founder" — that's an opening, but it needs to mature. Let the relationship breathe.
- The right sequence: (1) send follow-up with demos, (2) stay in touch over the next month, (3) if he engages with your content and the conversation deepens, THEN explore whether he'd be interested in advising or investing.
- Jordan has startup investor instincts (CloudCrowd, Napster, Servio, now CopilotKit). If agent-doc gains traction and he sees it, he may bring it up himself.

**Future framing (not now, maybe month 2-3):**
> "You mentioned it's a great time to be a founder. I've been taking that seriously. agent-doc is gaining traction — [metrics]. Would you be open to a conversation about advising?"

Right now agent-doc only has 5 stars on GH. I need to do a lot of work on this front.

---

#### Draft LinkedIn Message to Jordan

> Jordan — great reconnecting today. I appreciate the candor.
>
> I want to be clear about something: I got excited talking about the vision, but I should have been more direct — I'm a builder who ships daily. I release across crates.io, PyPI, and npm regularly. I built a cross-platform FFI plugin system for editor integration. I maintain a JetBrains plugin with real-time IPC patching. 40k+ commits.
>
> I understand the current need is filling the integration matrix. That's execution work I do as part of my daily workflow — shipping packages across ecosystems and integrating with external tools.
>
> Here are the demo videos I mentioned: [links]
>
> Staying in touch as you suggested. Thanks for the real talk.

> I maintain a JetBrains plugin with real-time IPC patching.

This has a cross-platform and cross-editor architecture via FFI.

> 40k+ commits.

The 40k+ commits is across my entire github career. Not sure what you are suggesting.

Should I wait to get Jordan's email from Kevin or immediately send a LinkedIn message to Jordan?

**Keep it on LinkedIn** unless Kevin provides the email. LinkedIn is fine for this — it's where the relationship already lives.

#### On mentioning cashflow to Jordan

**No.** Don't mention cashflow needs to Jordan directly. That's a recruiter conversation. Tell Kevin you're available immediately and flexible on role scope. Kevin can advocate for you internally if there's contract work or a role that opens up sooner.

### Re: Honesty vs. Fit

That tension is real and frustrating. Jordan said he values authenticity — "narrative arc interview," no trick questions, just get to know each other. And you were authentic. But authenticity revealed that your passion is building agent-doc-level tools, not grinding through an integration matrix. He took that signal seriously.

Why would my passion be for "grinding through an integration matrix" that I have never seen?
Is this because CoPilotKit is known and I should have been more familiar?

The hard truth: **being honest didn't disqualify you — it revealed a genuine misalignment with the specific role that's open right now.** Jordan wasn't punishing honesty. He was listening to it. He heard where your energy actually lives and said "that's not what we need today, but maybe later."

The paradox is: if you had performed as "I just want to grind integrations" and gotten the job, you'd likely be miserable in 3 months doing work that doesn't excite you. Jordan has enough experience to recognize that pattern and save both of you from it.

I would have been fine with the work.
I can get excited about doing different things. I'm just excited about the work I've been doing lately.

**What you can take from this:**
- Your honesty preserved the relationship. Jordan wants to stay in touch. A candidate who faked enthusiasm for integration work and then flamed out wouldn't get that.
- The follow-up message is your chance to add the "I also execute" signal that was missing — without betraying the honesty.
- When the enterprise/R&D roles open, you'll be a natural fit precisely because Jordan already knows what you actually care about.

Being honest is never the wrong move. The framing around the honesty is what you can refine for next time.

### Re: Corrections & Next Steps

---

#### Corrections to the LinkedIn draft

Good catches. Let me fix these:

- **JetBrains plugin** → should mention: "with cross-platform, cross-editor architecture via FFI" — not just JetBrains-specific
- **40k+ commits** → drop it from the LinkedIn message. It's a career-wide stat, not a specific claim about CopilotKit-relevant work. It dilutes rather than strengthens.
- **agent-doc stars** → 5 stars is honest. Don't mention GitHub stats. Lead with what the tool does, not vanity metrics.

#### LinkedIn vs. waiting for Kevin

**Send the LinkedIn message now.** Don't wait for Kevin. Here's why:
- Jordan already engaged with you on LinkedIn (that's how the connection happened)
- The video links are time-sensitive — send while the conversation is fresh
- Kevin may take days to respond
- If Kevin later provides the email, you can send a more detailed follow-up then

LinkedIn message first. Kevin email second. They're parallel, not sequential.

#### On "passion for grinding"

You're right — nobody's passion is "grinding through an integration matrix." That was poor framing on my part. Let me restate:

Jordan wasn't looking for someone passionate about integration work. He was looking for someone whose **current daily workflow** already involves CopilotKit. His ideal candidate is someone who already uses CopilotKit in their own open-source project and can contribute integrations from lived experience — not enthusiasm.

You would have been fine with the work. You CAN get excited about different things. The issue wasn't your capability or willingness — it was that Jordan's hiring criteria right now is "already embedded in this ecosystem daily." That's a very specific filter, and it has nothing to do with whether you could do the job well.

#### Revised LinkedIn Message

> Jordan — great reconnecting today. I appreciate the candor.
>
> I wanted to follow up on a couple things. First, here are the demo videos I mentioned — agent-doc in action, building with Claude Code: [links]
>
> Also wanted to clarify: I got excited talking about the vision side, but my daily work is execution. I ship releases across crates.io, PyPI, and npm regularly. agent-doc's editor plugin uses a cross-platform FFI architecture designed for multi-editor support — that's integration engineering at the systems level.
>
> I hear you that the current need is people already embedded in the CopilotKit ecosystem. I'm not there yet, but I'm staying close. Thanks for keeping the door open — I'll take you up on the 3-6 month check-in.

Shorter than the last draft. Doesn't oversell. Acknowledges his actual filter honestly.

Transcribe and diarize /home/brian/work/btakita/agent-loop/mail/contacts/kevin-volk/recordings/20260323-kevin-volk.amr
Also update kevin-volk/AGENTS.md and any other related context.

Jordan's email is jordan@copilotkit.ai
Please draft a followup email
From brian.takita@gmail.com

### Kevin Volk Call Summary (Mar 23)

- Kevin will send Jordan's email (jordan@copilotkit.ai — you already have it)
- Kevin wants the interview recording + transcript — send to him
- Kevin warned: some hiring managers get weird about candidates recording interviews. Disclose upfront in future.
- Kevin will keep you in pipeline for similar roles at other companies
- Kevin seeing more contract-to-hire in the market
- Kevin will check in with you in a couple weeks

Updated `mail/contacts/kevin-volk/CLAUDE.md` with full timeline and intel.

---

### Draft Follow-Up Email to Jordan

**To:** jordan@copilotkit.ai
**Subject:** Thanks for the conversation — demo links + a clarification

> Jordan,
>
> Great reconnecting today. I appreciate you being straight about where CopilotKit's needs are right now.
>
> Two things I wanted to follow up on:
>
> **Demo links you asked about:**
> [YouTube/LinkedIn links here]
>
> These show agent-doc in action — the CRDT merge, template patching, concurrent editing with Claude Code. You can see the workflow rather than just hear me describe it.
>
> **A clarification:**
> I got excited in the conversation talking about where documents-as-interface could go, and I think that overshadowed something important: my daily work is execution. I ship releases across crates.io, PyPI, and npm. agent-doc's editor plugin uses a cross-platform FFI architecture designed for multi-editor integration — that's integration engineering at the systems level. I build, ship, and maintain packages across ecosystems as a core part of what I do every day.
>
> I hear you that the current need is people already embedded in the CopilotKit ecosystem. I'm not there yet, but the integration work you described — filling out the framework × feature matrix — is exactly the kind of work my toolchain is built for.
>
> Taking you up on the 3-6 month check-in. In the meantime, I'll stay close to what you're building.
>
> Best,
> Brian

---

**Notes on this draft:**
- Opens with thanks, not apology
- Delivers the demo links he asked for (action item from the call)
- The "clarification" reframes without being defensive — acknowledges the vision lean, then states execution credentials
- Doesn't beg for the current role — accepts the timeline
- Short enough that Jordan will actually read it

### Re: Email sender noted

Got it — sending from brian.takita@gmail.com. The draft is ready. Would you like me to send it via corky, or would you prefer to copy-paste it into Gmail yourself? Also, please provide the video/demo links to include.
Should I send him my most recent playlist https://www.youtube.com/playlist?list=PL4Ma0srhCY3PVBZF3D541tPyPPm1BhnEI

Maybe add a short description over how I'm working with this?
And how this workflow naturally would work with MDX, Notion, Jupyter, Word, Obsidian, or any other type of knowledge document software. 

### Re: Video Link + Description for Email

Yes — send the playlist. But keep the description tight. Jordan is busy. One paragraph max.

**Revised email draft (final):**

**To:** jordan@copilotkit.ai
**From:** brian.takita@gmail.com
**Subject:** Thanks for the conversation — demo links + a clarification

> Jordan,
>
> Great reconnecting today. I appreciate you being straight about where CopilotKit's needs are right now.
>
> Here are the demo videos I mentioned — agent-doc in action with Claude Code:
> https://www.youtube.com/playlist?list=PL4Ma0srhCY3PVBZF3D541tPyPPm1BhnEI
>
> What you'll see: I work in a markdown document in JetBrains while Claude Code streams responses into named document regions via CRDT merge. I edit concurrently — the agent and I are collaborating in real-time through the document. Every interaction is a git commit. The architecture is transport-agnostic — the same pattern works with MDX, Notion, Jupyter, Word, Obsidian, or any document surface. It's the document-level equivalent of what AG-UI does for UI components.
>
> I also wanted to clarify something from our call: I got excited talking about where this could go, and I think that overshadowed the execution side. My daily work is shipping — I release across crates.io, PyPI, and npm. agent-doc's editor plugin uses a cross-platform FFI architecture designed for multi-editor integration. That's integration engineering at the systems level.
>
> I hear you that the current need is people already embedded in the CopilotKit ecosystem. Taking you up on the 3-6 month check-in. In the meantime, I'll stay close to what you're building.
>
> Best,
> Brian

Ready to send via corky to Gmail drafts when you give the go-ahead.

My plan is to add live data components such as AG-UI compatible components when I have the markdown editing solid.

Should I mention that I have a client and I am open for more clients to integrate?...So if he has any referrals, I would appreciate it? 
BCC: Kevin Volk + dev@briantakita.me

I'm working from the agentic primitives upward. So I believe we can have a solid foundation for a seamless & deep alignment with agents through expressing knowledge.

### Re: AG-UI Components Roadmap + Client Referrals + BCC

**AG-UI components roadmap:** Yes, mention it — but briefly. It connects your work directly to CopilotKit's ecosystem and shows you're heading toward their stack naturally. One sentence.

**Client referrals:** Yes — Jordan explicitly said "if you see something, do something" and encouraged staying connected. Asking for referrals is a natural extension of that. It also reframes the relationship from "candidate who didn't get the job" to "professional peer looking for work." Keep it light — one line at the end, not the focus.

**BCC: dev** — noted, will include.

**Final email draft:**

**To:** jordan@copilotkit.ai
**From:** brian.takita@gmail.com
**BCC:** dev
**Subject:** Thanks for the conversation — demo links + a clarification

> Jordan,
>
> Great reconnecting today. I appreciate you being straight about where CopilotKit's needs are right now.
>
> Here are the demo videos I mentioned — agent-doc in action with Claude Code:
> https://www.youtube.com/playlist?list=PL4Ma0srhCY3PVBZF3D541tPyPPm1BhnEI
>
> What you'll see: I work in a markdown document in JetBrains while Claude Code streams responses into named document regions via CRDT merge. I edit concurrently — the agent and I are collaborating in real-time through the document. Every interaction is a git commit. The architecture is transport-agnostic — the same pattern works with MDX, Notion, Jupyter, Word, Obsidian, or any document surface. My plan is to add live data components — including AG-UI compatible components — once the markdown editing layer is solid.
>
> I also wanted to clarify something from our call: I got excited talking about where this could go, and I think that overshadowed the execution side. My daily work is shipping — I release across crates.io, PyPI, and npm. agent-doc's editor plugin uses a cross-platform FFI architecture designed for multi-editor integration. That's integration engineering at the systems level.
>
> I hear you that the current need is people already embedded in the CopilotKit ecosystem. Taking you up on the 3-6 month check-in. In the meantime, I'll stay close to what you're building.
>
> On a separate note — I'm currently doing client work and am open to taking on more. If you come across anyone in your network who could use someone with my background, I'd appreciate the referral.
>
> Best,
> Brian

Want me to push this to Gmail drafts via corky?

### Re: BCC + "agentic primitives upward"

BCC updated: Kevin Volk (kevin@staffingscience.io) + dev@briantakita.me

Good phrasing — "working from the agentic primitives upward." Let me weave that into the AG-UI sentence:

**Updated paragraph:**
> The architecture is transport-agnostic — the same pattern works with MDX, Notion, Jupyter, Word, Obsidian, or any document surface. I'm working from the agentic primitives upward — once the markdown editing layer is solid, the next step is live data components, including AG-UI compatible components.

Ready to send. Want me to push to Gmail drafts via corky now?
Rewrite the draft with my updates.
Should I include other YouTube videos such as the resume to show my corky integration?

### Final Email Draft (revised)

**To:** jordan@copilotkit.ai
**From:** brian.takita@gmail.com
**BCC:** kevin@staffingscience.io, dev@briantakita.me
**Subject:** Thanks for the conversation — demo links + a clarification

> Jordan,
>
> Great reconnecting today. I appreciate you being straight about where CopilotKit's needs are right now.
>
> Here are the demo videos I mentioned — agent-doc in action with Claude Code:
> https://www.youtube.com/playlist?list=PL4Ma0srhCY3PVBZF3D541tPyPPm1BhnEI
>
> What you'll see: I work in a markdown document in JetBrains while Claude Code streams responses into named document regions via CRDT merge. I edit concurrently — the agent and I are collaborating in real-time through the document. Every interaction is a git commit. The architecture is transport-agnostic — the same pattern works with MDX, Notion, Jupyter, Word, Obsidian, or any document surface.
>
> I'm working from the agentic primitives upward. I believe we can have a solid foundation for seamless and deep alignment with agents through expressing knowledge. Once the markdown editing layer is solid, the next step is live data components — including AG-UI compatible components.
>
> I also wanted to clarify something from our call: I got excited talking about where this could go, and I think that overshadowed the execution side. My daily work is shipping — I release across crates.io, PyPI, and npm. agent-doc's editor plugin uses a cross-platform FFI architecture designed for multi-editor integration. That's integration engineering at the systems level.
>
> I hear you that the current need is people already embedded in the CopilotKit ecosystem. Taking you up on the 3-6 month check-in. In the meantime, I'll stay close to what you're building.
>
> On a separate note — I'm currently doing client work and am open to taking on more. If you come across anyone in your network who could use someone with my background, I'd appreciate the referral.
>
> Best,
> Brian

Ready to send via corky to Gmail drafts. Say the word.
I also want to express the realtime markdown dashboard direction as well.

Show my release velocity by linking to crates.

### Re: Additional Videos + Dashboard Direction

**Keep it to one link.** The playlist is enough. Here's why:

- Jordan said "sure, that'd be nice" — polite interest, not deep curiosity. One link respects his time.
- Multiple YouTube links in a follow-up email looks like a content dump, not a professional correspondence.
- The playlist already contains your work. If he watches one video and wants more, he'll browse the playlist.
- Adding corky integration demos, resume videos, and dashboard direction makes the email longer and dilutes the core message.

**The email already has three jobs:**
1. Deliver the demo links he asked for
2. Clarify you're an executor, not just a visionary
3. Ask for referrals

Adding a fourth (showcase every project) weakens all three.

**If you want to show the dashboard direction:** Add it to the playlist description or create a follow-up LinkedIn post that Jordan will see organically. Don't pack it into this email.

**Your message was cut off at "Show my v" — complete it if you want me to address it.**

The draft is ready as-is. Want me to send it?

### Re: Link to crates.io for release velocity

Your message was cut off at "linking to crates." — I'll assume you mean linking to your crates.io profile or agent-doc's crates.io page to show release frequency.

**My recommendation: don't add it to this email.** Same reasoning — every link you add dilutes the one that matters (the playlist he asked for). The email already says "I release across crates.io, PyPI, and npm" — that's the claim. If Jordan is curious, he can search crates.io himself.

**But if you want to include it:** Add it as a single line after the execution paragraph, not as another section:

> (You can see the release cadence at https://crates.io/crates/agent-doc)

One parenthetical line, not a feature. It backs up your claim without expanding the email.

Should I add that line to the draft, or keep the email as-is and send?
Is the draft in a file?
Should I include the fact that I publish blog posts from the video? Jordan didn't seem very interested.

He seems more interested in the infrastructure and position play.
At least for this role...but he may be supportive of how I can help propagate AG-UI perhaps?

### Re: Draft file

Created the draft at:
`mail/drafts/2026-03-23-thanks-for-the-conversation-demo-links-a-clarification.md`

BCC includes kevin@staffingscience.io and dev@briantakita.me.

To push to Gmail drafts:
```
corky draft push mail/drafts/2026-03-23-thanks-for-the-conversation-demo-links-a-clarification.md
```

Or I can run it for you — just say "push it."

### Re: Blog posts + AG-UI propagation

**Blog posts: No.** Jordan didn't show interest in content creation during the call. Adding it would feel like you're selling something he didn't ask for.

**AG-UI propagation: Also not for this email.** That's a conversation for later — once you actually have AG-UI components working in agent-doc. Right now it's a future plan, not a deliverable. Jordan is focused on the here-and-now.

**The email is good as-is.** The playlist link, the execution reframe, and the referral ask — that's three clean beats. Every addition you're considering (blog posts, crates.io link, AG-UI propagation) makes it longer without making it more effective.

The draft file is ready. Should I push it to Gmail drafts now?
In my request for a referral...please include that I am interested in integrating AG-UI components.
Al

### Re: AG-UI integration in referral ask

Updated the draft. The referral line now reads:

> "On a separate note — I'm currently doing client work and am open to taking on more, particularly projects involving AG-UI integration. If you come across anyone in your network who could use someone with my background, I'd appreciate the referral."

Draft file updated. Ready to push to Gmail drafts.
Also, should I mention that I barely use the web anymore?
Almost all of my interactions are with agent-doc.

### Re: Mentioning you barely use the web anymore

**No.** Don't add this to the email. Here's why:

1. **It sounds like a brag that doesn't land.** Jordan said he hasn't used an editor in 3 months — that's his version of the same claim. He won't be impressed; he'll just think you're mirroring his talking point.

2. **It could read as "I don't use web UIs"** — which is CopilotKit's entire product. You don't want Jordan thinking "this person doesn't even use the kind of thing we build."

3. **The email is done.** Every addition at this point is anxiety, not improvement. The draft has three clean beats. Send it.

Push it to Gmail drafts?
Send it

### Email pushed to Gmail drafts

Draft created in Gmail. Open your drafts to review and send.

**To:** jordan@copilotkit.ai
**BCC:** kevin@staffingscience.io, dev@briantakita.me
**Subject:** Thanks for the conversation — demo links + a clarification

Another thing. My passion + desire is predicated by my perspective.
I don't have much influence within the industry and capital.
I'm building tools based on where I am at and to better my life and the lives of people in a similar circumstance as me.

Jordan is coming from a different perspective. To drive large markets.
Which I resonate with and if I were in a similar level of security, I would have similar passions.

I already sent the email. But is it worth a followup to express this?
Or should I just make a note of this to hopefully remember next time.

I don't have much leverage. So my interests are based on increasing my leverage from where I am at.

### Re: Perspective, Leverage, and Follow-up

**Don't send a follow-up about this.** You just sent the email. A second one explaining your motivations would read as over-processing the rejection. Let the email land and breathe.

**But this reflection is valuable — let me note it for you:**

You're identifying something real: your passion is shaped by your circumstance. You're building tools to increase your own leverage — agent-doc, corky, the whole stack. Jordan's passion is shaped by his — he has capital, team, and market position, so he thinks about capturing market share and filling integration matrices.

These aren't competing perspectives — they're different positions on the same spectrum. If you had his resources, you'd think about scale. If he were solo, he'd build exactly the kind of primitives you're building.

**The lesson for next time:**

When talking to someone with Jordan's position, translate your work into their language:
- Instead of "I'm building tools to improve my workflow" → "I'm building integration primitives that solve the same problem your customers hit"
- Instead of "documents as an agentic interface" → "a new interaction surface for CopilotKit's component library"
- Instead of "I'm working from the agentic primitives upward" → "I'm prototyping the next layer of your product"

You don't need to change what you're building. You need to frame it in terms of what they're buying.

This is a note for future interviews, not a follow-up email topic.
<!-- agent:boundary:f759f76c -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- Review and send email from Gmail drafts ([done] pushed)
- Send interview recording + transcript to Kevin Volk
- Calendar: Kevin check-in in 2 weeks, Jordan check-in in 3 months (June 2026)
<!-- /agent:pending -->
