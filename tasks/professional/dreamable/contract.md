---
agent_doc_session: 4bb5cf54-3e0f-4f83-8d59-ef15dee2c4db
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Read https://signrequest.com/#/document/9a34ccd5-cb2f-4c33-bde8-61ffdca34cdc/972cf49a-0287-4d8c-833f-4c3ff502290b/?utm_source=signrequest&utm_medium=email&utm_campaign=signrequest-received&signer_token=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJ1dWlkIjoiOTcyY2Y0OWEtMDI4Ny00ZDhjLTgzM2YtNGMzZmY1MDIyOTBiIiwiZW1haWwiOiJicmlhbi50YWtpdGFAZ21haWwuY29tIiwiaWF0IjoxNzc1NDI3MjQ0Ljg5ODkyMywiZXhwIjoxNzc2MDMyMDQ0Ljg5ODk0MiwibGlua190eXBlIjoic2lnbmluZyJ9.kswkfuaPV7Ohz-x3MHpAmxJ2crD4BLsltUmcw_Wj1v8

Please analyze
### Re: Contract analysis

I'm unable to fetch the contract content from that SignRequest link. It's a JavaScript-rendered signing page that requires browser authentication.

**Options to proceed:**
1. **Open the link in your browser**, then copy-paste the contract text into this document — I'll analyze it immediately
2. **Download the PDF** from SignRequest and place it somewhere I can read (e.g., `tasks/dreamable/contract.pdf`)
3. **Use the browser tool** — I can open the link in Chromium if you'd like me to try that route

Which approach works best?
Use chromium-bridge
### Re: Dreamable Consulting Agreement Analysis

**Parties:**
- **Company:** Dreamable, Inc., 846A North Point St, San Francisco, CA
- **Consultant:** Brian Takita, PO Box 571 North Grosvenordale, CT 06255

**Effective Date:** April 5, 2026
**Signed by Company:** Robert Ross, CEO/Founder

---

**Key Terms:**

1. **Services** (§1): Consultant provides consulting services; no specific scope defined in the agreement itself.

2. **Compensation** (§2): Rate of $4,000/week = $17,333.33/month.

3. **Confidentiality** (§3): Broad NDA covering all non-public Company information — trade secrets, product plans, customer lists, financials, etc. Survives termination.

4. **Company Property** (§4): All documents, digital records, and materials created in connection with Services are Company property. Must return everything on termination.

5. **Third Party Information** (§5): Duty not to misuse trade secrets. Indemnification obligation on Consultant for third-party IP claims. Also requires Consultant to hold third-party confidential info in strict confidence.

6. **Inventions** (§6): **Broad IP assignment.** "Inventions" = all copyrightable material, notes, records, designs, inventions, trade secrets conceived/developed that relate to Company's actual or proposed business. Prior Inventions should be listed in Exhibit A (or represented as none). Company gets a nonexclusive, royalty-free, sublicensable, perpetual, worldwide license to any Prior Invention incorporated into Company work.

7. **Assignment of Inventions** (§7): **Full assignment** — Company is sole owner of all IP rights in all Inventions related to Services. Includes forever waiver of rights. All Inventions created as part of the Services are Company property.

8. **Patent and Copyright** (§8): Must assist Company in securing IP rights in Inventions. Obligation survives termination.

9. **Conflict of Interest** (§9): Cannot engage in other work that conflicts with Company obligations during the term.

10. **Independent Contractor** (§10): Not an employee/agent. No authority to bind Company. No benefits. Must hold Company harmless from negligent acts.

11. **Termination** (§11): Either party can terminate in writing at any time. **2 weeks' notice** required. Sections 3, 4, 5, 6, 7, 8, 11, 12, 13, 14, 15, 16, and 17 survive.

12. **Jurisdiction** (§12): California law, San Francisco County courts.

13. **Arbitration** (§13): Binding arbitration via JAMS. Each party bears own attorney's fees, equal share of arbitrator costs.

14. **Entire Agreement** (§14): Supersedes all prior agreements. Modifications must be in writing.

15. **Provisions** (§15): Severability; no waiver by breach.

16. **Liability** (§16): **Company's aggregate liability capped at $0.00 including expenses.** This is unusual and worth flagging.

17. **Notices** (§17): Written, by registered/certified mail or overnight courier.

