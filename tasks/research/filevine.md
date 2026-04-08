---
agent_doc_session: aace927e-3f52-4e08-8677-ae851d9d9825
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Give me a 1 page pdf of research for filevine.com for an interview.

Help me prepare for an interview with the Filevine development team.
- https://www.filevine.com/
- https://www.linkedin.com/company/filevine
- Chris Smith
- Cain Elliott
- Stuart Hiller
- Emily Lippincott

### Re: Filevine Interview Prep

---

## Company Overview

**Filevine** (founded 2014, ~500 employees, HQ Salt Lake City UT) is a **legal work platform** — case management, document automation, contract lifecycle, billing, and AI — for law firms, government agencies, and in-house counsel. Scale: 5,000+ clients, 100,000 active users, 1.8B documents processed. Offices in Chicago, New York, Prague, Bratislava.

**Recent milestones:**
- Raised **$400M all-equity round** (Sept 2025)
- Achieved **FedRAMP 20x Authorization** (Oct 2025)
- Acquired **Pincites** (AI contract redlining for Word, Jan 2026)
- Launched **LOIS** (Legal Operating Intelligence System) at LEX Summit 2025

---

## Flagship Product: LOIS

LOIS is the core AI narrative right now — know it cold.

- **Built into the system of record**, not bolted on — unifies case management, docs, comms, billing, compliance, analytics, and AI
- **Intent-aware routing**: routes each query (summarize depositions, extract facts, calculate deadlines) to the best capability
- **Zero-retention AI**: no data retained by AI providers — enterprise/HIPAA/FedRAMP grade
- **Verifiable outputs**: every AI answer has source citations + excerpted evidence for attorney audit

Key tools under LOIS: **AskAI** (chat with a case file), **DraftAI** (document drafting), **AIFields** (auto-populate structured data), **Timely** (jurisdiction-specific deadline automation), **MedCron** (HIPAA-compliant medical chronology).

---

## Tech Stack

From job postings and engineering signals:

| Layer | Technologies |
|-------|-------------|
| Backend | Python, Node.js, C#/.NET |
| Frontend | React, TypeScript, Svelte |
| Mobile | React Native, Swift, Kotlin |
| Infra | AWS, Kubernetes |
| Orchestration | Temporal |
| Data/Search | MS SQL, Redis, Elasticsearch |
| Testing | Playwright, Groovy |
| Architecture | Microservices, SOC 2 / HIPAA / FedRAMP |

---

## Your Interviewers

**Chris Smith** — Principal Software Engineer
Engineering track. Expect systems design, Python/React/AWS, architecture tradeoffs. Senior IC role, so probably asking about technical depth and ownership.

**Stuart Hiller** — Full Stack Developer
Engineering track. Background: Node/Express/React/MongoDB/GraphQL. Previous work at Strategic Financial Solutions and LemonBrew. Likely assessing full-stack execution and code quality.

**Cain Elliott** — Chief Legal Futurist (PhD Philosophy, Polish Academy of Sciences)
*Not an engineer* — product/strategy/AI vision side. Former philosophy professor (UPenn, Warsaw), former CIO at Margolis Edelstein. Led the "Legal Futurists" team, spearheaded ImmigrationAI. Expect conversation about AI in legal, product philosophy, and "what does intelligent legal software look like?" Knows legal workflows deeply.

**Emily Lippincott** — Senior Legal Futurist
*Not an engineer* — JD attorney (Creighton Law 2005), 16+ years Silicon Valley, Monica Bay Women of Legal Tech award winner. Joined as Sales Engineer 2022, now Senior Legal Futurist. ABA Techshow speaker. Expect customer-centric product conversations and how tech maps to attorney workflows.

---

## Key Talking Points

1. **AI grounded in truth**: LOIS's design principle — verifiable outputs with citations. This matters in legal (malpractice risk from hallucination). Filevine differentiates on trustworthy AI.
2. **System of record first**: AI is only as good as the data it touches. Filevine owns the data layer, which is a competitive moat.
3. **Compliance as a feature**: FedRAMP, HIPAA, SOC 2 aren't afterthoughts — they're part of the product story for government/enterprise customers.
4. **Legal workflows ≠ generic SaaS**: Deadlines are jurisdictional, documents are privileged, billing is regulated. The domain complexity is why vertical SaaS wins here.

How can we use continuous evals for LOIS?
Is there a way to continuously the evals?
If the cost of failure is high, how does LOIS ensure there are no errors?

---

## Questions to Ask

- *What does a sprint cycle look like for LOIS features — how close is engineering to the legal domain experts?*
- *How does Filevine handle schema evolution for 1.8B documents across 5,000+ firm configurations?*
- *With FedRAMP authorization just landed, how is that shaping the product roadmap?*
- *What's the boundary between the Pincites acquisition and Filevine's existing DraftAI?*
- *How does the Legal Futurists team (Cain/Emily) interface with engineering on LOIS?*

