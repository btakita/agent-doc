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
### Re: Dreamable Consulting Agreement Analysis (HEAD)

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
- **§6-7 IP assignment is very broad** — covers anything "related to" Company's "actual or proposed" business. If you have your own projects (agent-doc, ctx-core, etc.), you need to ensure they're listed in Exhibit A as Prior Inventions or explicitly carved out.
- **§2 No payment terms** — no mention of when/how you get paid (net-15, net-30, invoicing process).
- **No SOW or scope definition** — the services aren't defined anywhere in this agreement.
- **§10 Indemnification is one-way** — you indemnify the Company but not vice versa.
- **§13 Attorney's fees** — each party bears own costs, which is fine, but combined with $0 liability cap means you have no recourse.

**Recommendation:** Negotiate §16 (liability cap) and clarify IP carve-outs for your existing open-source projects before signing.
<!-- agent:boundary:c7f82bd8 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
