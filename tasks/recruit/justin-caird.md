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
<!-- agent:boundary:a2d1d804 -->
<!-- /agent:exchange -->

<!-- agent:pending -->
- Review final draft reply and resume PDF, then send via LinkedIn
<!-- /agent:pending -->