---

**Items to Consider Before Signing:**

- **§16 Liability cap of $0.00** — This means the Company has zero liability to you under any circumstances. This is extremely one-sided. Typically there's at least some liability cap (e.g., fees paid in last 12 months).
- **§6-7 IP assignment is very broad** — covers anything "related to" Company's "actual or proposed" business. If you have your own projects (agent-doc, ctx-core, etc.), you need to ensure they're listed in Exhibit A as Prior Inventions or explicitly carved out.: I may work on side projects. Am I in danger of losing any rights? All of my previous open source projects are either Apache2 or MIT licensed. So Dreamable will be able to use the projects anyways. Am I in danger of losing my rights to these projects
- **§2 No payment terms** — no mention of when/how you get paid (net-15, net-30, invoicing process).
- **No SOW or scope definition** — the services aren't defined anywhere in this agreement.s
- **§10 Indemnification is one-way** — you indemnify the Company but not vice versa.
- **§13 Attorney's fees** — each party bears own costs, which is fine, but combined with $0 liability cap means you have no recourse.

**Recommendation:** Negotiate §16 (liability cap) and clarify IP carve-outs for your existing open-source projects before signing.
### Re: Side projects and IP risk

**Short answer:** Yes, there is real risk under §6-7 as written, but the Apache2/MIT licensing provides some protection.

**The risk:**
- §6 defines "Inventions" as anything that "relate[s] in any way to the Company's actual or **proposed** business." The word "proposed" is dangerously broad — if Dreamable ever proposes to do anything in the AI agent tooling space, your agent-doc, ctx-core, etc. could arguably fall within scope.
- §7 assigns **all** IP rights in such Inventions to Dreamable, including "forever waiver" of your rights.
- If you continue developing agent-doc *during* the consulting engagement, and Dreamable's business touches AI/developer tooling, they could claim ownership of that work.

**What Apache2/MIT protects:**
- The existing published code is already irrevocably licensed. Dreamable (and anyone) can already use it. **They can't revoke that.**
- However, the contract wouldn't give Dreamable rights to *existing* code — it targets *new* work during the engagement. The risk is they could claim ownership of **new commits** to your open-source projects if those projects "relate to" their business.

**What to do:**
1. **List all your active projects in Exhibit A** as Prior Inventions — agent-doc, ctx-core, rappstack, existence-lang, corky, etc.
2. **Add an explicit carve-out clause** like: *"Consultant retains all rights to Prior Inventions and any modifications, enhancements, or derivative works thereof, even if related to the Company's business, provided such work is performed on Consultant's own time and not using Company resources."*
3. **Clarify "proposed business"** — ask Rob to narrow §6 to "actual" business or at least define what's in scope.

Without these protections, continuing to develop your open-source projects during the engagement creates a gray area that could be exploited.

I also have been creating open source "primitive" libraries. For example, yesterday, I created:
- skill-harness
- agent-memories
- agent-rules

How can I preserve my rights to create similar libraries containing such primitives. Should I communicate whenever I create an open source project for signoff?
### Re: Protecting rights to new primitive libraries

**The practical approach — blanket carve-out, not per-project signoff:**

Asking for signoff on every new open-source project would be burdensome and unsustainable. Instead, negotiate a **categorical carve-out** in the contract. Here's what to propose:

**Add to the agreement (or as an addendum):**

> *"Notwithstanding Sections 6 and 7, Consultant retains all rights to:*
> *(a) Prior Inventions listed in Exhibit A and all modifications, enhancements, and derivative works thereof;*
> *(b) Any general-purpose, open-source software libraries, tools, frameworks, or specifications created by Consultant on Consultant's own time and not using Company confidential information, even if such works could be used in connection with Company's business, provided they are published under an OSI-approved open-source license (e.g., MIT, Apache-2.0);*
> *(c) Consultant's pre-existing methodologies, know-how, and general programming techniques."*

**Why this works:**
- It carves out **categories** (general-purpose, open-source, OSI-licensed) rather than requiring you to name every project
- skill-harness, agent-memories, agent-rules — these are clearly general-purpose primitives, not Dreamable-specific IP
- The key distinction is: **general-purpose tooling** (yours) vs **Dreamable-specific product code** (theirs)
- Publishing under MIT/Apache-2.0 is the bright line — it proves the work wasn't meant to be proprietary

