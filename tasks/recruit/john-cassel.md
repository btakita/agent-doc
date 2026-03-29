---
agent_doc_session: 9af37216-1e49-4cd5-b32a-d9b2da189ee5
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
Publication analysis complete. Draft reply pending approval.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Create a contact for john-cassel: https://www.linkedin.com/in/john-cassel-6480502/

Read LinkedIn message from john-cassel. Save message in mail/contacts/john-cassel. Use chromium-bridge via brave.

### Re: Contact setup & LinkedIn message

**Contact created:** `mail/contacts/john-cassel/`
- `AGENTS.md` — full profile with research, publications, communication timeline
- `CLAUDE.md` → symlink to AGENTS.md
- `20260314-linkedin-john-cassel.txt` — archived LinkedIn message

**Profile summary:**
- **Role:** Principal Software Engineer at Regrow Ag
- **Location:** Champaign, Illinois
- **Education:** Ontario College of Art and Design (OCAD University)
- **Background:** Discovery-driven domain designer, permaculture systems thinker
- **Previous:** Indigo (incentive programs for agricultural practices)
- **Publications:** 5 academic papers (Springer, Mathematica Journal, IGI Global) on permaculture as systemic design, probabilistic programming, risk governance, Wolfram|Alpha
- **Website:** https://johnbenjamincassel.github.io

**LinkedIn message (2026-03-14 4:58 PM):**
John responded to Brian's connection request. Key points:
- Accepted because of shared permaculture background
- Working on multifunctional landscapes — incentive programs at Indigo and Regrow
- Interested in transitioning to design/planning focused work
- Open to hearing about Brian's work in the area
- **Status:** No reply sent yet

**Strong alignment with Brian's interests:** systems design, ontology/knowledge representation, permaculture. His academic work on discovery processes and domain design resonates with Existence Lang concepts.

Draft a response

### Re: Draft a response

**Draft LinkedIn reply to John Cassel:**

> Hi John,
>
> Thanks for the thoughtful reply -- I'm glad you accepted the connection. Permaculture is definitely common ground for us.
>
> I came across your "Permaculture as a Systemic Design Practice" paper a while back & it stuck with me. The way you frame discovery processes and multi-criteria design resonates with how I think about systems too -- I've been building an ontology framework (Existence Lang) that tries to capture how entities, systems, and domains relate across different scopes & contexts. It started from a philosophical angle but increasingly drives the software I build.
>
> Right now I'm working on AI agent tooling -- correspondence automation, document sessions, browser orchestration -- all in Rust. The permaculture lens still shapes how I approach architecture though: stacking functions, designing for emergent behavior, thinking in terms of whole systems rather than isolated components.
>
> Your work on incentive programs at Regrow sounds fascinating -- shaping demand for new practices is such a leverage point. If you're moving toward design & planning, I'd love to hear more about what that looks like for you. Happy to jump on a call sometime if you're up for it.
>
> -- Brian

Ready to send via LinkedIn messaging when you approve. Let me know if you'd like any edits.

Please analyze "Permaculture as a Systemic Design Practice" and report back here. Let me know about John's other publications as well.

### Re: Publication analysis

#### 1. "Permaculture as a Systemic Design Practice" (Springer, 2018)
*Co-authored with Susan V. Cousineau. Published in "Systemic Design" (Springer, Tokyo). Presented at RSD4 symposium.*

**Core argument:** Design discourse frequently frames design as a "science of the artificial," but we must also design our interaction with natural systems. Permaculture is a systemic design approach for developing permanent agriculture and settlements.

**Key contributions identified:**
- Simple-to-remember guiding ethics and principles
- A vocabulary of categories for discussing interactions between system elements
- A toolbox of design methods for selecting and assembling systems of elements
- Overall design processes for whole-system planning
- Agroecological and social system design insights

**Current challenges where systemic design can assist permaculture:**
- Forming stable objectives
- Assessing appropriate technology
- Stakeholder engagement
- Launching viable projects

**Relevance to Brian:** This is the most directly resonant paper. John frames permaculture as a *design practice* with systemic principles -- the same lens Brian applies to software architecture. The vocabulary of interactions and element assembly maps naturally to Existence Lang's entity/system/domain ontology.

---

#### 2. "The Methodological Unboundedness of Limited Discovery Processes" (FormAkademisk, 2014)
*Published in Vol. 7, No. 4. Special issue: "Relating Systems Thinking and Design II." DOI: 10.7577/formakademisk.755*

**Core argument:** Designers approach systems differently than scientists. Rather than discovering complete systems, design practitioners manage calculated risks through strategic intervention. The key insight: *we can suppose there might be an infinite number of factors, but we can reason about their concentration* -- enabling parametric analysis even when specific parameters remain unknown.

