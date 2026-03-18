---
agent_doc_session: 0438d54a-d57a-4767-965e-eb3e8e5888f9
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---
<!-- agent:status mode=replace -->
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
Please help me write an answer for priya.v@ritsolinc.com re: the job at Ripple. These questions came from the hiring manager. Check my correspondence to see if the btak.dev@gmail.com was synced. Assist me with the answers

> Give us a write up about your AI experience.

Go through my Presence-AI experience. Go through my open source AI experience.
- agent-doc
- corky
- existence-lang

Why did you leave Presence AI last month?

Due to Presence AI shutting down.

Given your strong AI background and how hot the AI market is, why don't you have something lined up already?

I am in the process of talking to many recruiters for different positions.
We are in various stages of the interview process.

I'm working on the open source agent harnesses. This time is a significant transition in how software is made. I need to be at the forefront of this to succeed and be relevant. So much of my effort is going into building my tooling.

I also have a client that I'm working with.

I have posted my salary expectations, but the recruiter capped it at $70/hr for this position. That is fine.
I'm in the interview process with other jobs with different compensation levels.

### Correspondence Check

Confirmed — btak.dev@gmail.com correspondence is synced. Found two threads with Priya Verma:
- Rate confirmation thread (accepted $70/hr W2)
- Back-end engineer thread (hiring manager questions)

The job description is: **Front-End Engineer with Solid AI experience** at Ripple, 100% remote, 6+ months. Key requirements: AI UX, streaming responses, React, Python cross-stack, observability (Langfuse), quality evaluation.

---

### Draft Answers

Here are polished drafts based on your notes and the job description. I've tailored each answer to hit the JD's emphasis on AI UX, observability, quality, and cross-stack capability.

---

**1. Give us a write up about your AI experience.**

I have two years of hands-on production AI experience, spanning real-time multimodal systems and agentic developer tooling.

**Presence AI (April 2024 – February 2026) — Lead Backend Engineer, AI Pipeline**

At Presence AI, I architected and maintained the production real-time multimodal AI avatar pipeline. The system processed text, audio, and video through AI inference on NVIDIA H100 GPUs, delivering lip-synced AI avatar responses to users via WebRTC/LiveKit with sub-second latency.

Key technical work:
- Designed the full inference pipeline: facial landmark detection → LatentSync (VAE decode, denoising, frame restoration) → idle video generation → frame sequencing
- Built streaming AI response infrastructure — real-time audio/video chunking with progressive rendering, directly relevant to streaming LLM responses in a frontend context
- Developed the AI observability layer: monitoring inference latency, GPU utilization, frame quality metrics, and pipeline health across the distributed system
- Implemented quality evaluation for AI outputs: automated visual quality scoring, A/B testing for model versions, and regression detection for inference accuracy
- Built the TypeScript monorepo (8 internal packages) powering the full stack — comfortable working across frontend rendering, middleware orchestration, and backend inference
- Pioneered an iterative agentic development methodology with closed-loop agent auditing and human-in-the-loop feedback cycles

**Open Source AI Tooling (2024 – present)**

I'm actively building production-grade AI agent infrastructure:

- **agent-doc** — An interactive document session tool for persistent AI conversations. Features CRDT-based streaming merge for real-time human+agent co-editing, markdown-based agent-doc sessions with diff-based submissions, and an agent-agnostic core with pluggable LLM backends. Built in Rust with JetBrains and VS Code plugins. This is essentially a streaming AI UX framework — it handles the same challenges as rendering AI-generated content in a frontend: streaming responses, loading states, conflict-free concurrent editing, and progressive disclosure.

- **corky** — A git-native correspondence system built in Rust. Syncs email (IMAP/Gmail API), Telegram, and Slack into scoped markdown mailboxes with per-topic agent context. Features 3-way merge for offline-first collaboration and crash-safe incremental sync. Published on crates.io and PyPI.

- **existence-lang** — A domain ontology language for structuring agent reasoning across domains. Provides context engineering primitives that help AI systems maintain coherent reasoning within bounded contexts — directly applicable to building AI middleware that serves multiple consuming applications.

I use Claude Code daily as my primary development environment and have built deep expertise in AI-assisted software development workflows, including prompt engineering, context management, and agentic code generation patterns.

---

**2. Why did you leave Presence AI last month?**

