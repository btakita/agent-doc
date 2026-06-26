---
agent_doc_session: e9e4addf-dd89-4638-8557-79a3696e473f
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

❯ Please create a profile for adduro https://adduro.io/.
❯ Create a profile katie-lynch https://www.linkedin.com/in/katielynch21/

❯ I will be having an interview. Please help me prepare for a 15 minute interview. How can I contribute value to Adduro?

### Re: Company profile — Adduro — opus-4-8

**Adduro** (adduro.io) — bootstrapped, profitable streaming-advertising (CTV/OTT) startup. *LinkedIn profiles return HTTP 999, so headcount/founding-date come from third-party aggregators + one founder podcast — treat as approximate.*

**What they do**
- Full-service streaming-ad platform: Connected TV, streaming audio, OTT, online video, display, "programmatic linear" — ads across Hulu, Peacock, Roku, Tubi, Paramount+.
- **Thesis:** traditional programmatic streaming is opaque — middlemen, hidden markups, fraud-padded metrics, misleading last-touch attribution. Adduro sells **transparency + outcomes**: direct publisher relationships (no exchanges → lower CPMs), 100% living-room screen verification, fraud blocking, household-level frequency, fractional (exposure-weighted) attribution, real-time cost-per-lead reporting.
- **Service tier "Ignite":** full-service performance team (strategy, creative, setup, optimization, weekly reviews).

**Proprietary tech (relevant to an eng interview)**
- **GammaRay** — in-house bidding engine: frequency capping before delivery, daypart pacing, real-time publisher-level bid optimization.
- **GammaBurst** — analytics dashboard: publisher-level + ZIP-code + hourly-pacing + household-frequency + cost-per-lead reporting, real-time.
- **Hard Check** — anti-fraud blocking spoofed traffic / VPNs / bot farms (claims 250M+ invalid impressions blocked/month).
- Underlying languages/cloud are **not disclosed publicly** — a good thing to ask.

**Stage / team / market**
- **Bootstrapped, self-funded, profitable monthly** — no VC. Founded ~2022–2023, ~9–11 people. Offices: San Diego, Denver, Brooklyn.
- **Founder & CEO: Brandt Bodamer** (Denver; CSU MBA; ex-Madwire/Marketing 360). Public theme: hires for **curiosity and adaptability over polish**. Director of Software Dev: TJ Romanowski. Head of Sales: Drew Fasold.
- Market tailwind: CTV ad spend rising (~70% of CTV advertisers expect to raise spend in 2026; political/midterm cycle helps).
- **Amazon Ads Verified Partner**; direct integrations with the major streamers; Goodwill named as a case study.

**Mission / values:** *"Advertisers First. AdTech Company Second."* Six values: Invent the Box · Obsess Over Better · Curiosity Is a Requirement · Disrupt With Purpose · Be the Partner You'd Want · Teach, Don't Gatekeep.

Sources: [adduro.io](https://adduro.io/), [/platform](https://adduro.io/platform/), [/company](https://adduro.io/company/), [/results](https://adduro.io/results/), [founder podcast](https://marketingcareersuncovered.com/learning-fast-leading-faster-with-brandt-bodamer-founder-at-adduro-io/), [RocketReach](https://rocketreach.co/adduroio-management_b6f90c31c629104f).

### Re: Person profile — Katie Lynch — opus-4-8

**Katie Lynch, PHR** — [linkedin.com/in/katielynch21](https://www.linkedin.com/in/katielynch21/). Greater Fort Collins, CO. *LinkedIn blocked direct fetch — details below are from search snippets of her profile/posts; confidence flagged.*

- **At Adduro (confirmed)** — profile lists adduro.io as current company. Holds a **PHR** (Professional in Human Resources) cert. Role reads as **recruiter / talent acquisition** (exact title unconfirmed).
- **Specializes in sales recruiting (confirmed)** — explicitly says she chose sales over tech recruiting ("humans with high energy, big personalities"). At a ~10-person company she's likely the general first-round/culture screen and routes to Brandt (CEO) or Drew (Head of Sales).
- **Interview style (confirmed from her posts):** **conversational, anti-checklist, values authenticity.** For sales candidates she drills numbers — "What's your quota? How'd you perform against it?" She wants people who can **articulate their own performance concretely**, and who'd thrive in an energetic, cameras-on, cheer-each-other-on remote culture.
- **Not confirmed:** her exact title, prior employers, CSU degree/year. (Many unrelated "Katie Lynch" profiles exist — ignore the ADP/Diageo/Starbucks ones.)

**Read-through:** expect a **rapport-driven behavioral screen**, not a deep technical grilling. Be high-energy, tell stories (not a recited résumé), and **know your numbers/impact cold**.

### Re: 15-minute interview prep — how to contribute value to Adduro — opus-4-8

A 15-min screen with a recruiter = **fit, energy, and a sharp "why you."** You won't get deep technical time — make 2–3 crisp points land.

**Your one-sentence value thesis:**
> "Adduro's edge is proprietary, real-time tech that makes streaming ads transparent and outcome-driven — GammaRay, GammaBurst, Hard Check. That's exactly the systems work I do: low-latency, data-heavy, real-time tooling built for correctness and clarity. I'd help the small eng team ship that faster."

**Concrete contribution angles** (your strengths → their stack):
- **Real-time bidding (GammaRay):** performance-critical, latency-sensitive systems work — your wheelhouse (Rust, high-throughput tooling). Frequency-capping-before-delivery and daypart pacing are an event-stream/state-machine problem.
- **Analytics (GammaBurst):** real-time, multi-dimensional reporting (publisher × ZIP × hour × household). You build data/observability tooling that turns noisy streams into legible signal — their whole transparency pitch.
- **Anti-fraud (Hard Check):** pattern/anomaly detection at scale (250M+ impressions/mo) — signal extraction from noisy data.
- **Leverage for a tiny team:** bootstrapped with ~2–4 engineers. Emphasize you **multiply a small team** — build tooling/automation that punches above headcount (your whole body of work). Resonates with a profitable, no-VC culture.

**Mirror their values:** curiosity, "teach don't gatekeep," disrupt-with-purpose, advertisers-first transparency. Brandt hires for **curiosity + adaptability over polish** — show how you self-teach and adapt fast.

**3 sharp questions to ask:**
1. "What's the stack behind GammaRay/GammaBurst, and where's the biggest build-vs-buy tension right now?"
2. "How big is engineering, and what would the first 90 days look like?"
3. "Bootstrapped and profitable is rare — how do you decide what to build next without VC pressure to chase scale?"

**Tactics for the 15 min:** lead with energy + a 30-sec "who I am + why Adduro"; have **one concrete impact story with a number**; treat it as a conversation; close by asking the next step. Don't over-rotate on deep tech with Katie — save that for the hiring-manager round, but signal you have it.

*Caveat: if this is a sales/GTM role rather than engineering, flip the framing to quota/attainment numbers and customer outcomes — tell me the role title and I'll re-tune.*
<!-- agent:boundary:bb574986 -->
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