**What NOT to do:**
- Don't rely on informal verbal agreement — if it's not in the contract, it doesn't exist
- Don't ask for per-project signoff — it creates a paper trail that implies they have authority over your side work
- Don't assume the current contract protects you just because you publish open-source — §6-7 as written would still assign your rights even for openly published code

**Suggested conversation with Rob:**
*"I regularly create and publish open-source libraries as part of my professional practice. I'd like to add a carve-out that preserves my rights to general-purpose open-source work done on my own time. This doesn't affect Dreamable — you'd still have full rights to everything I build specifically for Dreamable, and you can freely use any of my open-source libraries under their published licenses."*

How can I propose these amendment? By email?

Please create an amendment based on https://github.com/btakita
Should I also include my private repositories?
### Re: Proposing the amendment and Exhibit A

**How to propose:** Yes, email is the right channel. Send it to Rob before signing. Frame it as: "Before I sign, I'd like to propose one addendum — here's the language." Keep it professional and brief. This is standard practice for contractors with active side projects.

**Private repositories:** Yes, absolutely include them. Private repos are *more* vulnerable than public ones — with public repos you at least have a timestamped publication trail proving independent creation. Private repos have no such defense. List them by category (e.g., "all private repositories under github.com/btakita") without needing to name each one.

---

**Draft Amendment / Exhibit A:**

---

**ADDENDUM TO 2026 DREAMABLE CONSULTING AGREEMENT**

*This Addendum is entered into as of April 5, 2026, by and between Dreamable, Inc. ("Company") and Brian Takita ("Consultant") and amends the Dreamable Consulting Agreement dated April 5, 2026 (the "Agreement").*

**1. Exhibit A — Prior Inventions**

Consultant has the following Prior Inventions, including all repositories (public and private) owned by Consultant on GitHub (github.com/btakita) as of the Effective Date, and specifically including but not limited to:

- **agent-doc** — Interactive document session CLI tool (Rust)
- **agent-kit** — Toolkit for CLI tools integrating with AI agent loops (Rust)
- **agent-memories** — Convention for externalizing learned lessons from AI agents
- **agent-rules** — Convention for prescribed policy in AI agent instruction files
- **agent-runbooks** — Convention for externalizing step-by-step procedures from AI agents
- **skill-harness** — Agent skill harness framework
- **module-harness** — Language-agnostic module-level harness context management
- **corky** — Correspondence kit (Rust)
- **instruction-files** — Shared audit crate
- **chromium-bridge** — Chromium browser bridge
- **eval-runner** — Evaluation runner
- **tagpath** — Tag-based identifier parser
- **tmux-router** — Tmux routing tool
- **lazily-rs** — Lazy evaluation library (Rust)
- **lazily-py** — Lazy evaluation library (Python)
- **ctx-core** / **rappstack** — Context and application framework ecosystem
- **briantakita.me-dev** — Personal website
- **philosophy** — Existence Lang ontology
- **python-intentions** — JetBrains plugin
- **All other repositories** (public and private) under github.com/btakita existing as of the Effective Date

**2. Section 6 Amendment — Open-Source Carve-Out**

The following is added to Section 6 of the Agreement:

*"Notwithstanding anything in Sections 6 and 7 to the contrary, Consultant retains all rights, title, and interest in and to:*

*(a) All Prior Inventions listed in Exhibit A and any modifications, enhancements, updates, or derivative works thereof;*

*(b) Any general-purpose, open-source software libraries, tools, frameworks, specifications, or conventions created by Consultant on Consultant's own time and without use of Company confidential information, even if such works could be used in connection with Company's business, provided they are published under an OSI-approved open-source license (e.g., MIT, Apache-2.0);*

*(c) Consultant's pre-existing methodologies, know-how, professional techniques, and general programming knowledge.*

*For avoidance of doubt, Company receives a nonexclusive, royalty-free, perpetual license to use any such works under their published open-source license terms."*

**3. Section 16 Amendment — Liability**

Section 16 is amended to read: *"Company's aggregate liability, whether to Consultant or to third parties caused hereunder, shall not exceed the total fees paid to Consultant under this Agreement in the twelve (12) months preceding the claim."*

