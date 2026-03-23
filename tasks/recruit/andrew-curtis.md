---
agent_doc_session: 26b3d492-9bea-4bd6-8dca-4a8350538d2d
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Andrew replied: role is hybrid/Boston. Awaiting your decision on location before responding.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Make a contact for andrew-curtis.
https://www.linkedin.com/in/andrew-curtis-022513b1/

Read mail/contacts/andrew-curtis/20260323-linkedin-andrew-curtis.txt

Draft a response. Make a custom resume.
### Re: Contact + Resume + Draft Reply

**Contact created:** `mail/contacts/andrew-curtis/AGENTS.md` (with `CLAUDE.md` symlink)

**Role summary:** Senior Full-Stack Engineer (Python/React) — 6-month contract-to-hire. Unnamed client building advanced data analysis/decision-making platforms. Python backend, React/TypeScript frontend, REST/GraphQL APIs. AWS, Docker, K8s preferred. May involve regulated/sensitive data.

**Custom resume built:** `resume/BrianTakita-andrew-curtis.pdf`
- Highlights: Python (FastAPI/Flask/Django/SQLAlchemy), React/TypeScript, GraphQL/REST, AWS, Docker, CI/CD
- JD mentions "AI-assisted coding tools" — agentic dev methodology highlighted
- Profile: `resume/profiles/andrew-curtis.toml`

**Draft LinkedIn reply:**

Hi Andrew,

Thanks for sending over the full JD — this looks like a strong fit. I have 20+ years of full-stack experience with deep Python backend expertise (FastAPI, Flask, SQLAlchemy) and extensive React/TypeScript frontend work. I've built and consumed REST and GraphQL APIs across multiple production systems, and have strong AWS and Docker experience.

I'm particularly interested in the data analysis/decision-making platform aspect — I've built data pipelines and large-scale analytics systems (ESG portfolio analytics at Censible, real-time multimodal AI pipeline at Presence AI).

Happy to schedule a screening call. I'm available this week — what times work for you?

Best,
Brian

Ready to tweak or send?

> We are partnering with a leading organization focused on building advanced technology platforms that support large-scale data analysis and real-world decision-making. This team is developing mission-critical software systems that transform complex datasets into actionable insights across a highly impactful domain.
>
> Collaborate with cross-functional stakeholders to translate complex datasets into intuitive software solutions

Should we include agent-doc? I'm planning on MDX + Jupyter integration for realtime AI driven documents.

Any gaps in the resume?
### Re: agent-doc + Resume Gap Analysis

**On agent-doc:** Keep it on the resume (already there under Open Source) but skip it in the LinkedIn reply — the JD's "AI-assisted coding tools" line is nice-to-have, not the core ask. The MDX + Jupyter integration story is a strong talking point for the screening call though. "Translating complex datasets into intuitive software solutions" maps directly to interactive AI-driven documents. Bring it up when Andrew asks about your approach to building data-driven applications.

**Resume gap analysis against JD:**

| JD Requirement | Status | Notes |
|---------------|--------|-------|
| 6-8+ years full-stack | Covered | 20+ years |
| Strong Python backend | Covered | FastAPI, Flask, SQLAlchemy, Django |
| React/TypeScript | Covered | Presence AI, SocialChorus, Galler AI |
| REST/GraphQL APIs | Covered | Presence AI (GraphQL/urql), TrueCar |
| System design | Covered | Multi-service monorepo, pipeline architecture |
| CI/CD + automated testing | Covered | BDD/TDD, CI/CD skill |
| Cloud (AWS) + Docker/K8s | Covered | AWS, Docker, EC2, Lambda |
| Data pipelines / large datasets | Light | Censible (10k+ rows), Presence AI pipeline — use the phrase on the call |
| Regulated/sensitive data | Light | USAA security review at TrueCar covers this — mention on call |
| Cross-functional collaboration | Covered | Pivotal Labs (20+ startups), Presence AI |

**No critical gaps.** Two minor ones (data pipelines, regulated data) are covered by experience but not explicitly phrased on the resume. Best addressed as talking points on the screening call.

Draft reply is ready to send as-is — want to proceed?
Is this a 100% remote role?
### Re: Remote Status

The JD doesn't specify remote/hybrid/on-site. "Work Environment" section says "collaborative, cross-functional team environment" but no location details. Andrew's messages didn't mention it either.

**Updated draft with remote question added:**

Hi Andrew,

Thanks for sending over the full JD — this looks like a strong fit. I have 20+ years of full-stack experience with deep Python backend expertise (FastAPI, Flask, SQLAlchemy) and extensive React/TypeScript frontend work. I've built and consumed REST and GraphQL APIs across multiple production systems, and have strong AWS and Docker experience.

I'm particularly interested in the data analysis/decision-making platform aspect — I've built data pipelines and large-scale analytics systems (ESG portfolio analytics at Censible, real-time multimodal AI pipeline at Presence AI).

One question — is this role fully remote?

Happy to schedule a screening call. I'm available this week — what times work for you?

Best,
Brian
Reread the sms transcript 
I am ok with a hybrid role. But I plan on moving away fr
### Re: SMS Transcript Review (HEAD)

Andrew responded (after your message was sent):

> "Hi Brian, glad to be connected. Seems like it could be a really good fit potentially! My understanding is that they want someone local to Boston so that they can be on-site as needed. So probably more hybrid than fully remote, but hopefully not too inconvenient for you. Would that work?"
>
> "And I have time to connect this afternoon if it works for you!"

**Key finding:** Role is **hybrid in Boston** — client wants someone local/on-site as needed.

**Decision needed:** Is Boston hybrid feasible for you? Options:

1. **Decline** — if Boston isn't workable
2. **Negotiate remote** — express interest, ask if fully remote with occasional visits works
3. **Accept hybrid** — if you're in the Boston area
4. **Ask for details** — how many days on-site per week?

**Draft reply (negotiate remote):**

Hi Andrew,

Thanks for the quick reply! The role sounds like a great fit technically.

On location — I'm not local to Boston. Given the seniority of the role and my remote work track record, would the client consider fully remote with occasional on-site visits? Happy to discuss flexibility on a screening call.

Let me know — I'd still love to connect.

Best,
Brian
<!-- agent:boundary:97ac5113 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- **BLOCKER:** Decide on Boston hybrid — decline, negotiate remote, or accept
- Draft and send reply to Andrew based on location decision
- Attach resume PDF when sending
- Schedule screening call once times confirmed
- Prep talking points: MDX+Jupyter data viz, USAA regulated data, data pipeline framing
<!-- /agent:pending -->
