---
agent_doc_session: fb7495d9-25a5-4962-9cd9-c2726a37aa21
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---
<!-- agent:status mode=replace -->
- [x] Research ECI Solutions & Justin Caird
- [x] Draft response
- [x] Build tailored resume (eci-solutions profile)
- [x] Draft follow-up reply with resume & availability
- [x] Update resume to close gaps (existence-lang, avatars, highlights)
- [x] Rebuild PDF
- [x] Review and send
- [x] Interview preparation
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
Please draft a response to https://www.linkedin.com/in/jcaird/:

Want to ship daily and build the agents of the future? We're forming a team of AI experts and would love to chat.
Hey Brian,

I hope this message finds you well. Your background in Agentic AI is very impressive!

I'll cut to the chase as I'm sure you get a lot of messages from recruiters. I'm working directly with the CTO at ECI (ecisolutions.com), a leading ERP solutions company, to build out a very strategic team of AI experts and Prompt Architects stretching across, Development, QA, Data. These are not traditional coding roles, instead they're focused on building agents that will write and maintain the code.

We're searching for experienced engineers who are deeply passionate about AI, true builders (think running models locally, building agents to automate boring tasks just for fun, despise manual work/documentation and live inside LLMs/Claude/Cursor/Copilot) to modernize 20-year-old ERP systems. This team will have extensive ownership, and be encouraged to move with speed, ship daily and worry about perfection later. If you're looking for a role with freedom and impact, this is it.

These roles are fully remote and pay competitively for the right candidate. After reviewing your profile, it looks like it could be a good match!

I'd love to hop on a quick call and share more about the company, roles and their AI vision. Do you have time early next week to discuss further?

Looking forward to your feedback!

Justin Caird
Sr. Recruiter
### Research: ECI Solutions

**ECI Software Solutions** — 30-year-old cloud ERP provider for SMBs. 25,000+ customers, 90+ countries, 2,100 employees, Fort Worth TX. Verticals: manufacturing, construction, distribution, building supply, office tech, field service.

**Tech stack:** C#.NET, JavaScript, Python, cloud-first. Products include M1 (manufacturing ERP), EvolutionX (B2B e-commerce), Cognytics (BI), Davisware (field service).

**AI push:** "Practical AI in Business" focus at ECI Connect 2026, actively embedding AI features into ERP suite. This role is about building agents that write/maintain code to modernize 20-year-old systems.

**Justin Caird:** Sr. Technical Recruiter at ProSearch Strategies (external agency), not directly at ECI. Based in Portland, ME. Background: Brightcove, Philips, CoreInformation Technology.

**No existing contact file** in mail/contacts/.

### Match Analysis

Strong fit for the agentic systems angle:
- agent-doc IS "building agents that write/maintain code"
- Presence AI pipeline = production AI systems at scale
- 20+ years of legacy modernization experience (Backbone→React at SocialChorus, Rails upgrades at TrueCar, etc.)
- C# is on your skills list (Generic Query Analyzer, .NET at SafeSchoolz/NASPP)

Concern: C#.NET isn't your primary recent language, but the patterns transfer. The role emphasizes AI/agents over language-specific expertise.

### Draft LinkedIn response:

Hi Justin,

Thanks for reaching out — this resonates with what I've been building.

I've been shipping agentic developer tooling in Rust and TypeScript: agent-doc (a document-as-interface for persistent AI sessions with CRDT-based concurrent editing), corky (git-native correspondence with agentic workflows), and lazily (cross-language reactive signals). Before that, I architected the real-time multimodal AI pipeline at Presence AI on NVIDIA H100 GPUs.

The idea of building agents that write and maintain code on legacy systems is exactly the problem space I'm in — agent-doc is literally an agent runtime for this kind of work. I'd be happy to chat about how these patterns apply to modernizing ERP systems.

I'm available this week. What works for you?

Brian

**Notes on the draft:**
- Leads with what you're building (agents) not where you worked
- Names specific projects to demonstrate "true builder" — not just someone who uses AI tools
- Connects agent-doc directly to their problem ("agents that write and maintain code")
- Keeps it short — Justin asked for a quick call, not a thesis
- Doesn't mention C# since the role de-emphasizes traditional coding

Read updated conversation /home/brian/work/btakita/agent-loop/mail/contacts/justin-caird/20260322-linkedin-justin-caird.txt
Note Justin's reply.

Draft a reply.
Prepare a resume based on what his client is looking for.
### Re: Justin's reply — resume & availability

Justin's latest message (exported from LinkedIn conversation):

> Hey Brian - I'm sorry for the delay! I have been out of town and just catching up. I'd love to explore this further with you. Can you share your resume and times you can speak this week? Looking forward to catching up on this.

**Built tailored resume:** `resume/BrianTakita-eci-solutions.pdf`

The profile emphasizes:
- Agentic systems and agent tooling (agent-doc, corky, agent-kit)
- AI Products Shipped section (their core interest)
- Legacy modernization experience (SocialChorus Backbone→Svelte/React, CrystalCommerce Rails migration)
- C#/.NET bolded in skills (NASPP experience included for .NET/CRM background)
- Context engineering and spec-driven development
- Full career breadth showing 20+ years

### Draft LinkedIn reply:

Hi Justin,

No worries at all! Here's my resume — I've highlighted the agentic systems and AI pipeline work that maps closest to what you described.

A few things that might be particularly relevant for the CTO conversation:
- agent-doc is a production tool where AI agents write and maintain code through document-as-interface sessions — exactly the pattern ECI is building toward
- At Presence AI, I defined the agentic development methodology the engineering team used to ship a real-time multimodal pipeline
- I have experience modernizing legacy systems (Backbone→Svelte/React at SocialChorus, Rails migrations, .NET at NASPP)

