See https://www.linkedin.com/in/briantakita/

Please review my LinkedIn profile for more details. Here is my summary and most recent work experience. I will also add corky/agent-doc to highlight my work in the coding agent space. I want to emphasize that I am striving for a good process and making significant gains.

Over 20 years of software development experience developing full-stack applications. Currently focused on the full pipeline of Realtime AI Avatars using Generative models.

Utilizes philosophy, logic, language, & qualitative/quantitative approaches to create software models to explore & accurately represent the domain.

Presence AI logo
Lead Backend Engineer, AI Pipeline
Lead Backend Engineer, AI Pipeline
Presence AI · Full-timePresence AI · Full-time
Apr 2024 - Present · 1 yr 11 mosApr 2024 to Present · 1 yr 11 mos
- Architected and shipped the entire real-time video pipeline: facial landmark detection, LatentSync lip-sync inference (VAE decode, denoising, restoration), idle video generation, and frame sequencing on NVIDIA H100 GPUs via LiveKit
- Architected a multi-service AI monorepo with closed-loop agentic workflows and self-auditing development infrastructure (Claude Code / Codex)
- Developed an iterative agentic development methodology: lightweight living specs refined through closed-loop agent auditing and continuous human-in-the-loop feedback, accelerating design velocity while staying in flow with the emerging design
- Built reusable async primitives for high-quality agent-authored code and human comprehension: task lifecycle management, scoped cancellation, lazily resolved dependencies that decouple initialization order and cascade invalidation through the context, runtime profile logging, and stable cross-stack naming to maintain conceptual coherence for both human and agent reasoning
- Improved lip-sync quality through landmark temporal smoothing, LatentSync model upgrades, and optimized Whisper audio feature extraction via improved audio stream batching

## Assistant

### Review of Current Content

**Presence AI bullets are strong.** The arc from pipeline architecture → agentic methodology → async primitives → measurable quality improvement tells a coherent story. Key observations:

- **Bullet 3 (iterative agentic methodology)** is your most differentiated point — it positions you as someone who's designed *how to work with agents*, not just used them. This is rare and valuable.
- **Bullet 4 is dense.** "Lazily resolved dependencies that decouple initialization order and cascade invalidation through the context" packs two major ideas into one clause. Consider splitting into two bullets or simplifying to the core insight: dependency injection that works for both agent-generated and human-authored code.
- **Bullet 2** — "self-auditing development infrastructure" is compelling but vague. What does the audit loop catch? Spec drift? Regressions? Dead code? A concrete example would sharpen this.

### Suggested Additions: corky & agent-doc

These projects reinforce your "agentic development" narrative. Here's how I'd frame them:

**Open Source / Personal Projects**

**agent-doc** — Document-as-UI for Persistent AI Conversations (Rust)
- Built a novel interaction model where a markdown document becomes the interface for long-running AI agent sessions — edit offline, submit diffs, agent responds inline
- Engineered write-safe concurrent editing via 3-way merge (user edits + agent response + original), enabling uninterrupted human-in-the-loop workflows
- Designed agent-agnostic architecture with trait-based LLM backends, snapshot-based diffing (SHA256 + `similar` crate), and automatic git integration for version history

**corky** — Multi-Source Email Unification & AI-Assisted Drafting (Rust, ~9,200 LOC)
- Developed a correspondence toolkit that syncs email threads from multiple IMAP accounts (Gmail, Protonmail) into unified markdown conversations with smart deduplication
- Built crash-safe incremental IMAP sync with streaming merge — messages written immediately, sync state committed only on success
- Implemented 3-way contact sync using FNV-1a content hashing, plus bulk import normalization for Slack workspace exports and Telegram archives

### Framing Advice

The thread connecting Presence AI + agent-doc + corky is: **you're building the tooling and methodology for agent-augmented development, not just consuming it.** Your resume should make that explicit. Consider a brief "Technical Philosophy" or updated summary line like:

> "Building the tooling, primitives, and methodology for agent-augmented software development — from real-time AI pipelines to the developer workflows that produce them."

This positions you at the intersection of AI engineering and developer tooling, which is exactly where the industry is heading.

## User

Does linkedin provide an api so we can make these changes together?