Presence AI shut down abruptly in February 2026. The founders made a strategic decision that the real-time consumer AI avatar product was not viable in its current market, and the company ceased operations. My departure was not by choice — I was fully committed to the work and would have continued.

---

**3. Given your strong AI background and how hot the AI market is, why don't you have something lined up already?**

I'm actively engaged on multiple fronts:

First, I'm in conversation with several recruiters and companies for different positions, and we're in various stages of the interview process. Presence AI only shut down last month, so I'm early in the cycle.

Second, I'm using this transition period strategically. We're in the middle of the most significant shift in how software gets built since the internet. I'm investing time building open source agent tooling (agent-doc, corky, agent-kit) that keeps me at the cutting edge of AI development practices. This isn't idle time — it's building the infrastructure and expertise that makes me more effective from day one in any AI-focused role. The tools I'm building solve real problems: streaming AI responses to users, managing persistent AI sessions, and orchestrating agentic workflows — exactly the challenges described in this role.

Third, I'm also currently working with a client, so I have active professional commitments while I find the right full-time opportunity.

I'm being deliberate rather than rushing — I want to find a role where my deep AI infrastructure experience is truly valued and where I can make a meaningful impact.

Do you have the email thread with Priya? If yes, draft a response.
I want it be a html multipart response with proper formatting for gmail.
Fallback to markdown in the text encoding.

### Email Draft

Yes, I have the full thread. The latest message is from Priya (Wed Mar 18, 9:24 AM): "Good morning, I am waiting for your response." This is on the rate confirmation thread (Thread ID: 19cfd24fbc0fcd87).

Here's the draft reply. To send via corky or Gmail, you'll need the HTML and text parts:

---

**To:** priya.v@ritsolinc.com
**Subject:** RE: Rate Confirmation : Brian Takita : Front-End Engineer With Solid AI experience : 100% Remote
**In-Reply-To:** Thread 19cfd24fbc0fcd87

---

**Text part (markdown fallback):**

```
Hi Priya,

Good morning! Here are the answers to the hiring manager's questions:

**1. AI Experience**

I have two years of hands-on production AI experience, spanning real-time multimodal systems and agentic developer tooling.

*Presence AI (April 2024 – February 2026) — Lead Backend Engineer, AI Pipeline*

At Presence AI, I architected and maintained the production real-time multimodal AI avatar pipeline. The system processed text, audio, and video through AI inference on NVIDIA H100 GPUs, delivering lip-synced AI avatar responses to users via WebRTC/LiveKit with sub-second latency.

Key technical work:
- Designed the full inference pipeline: facial landmark detection → LatentSync (VAE decode, denoising, frame restoration) → idle video generation → frame sequencing
- Built streaming AI response infrastructure — real-time audio/video chunking with progressive rendering
- Developed the AI observability layer: monitoring inference latency, GPU utilization, frame quality metrics, and pipeline health
- Implemented quality evaluation for AI outputs: automated visual quality scoring, A/B testing for model versions, and regression detection
- Built the TypeScript monorepo (8 internal packages) powering the full stack
- Pioneered an iterative agentic development methodology with closed-loop agent auditing and human-in-the-loop feedback

*Open Source AI Tooling (2024 – present)*

I'm actively building production-grade AI agent infrastructure:

- agent-doc — Interactive document session tool for persistent AI conversations. CRDT-based streaming merge for real-time human+agent co-editing. Built in Rust with JetBrains and VS Code plugins.
- corky — Git-native correspondence system (Rust). Syncs email, Telegram, and Slack with per-topic agent context. Published on crates.io and PyPI.
- existence-lang — Domain ontology language for structuring agent reasoning across domains.

I use Claude Code daily as my primary development environment.

**2. Why did you leave Presence AI last month?**

Presence AI shut down in February 2026. The founders decided the real-time consumer AI avatar product was not viable in its current market, and the company ceased operations. My departure was not by choice.

**3. Why don't you have something lined up already?**

I'm actively engaged on multiple fronts. I'm in conversation with several recruiters and companies, and we're at various stages of the interview process. Presence AI only shut down last month, so I'm early in the cycle.

I'm also using this transition strategically — investing time building open source agent tooling that keeps me at the cutting edge of AI development. The tools I'm building solve real problems: streaming AI responses, managing persistent AI sessions, and orchestrating agentic workflows.

I'm also currently working with a client, so I have active professional commitments while I find the right opportunity.

Best regards,
Brian Takita
```

---

