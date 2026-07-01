---
agent_doc_session: 582d94f7-308c-4d9e-8dba-919c46d2c9ae
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
agent_doc_lint_dialect: off
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Create a profile for:
- alana-owen https://www.linkedin.com/in/a-own/
- qualia-software https://www.linkedin.com/company/qualiasoftware/ https://www.qualia.com/

I will be having an interview with Alana Owen at Qualia Software in 40 minutes. Please give me a brief.

### Re: Profiles + interview brief — opus-4-8

Profiles committed to the `mail` submodule (`8908dff`, local only):
- `mail/contacts/alana-owen/AGENTS.md` — your interviewer
- `mail/topics/qualia/AGENTS.md` — the company

## Interview brief — Alana Owen @ Qualia

### Who you're talking to
**Alana Owen is a Lead Recruiter, not a technical hiring manager** (Austin TX, ~6 yrs at Qualia, recruiting/business-ops background). This is a **recruiter screen**: motivation, background walk-through, salary expectations, logistics, culture/communication fit — **not** a deep technical grill. Be warm, concise, mission-aligned. Save deep architecture talk for later rounds, but show you understand their domain.

### What Qualia does (30-sec version)
Cloud **title, escrow & closing platform** for US real estate — one shared "digital closing" workspace connecting title agents, escrow, lenders, agents, and buyers/sellers. Founded 2015, CEO **Nate Baker**, SF HQ, ~630 people, **unicorn since 2020** (~$207M raised). Claims 1M+ real estate/mortgage pros on the platform.

Products worth name-dropping: core closing platform, **Qualia Clear** (settlement software w/ AI), **Qualia Connect** (2025 comms), **Shield** (wire-fraud detection), an API, and the 2025 **Bank Partner Network**. Recent move: **acquired RamQuest** and partnered with **Old Republic Title** (Jan 2025), framed around "the coming AI wave."

### Get the stack right (common trap)
Their real stack — from an actual Qualia job posting — is **JavaScript / Node.js, MongoDB, Semantic UI, microservices, Kubernetes, AWS**. It is **not** Ruby/Rails or a classic React setup, so don't assume. They also hire heavily for **security engineering** — fitting, since they move money and fight wire fraud.

### Your angle (what to emphasize)
- **Comfort owning high-stakes, high-liability systems** — money movement / fraud is their core risk. Map your infra/reliability/security experience to that.
- **Full-stack product sense + attention to detail** — they want conscientious builders who care about the end user (home closings are stressful, high-dollar moments).
- **Remote-first collaboration + mentoring** — they're remote-first and value teamwork with design and customer-facing teams.
- **Genuine mission interest** — making home closings simple and secure. Real estate closings are famously painful; that's the pitch.

### Smart questions to ask her
- What does the interview process look like after this screen, and timeline?
- Is this role on a specific product (Clear / Connect / Shield / API / platform core)?
- What's the team's split between new product and platform reliability/security work?
- Is the team fully remote, and what does collaboration look like day-to-day?
- (Logistics) confirm comp range, location expectations, and level.

### Handle these directly
- **Salary expectations** — have a range ready; recruiters ask early.
- **Why Qualia / why now** — tie to the mission + high-stakes financial software.
- **Your background walk-through** — keep it a tight 2–3 min narrative.

### Uncertainty flags
Alana's exact current title/tenure and Qualia's precise headcount ($2.2B valuation figure) are third-party — verify live if it comes up. The confirmed valuation milestone is $1B+ (2020 Series D).

Good luck — you've got this. Want a one-line "why Qualia" opener drafted, or a comp-range talking point?

### Re: JD fit review — Senior Software Engineer I @ Qualia — opus-4-8

