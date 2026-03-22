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
- [ ] Review and send
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
Please draft a response to https://www.linkedin.com/in/jcaird/:

Want to ship daily and build the agents of the future? We're forming a team of AI experts and would love to chat.
Hey Brian,

I hope this message finds you well. Your background in Agentic AI is very impressive!

I'll cut to the chase as I'm sure you get a lot of messages from recruiters. I'm working directly with the CTO at ECI (ecisolutions.com), a leading ERP solutions company, to build out a very strategic team of AI experts and Prompt Architects stretching across, Development, QA, Data. These are not traditional coding roles, instead they're focused on building agents that will write and maintain the code.

We're searching for experienced engineers who are deeply passionate about AI, true builders (think running models locally, building agents to automate boring tasks just for fun, despise manual work/documentation and live inside LLMs/Claude/Cursor/Copilot) to modernize 20 year old ERP systems. This team will have extensive ownership, and be encouraged to move with speed, ship daily and worry about perfection later. If you're looking for a role with freedom and impact, this is it.

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
### Re: Resume updates complete (HEAD)

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
<!-- agent:boundary:d66a4bc3 -->
<!-- /agent:exchange -->

<!-- agent:pending -->
- Review final draft reply (v8) and updated resume PDF, then send via LinkedIn
<!-- /agent:pending -->