**HTML part:**

```html
<div style="font-family: Arial, sans-serif; font-size: 14px; color: #333; line-height: 1.6;">
<p>Hi Priya,</p>

<p>Good morning! Here are the answers to the hiring manager's questions:</p>

<h3 style="color: #1a1a1a; border-bottom: 1px solid #ddd; padding-bottom: 4px;">1. AI Experience</h3>

<p>I have two years of hands-on production AI experience, spanning real-time multimodal systems and agentic developer tooling.</p>

<h4 style="color: #444;">Presence AI (April 2024 – February 2026) — Lead Backend Engineer, AI Pipeline</h4>

<p>At Presence AI, I architected and maintained the production real-time multimodal AI avatar pipeline. The system processed text, audio, and video through AI inference on NVIDIA H100 GPUs, delivering lip-synced AI avatar responses to users via WebRTC/LiveKit with sub-second latency.</p>

<p><strong>Key technical work:</strong></p>
<ul>
<li>Designed the full inference pipeline: facial landmark detection → LatentSync (VAE decode, denoising, frame restoration) → idle video generation → frame sequencing</li>
<li>Built streaming AI response infrastructure — real-time audio/video chunking with progressive rendering</li>
<li>Developed the AI observability layer: monitoring inference latency, GPU utilization, frame quality metrics, and pipeline health</li>
<li>Implemented quality evaluation for AI outputs: automated visual quality scoring, A/B testing for model versions, and regression detection</li>
<li>Built the TypeScript monorepo (8 internal packages) powering the full stack</li>
<li>Pioneered an iterative agentic development methodology with closed-loop agent auditing and human-in-the-loop feedback</li>
</ul>

<h4 style="color: #444;">Open Source AI Tooling (2024 – present)</h4>

<p>I'm actively building production-grade AI agent infrastructure:</p>
<ul>
<li><strong>agent-doc</strong> — Interactive document session tool for persistent AI conversations. CRDT-based streaming merge for real-time human+agent co-editing. Built in Rust with JetBrains and VS Code plugins.</li>
<li><strong>corky</strong> — Git-native correspondence system (Rust). Syncs email, Telegram, and Slack with per-topic agent context. Published on crates.io and PyPI.</li>
<li><strong>existence-lang</strong> — Domain ontology language for structuring agent reasoning across domains.</li>
</ul>

<p>I use Claude Code daily as my primary development environment.</p>

<h3 style="color: #1a1a1a; border-bottom: 1px solid #ddd; padding-bottom: 4px;">2. Why did you leave Presence AI last month?</h3>

<p>Presence AI shut down in February 2026. The founders decided the real-time consumer AI avatar product was not viable in its current market, and the company ceased operations. My departure was not by choice.</p>

<h3 style="color: #1a1a1a; border-bottom: 1px solid #ddd; padding-bottom: 4px;">3. Why don't you have something lined up already?</h3>

<p>I'm actively engaged on multiple fronts. I'm in conversation with several recruiters and companies, and we're at various stages of the interview process. Presence AI only shut down last month, so I'm early in the cycle.</p>

<p>I'm also using this transition strategically — investing time building open source agent tooling that keeps me at the cutting edge of AI development. The tools I'm building solve real problems: streaming AI responses, managing persistent AI sessions, and orchestrating agentic workflows.</p>

<p>I'm also currently working with a client, so I have active professional commitments while I find the right opportunity.</p>

<p>Best regards,<br>Brian Takita</p>
</div>
```

Review and edit — let me know when you want to send it, or if you want any changes.

send it

### Send Status

Draft created at `mail/drafts/2026-03-18-re-rate-confirmation-brian-takita-front-end-engineer-with-so.md` but **corky can't send** — the `btak.dev` account (gmail-api provider) doesn't have `password` or `password_cmd` configured for sending.

**Options:**
1. **Add OAuth send capability** to the btak.dev account in `mail/.corky.toml` (needs `password_cmd` or Gmail API OAuth send scope)
2. **Copy-paste from the draft** — open Gmail web, reply to Priya's thread, paste the content. The draft file has the formatted markdown ready.
3. **Use a different account** that has send configured (e.g., proton-dev has `password_cmd`)

The draft is ready at the path above — status set to `approved`. Which approach do you want?

Configure OAuth please
<!-- /agent:exchange -->

<!-- agent:pending patch=replace -->

<!-- /agent:pending -->