**Key concepts:**
- Design vs. scientific discovery -- designers stop when marginal returns diminish relative to costs
- The "Unbounded Reasoning Principle" -- reasoning about factor concentration without knowing each factor
- Extends parametric design frameworks to include discovery-based metrics
- Diminishing returns analysis across discovery activities

**Relevance to Brian:** The "unbounded reasoning" principle parallels how Existence Lang handles scope and resolution -- you don't need to model everything, just enough to operate coherently at the right resolution level.

---

#### 3. "Probabilistic Programming with Stochastic Memoization" (The Mathematica Journal, Vol. 16, 2014)
*Written while at Wolfram|Alpha LLC, Champaign, IL.*

**Core argument:** Implements nonparametric Bayesian inference through Mathematica's memoization capabilities -- functions that retain values for particular calls, enabling statistical modeling of unobserved factors in data generation.

**Technical approach:**
- Chinese Restaurant Process (discrete categories)
- Indian Buffet Process (feature sets)
- Dirichlet Process (infinite category models)
- Metropolis-Hastings querying via MCMC for inference

**Applications demonstrated:**
- **Infinite Relational Model:** Inferring category memberships from relational predicates (classroom acquaintance patterns)
- **Arithmetic Expression Learning:** Probabilistically generating programs matching input-output constraints

**Relevance to Brian:** This is John's most technical paper -- demonstrates serious computational chops. The nonparametric Bayesian approach (reasoning about infinite unknown factors) directly connects to his design theory work on unbounded discovery.

---

#### 4. "Non-parametric Stakeholder Discovery" (IGI Global)
*Chapter in "Emerging Methods in Predictive Analytics: Risk Management and Decision-Making."*

Extends the thesis work into a practical protocol for risk governance. The "non-parametric" framing means: don't assume you know all the stakeholders or factors in advance -- use open-ended protocols to discover them.

---

#### 5. "Wolfram|Alpha: A Computational Knowledge Search Engine" (Springer, 2016)
*Chapter in a Springer volume. Reflects his time working at Wolfram Research/Wolfram|Alpha.*

Describes Wolfram|Alpha's architecture as a computational knowledge engine -- distinct from traditional search. Written from an insider perspective during his time at the company.

---

#### 6. Master's Thesis: "Addressing Risk Governance Deficits through Scenario Modeling Practices" (OCAD University, 2011)
*Master of Design in Strategic Foresight and Innovation. Advisors: Peter Jones, Walter Derzko.*

**Abstract:** Risk governance requires practices that withstand actual events by demonstrating reasonable efforts. But "reasonable" depends on worldview -- not giving different worldviews appropriate consideration leads to governance deficits. The thesis develops foresight methods for eliciting, discovering, representing, and modeling scenarios from disparate worldviews.

**Key methods:**
- Structural/perceptual distinctions in risk modeling
- Interview-based elicitation processes with bias avoidance
- Causal Bayesian networks for assembling risk models
- Non-parametric scenario discovery model
- Conversion of contingency paths to probability scores

**Example application:** Carbon mitigation technologies -- analyzing perspectives from businesses with carbon liabilities, environmental regulation, and insurance regulation.

**Intellectual influences:** Philip Tetlock, Joshua Tenenbaum, Judea Pearl, Alison Gopnik, Vaclav Smil, Cosma Shalizi, Ortwin Renn, Manuel de Landa, Jamais Cascio.

**Relevance to Brian:** The thesis reveals John as a deep systems thinker who bridges quantitative modeling (Bayesian networks, probability) with qualitative design research (stakeholder interviews, worldview analysis). The "perspective" concept in the thesis maps directly to Existence Lang's Perspective term.

---

### Summary: Intellectual Profile

John's work forms a coherent arc: **how do we make good decisions about complex systems when we can't know everything?**

His answer spans:
1. **Theory** -- unbounded reasoning about unknown factors (FormAkademisk)
2. **Methods** -- nonparametric Bayesian inference for discovery (Mathematica Journal)
3. **Practice** -- permaculture as systemic design (Springer)
4. **Application** -- risk governance through scenario modeling (OCAD thesis)
5. **Infrastructure** -- computational knowledge systems (Wolfram|Alpha)

The through-line is remarkably aligned with Existence Lang's philosophy: entities exist at multiple resolutions, perspectives shape what we observe, and good systems design acknowledges what we don't (and can't) know while still enabling coherent action.
<!-- agent:boundary:6468e1d3 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Send approved reply to John Cassel via LinkedIn messaging
- [ ] Consider updating draft reply to reference specific publications more precisely
<!-- /agent:pending -->