I'm available:
- Tuesday 3/25: morning or afternoon ET
- Wednesday 3/26: afternoon ET

Happy to work around your schedule. Looking forward to it!

Brian

**Notes:**
- Connects agent-doc directly to "agents that write and maintain code" — their stated goal
- Mentions legacy modernization since that's the actual work (20-year-old ERP systems)
- Gives specific availability windows rather than "I'm flexible"
- Keeps it concise — Justin already knows the background from the first exchange
Where is the resume?
### Re: Where is the resume?

The resume PDF is at: `resume/BrianTakita-eci-solutions.pdf`

Open it [file:///home/brian/work/btakita/agent-loop/resume/BrianTakita-eci-solutions.pdf](file:///home/brian/work/btakita/agent-loop/resume/BrianTakita-eci-solutions.pdf)

Also — typo in the draft: "afternoo ET" → "afternoon ET" on the Wednesday line. Fix before sending.

To rebuild if you edit the profile: `make -C resume PROFILE=eci-solutions`

> exactly the pattern ECI is building toward

This needs to be supported.
I could mention dashboard as interface with bi-directional real-time updates in the markdown using CRDT
Along with human-agent align orchestration
### Re: Strengthening the agent-doc bullet

Good call. "Exactly the pattern ECI is building toward" is a claim without evidence. Revised bullet:

> agent-doc is a production tool where AI agents write and maintain code through a dashboard-as-interface model — bi-directional real-time updates in markdown documents using CRDT, so human and agent edits merge without conflicts

This makes it concrete: names the mechanism (CRDT, bi-directional updates) and shows how it maps to their problem (agents writing code while humans review/steer in real time).

**Updated draft reply:**

Hi Justin,

No worries at all! Here's my resume — I've highlighted the agentic systems and AI pipeline work that maps closest to what you described.

A few things that might be particularly relevant for the CTO conversation:
- agent-doc is a production tool where AI agents write and maintain code through a dashboard-as-interface model — bi-directional real-time updates in markdown using CRDT, so human and agent edits merge without conflicts
- At Presence AI, I defined the agentic development methodology the engineering team used to ship a real-time multimodal pipeline
- I have experience modernizing legacy systems (Backbone→Svelte/React at SocialChorus, Rails migrations, .NET at NASPP)

I'm available:
- Tuesday 3/25: morning or afternoon ET
- Wednesday 3/26: afternoon ET

Happy to work around your schedule. Looking forward to it!

Brian
### Re: Adding human-agent alignment orchestration

Incorporated. Updated bullet now covers both the technical mechanism (CRDT, bi-directional) and the orchestration story (human-agent alignment):

**Final draft reply:**

Hi Justin,

No worries at all! Here's my resume — I've highlighted the agentic systems and AI pipeline work that maps closest to what you described.

A few things that might be particularly relevant for the CTO conversation:
- agent-doc is a production tool where AI agents write and maintain code through a dashboard-as-interface model — bi-directional real-time updates using CRDT for conflict-free human-agent co-editing, with orchestration that keeps human and agent aligned throughout the development cycle
- At Presence AI, I defined the agentic development methodology the engineering team used to ship a real-time multimodal pipeline
- I have experience modernizing legacy systems (Backbone→Svelte/React at SocialChorus, Rails migrations, .NET at NASPP)

I'm available:
- Tuesday 3/25: morning or afternoon ET
- Wednesday 3/26: afternoon ET

Happy to work around your schedule. Looking forward to it!

Brian
Also mention Corky for the correspondence.
He is probably looking for something like OpenClaw for the enterprise.
agent-doc, corky, and a well-defined domain language system (existence-lang) bring human-agent alignment to agent orchestration.
### Re: Adding corky + existence-lang to the pitch

Good instinct — the three tools together tell the story of a complete agentic infrastructure stack, not just a single tool. Revised to add a bullet about corky and the domain language layer:

**Final draft reply (v3):**

Hi Justin,

No worries at all! Here's my resume — I've highlighted the agentic systems and AI pipeline work that maps closest to what you described.

A few things that might be particularly relevant for the CTO conversation:
- agent-doc is a production tool where AI agents write and maintain code through a markdown-as-interface model — bi-directional real-time updates using CRDT for conflict-free human-agent co-editing, with orchestration that keeps human and agent aligned throughout the development cycle
- corky is a git-native correspondence system where agents manage email, Telegram, and Slack threads with per-contact context
- Both tools share a domain language layer (existence-lang) that gives agents and humans a consistent vocabulary across systems — critical for maintaining coherence when multiple agents operate across a large codebase
- At Presence AI, I defined the agentic development methodology the engineering team used to ship a real-time multimodal pipeline

I'm available:
- Tuesday 3/25: morning or afternoon ET
- Wednesday 3/26: afternoon ET

Happy to work around your schedule. Looking forward to it!

Brian

> the kind of structured agent orchestration that scales across an enterprise

I don't need to oversell this.
Let's focus on things like live audio/video notes to capture insights during development or knowledge work session

**Notes:**
- Now tells the story of a platform (agent-doc + corky + existence-lang), not just one tool
- corky shows agents handling real-world communication, not just code — relevant for ERP where data flows across departments
- existence-lang as "domain language layer" maps directly to ERP modernization: you need shared semantics when agents touch 20-year-old systems
- Keeps Presence AI as the production credibility anchor
### Re: Toning down the pitch, focusing on practical capabilities

Agreed — no overselling. Revised corky bullet to focus on the practical: voice transcription, capturing insights during work sessions, not enterprise scaling.

**Final draft reply (v4):**

Hi Justin,

No worries at all! Here's my resume — I've highlighted the agentic systems and AI pipeline work that maps closest to what you described.

A few things that might be particularly relevant for the CTO conversation:
- agent-doc is a production tool where AI agents write and maintain code through a markdown-as-interface model — bi-directional real-time updates using CRDT for conflict-free human-agent co-editing, with orchestration that keeps human and agent aligned throughout the development cycle
- corky is a git-native correspondence system with Whisper-powered voice transcription — agents capture insights from live audio/video during development sessions and route them to the right context automatically
- Both tools share a domain language layer (existence-lang) that gives agents and humans a consistent vocabulary across systems — critical for maintaining coherence when multiple agents operate across a large codebase
- At Presence AI, I defined the agentic development methodology the engineering team used to ship a real-time multimodal pipeline

I'm available:
- Tuesday 3/25: morning or afternoon ET
- Wednesday 3/26: afternoon ET

Happy to work around your schedule. Looking forward to it!

Brian

> when multiple agents operate across a large codebase

I want to focus both on code and data context. Correspondence, documents, events, etc.
I also want to emphasize capturing insights over voice & video using diarized transcription.
### Re: Broadening context and emphasizing insight capture

Updated the existence-lang bullet to cover code + data context (correspondence, documents, events), and sharpened the corky bullet around insight capture.

**Final draft reply (v5):**

Hi Justin,

No worries at all! Here's my resume — I've highlighted the agentic systems and AI pipeline work that maps closest to what you described.

A few things that might be particularly relevant for the CTO conversation:
- agent-doc is a production tool where AI agents write and maintain code through a markdown-as-interface model — bi-directional real-time updates using CRDT for conflict-free human-agent co-editing, with orchestration that keeps human and agent aligned throughout the development cycle
- corky captures insights from live audio/video via Whisper transcription and routes them into git-native context — correspondence, documents, events all flow into agent-accessible knowledge alongside code
- Both tools share a domain language layer (existence-lang) that gives agents and humans a consistent vocabulary across code and data contexts — correspondence, documents, events, not just source files
- At Presence AI, I defined the agentic development methodology the engineering team used to ship a real-time multimodal pipeline

I'm available:
- Tuesday 3/25: morning or afternoon ET
- Wednesday 3/26: afternoon ET

Happy to work around your schedule. Looking forward to it!

Brian
### Re: Diarized transcription emphasis

Updated corky bullet to specifically mention diarized transcription (speaker-attributed insights from voice/video):

**Final draft reply (v6):**

Hi Justin,

No worries at all! Here's my resume — I've highlighted the agentic systems and AI pipeline work that maps closest to what you described.

A few things that might be particularly relevant for the CTO conversation:
- agent-doc is a production tool where AI agents write and maintain code through a markdown-as-interface model — bi-directional real-time updates using CRDT for conflict-free human-agent co-editing, with orchestration that keeps human and agent aligned throughout the development cycle
- corky captures insights from voice and video using diarized Whisper transcription, then routes them into git-native context — correspondence, documents, and events all flow into agent-accessible knowledge alongside code
- Both tools share a domain language layer (existence-lang) that gives agents and humans a consistent vocabulary across code and data contexts — correspondence, documents, events, not just source files
- At Presence AI, I defined the agentic development methodology the engineering team used to ship a real-time multimodal pipeline

I'm available:
- Tuesday 3/25: morning or afternoon ET
- Wednesday 3/26: afternoon ET

Happy to work around your schedule. Looking forward to it!

Brian
Corky's primary function is to be a git-native tool to enhance correspondence, social media, and audio/video media for knowledge capture & expression. Start with something like that. Rephrase for clarity and impact.
### Re: Reframing corky bullet

Reframed around corky's actual identity — git-native correspondence, social media, and audio/video media for knowledge capture & expression:

**Final draft reply (v7):**

Hi Justin,

No worries at all! Here's my resume — I've highlighted the agentic systems and AI pipeline work that maps closest to what you described.

A few things that might be particularly relevant for the CTO conversation:
- agent-doc is a production tool where AI agents write and maintain code through a markdown-as-interface model — bi-directional real-time updates using CRDT for conflict-free human-agent co-editing, with orchestration that keeps human and agent aligned throughout the development cycle
- corky is a git-native toolkit for correspondence, social media, and audio/video media — it turns conversations, voice notes, and video into structured knowledge that agents can act on, with diarized transcription and per-contact context
- Both tools share a domain language layer (existence-lang) that gives agents and humans a consistent vocabulary across code and data contexts — correspondence, documents, events, not just source files
- At Presence AI, I defined the agentic development methodology the engineering team used to ship a real-time multimodal pipeline with live video avatars, audio avatars, text avatars, and image generation

I'm available:
- Tuesday 3/25: morning or afternoon ET
- Wednesday 3/26: afternoon ET

Happy to work around your schedule. Looking forward to it!

Brian
### Re: Expanded Presence AI bullet

Good addition — specifying the multimodal outputs (live video avatars, audio avatars, text avatars, image generation) makes the pipeline concrete rather than abstract. Shows breadth of production AI experience.

Draft v7 is now final with this edit incorporated.
Does the resume need to be updated to support the message?
Any gaps in the resume?
### Re: Resume gaps analysis

Checked the resume against the draft reply. Two gaps:

**1. existence-lang not named in resume**
The resume mentions "philosophy" and "domain ontology system (extensible)" but never says "existence-lang". The draft reply uses "existence-lang" — Justin/CTO won't find that term in the resume. Options:
- (a) Add existence-lang to the resume's Open Source section
- (b) Use "domain ontology" in the reply instead of "existence-lang"
- (c) Both

C
Update data.toml

**2. Presence AI — "avatars" and "image generation" not in resume**
The resume says "text, audio, and video lip-sync on NVIDIA H100 GPUs via WebRTC". The draft reply says "live video avatars, audio avatars, text avatars, and image generation." The resume doesn't use "avatar" or mention image generation. Should update the resume to match.

Yes. Also update data.toml

**3. Highlight phrases could be stronger**
The ECI profile doesn't bold "diarized transcription", "social media posting", or "image generation". Adding these to the profile's highlight phrases would make the resume visually reinforce the reply's claims.

Yes

**No gaps on:**
- corky (diarized transcription, social media, correspondence — all covered)
- agent-doc (CRDT, streaming merge, document-as-interface — all covered)
- Legacy modernization (SocialChorus, CrystalCommerce, NASPP — all included)

**Recommendation:** Update the resume to close gaps 1-3, then rebuild the PDF. Want me to proceed?
go
### Re: Resume updates complete

Done. Updated all three files and rebuilt the PDF:

**Changes:**

1. **BrianTakita.md** (master resume):
   - Summary: "domain ontology system (existence-lang, 2014)"
   - Career Highlights: "existence-lang" replaces "extensible", adds "consistent vocabulary across code, correspondence, and data contexts"
   - Presence AI highlight: "live video avatars, audio avatars, text avatars, and image generation"
   - AI Products Shipped: Presence AI entry expanded with avatar types + image generation
   - Open Source: "philosophy" → "existence-lang" with updated description

2. **data.toml**:
   - Philosophy entry → existence-lang with expanded description
   - Multimodal pipeline entry adds avatars + image generation

3. **profiles/eci-solutions.toml** — added highlight phrases:
   - `live video avatars`, `audio avatars`, `text avatars`, `image generation`
   - `existence-lang`, `consistent vocabulary`
   - `diarized transcription`, `social media posting`

**Rebuilt:** `resume/BrianTakita-eci-solutions.pdf`

Open it: [file:///home/brian/work/btakita/agent-loop/resume/BrianTakita-eci-solutions.pdf](file:///home/brian/work/btakita/agent-loop/resume/BrianTakita-eci-solutions.pdf)

The draft reply should also use "domain ontology (existence-lang)" instead of bare "existence-lang" — updated final draft:

**Final draft reply (v8):**

Hi Justin,

No worries at all! Here's my resume — I've highlighted the agentic systems and AI pipeline work that maps closest to what you described.

A few things that might be particularly relevant for the CTO conversation:
- agent-doc is a production tool where AI agents write and maintain code through a markdown-as-interface model — bi-directional real-time updates using CRDT for conflict-free human-agent co-editing, with orchestration that keeps human and agent aligned throughout the development cycle
- corky is a git-native toolkit for correspondence, social media, and audio/video media — it turns conversations, voice notes, and video into structured knowledge that agents can act on, with diarized transcription and per-contact context
- Both tools share a domain ontology layer (existence-lang) that gives agents and humans a consistent vocabulary across code, correspondence, and data contexts
- At Presence AI, I defined the agentic development methodology the engineering team used to ship a real-time multimodal pipeline with live video avatars, audio avatars, text avatars, and image generation

I'm available:
- Tuesday 3/25: morning or afternoon ET
- Wednesday 3/26: afternoon ET

Happy to work around your schedule. Looking forward to it!

Brian
Help me prepare for the interview tomorrow morning.
https://www.ecisolutions.com/
### Re: Interview Preparation — ECI Solutions

Here's your comprehensive interview prep for tomorrow morning.

---

## Company Deep Dive

**ECI Software Solutions** — 30-year-old cloud ERP provider for SMBs. 25,000+ customers, 90+ countries, ~2,100 employees, HQ in Westlake/Fort Worth, TX. Backed by Leonard Green & Partners (majority, Feb 2025), Apax Partners (minority co-control), and GIC (Singapore sovereign wealth fund).

**Verticals:** Manufacturing, Residential Construction, Distribution, Building Supply, Office Technology, Field Service.

**Products:** Massive portfolio of acquired vertical ERPs — JobBOSS2, M1, Deacom (manufacturing), MarkSystems/Lasso CRM (construction), DDMSPLUS/EvolutionX (distribution), Spruce/RockSolid MAX (building supply), e-automate/Printanista (office tech), Davisware (field service, acquired April 2025).

**Tech stack:** C#/.NET, JavaScript, Python, cloud-first. Job postings mention OpenAI, Azure OpenAI, and **Anthropic APIs**.

---

## The AI Strategy (Critical Context)

ECI has a **three-phase AI roadmap:**

| Phase | Description | Status |
|-------|-------------|--------|
| 1. Automated Awareness | Rule-based systems, dashboards, alerts | Done |
| 2. Assisted Intelligence | AI as intelligent assistant, human oversight | Current |
| 3. Fully Agentic Operations | AI as trusted strategic teammate, performs routine tasks, escalates when human judgment needed | **Building now — this is the role** |

Their mantra is **"Practical AI"** — embedded in existing workflows, not flashy demos. CEO Gruenewald: *"AI is most powerful when it's practical, secure, and embedded in the workflows our customers rely on."*

**Recent AI moves:**
- Ecommerce AI Agent launched (Nov 2025) — built into EvolutionX
- FormedAI partnership for AP automation (Feb 2026) — cutting manual accounting by 80%
- AI-driven Homebuilder Sales & Marketing Suite (Feb 2026)
- Frost & Sullivan named them North American ERP Product Leader for SMB manufacturing
- AI Readiness Report (March 2026) — 550+ SMB leaders surveyed; 60% want AI for data analysis/reporting, top barrier is lack of in-house expertise

---

## CTO Jack Wood (Your Ultimate Audience)

**Appointed March 3, 2026** — hired specifically to "scale practical AI innovation."

**Background:**
- **NASA JPL** and **Bell Labs** (early career)
- **Capital One** — engineering leadership
- **Wayfair** — engineering through hyper-growth ($450M → $2.8B revenue)
- **Symetra** — SVP/Head of Engineering, 350+ engineers, platform modernization and AI/automation

**His philosophy:** *"We've proven how practical AI can create real value for customers"* — aims to scale workflow-native AI across the entire portfolio.

**What this means for you:** Jack understands platform modernization at massive scale. Speak his language about engineering infrastructure, tooling, and practical AI that ships.

---

## Justin Caird (Tomorrow's Call)

**Role:** Sr. Technical Recruiter & Co-Owner at ProSearch (Portland, ME). Working directly with CTO Jack Wood.

**Background:** 10+ years in talent acquisition. Philips (6.5 years), Brightcove (Sr. Manager TA for Product Engineering), now ProSearch co-owner.

**Style:** Direct, transparent, relationship-oriented. Looks for passion and builder mentality over checkbox qualifications.

**Likely call structure (~30 min):**

### Questions He'll Probably Ask

**1. Motivation & Fit**
- "What drew you to this opportunity?"
- "Tell me about your experience with AI and agentic systems"
- "Do you run models locally? What tools do you use day-to-day?"

**2. Technical Depth (conversational)**
- "Walk me through what you've been building"
- "What's your experience with legacy system modernization?"
- "How do you think about agents writing and maintaining code in production?"

**3. Work Style & Culture**
- "How do you feel about shipping fast vs. perfection?" (his pitch: "ship daily, worry about perfection later")
- "Describe extensive ownership over a technical project"

**4. Logistics**
- Salary expectations / comp range
- Start date availability
- Other processes you're in

---

## Your Talking Points (Mapped to Their Needs)

### "Agents that write and maintain code" → agent-doc
- agent-doc IS this. Production tool where AI agents write and maintain code through markdown-as-interface with CRDT-based human-agent co-editing.
- Maps directly to Phase 3: "AI as trusted strategic teammate" with human oversight.
- Dashboard-as-interface = bi-directional real-time updates, conflict-free merging.

### "Modernize 20-year-old ERP systems" → Legacy modernization track record
- SocialChorus: Backbone → Svelte/React migration
- CrystalCommerce: Rails modernization
- NASPP: .NET/CRM systems
- The pattern: done this multiple times across different stacks and eras

### "True builders" → Your stack is the proof
- agent-doc (Rust + FFI + CRDT + JetBrains/VS Code plugins)
- corky (git-native correspondence, diarized transcription, audio/video knowledge capture)
- existence-lang (domain ontology — shared vocabulary for agents and humans)
- These aren't side projects — production infrastructure used daily

### "Practical AI" → Presence AI pipeline
- Defined agentic development methodology at Presence AI
- Shipped real-time multimodal pipeline: live video avatars, audio avatars, text avatars, image generation on NVIDIA H100 GPUs via WebRTC

### "They use Anthropic APIs" → Deep in the ecosystem
- agent-doc runs on Claude (Opus, Sonnet)
- You understand prompt architecture from the inside — context engineering, spec-driven development, tool-use patterns

---

## Questions You Should Ask

1. **"How is Jack thinking about the AI Foundry team structure? Centralized platform team or embedded across verticals?"** — Shows org design understanding.

2. **"What does 'Prompt Architect' mean in practice at ECI? Agent systems or prompt templates?"** — Clarifies scope.

3. **"Which systems are first for agentic modernization? M1? Deacom? e-automate?"** — Shows homework.

4. **"How does the team think about Phase 2 → Phase 3 transition?"** — Strategic thinking.

5. **"What's the tech stack for the AI layer? Model-agnostic or preference?"** — Ready to build.

---

## Potential Concerns to Address

| Concern | Your Response |
|---------|---------------|
| C#/.NET isn't primary | "I've worked with .NET at NASPP and SafeSchoolz. These roles are about building agents — patterns transfer across languages. I've shipped in Rust, TypeScript, Python, Ruby, and C#." |
| Solo builder vs. team | "At Presence AI I defined methodology for the engineering team. At SocialChorus I led architecture across multiple teams. agent-doc is solo, but corky and the toolchain are designed for team workflows." |
| Startup tools vs. enterprise | "Jack Wood scaled Wayfair from $450M to $2.8B. I've worked both ends — startups to enterprise (TrueCar, SocialChorus/Firstup). The agentic patterns I've built are designed to scale." |

---

## Day-of Checklist

- [ ] Have resume PDF open: [file:///home/brian/work/btakita/agent-loop/resume/BrianTakita-eci-solutions.pdf](file:///home/brian/work/btakita/agent-loop/resume/BrianTakita-eci-solutions.pdf)
- [ ] Have agent-doc demo ready to screen-share if asked
- [ ] Know the three-phase AI roadmap cold
- [ ] Know Jack Wood's background (JPL, Bell Labs, Wayfair, Symetra)
- [ ] Prepare 2-minute pitch: "Here's what I've been building and why it maps to what ECI needs"
- [ ] Have salary range ready

Please transcribe + diarize /home/brian/work/btakita/agent-loop/mail/contacts/justin-caird/recordings/20260324-justin-caird.amr
Use corky --gpu
### Re: Call Transcription (March 24, 2026)

Transcribed the ~63-minute screening call. Full transcript saved to: `mail/contacts/justin-caird/recordings/20260324-justin-caird.txt`

**Note:** Diarization not available — corky needs rebuild with `--features diarize`. Speakers are clearly distinguishable from context though.

---

#### Key Intel from the Call

**ECI Organization:**
- ~1,000 employees total, ~600 in engineering
- ~160 different ERP products (heavily customized by industry vertical)
- 20-year-old company, tons of acquisitions and legacy code ("spaghetti code")
- Remote-friendly, HQ in Texas

**AI Cohort Structure:**
- 20-25 open positions, split into 5-7 "pods"
- Each pod: 3-4 people (dev, QA, data, data science)
- 100% focused on building agents — nobody writes code manually
- Board mandate to aggressively accelerate AI transformation
- No tickets — they do "missions"
- Full autonomy: choose model, framework, agent architecture
- Ship if it passes eval — days, not weeks

**CTO Jack Wood:**
- Month 3 at ECI, brought ProSearch with him from 4 prior companies
- Direct line of sight from pod members to CTO
- Building exclusive leveling/grading system for this team (separate from rest of org)
- "AI obsessed" is the hiring filter — not language/framework expertise

**What They Want:**
- Agent-first thinkers (capabilities, not functions)
- Can demonstrate at least one autonomous agent or complex LLM chain
- Hate manual work — automate everything mindset
- Not intimidated by legacy code (see it as goldmine for AI refactoring)
- Production-grade thinking: evals, guardrails, adversarial/judge agents
- "AI hacks and custom automation scripts are celebrated as greatest contribution"

**Interview Process:**
1. **CTO vibe coding** — from a shared markdown file (pre-work strongly encouraged; top candidate did entire challenge + bonus tasks before interview)
2. **Agentic review** — spot hallucinations in AI-generated PR (80% correct, 20% dangerous flaws)
3. No manual coding, no whiteboard, no inverting binary trees

**Compensation:**
- Base + bonus (annual + project-based), no equity
- Mid-Senior: $170-190K base
- Lead/Principal: $200-230K+ base (potentially higher for principal)
- Justin positioned Brian at lead/principal level based on the call

**What Landed Well:**
- agent-doc as markdown-as-interface with CRDT — Justin said "that's really cool"
- YouTube dev recordings — "first time I've heard of a candidate doing that," wants to share with CTO
- Domain-driven design + existence-lang philosophy
- GPU setup (5090, previously 4090) — signals hardware investment
- Running Opus directly
- Genuine excitement about legacy code as knowledge extraction opportunity
- Human-agent alignment framing resonated

**Action Items:**
- [x] Send YouTube channel link to Justin
- [ ] Watch for email with job description, CTO insights, and markdown challenge file
- [ ] Prepare for vibe coding challenge (do pre-work before interview)
- [ ] Prepare for agentic review (hallucination spotting in PRs)
- Updated email: brian.takita@gmail.com
- Updated location: Thompson, CT

This conversation happened on Tuesday morning. It's already Thursday morning.
Should I follow up with Justin via LinkedIn?
I have my recent LinkedIn post with the video + blog post.
### Re: Follow-up timing

Yes — a follow-up now is well-timed. It's been ~48 hours, right in the window Justin mentioned ("24-48 hours"). He specifically asked for the YouTube channel and said the CTO would love to see your dev recordings.

Sharing the LinkedIn post with the video + blog post is a natural value-add rather than a "just checking in" message.

**Draft LinkedIn follow-up:**

> Hi Justin, great speaking with you Tuesday — really enjoyed learning about the AI cohort structure and the mission-based approach.
>
> Wanted to share a recent post with a video walkthrough and blog post on agent-doc's CRDT-based workflow: [link to LinkedIn post]
>
> Thought it might be useful context for the CTO conversation. Looking forward to next steps!

**Why this works:**
- References something specific from the call (shows you were paying attention)
- Delivers value (the video he asked for) rather than just "checking in"
- Short and doesn't pressure for a response
- The video demonstrates exactly what they're hiring for — agents writing code through markdown

Replace `[link to LinkedIn post]` with your actual LinkedIn post URL before sending.
Here is the post. I get into module-harness and existence-lang as well. Please assess the noteworthyness for the role.
https://www.linkedin.com/feed/update/urn:li:activity:7442567335229444096/?originTrackingId=NC47vVIFUVz1xLk5Us2qxg%3D%3D
### Re: LinkedIn post assessment for ECI relevance

**Highly relevant.** This post hits multiple signals Justin and the CTO are screening for:

**Direct role alignment:**
- **Module-level specs, contracts, and evals** — maps directly to their eval-driven culture ("if it passes eval, you ship"). ECI needs exactly this when agents touch 160 different ERP products
- **Rust CLI tool built in a live session** — "true builder" proof, not just someone who prompts
- **Ontology/existence-lang** — you discussed domain language extensively on the call and Justin engaged with it. This post shows it in action
- **2h50m live coding video** — Justin said he'd never seen a candidate record dev sessions. This is concrete evidence of "AI obsessed"

**What the CTO specifically will notice:**
- module-harness solves the problem of agents ignoring project-level specs — ECI's 160-product environment will need granular agent guidance per module/product
- The eval/contract/adversarial framing speaks their language (they're building adversarial agents, judge agents, eval-driven pipelines)
- Full toolchain demo (agent-doc + corky + Claude Code + tmux-router) = production infrastructure, not a weekend hack

**Verdict:** Include it. The post demonstrates exactly what they're hiring for — someone who builds agent infrastructure and thinks about production-grade agent governance.

**Updated follow-up draft:**

> Hi Justin, great speaking with you Tuesday — really enjoyed learning about the AI cohort structure and the mission-based approach.
>
> Wanted to share a recent post where I built module-harness during a live coding session — it adds specs, contracts, and evals at the module level so AI agents get granular guidance instead of just project-wide instructions. Relevant to ECI's multi-product environment: https://www.linkedin.com/feed/update/urn:li:activity:7442567335229444096/
>
> The video walkthrough and blog post are linked in the post. Looking forward to next steps!

Also, note the usage of deterministic software + SKILL context complimenting each other and being deployed as a Rust crate.
### Re: Deterministic software + SKILL context pattern

Good call highlighting this. The architecture pattern is itself a selling point:

- **Deterministic Rust crate** handles structured, repeatable operations (file scanning, comment parsing, validation)
- **SKILL context** (Claude Code skill) handles judgment — interpreting specs, generating contracts, evaluating quality
- **Deployed as a crate** — installable, versionable, distributable via crates.io

This is the exact pattern ECI needs at scale. With 160 products, you can't rely on prompts alone — you need deterministic tooling that enforces consistency while AI handles the nuanced judgment. The crate is the guardrail; the skill is the intelligence.

**Updated follow-up draft (final):**

> Hi Justin, great speaking with you Tuesday — really enjoyed learning about the AI cohort structure and the mission-based approach.
>
> Wanted to share a recent post where I built module-harness during a live coding session — it adds specs, contracts, and evals at the module level so AI agents get granular guidance instead of just project-wide instructions. The architecture pairs deterministic Rust tooling with AI skill context — the crate enforces structure, the agent handles judgment. Relevant to scaling across a multi-product environment like ECI's: https://www.linkedin.com/feed/update/urn:li:activity:7442567335229444096/
>
> Video walkthrough and blog post linked in the post. Looking forward to next steps!

Let Justin know that I'm working on some end-to-end eval tooling via module-harness and other general purpose eval tooling for my projects.

### Re: eval tooling update

Draft LinkedIn follow-up to Justin about eval tooling:

---

Hi Justin — quick update on something relevant to the eval/quality side of the conversation we had.

I've been building end-to-end eval tooling via module-harness — it generates specs, contracts, and evals at the module level so AI agents get granular, testable guidance rather than just project-wide instructions. I'm also working on general-purpose eval tooling across my projects that validates agent output against deterministic contracts.

This connects directly to the "hallucination spotting in AI-generated PRs" skill we discussed — the eval harness catches drift between what agents produce and what the contracts specify. Happy to demo if useful for the cohort's workflow.

---

Add a link to the my latest LinkedIn YouTube video post as you did in a previous revision of the message.
I just wanted to a 1-2 sentence mention the of my current eval work on the previous post revision.

Ready to send when you are. Want me to push this to a LinkedIn draft via corky?
I don't think corky can send LinkedIn messages without a partner agreement with LinkedIn. Is that correct?
If that is the case, then corky cannot support it until we get such an agreement.
So messages will have to be manual unless we automate this via the browser.

You're correct — corky cannot send LinkedIn messages. LinkedIn's messaging API requires a partner/developer agreement and OAuth app approval, which we don't have. There's no workaround via their public API.

**Current options:**
1. **Manual copy-paste** — I draft the message here, you paste it into LinkedIn (most reliable)
2. **Browser automation** — fragile; LinkedIn actively blocks automation scripts and changes selectors frequently
3. **Clipboard helper** — I copy the draft to your clipboard, you paste

Manual is the practical path for now. When you're ready to send the follow-up to Justin, I'll format it cleanly for copy-paste.

### Re: Add LinkedIn video link + eval mention to draft

The latest LinkedIn YouTube video post is: https://www.linkedin.com/feed/update/urn:li:share:7442567333950345217

Here's the revised follow-up draft with the link re-added and eval work woven in naturally:

---

Hi Justin — quick update on something relevant to the eval/quality side of our conversation.

I've been building end-to-end eval tooling via module-harness — specs, contracts, and evals at the module level so agents get granular, testable guidance. Here's a recent post with a live coding session and blog walkthrough showing it in action: https://www.linkedin.com/feed/update/urn:li:share:7442567333950345217

This connects directly to the "hallucination spotting in AI-generated PRs" skill we discussed — the eval harness catches drift between what agents produce and what the contracts specify. Happy to demo if useful for the cohort's workflow.

---

> This connects directly to the "hallucination spotting in AI-generated PRs" skill we discussed

I'm not directly addressing this atm. Just adding module-harness and coming up with a general purpose eval workflow.
I'm currently not creating PRs, but I will start.
Should we rephrase or keep it as is?

Changes from previous revision:
- Added the LinkedIn post link back in (live coding session + blog walkthrough)
- Kept the eval tooling mention concise (1-2 sentences as requested)
- Link appears mid-message as a value-add, not as the opener

Ready for manual copy-paste when you want to send it.

### Re: Rephrase hallucination spotting claim

Good call — let's keep it honest. You're building module-harness and general-purpose eval tooling, not PR hallucination spotting specifically. The revised draft drops that claim and describes your actual current work:

**Revised follow-up draft:**

---

Hi Justin — quick update on something relevant to the eval/quality side of our conversation.

I've been building end-to-end eval tooling via module-harness — specs, contracts, and evals at the module level so agents get granular, testable guidance. Here's a recent post with a live coding session and blog walkthrough showing it in action: https://www.linkedin.com/feed/update/urn:li:share:7442567333950345217

Building toward a general-purpose eval workflow that validates agent output against deterministic contracts — the kind of quality gate that scales across a multi-product environment. Happy to demo if useful for the cohort's workflow.

---

I sent the message.
Should I send another asking about the status?

**What changed:**
- Removed "hallucination spotting in AI-generated PRs" — not your current work
- Replaced with "general-purpose eval workflow that validates agent output against deterministic contracts" — accurate to what you're building
- Still connects to ECI's multi-product scaling needs
- Keeps the demo offer as a soft CTA

### Re: Follow-up status check

Message sent — good. On sending another asking about status: **hold off for now.** You just sent the eval tooling update, so a status check immediately after would feel pushy.

**Timing:** Wait until Monday March 30 (2-3 business days). If no response by then, a brief check-in is appropriate:

> Hi Justin — just checking in on next steps. Happy to work on the Markdown challenge whenever it's ready.

Short, low-pressure, references the specific next step he mentioned (challenge file + job description via email).

Please read the latest message from Justin on LinkedIn using chromium-bridge.
Also please include the LinkedIn page of the company where I will be talking to the CTO.
Also include the CTO's LinkedIn profile.

### Re: LinkedIn message check (Mar 28)

Read Justin's latest message via chromium-bridge. Here's the most recent exchange:

**Justin Caird** (4:41 PM):
> Thanks for sharing Brian. This is great. The CTO is coming back from travel next week. We should have times to interview soon. Stay tuned!

This was in response to your eval tooling / module-harness LinkedIn post. Positive signal — he's engaged and the CTO interview is imminent once travel wraps up (expected week of March 30).

**Re: "Also please include the"** — this edit appears truncated (cut off mid-sentence). Please complete the thought in your next edit and I'll act on it.

### Re: Company & CTO LinkedIn profiles (Mar 28)

**ECI Software Solutions** (ecisolutions.com)
- LinkedIn: https://www.linkedin.com/company/eci-software--solutions/
- Software Development | Westlake, Texas | 40K followers | 1K-5K employees
- "Your Business. Connected." — ERP solutions for manufacturing, building supply, field service industries

**CTO / CPTO — Brian Winters**
- LinkedIn: https://www.linkedin.com/in/brianwinters1
- Title: CPTO (Chief Product & Technology Officer) at ECi Software Solutions
- This is most likely who Justin is referring to when he says "working directly with the CTO"

**Also notable — Paxton Lamons**
- LinkedIn: https://www.linkedin.com/in/paxton-lamons-7b48522
- Title: CTO | Software Engineering VP/SVP | Platform Engineering VP/SVP
- Background includes SaaS Product Development, AWS, DevSecOps, Cloud Architecture, Application Modernization

Both have CTO-level titles. Brian Winters as CPTO is the more senior product+tech role. Paxton Lamons has the explicit CTO title with platform/modernization focus — which aligns well with the "modernize 20-year-old ERP systems" mission Justin described.

Justin mentioned that the new CTO was the CTO of Wayfair and some other companies. Please look at the phone transcript and the linked in message history.

**Re: CTO identification —** The CTO Justin mentioned who came from Wayfair is **Jack Wood**, already documented earlier in this file (lines ~450-463). He was appointed March 3, 2026 specifically to "scale practical AI innovation."

**Jack Wood's background chain:**
- NASA JPL → Bell Labs → Capital One → **Wayfair** (engineering through hyper-growth, $450M → $2.8B revenue) → Symetra (SVP/Head of Engineering, 350+ engineers) → **ECI Software Solutions** (CTO, March 2026)

**From the call transcript** (lines 588-590): Jack Wood is ~3 months into his role at ECI, brought ProSearch (Justin's firm) with him from 4 prior companies. Direct line of sight from pod members to CTO.

**From the LinkedIn messages** (`mail/contacts/justin-caird/20260322-linkedin-justin-caird.txt`): Justin describes working "directly with the CTO" to build out a strategic AI team to modernize 20-year-old ERP systems.

**Clarification on the other two profiles found earlier:**
- Brian Winters (CPTO) and Paxton Lamons (CTO) are separate executives at ECI found via LinkedIn search. Jack Wood is the specific CTO Justin is referring to — the one with the Wayfair background who is driving the AI transformation initiative and directly overseeing the hiring pods.

Does Jack Wood have a LinkedIn profile?

Go:
- [ ] Review Justin's LinkedIn message response about the eval tooling video


**Re: Jack Wood LinkedIn —** Yes. The profile matching his background (Cape Elizabeth, Maine; Wayfair tenure) is:

**https://www.linkedin.com/in/jack-wood-0339361/**

Note: LinkedIn's public snippets don't explicitly list the CTO title at ECI yet (he's only ~3 months in), but the profile location and Wayfair history match the person Justin described. The ECI company page is at https://www.linkedin.com/company/eci-software--solutions.

**Re: Review Justin's LinkedIn response about eval tooling video —**

This is already captured in the document (around line 805). Justin responded on March 28:

> "Thanks for sharing Brian. This is great. The CTO is coming back from travel next week. We should have times to interview soon. Stay tuned!"

**Status:** Positive signal. CTO Jack Wood is returning from travel the week of March 30. Interview scheduling expected soon. No follow-up action needed right now — wait for Justin to share interview times.
<!-- agent:boundary:1a21cb4e -->
<!-- /agent:exchange -->

<!-- agent:pending -->
- [x] Review Justin's LinkedIn message response about the eval tooling video → positive, CTO interview pending next week
- [x] Confirm CTO Jack Wood LinkedIn profile URL → https://www.linkedin.com/in/jack-wood-0339361/
- [ ] Prepare for CTO interview (Jack Wood returning from travel week of March 30)
<!-- /agent:pending -->