---

*Sources: filevine.com, LinkedIn, LEX Summit 2025 announcements, Crunchbase, ZoomInfo, job postings (April 2026)*

How receptive would filevine be to agent-doc?

### Re: Filevine receptivity to agent-doc

Short answer: **low as a product fit, moderate as a conversation piece, high with the right people.**

**Why low as a product fit:**
Filevine is building LOIS — an AI-native legal platform for attorneys, not developers. Their customers use a web app, not a CLI. agent-doc's markdown/tmux/editor workflow is a dev tool with no natural path to a law firm's desktop. They're also deep into their own AI platform investment ($400M raised) — they're building, not buying dev tooling externally.

**Why moderate as a conversation piece:**
The *concept* behind agent-doc — document as the UI, AI responding to edits in a living document, preserving context across sessions — maps interestingly onto legal work. Legal documents are living artifacts: they accumulate annotations, revisions, cited sources. Filevine's DraftAI and AIFields are solving adjacent problems (AI-assisted drafting, structured extraction from case files).

Cain Elliott in particular (PhD philosopher, "Chief Legal Futurist") would likely find the session/context model intellectually interesting — he thinks about the future of legal workflows, not just current UI patterns.

**Why high with the right people:**
Chris Smith and Stuart Hiller are developers. If you frame agent-doc as "how I work with AI on complex, multi-session engineering tasks" — using it to demonstrate your own process — that's a credibility signal, not a pitch. Engineers who see you dog-fooding a tool you built are more interested than if you sell it abstractly.

**The interview angle:**
Don't pitch agent-doc to Filevine. Use it as evidence of how you think about AI tooling, then ask: *"How does your engineering team actually work with AI day-to-day — in the codebase, in PRs, in design docs?"* That reframes the conversation toward their workflow and signals you're someone who builds for real-world AI integration, not just demos.

What is the scope of work for lois? Does filevine support webrtc?

It seems that making the artifacts interactive is important in the case of lois. 
### Re: Continuous evals for LOIS + cost of failure

High-value interview angle — this is exactly what separates people who think about AI in production from people who just use it.

**How LOIS mitigates failure today:**
- **Verifiable outputs**: every AI response is paired with source citations and excerpted evidence. This pushes error detection to the attorney (human-in-the-loop) rather than catching it at inference time. It's pragmatic but not automatic.
- **Zero-retention AI**: reduces data leakage risk, but doesn't catch factual errors.
- **FedRAMP compliance**: process controls, audit trails — governance layer, not correctness layer.

**What continuous evals could look like for LOIS:**
- **Regression suite on known case facts**: fixed case file + fixed question → expected answer. Run on every model or prompt change. Catch regressions before they hit production.
- **Citation grounding checks**: for every AI response, verify the cited source actually supports the claim (automated RAG faithfulness scoring — RAGAS, TruLens, or custom).
- **Jurisdiction deadline accuracy**: Timely's deadline calculations are high-stakes. Evals with known deadline scenarios per jurisdiction — deterministic, automatable.
- **Adversarial inputs**: red-team prompts designed to elicit hallucinations or leak privileged info. Run nightly.
- **Human-labeled golden sets**: sample of real attorney queries + expert-validated correct answers. Measure drift over time.

**The deeper point for the interview:**
Legal AI's cost of failure is malpractice-shaped — a missed deadline or fabricated case cite can lose a client's case. That asymmetry means you want *defense in depth*: model-level (better grounding), output-level (citation verification), UX-level (clear uncertainty signals to attorneys), and process-level (attorney review before any AI output is acted on). No single eval catches everything.

Good question to ask Filevine: *"What does your eval pipeline look like for LOIS? Do you have automated regression suites, or is human review the primary quality gate?"*

---

### Re: Scope of LOIS + WebRTC