Reviewed [the Greenhouse posting](https://job-boards.greenhouse.io/qualia/jobs/6001226) against your background. **This is a strong fit — the payments/fraud overlap is the bullseye.** One caveat on leveling.

**The role:** Senior Software Engineer I — Austin or remote. 5+ yrs full-stack; build frontend + backend, extend core web apps, **software integrations, and payment-processing systems**; make architectural decisions on large projects; mentor juniors; work across teams. Stack: **JavaScript/Node.js, Semantic UI, MongoDB, microservices/Kubernetes, AWS**. No salary posted.

**Strong matches (lead with these):**
- **Payment-processing + fraud — your single best differentiator.** Stripe billing on the EFS platform; Stripe + PayPal checkout hardening at MonsterRodHolders, where you traced a sales dip to a card-testing bot (170+ fraudulent PayPal charges) and shipped Turnstile + WAF mitigation. Qualia *is* money movement (title/escrow) and runs **Shield** for wire-fraud — this maps almost perfectly.
- **Node.js / full-stack** — Deloitte Node.js, Hono, Next.js, Bun, TypeScript-heavy. Solidly in their language.
- **Architectural decisions on large projects** — EFS platform (Cloudflare Workers + D1, 741+ tests, 74 prod migrations); architected Presence AI's H100 real-time pipeline.
- **Mentoring + multiple teams** — Pivotal Labs (mentored devs across 20+ startups), RSpec/BDD core. Exceeds the bar.
- **AWS** — Lambda, CDK, Terraform. ✅
- **20+ yrs** vs their 5+ minimum — well past tenure.

**Gaps (all learn-on-the-job, none blocking — be upfront):**
- **MongoDB** — your DB depth is Postgres/TimescaleDB/Supabase/Firebase/Convex/D1. No Mongo specifically, but Firebase/Convex are document stores, so the NoSQL model transfers. Frame it that way honestly.
- **Semantic UI** — you're React/Svelte/Angular/Vue. It's just a component library; trivial pickup. Non-issue.
- **Kubernetes** — you have Docker/Terraform/AWS/CI-CD and microservice experience; K8s specifically isn't on your resume. Learnable; frame the container/infra foundation.

**Leveling caveat (raise with Alana):** "Senior Software Engineer I" reads as *entry* senior. Your depth — 20+ yrs, architecture ownership, mentoring across 20+ startups — arguably fits Senior II / Staff. Ask about the **leveling framework and comp band** so you're not slotted below your experience. This is also your natural "salary expectations" pivot.

**One-liner for "why you":** *"You move money in high-stakes, high-liability closings — that's exactly the surface I've been hardening: Stripe/PayPal payments, fraud mitigation, and reliability under real financial risk."*

Want me to spin up a tailored resume profile (`resume/profiles/qualia.toml`) emphasizing payments/fraud + Node, or draft an application note?
Should I mention multi-layer perceptron models? And my signal-space work?

### Re: MLP models + signal-space — mention them? — opus-4-8

Short answer: **signal-space yes (selectively), MLP mostly no.** Match each to the audience.

**Today's recruiter screen with Alana — skip both.** Keep the narrative sharp and on-target: payments/fraud + full-stack Node in a high-liability domain. Both topics are technical and niche; with a non-technical recruiter they dilute a clean story and invite "why isn't this on the resume for *this* role" questions. Save them for the technical rounds.

**Signal-space — yes, in technical rounds, because it has a real hook here.** Signal-space is realtime reactive signal/graph mirroring (snapshot/delta, CRDT-adjacent) — and Qualia's whole product is **multiple parties collaborating on one live closing** (title agent, escrow, lender, buyer/seller). Realtime state sync and conflict-free collaboration is a genuine domain overlap. Pair it with **agent-doc's CRDT co-editing** (human + agent editing the same doc without conflicts) — together they show you think hard about realtime multi-user consistency, which is exactly the hard part of a shared closing platform. That's a *stronger, more on-point* angle than MLP. Keep it to ~30 seconds and tie it to their multi-party workflow, not to your framework's lineage.

**MLP / ML — only if *they* bring up AI, and frame it honestly.** Qualia does have AI surfaces (Qualia Clear AI, **Shield** wire-fraud detection), so ML *can* come up. If it does:
- Lead with your **real production ML experience — Presence AI** (real-time inference pipelines: facial-landmark detection, lip-sync inference on H100s via WebRTC). That's concrete and shippable, not academic.
- **Do not** imply you've built ML-based fraud models. Your fraud work (MonsterRodHolders) was **rule/heuristic-based** — Turnstile, WAF, ModSecurity, order-data forensics — not trained models. Conflating them is the kind of overclaim a technical interviewer will probe and you'll lose credibility. It's fine (better, even) to say "my fraud work was heuristic/rules; my ML production work was the Presence AI inference stack."
- Naming "multi-layer perceptron models" specifically reads academic and invites depth you may not want to defend for a web-platform role. Say "production ML/inference systems" and only go deeper if they push.

**Bottom line:** For the win today, don't volunteer either — stay on payments/fraud + Node. In technical rounds, **lead with signal-space + agent-doc CRDT when realtime/collaboration/frontend architecture comes up** (best fit), and reach for ML only if they raise their AI stack, anchored on Presence AI and honest about the fraud distinction.

Want me to draft a tight 30-second signal-space + CRDT talking point you can drop in when collaboration architecture comes up?
yes

### Re: Signal-space + CRDT talking point (30 sec) — opus-4-8

**Say this when realtime / collaboration / frontend architecture comes up:**

> "One thread through my open-source work is realtime multi-user consistency. I built **signal-space** — reactive signal and graph mirroring with snapshot/delta sync — and **agent-doc**, which uses CRDTs so a human and an AI agent can edit the same document at once without conflicts. That's the same hard problem a closing platform has: title, escrow, lender, and buyer all touching one live deal, everyone needing a consistent view without stepping on each other. I've spent real time in that conflict-resolution space, so your multi-party model feels like familiar territory."

~35 seconds at a natural pace. Two ways to flex it:

- **Even shorter (10 sec, if you just want to plant the flag):** *"I've built CRDT-based collaborative editing and reactive state-sync tooling in open source — conflict-free multi-user consistency is a problem I've gone deep on, and it's exactly what a shared closing workspace lives or dies on."*
- **If they push for depth:** be ready to name the specifics — CRDTs for conflict-free merge, snapshot/delta for efficient sync, and the tradeoff you hit (e.g. per-cell vs whole-document merge granularity). Don't overreach into distributed-systems theory; keep it grounded in what you shipped.

**Delivery notes:** lead with the *problem* (multi-party consistency), not the project names — the names are proof, not the point. Land it on "your multi-party model," so it's about Qualia, not a tour of your repos. And only deploy it once; it's a sharp differentiator, not a theme to repeat.

Anything else you want prepped before the call — a comp-range talking point, or a tight "why Qualia" open?

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/recruit/qualia.md source=socket_already_applied patch_id=9abebc13-998c-4cef-a5ca-2aa5171b2038 invariant=disk_missing_response_probe recovery=content_ours_snapshot_visible_response_repair response_sha256=1bd97fddbfca53717c25c4dfaf8a7040dea89af7f237a1c00536231704fd232f current_len=2094 current_hash=033247efe9ccc71d46dada55fe8c29131cd0f7bfc792a4ef285effaa1e5ad404 repaired_len=5513 repaired_hash=c138ad97ae655e5163f8beef2f7f87f56637591d96cf9f95f1a5ebe02f243ade doc=qualia session=582d94f7-308c-4d9e-8dba-919c46d2c9ae turn=cycle-1782934039471
```
<!-- agent:boundary:815c4701 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
- ~~https://job-boards.greenhouse.io/qualia/jobs/6001226~~ — reviewed (JD fit review above)
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
