# Haiven — Backend Platform Software Engineer — Interview & Application Prep

Companion to the session doc `tasks/recruit/haiven.md`. Tailored resume: `resume/BrianTakita-haiven.pdf` (profile `resume/profiles/haiven.toml`).

- **Role:** Backend Platform Software Engineer (Go, WebSocket real-time, PostgreSQL, AWS, NATS/JetStream, SolidJS)
- **Comp/level:** 3–5 yr IC, $80k–$100k + equity, remote US-preferred (WA company, state-side US citizens preferred)
- **Job poster:** Craig Fryar (CEO & C-Suite Advisor); role reports to VP of Product
- **Job post:** https://www.linkedin.com/jobs/view/4433645177/

---

## Part 1 — LinkedIn Application Survey Answers

Short, direct, keyword-aligned to their JD. Trim to their field limits.

### "Tell us why you're a perfect fit for this role."

> I build the real-time, reactive, event-driven backend plumbing this platform runs on. At Presence AI I architected a production real-time multimodal avatar pipeline — WebRTC media, low-latency streaming, PostgreSQL, a TypeScript monorepo — on NVIDIA H100 GPUs. At Milyoni I built real-time video social events at scale (live concerts for Warner Bros, Lionsgate, Paramount, Universal Music) plus video-on-demand — a gaming/community-adjacent platform with real users.
>
> On the reactive + Go side: I author `lazily`, a cross-language reactive-signals and CRDT family, including a Go port (lazily-go) built on goroutines and channels with a distributed CRDT anti-entropy plane. That means SolidJS-style fine-grained reactivity and a Go concurrency backend are the same problem to me — and my agent-doc project syncs a document across four replicas conflict-free with a live, reorderable work queue, which is the same shape as your WebSocket chat/presence plus NATS/JetStream signals.
>
> I've shipped payments (Stripe + PayPal, checkout hardening), RAG/AI features (tsift), and I own features end-to-end. My Go is newer than my 20+ years of systems work, but I write it today and ramp fast because the hard part — real-time state, event pipelines, reactive UIs — is already what I do.

### Optional shorter variant (if the field is tight)

> Real-time + reactive backend engineer. Architected Presence AI's production WebRTC avatar pipeline; ran real-time video social events at scale at Milyoni; author `lazily` (cross-language reactive-signals + CRDT, incl. a Go port on goroutines/channels). SolidJS-style reactivity and a Go real-time backend are the same problem to me. Shipped Stripe/PayPal payments and RAG/AI features. Own features end-to-end.

---

## Part 2 — AI Screen Rehearsal Script (spoken, 60–120s each)

Async recorded video. Lead with the outcome, name real systems, keep it tight. Say the company/role back once early.

### Q1 — "Why Haiven, and why this role?"

> What drew me to this role is that it's exactly where my work already lives — backend and real-time systems for a gaming and community platform. I've spent years building real-time, reactive, event-driven systems: live video social events at scale at Milyoni, and a production WebRTC avatar pipeline at Presence AI. Haiven's stack — a Go monolith with WebSocket chat and presence, NATS/JetStream event pipelines, and a SolidJS reactive frontend — maps almost one-to-one onto what I build for fun: I author a cross-language reactive-signals and CRDT library. And I want to do it on a small team where I own features end-to-end and ship to real developers and players, not in a big-company silo. The player-first, revenue-to-developers mission is one I actually care about.

### Q2 — "Tell me about a backend or real-time system you built end-to-end."

> At Presence AI I architected and shipped the entire real-time video pipeline for AI avatars — facial landmark detection, lip-sync inference, frame sequencing — running on NVIDIA H100 GPUs and delivered over WebRTC with LiveKit, backed by a GraphQL API and PostgreSQL in a TypeScript monorepo. The hard part was keeping latency low and quality stable under a live stream, so I built distributed parallel compute with Ray, low-latency message serialization, and temporal smoothing on the landmarks. Separately, in my agent-doc project, I built a document that syncs in real time across four replicas — a controller process, editor plugins, a disk watcher, and a task supervisor — using CRDTs so concurrent edits merge without conflicts. That's the same shape as real-time chat and presence: many clients, one eventually-consistent state. Those two together are exactly the WebSocket-plus-eventual-consistency problem this role centers on.

### Q3 — "What's your experience with Go?"

> Let me be straight about it: Go is newer for me than my overall systems experience, but I do write it. I authored the Go port of my reactive library, lazily-go — it's Slots, Cells, and Signals with a CRDT collection layer and a distributed anti-entropy plane, all built on goroutines and channels, sharing state by communicating rather than locking. So I'm writing idiomatic concurrent Go, not just tinkering. I've also worked with go-fuse and Cerebrium's CLI. What I'd bring is 20-plus years of systems fundamentals — real-time state, concurrency, event pipelines, API design — so ramping on Go's ecosystem and your codebase is fast, because the concepts underneath are already what I do every day.