**LOIS scope** (what's confirmed):
- AskAI: RAG over case files
- DraftAI: document drafting + redlining (Word via Pincites)
- AIFields: structured data extraction from documents
- Timely: jurisdictional deadline calculation
- MedCron: medical chronology builder for personal injury/mass tort
- Deposition prep tools (specific AI features unclear)

Not in scope: anything outside case/document/billing workflows. LOIS is not a general assistant — it's scoped to legal matter management.

**WebRTC**: No confirmed information. Filevine has a deposition product, which *could* involve WebRTC for video depositions — but their deposition product is more likely built around document management + testimony tracking than live video hosting. That would typically integrate with Zoom/Teams rather than rolling their own WebRTC stack.

Worth asking directly in the interview if it's relevant to what you'd be working on: *"Does the platform have any real-time communication features — video, streaming, WebRTC?"*

---

### Re: Interactive artifacts + LOIS (incomplete thought?)

Your note — *"It seems that [?] make the artifacts interactive. In the case of LOIS..."* — looks like it got cut off. What were you thinking?

My best guess at the direction: LOIS produces *static* outputs (drafted documents, summaries, extracted fields) but the artifact itself isn't interactive — you can't ask follow-up questions about a specific paragraph inline, for instance. That's a gap. Contrast with tools like Cursor or agent-doc where the document *is* the interface. If that's the direction you were going, it's a genuinely interesting product observation: LOIS generates artifacts, but attorneys can't currently converse *with* the artifact in context.

Would the existence-lang ontology be important for lois?
### Re: Interactive artifacts — confirmed direction

Yes — making artifacts interactive is the key gap. LOIS generates outputs (summaries, drafted documents, extracted fields, medical chronologies) but those artifacts are endpoints, not starting points. An attorney receives the output, then has to context-switch to a different tool or conversation to ask follow-ups.

The more powerful model: the artifact *is* the interface. Click on a sentence in a DraftAI document → ask "why did you word it this way?" or "what's the source for this?" inline. Annotate a MedCron entry → get the AI to expand it. This is where agent-doc's design pattern is ahead — the document accumulates context over a session rather than resetting each time.

For the interview with Cain or Emily: *"LOIS generates artifacts — what's the plan for making those artifacts conversational rather than terminal?"* That's a sharp question that signals you've thought beyond the current product.

---

### Re: Existence-lang ontology for LOIS

Potentially yes — especially given that Cain Elliott has a philosophy background (PhD from the Polish Academy of Sciences). He's exactly the person who'd engage with formal ontology thinking in legal AI.

**Where existence-lang maps onto LOIS's actual problems:**

*Entities and Scope.* Legal work is fundamentally about entities and their relationships: parties, matters, documents, claims, events, deadlines, jurisdictions. LOIS currently extracts these via AIFields (unstructured → structured), but without a formal ontology governing what entities exist and how they relate, the extractions are schema-dependent and brittle across different firm configurations.

An existence-lang-style ontology would let you define `LegalMatter → [Parties, Claims, Evidence, Deadlines, Jurisdiction]` as a domain model, then use that model to guide extraction, query routing, and verifiable output generation.

*Context and Resolution.* The Scope → Context → Resolution chain maps directly onto what LOIS needs for good RAG. "Scope" = what documents are in this case. "Context" = what's relevant to this query within those documents. "Resolution" = how detailed does the answer need to be (case summary vs. paragraph-level citation). Without a formal model for these, intent routing is heuristic.

*Verifiable outputs.* The hardest problem in legal AI. If you have a formal ontology of case facts (Entity × Claim × Evidence × Confidence), you can check AI outputs against the ontology rather than against raw text. "The AI claimed X — does the ontology's evidence graph support X?" This turns verification from citation-lookup into formal inference.

**The practical pitch to Cain:**
"Your verifiable outputs feature checks AI answers against source citations — that's document-level grounding. An ontology layer would give you claim-level grounding: a formal graph of what's true in this case, so you can verify not just 'did you cite something' but 'is the citation sufficient for the claim.'"

That's a conversation Cain would find genuinely interesting, and it's differentiated from what most engineers would bring to that interview.

Please create a 1 page pdf summary for this interview.
Use small margins.
### Re: 1-page PDF

Created: `tasks/research/filevine-interview.html`

Open in browser and print to PDF (Ctrl+P → Save as PDF). Set margins to "None" or "Minimum" — the HTML has its own `@page` margins at 0.4in.

Content fits one letter page: company overview, milestones, LOIS, tech stack table, all four interviewers (with track labels), key talking points, and questions to ask.

❯ transcribe + diarize [2026-04-07 16-00-17-compressed.mp4](../../../../../Videos/2026-04-07%2016-00-17-compressed.mp4)
### Re: Transcription (HEAD)

Transcript written to [`tasks/research/transcripts/filevine-interview-2026-04-07.md`](transcripts/filevine-interview-2026-04-07.md) — 71:19, 893 segments, transcribed in 30s (144x realtime).

**Diarization caveat:** Only 4 speaker segments / 2 labels produced. All participants were physically in the same room sharing a single audio channel, so voice separation collapsed — the entire middle of the interview landed as one "Brian" block that actually contains Cain's introductions + everyone's dialogue. The text is accurate; the speaker labels are not.

**What the transcript captures:**

Cain opened by introducing the team — confirming their roles match our prep:
- Stuart → Principal Developer, Futurist team (taking Chris's old IC role)
- Emily → Senior Legal Futurist
- Chris Smith → leading the engineering squad ("four deployed engineers")
- Cain himself MC'd

Brian's intro covered: agentic loops/harnesses, human-agent alignment via ontologies, agent-doc as document-centric AI UI, workflow automation (transcription → blog posts). Landed well — Cain's framing confirms the team is doing exactly the kind of work the prep notes predicted.

**Next:** If you want a summary of the full conversation, a follow-up email draft, or notes from specific parts of the transcript, just ask.
<!-- agent:boundary:9b67bc29 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
