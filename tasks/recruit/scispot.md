---
agent_doc_session: ea31c351-1a7f-4e41-8afa-ebae6e83efbb
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->

<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

Please help me prepare for an interview. Add the scispot company. https://www.scispot.com/ https://www.linkedin.com/company/scispot-com/
Add satya-singh profile https://www.linkedin.com/in/satyaravisingh/

### Re: Scispot — company profile — opus-4-8

**Scispot** — "The Operating System for the Lab of the Future" (YC S21)

AI-native lab informatics platform that replaces the fragmented stack of disconnected instruments, spreadsheets, reports, and legacy LIMS with one operating layer. It captures lab context automatically and turns physical lab behavior into structured, traceable data that both research teams and AI agents can act on.

**Company facts**
- Founded 2020, HQ in Kitchener, Ontario (Canadian company); Y Combinator S21
- **$8M Series A** led by **Avenue Growth Partners**, announced **June 9, 2026** (very recent — bring it up)
- ~23 employees; funding is earmarked to scale product, engineering, **AI**, implementation, and customer-success teams, with a focus on high-skill roles in Canada
- CEO/founder: **Guru Singh**; Co-founder & Chief Product/Operating Officer: **Satya Singh** (your interviewer)

**Scale / traction**
- 100+ enterprise labs across biotech, pharma, diagnostics, genomics
- 250+ instrument profiles supported; millions of production samples managed
- 50+ named industry/academic customers

**Product surface (the "alt-" suite + LabOS)**
- **LabOS™** — unified data & workflow operating system
- **alt-ELN** — electronic lab notebook (wet + dry labs)
- **alt-LIMS** — no-code-configurable LIMS
- **alt-LIS** — lab information system with API
- **alt-QMS** — quality/compliance management
- **alt-SDMS** — data-staging lakehouse
- **GLUE** — integration toolkit connecting apps + instruments, harmonizing data
- **Scispot AI** — lab assistant for data queries & workflow recommendations
- Enabling layer: Labflows (automation), Labsheets (scientific DBs), Labspace (collaborative design), Computational Toolkit (APIs, Jupyter, R Studio, GPT-4), Connectors & Agents (instrument integration)

**Why it matters for the interview:** the strategic thesis is *agents over a structured lab data layer*. Recent launches (e.g. a universal lab-instrument agent, a multi-well-plate agent on YC) show the direction: instrument integration + automation + LLM agents on top of clean, traceable data.

### Re: Satya Singh — interviewer profile — opus-4-8

