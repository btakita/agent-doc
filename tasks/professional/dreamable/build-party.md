---
agent_doc_session: fcc31f91-afed-4612-b5bc-70257fae21dc
agent: claude
---

# Session: Build Party

## User


<!-- agent:exchange -->
*Transfer from tasks/recruit/robert-ross.md*

## Build Party — LiveKit Repo Evaluation (5 repos)

| Repo | Stars | Category | Build Party Relevance |
|------|-------|----------|----------------------|
| [livekit-examples/livestream](https://github.com/livekit-examples/livestream) | 246 | Streaming starter | **High** — Watch Party shell, stage-summon, OBS ingress |
| [livekit/agents](https://github.com/livekit/agents) | 9,935 | Agent framework | **Very High** — Nova/Echo/Flare personas |
| [python-agents-examples](https://github.com/livekit-examples/python-agents-examples) | 262 | Agent examples | **High** — 70+ examples: content filter, RAG, multi-agent |
| [dev-day-demos](https://github.com/livekit-examples/dev-day-demos) | 9 | Multi-agent demos | **High** — Silent moderation agent = Echo blueprint |
| [AOSSIE-Org/Resonate](https://github.com/AOSSIE-Org/Resonate) | 337 | Discord alternative | **Medium** — Room browsing, scheduling, role-based mod |

### Strategic synthesis

1. **Start with `livestream`** — Watch Party shell with stage-summon UX, OBS ingress, sub-100ms WebRTC
2. **Wire `agents` on top** — three agents in one room:
   - **Nova** = RAG voice agent + LLM content filter + Q&A queue
   - **Echo** = silent observer emitting `highlight.detected` RPCs
   - **Flare** = Echo event consumer + LiveKit Egress API clip trigger
3. **Community layer** from Resonate — room browsing, scheduling, role-based permissions

**Greenfield gap:** Co-build rooms (shared code editor + LiveKit room) are genuinely new.

Look at tasks/dreamable/robert-ross.md and tasks/dreamable/robert-ross-granola.md
Investigate these repos in more detail. To see what is possible in for Dreamable features.
Should we build the livekit apps + libraries in rust or python?
We may be able to share libraries with Flutter using FFI for mobile + deskto apps.
WASM or a js port for browser logic. 
### Re: Deep repo investigation for Dreamable features

Investigated all 5 repos. Here's the detailed breakdown:

---

#### 1. livekit-examples/livestream (246 stars) — Reference Only

**Tech:** Next.js 14, TypeScript, `livekit-client` v1.x, `@livekit/components-react`. Vercel-deployable monolith.

**What it has:** WebRTC streaming, OBS ingress (WHIP protocol), stage management (X Spaces style), browser broadcast, text chat, emoji reactions.

**What it's missing:** No auth, no database, no multi-room, no agent integration, no recording, no moderation tools, no courseware. Dependencies are stale (pinned to v1.x when v2 is current). Effectively dormant — last meaningful commit April 2024.

**Verdict:** Borrow UX patterns (stage-summon, OBS ingress flow) but don't fork. Build Party should use LiveKit SDKs v2 directly.

---

#### 2. livekit/agents (9,935 stars) — Core Infrastructure ✓

**Latest:** v1.5.1 (2026-03-23). Python-first, 65+ plugins.

**Architecture:** `Agent` (instructions + tools) → `AgentSession` (manages STT/LLM/TTS pipeline) → `AgentServer` (dispatches jobs). Agents join rooms as server-side participants.

**Key v1.5 features:**
- Adaptive interruption handling (ML-based, 86% precision)
- Dynamic endpointing (adapts silence thresholds)
- Session usage tracking (per-model token/cost)
- MCPToolset support
- Action-aware context summarization

**Multi-agent:** Two patterns — (a) sequential handoff within one session (agent returns next agent from a tool call), (b) multiple independent AgentSessions in the same room via dispatch API.

**STT providers:** Deepgram, AssemblyAI, Google Chirp 3, Whisper, Soniox, Gladia, Speechmatics. TTS: Cartesia, ElevenLabs, Google, OpenAI, xAI Grok.

**RPC + Data Channels:** Built-in. `room.local_participant.perform_rpc()` and data channel publishing for custom events.

**Fit for Build Party personas:**

| Persona | Status | Notes |
|---|---|---|
| **Nova** (RAG voice + Q&A) | Ready | VoicePipelineAgent + function tools for RAG. Q&A queue is custom logic on top. |
| **Echo** (silent observer) | Feasible | Create minimal AgentSession with STT only (no LLM/TTS). Process transcripts in callbacks. Emit RPCs. |
| **Flare** (clip trigger) | Feasible | Subscribe to Echo's RPCs via data channels. Call Egress API (separate service, use `livekit-api` SDK). |

**Gap:** No built-in "passive listener" agent archetype, no first-class content filtering (use guardrails wrapper), Egress is a separate service not part of agents framework.

---

#### 3. python-agents-examples (262 stars) — Pattern Library ✓

**Scale:** 70+ examples in two tiers — 50+ single-concept demos + 20+ production-style apps with frontends.

**Directly relevant examples:**
- `simple_content_filter` + `llm_powered_content_filter` — **Nova's moderation layer**
- `agent_transfer`, `medical_office_triage`, `personal_shopper` — **Multi-agent coordination patterns**
- `doheny-surf-desk` — **Background observer agent** (silent monitoring, exactly Echo's pattern)
- `rpc_agent` — **CRUD state management via RPC** (Flare's event handling)
- `label_messages`, `state_tracking`, `conversation_monitoring` — **Non-speaking event listeners**

**Missing:** No Egress/clipping examples, no dedicated RAG example (wire into tool_calling pattern yourself).

**Quality:** Well-structured with YAML frontmatter, difficulty ratings, `console` mode for quick testing. Solid starting points, not production-ready.

---

#### 4. dev-day-demos (9 stars) — Echo Blueprint ✓✓

**This is the most directly useful repo for Build Party.** 7 demos, the key one being:

**`call-moderation/moderation-agent`** — Near-exact reference for Echo:
- **No TTS** — overrides `tts_node()` to no-op (agent never speaks)
- **Selective subscription** — `set_subscribed()` filtering by participant kind/identity
- **LLM-powered detection** — monitors transcripts against guidelines, calls `report_guideline_violation(category, description, severity)` via function tool
- **Emits RPCs** — `perform_rpc(method="moderation.show_violation", payload=...)` to non-agent participants
- **Frontend** renders violations as real-time safety banners

**Other gems:**
- `model-battleground` — 3 agents in one room, dispatched programmatically (multi-agent coordination)
- `call-queue` — call center with post-call survey agents (Q&A queue pattern)
- Agent Dispatch API usage — `api.agent_dispatch.create_dispatch()` with metadata

**Architecture patterns to adopt:**
- `function_tool()` decorator for LLM-triggered actions
- `ModerationUserdata` via `RunContext` for clean state
- Selective track subscription filtering
- Dispatch-with-metadata for spawning configured agents

---

#### 5. AOSSIE-Org/Resonate (337 stars) — Community Features Reference

**Tech:** Flutter/Dart mobile app, Appwrite backend, LiveKit for audio. GPL-3.0. GSoC project.

**Useful patterns for Build Party:**
- **Room lifecycle** — create, browse active, schedule upcoming rooms with subscriber notifications
- **Social layer** — following system, friends with voice calls, pair chat (random matching), profiles, notifications
- **In-room features** — chat, speaker promotion (listener → speaker), user reporting

**Limitations:** Mobile only (no web), tightly coupled to Appwrite, basic roles (creator/speaker/listener — no granular moderation), no screen sharing, no concept of "builder streams" or project showcases.

**Verdict:** Design reference for room discovery, scheduling, and social features. Reimplement on a web stack with richer roles.

---

### Synthesis: Build Party Implementation Path

**Priority order:**
1. **dev-day-demos `call-moderation`** → Fork the silent moderation agent as Echo's starting point
2. **livekit/agents** → Nova on VoicePipelineAgent + content filter examples from python-agents-examples
3. **Egress API** (docs, not in any example repo) → Flare's clip trigger
4. **livestream** → Borrow stage-summon UX patterns, rebuild on LiveKit SDK v2
5. **Resonate** → Design reference for room browsing, scheduling, social features

**Greenfield work required:**
- Q&A queue moderation (Nova) — no existing implementation
- Egress clip triggering from RPC events (Flare) — no examples exist
- Co-build rooms (shared editor + LiveKit) — genuinely new, no prior art in these repos
- Courseware system (slides, code sandboxes, progress tracking) — not covered anywhere
- Web-based room discovery + scheduling (Resonate is mobile-only)

**Biggest finding:** The `call-moderation` demo in dev-day-demos is a near-production blueprint for Echo. The silent observer + LLM classification + RPC emission pattern is exactly what Build Party needs, with real code to start from.
### Re: Rust vs Python + platform strategy

Good instinct. Here's the tradeoff analysis:

#### The constraint: LiveKit Agents SDK is Python-first

LiveKit's `livekit-agents` v1.5 is Python. There's a `livekit-agents-js` (Node/Deno) but it lags behind. No Rust agents SDK exists. The 70+ examples, all the STT/TTS plugins, the VoicePipelineAgent — all Python.

**For the agent layer (Nova/Echo/Flare): Python is the pragmatic choice.** Fighting the ecosystem means rewriting plugins, missing new features, and diverging from LiveKit's upgrade path.

#### Where Rust makes strategic sense

| Layer | Language | Rationale |
|---|---|---|
| **Agents** (Nova/Echo/Flare) | Python | LiveKit SDK ecosystem, rapid iteration, plugin access |
| **Core libraries** (session indexing, transcript processing, CRDT, document engine) | Rust | Performance, FFI to Flutter + WASM, type safety |
| **Frontend** (web) | TypeScript | LiveKit `livekit-client` SDK is JS/TS, React components |
| **Mobile/Desktop** | Flutter + Rust FFI | Shared core logic, native performance |
| **Browser-embedded logic** | Rust → WASM | Same codebase as native, no JS port needed |

#### The FFI + WASM strategy

This is where your insight pays off. A Rust core library can target:

1. **Flutter (mobile + desktop)** — `dart:ffi` + `ffigen` for direct Rust calls. No serialization overhead. This is production-proven (Mozilla, Signal).
2. **WASM (browser)** — `wasm-bindgen` + `wasm-pack` compiles the same Rust to WASM. Called from JS/TS via generated bindings. No separate "JS port" needed.
3. **Python (agents)** — `PyO3` + `maturin` for Python bindings to the same Rust core. Your agent-doc/corky stack already uses this pattern.

```
                    ┌──────────────┐
                    │  Rust Core   │
                    │  (libraries) │
                    └──────┬───────┘
              ┌────────────┼────────────┐
              │            │            │
        dart:ffi      wasm-bindgen    PyO3
              │            │            │
        ┌─────┴─────┐ ┌───┴────┐ ┌────┴─────┐
        │  Flutter   │ │ Browser│ │  Python  │
        │  mobile +  │ │  WASM  │ │  Agents  │
        │  desktop   │ │  (TS)  │ │  (LK)    │
        └────────────┘ └────────┘ └──────────┘
```

#### What goes in the Rust core?

- **Session/transcript processing** — parsing, indexing, topic segmentation (CPU-bound, benefits from Rust speed)
- **CRDT engine** — if Build Party has collaborative editing (co-build rooms), Rust CRDT → WASM is the standard approach (Yjs was originally JS but Yrs is the Rust port)
- **Content hashing / dedup** — for Shimmer session dedup and snapshot management
- **Search index** — if you self-host search (tantivy = Rust Lucene), it can run as WASM in browser or native via FFI

#### What stays in Python?

- All LiveKit agent logic (Nova/Echo/Flare)
- STT/TTS plugin orchestration
- RAG pipeline (langchain/llama-index ecosystem is Python)
- Quick prototyping and iteration on agent behavior

#### Recommendation

**Start Python for agents, Rust for core libraries from day one.** Don't write a "Python prototype then rewrite in Rust" — that's waste. Instead:

1. Agents in Python (use LiveKit SDK directly)
2. Core processing in Rust with PyO3 bindings (agents call Rust for heavy lifting)
3. Same Rust core → WASM for browser, FFI for Flutter
4. TypeScript for web frontend (LiveKit components-react + WASM imports)

This gives Rob's team a single Rust codebase for all non-agent logic, with three compile targets (native/WASM/Python). The agents stay in Python where the ecosystem lives.

**For Rob's pitch:** "We write core logic once in Rust, then compile to native (Flutter), browser (WASM), and Python (agent plugins). No JS port, no code duplication. The agent layer stays Python because that's where LiveKit's ecosystem lives."
<!-- agent:boundary:276eacf1 -->
<!-- /agent:exchange -->
