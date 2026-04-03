---
agent_doc_session: 23b19f30-f4fb-497b-abce-6d768a21c42d
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---
<!-- agent:status mode=replace -->
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
Help me answer the following questions given what you know about me.

1. Give us a write up about your AI experience.
2. Why did you leave Presence AI last month?
- PresenceAI abruptly shut down when the founders decided that the real-time consumer AI was not viable 
3. Given your strong AI background and how hot the AI market is, why don't you have something lined up already?

### Re: AI Experience & Interview Questions

**1. AI Experience**

I have nearly two years of hands-on production AI experience at **Presence AI** (April 2024 – February 2026) as **Lead Backend Engineer, AI Pipeline** — I architected and shipped a real-time multimodal AI avatar system. This involved text, audio, and video lip-sync inference on NVIDIA H100 GPUs delivered over WebRTC via LiveKit. The pipeline included facial landmark detection, LatentSync lip-sync inference (VAE decode, denoising, restoration), idle video generation, and frame sequencing — all within a TypeScript monorepo with 8 internal packages.

Beyond Presence AI, I've been building AI agent tooling since 2024:
- **agent-doc** — Document-as-interface for persistent AI conversations with CRDT streaming merge for real-time human+agent co-editing
- **corky** — Git-native correspondence system syncing email (IMAP/Gmail API), Telegram, Slack into scoped markdown mailboxes with per-topic agent context
- **agent-kit** — Rust SDK for CLI tools integrating with AI agent loops: session management, tmux pane routing, instruction file discovery

I developed an iterative **agentic development methodology** — lightweight living specs, closed-loop agent auditing, and human-in-the-loop feedback. My deeper technical foundation includes 15+ years of reactive programming (backbone-signal → ctx-core → rmemo → relysjs) and a domain ontology system since 2014, both of which directly inform my approach to context engineering for AI agents.

**2. Why I left Presence AI**

Presence AI abruptly shut down when the founders decided that the real-time consumer AI avatar product was not viable. The decision came suddenly — the team was actively building and shipping when the plug was pulled.

**3. Why I don't have something lined up already**

I left just 3 weeks ago (February 2026) and have been actively building my pipeline. I'm in process with CopilotKit (Senior/Staff Infrastructure Engineer, open-source AI copilot platform) — past the recruiter screen, resume forwarded, awaiting first technical interview. I've also connected with Rise Technical and DeepTech recruiters.

Meanwhile, I've been investing in my open-source agent tooling portfolio (agent-doc, corky, agent-kit) which demonstrates current, production-quality AI work. This is a deliberate strategy — showing rather than telling. These tools are in active daily use and showcase real-time systems design, Rust/TypeScript/Python cross-language work, and agentic workflow architecture.
<!-- /agent:exchange -->