**Satya Ravi Singh** — Co-Founder & Chief Product / Operating Officer, Scispot
- LinkedIn: https://www.linkedin.com/in/satyaravisingh/
- Based in Seattle, US
- Self-description: *"When he isn't out exploring the world, Satya builds products loved by humans and AI alike."* — product-led, AI-forward framing
- Prior experience at **Chevron, Expedia Group, Virgin Atlantic** — i.e., large-scale data/product orgs, not a pure-bio background, so he'll speak the language of product + platform engineering
- Hosts/features on Scispot content (e.g. the "redefining lab software" podcast episode #75) — he's the external product voice of the company
- Leads the product org; direct reports include a Founding Product Designer and a Growth Manager

**How to read him:** product + ops co-founder who cares about (1) products humans *and* AI agents love, (2) instrument/data integration done cleanly, (3) shipping velocity at a 23-person company. Expect questions on judgment, ownership, and how you'd reduce friction for scientists — not just raw coding trivia.

### Re: Interview prep — talking points & questions — opus-4-8

**Lead with these talking points**
1. **Agents on a structured data substrate** — Scispot's bet is that labs run on an operating layer where every sample/run/result/approval is connected as work happens. If you have built data-pipeline, integration, or LLM-agent systems, frame your work as "turning messy real-world signals into structured, traceable data that agents can act on" — that's their exact thesis.
2. **Integration breadth (GLUE)** — connecting heterogeneous instruments/apps and harmonizing data is core. Map your experience with connectors, protocol/format normalization, or API integration onto this.
3. **No-code configurability** — alt-LIMS/alt-ELN win by letting labs self-configure. Talk about designing systems that non-engineers can extend safely.
4. **Compliance/traceability** — life sciences = audit trails, QMS, regulated data. Even lightweight familiarity (versioning, provenance, immutable logs) is a plus.
5. **Small-team velocity** — at 23 people post-Series A, they need high-ownership builders who ship across the stack. Emphasize end-to-end delivery and pragmatism over specialization.

**Smart questions to ask Satya**
- "Post the $8M raise, where are you investing first — deeper instrument integration, the AI agent layer, or implementation scale?"
- "How do you draw the line between what the platform configures (no-code) and what needs engineering? Where does that boundary cause the most pain today?"
- "What does 'products loved by AI agents' mean concretely in the roadmap — agent-callable APIs, structured event streams, something else?"
- "What's the hardest data-harmonization problem GLUE faces across 250+ instrument profiles?"
- "What does the first 90 days look like for this role, and what would make you say a hire is clearly working out?"

**Homework before the call**
- Skim Scispot's blog/YC launches on their instrument agents (universal lab-instrument agent, multi-well-plate "Plato" agent) so you can reference the product direction by name.
- Be ready to articulate one concrete system you built where you normalized messy inputs into a structured model that downstream automation/AI consumed — that's the through-line they'll recognize.
- **Heterogeneous integration** — Plato harmonizes 4+ instrument formats (their GLUE story). If you've normalized disparate input formats/protocols, frame it as the same harmonization problem.
- **Automated QC / never-corrupt** — Plato flags well anomalies in real time; agent-doc's CRDT merge guarantees a human edit is never silently dropped and every cycle is traceable in git. That maps straight onto their **traceability / QMS** value and Satya's "loved by humans *and* AI agents" line.
- **Messy input → structured substrate → agent-actionable** — their exact thesis; agent-doc is your end-to-end proof you've built it.
- **Command surface over a live artifact** — Plato: backslash over a plate; agent-doc: slash over a document. Same "the artifact *is* the UI" instinct.
- **Interface** — "**backslash commands**" to open/adjust layouts, set rows & columns, and color-code wells by sample type, plus **Python scripts** for custom analysis.
- **Run** — real-time monitoring with **predictive error detection / automated QC** flagging anomalies (out-of-range fluorescence / absorbance / Ct values).
- **Integrate** — automated data flow from the major plate instruments: **BMG Labtech Clariostar, Tecan Fluent, Hamilton Microlab STAR, Applied Biosystems qPCR**.
- **Load** — AI-driven sample assignment with "balanced distribution of samples and precise control placements."
- **Design** — generates custom plate layouts (96/384-well) for specific assays (ELISA, qPCR, cytotoxicity, cell-viability), with duplicate/triplicate wells and randomized configs.

I've captured the company + interviewer profiles above. I filed a recommended follow-up to create a Corky contact for Satya (matching the Royce Stewart / Greenlight Guru pattern) if you want CRM tracking.
❯ Tell me more about the "Plato" agent. agent-doc is one concrete system where I'm normalizing messy inputs into a structured model that downstream automation/AI consumes.


### Re: The "Plato" agent — verified details + your agent-doc bridge — opus-4-8

Pulled from Scispot's own Plato page + YC launch so you can name real capabilities, not hand-wave.

**What Plato is:** Scispot's **multi-well-plate agent** (part of the Scibot AI-agent family). It lets a wet-lab scientist **design → load → integrate → run** microplate workflows in natural language instead of wrangling spreadsheets:

- **Design** — generates custom plate layouts (96/384-well) for specific assays (ELISA, qPCR, cytotoxicity, cell-viability), with duplicate/triplicate wells and randomized configs.
- **Load** — AI-driven sample assignment with "balanced distribution of samples and precise control placements."
- **Integrate** — automated data flow from the major plate instruments: **BMG Labtech Clariostar, Tecan Fluent, Hamilton Microlab STAR, Applied Biosystems qPCR**.
- **Run** — real-time monitoring with **predictive error detection / automated QC** flagging anomalies (out-of-range fluorescence / absorbance / Ct values).
- **Interface** — "**backslash commands**" to open/adjust layouts, set rows & columns, and color-code wells by sample type, plus **Python scripts** for custom analysis.

**One accuracy note for the room:** the Plato page itself does *not* claim 21 CFR Part 11 (some secondary blurbs imply it). Compliance lives at the platform layer (alt-QMS / LabOS), not necessarily in Plato — so say "Scispot's platform is compliance-oriented" rather than pinning Part 11 on Plato. Satya will know the difference, and it signals you actually read the product.

**Your agent-doc bridge — and it's uncanny; lead with this.** Plato and agent-doc are the *same architecture*, almost line-for-line:

> "Plato turns a scientist's intent and messy instrument output into a structured plate model the platform and AI can act on — driven by backslash commands over a live layout. agent-doc is the same shape: it turns a person's messy, free-form markdown edits into a typed, conflict-free structured model — components, a work queue, an exchange log — that downstream agents drain and act on, driven by slash commands over a live document, with every change git-committed for provenance."

Parallels that will land with a product + platform co-founder:

- **Command surface over a live artifact** — Plato: backslash over a plate; agent-doc: slash over a document. Same "the artifact *is* the UI" instinct.
- **Messy input → structured substrate → agent-actionable** — their exact thesis; agent-doc is your end-to-end proof you've built it.
- **Automated QC / never-corrupt** — Plato flags well anomalies in real time; agent-doc's CRDT merge guarantees a human edit is never silently dropped and every cycle is traceable in git. That maps straight onto their **traceability / QMS** value and Satya's "loved by humans *and* AI agents" line.
- **Heterogeneous integration** — Plato harmonizes 4+ instrument formats (their GLUE story). If you've normalized disparate input formats/protocols, frame it as the same harmonization problem.

Net: open with the *pattern* — messy → structured → agent-actionable, command-driven, traceable — then drop agent-doc as the concrete system where you built it. Plato gives you their vocabulary; agent-doc gives you your proof.

Sources: [scispot.com/plato](https://www.scispot.com/plato) · [YC launch: Plato](https://www.ycombinator.com/launches/MBx-plato-scispot-s-multi-well-plate-agent)
<!-- agent:boundary:6b20ddde -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-06-25 [#kesh] [recommended] Create Corky contact for Satya Singh (mail/contacts/satya-singh/AGENTS.md) — Co-Founder & CPO, Scispot, https://www.linkedin.com/in/satyaravisingh/ — matching the royce-stewart pattern, for CRM tracking.
<!-- /agent:done -->