### Q4 — "How have you worked with event-driven systems, pub/sub, or message brokers?"

> Event-driven design is the core of almost everything I build. My reactive libraries are fundamentally a pub/sub signal-propagation system — when a source cell changes, it invalidates and pushes updates through a dependency graph, across processes and even across languages over IPC and WebSockets. The lazily Go port includes a distributed CRDT anti-entropy plane, which is the same pattern as a durable event stream reconciling state between nodes. So when I look at your NATS and JetStream pipelines feeding the trust-and-safety and signals engine, that's a very familiar shape: producers emitting events, consumers reconciling state idempotently, ordering and replay guarantees. I've also built work queues with cell-identity merge so items can be reordered and consumed exactly once under concurrent edits.

### Q5 — "Tell me about AI or ML features you've shipped in production."

> Two things stand out. At Presence AI I shipped a production multimodal AI pipeline — real-time lip-sync inference, audio and image generation — and defined the agentic development methodology the team used. And I built tsift, a token-conscious code-RAG tool: AST-aware hybrid search, embeddings, and graph-database-backed traversal that turns noisy logs and sessions into bounded evidence for smaller models. That's directly relevant to an AI-powered trust-and-safety engine — content moderation, scoring, and recommendations are the same core problem of taking signals, embedding or scoring them, and acting on the result under latency constraints. I'm comfortable integrating ML-backed features at the application level, not just calling an API.

### Q6 — "How do you work on a small, fast-moving team, and can you give an example of a hard problem you solved?"

> Small teams are where I do my best work — at Pivotal Labs I paired and mentored across more than 20 startups, and today I deliver client work solo, end-to-end, from data model to production. A concrete hard problem: in agent-doc, concurrent edits from a human and an agent to the same document kept producing merge conflicts and duplicated or lost items in a live work queue. I solved it with cell-identity CRDT merge semantics — every item carries a stable identity, so reordering and updates from different replicas converge deterministically instead of clobbering each other. Getting that right under real concurrency, with an eventually-consistent guarantee, is exactly the kind of correctness problem that shows up in real-time chat, presence, and event queues — which is what this role is.

### Delivery checklist
- Camera on, framed, decent light + audio (it records video — `saveVideo=true`).
- Lead with the outcome, then the how. Name real systems: Presence AI, Milyoni, lazily-go, agent-doc, tsift.
- Volunteer the Go gap once (Q3), framed as a fast ramp — don't re-apologize elsewhere.
- Keep each answer 60–120s. Structure over depth; async screens reward clarity.
- If asked for a question: "How is the trust & safety signals engine architected on NATS/JetStream today, and where does it need to scale?"

---

## Part 3 — Company & Job Poster Quick Reference

### Haiven, Inc. (haiven.gg)
"The indie game platform that pays." Player-first digital distribution + communications platform for indie devs; WA-based; Alpha → Beta.
- **Product:** AI discovery/vibe-scoring; real-time chat/DM/social hubs with AI moderation + auto-translation; developer studios + SDK (Unity/Unreal/React Native/Roblox/Discord); virtual economy + on-chain payments; AI trust & safety signals engine; analytics/revenue dashboards.
- **Stack:** Go monolith → microservices; REST + WebSocket; PostgreSQL; AWS (ECS/S3/CloudFront/Cognito/CloudWatch); NATS + JetStream; Redis/Valkey; Terraform; SolidJS + TS + TanStack Query. Agile/SCRUM, AI-augmented review/testing.
- **Culture:** small team, equity, "fingerprints factor," meritocracy.

### Craig Fryar — job poster (CEO & C-Suite Advisor)
Data-driven product-in-games executive. Tailor to signals/telemetry/player-behavior + AI trust & safety + monetization.
- BioWare/EA — Director of Analytics (Mass Effect 3, SWTOR)
- Wargaming.net — Head of Global BI (EDW, 300B+ daily events)
- Tableau — Global Director of Enterprise Strategy
- CDO/exec roles (BankingON COO & CDO); investor ($30M+ raised); Chief AI & Product Officer, Market Holdings; VR training (Augmented Training Systems)
- LinkedIn: https://www.linkedin.com/in/craigfryar/

Sources: [Haiven](https://haiven.gg/) · [developers](https://haiven.gg/developers) · [Haiven LinkedIn](https://www.linkedin.com/company/haiven-inc) · [Job post](https://www.linkedin.com/jobs/view/backend-platform-software-engineer-at-haiven-4433645177) · [Craig Fryar](https://www.linkedin.com/in/craigfryar/)