**4. Conflict.** In the event of any conflict between this Addendum and the Agreement, this Addendum shall control.

**5. Remainder.** All other terms and conditions of the Agreement remain in full force and effect.

---

*Dreamable, Inc.*
Name: _______________
Title: _______________
Date: _______________

*Consultant*
Name: Brian Takita
Date: _______________

---

**Notes on the draft:**
- I included a §16 fix (liability cap changed to 12 months of fees) since that was the other major issue
- The Exhibit A lists your key active projects explicitly, then uses a blanket clause for "all other repositories" to catch everything
- The open-source carve-out (2b) covers *future* projects categorically — no need to update Exhibit A every time you create a new library
- Send this alongside a brief email explaining the rationale

Please draft an email to robert-ross with the proposed amendments. Also send Exhibit A in the email.
### Re: Email draft

Email drafted at `mail/drafts/2026-04-06-proposed-addendum-to-consulting-agreement-prior-inventions-i.md`.

Includes:
- Brief intro framing the 3 items (Prior Inventions, Open-Source Carve-Out, Liability cap)
- Full addendum language with Exhibit A listing your key projects + blanket clause
- Professional, direct tone matching Rob's communication style

Ready to push to Gmail Drafts on your approval.

Please explain. I don't know what this means exactly. Liability for what?
> 3. **Section 16 (Liability):** The current cap is set at $0.00... I'd like to adjust this to fees paid in the prior 12 months, which is standard for consulting agreements.
### Re: What §16 liability means

**In plain terms:** "Liability" here means the maximum amount of money Dreamable would owe you if they do something wrong under this contract.

**Examples of what could go wrong:**
- Dreamable doesn't pay you for work you've completed
- Dreamable breaches the contract (e.g., terminates without the required 2 weeks' notice)
- Dreamable causes you damages through their actions (e.g., misrepresents something that costs you money)

**What the current contract says:** Dreamable's total liability to you is capped at **$0.00**. That means even if they owe you $20,000 in unpaid invoices, technically the contract says their maximum liability is zero. It's essentially saying "we're never liable for anything."

**Why this matters practically:**
- If Dreamable stops paying you, you'd have a hard time recovering fees in arbitration because the contract literally caps their obligation at $0
- It's likely a template oversight (many boilerplate contracts have a fill-in-the-blank liability cap that defaults to $0), not necessarily bad faith
- But it should still be fixed before signing

**What the amendment proposes:** Cap liability at the total fees paid to you in the prior 12 months. So if you've been paid $50,000 over the past year, that's the maximum you could recover. This is the standard approach for consulting agreements — it's fair to both sides.

**Bottom line:** It's your payment protection. Without fixing this, you have no contractual recourse if Dreamable doesn't pay you.

In Exhibit A, I also need to include open source work for any organizations that I have control over such as:
- rappstack
- existence-lang
- aurovlog
- autostores
- cogov
- ctx-core
- esmfile
- foresight-host
- hyopjs
- lastinglogic
- linkument
- persontric
- proseui
- prosey
- rebuildjs
- relementjs
- relysjs
- rmemojs
- rr
- tag-vector
- wanimato
### Re: GitHub organizations added to Exhibit A

Updated the email draft to include all 21 GitHub organizations you listed. The Exhibit A now covers:

1. **Personal repos** — github.com/btakita (250+ repos, public and private)
2. **Organization repos** — rappstack, existence-lang, aurovlog, autostores, cogov, ctx-core, esmfile, foresight-host, hyopjs, lastinglogic, linkument, persontric, proseui, prosey, rebuildjs, relementjs, relysjs, rmemojs, rr, tag-vector, wanimato
3. **Key projects listed explicitly** — agent-doc, corky, lazily-rs, etc.

The blanket language ("owned by or administered by Consultant") covers all repos in those orgs without listing each one individually.

Email draft updated at `mail/drafts/2026-04-06-proposed-addendum-to-consulting-agreement-prior-inventions-i.md`. Ready to push to Gmail Drafts on your approval.

Please send email
### Re: Email pushed

Email pushed to Gmail Drafts. Open your Gmail drafts to review and send to rob@dreamable.com.

Subject: *Proposed Addendum to Consulting Agreement — Prior Inventions & IP Carve-Out*
<!-- agent:boundary:d57ea336 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
