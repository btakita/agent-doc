---
agent_doc_session: fec84885-6792-429c-9c0f-f8c7b32921b2
resume: 019de476-ea8f-72f0-9d3a-ca2dfb01ef85
agent: codex
claude_model: claude-opus-4-6
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
---

## Status

<!-- agent:status patch=replace -->
Contract reviewed. Main issues are invoice-forfeiture / termination asymmetry, uncapped indemnity with no liability cap, broad work-for-hire assignment, and a 12-month client non-circumvention restriction.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Please read through the following.
https://app.pandadoc.com/document/v2?token=baa3ba0e330c72424590a8e7e64de9b84b88c547?

Anything to note or be wary of?
### Re: PandaDoc review — gpt-5

I can’t verify the actual contents from that PandaDoc link in this environment. The document appears to require browser-side access, so I do not have the contract text itself.

Anything to be wary of, even before content review:

- A tokenized PandaDoc URL can itself be sensitive. Treat it like a shared-access link and rotate/restrict it if it was exposed broadly.
- Do not rely on PandaDoc metadata alone. Check the actual legal entity names, signer roles, effective date, and any incorporated exhibits or linked policies.
- Watch for auto-renewal, termination notice windows, payment triggers, late fees, and one-way change rights.
- Check liability allocation carefully: indemnity, warranty disclaimers, limitation of liability, carve-outs, and any uncapped obligations.
- Confirm IP, confidentiality, data use, security, privacy/DPA terms, and whether customer data can be reused for training or analytics.
- Verify acceptance mechanics: when the agreement becomes binding, whether email/PandaDoc actions count as acceptance, and whether there are order-form precedence clauses.
- Review venue, governing law, arbitration, and attorney-fee provisions if this matters commercially.

If you want an actual clause-by-clause review, paste the text here or export the document as PDF/plain text and I’ll mark specific risks instead of giving a generic checklist.
I’m checking whether that PandaDoc link is accessible from here and whether there’s enough visible content to review it directly. If the document is gated or requires interactive access, I’ll state that explicitly and keep the response scoped to what can be verified.
❯ The agreement was converted to md at /home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md

❯ Is there any claude about me not being able to subcontract myself?
### Re: subcontracting clause — gpt-5

Assuming you meant `clause`: I do not see a clause that says you cannot personally perform the work yourself. The agreement is actually written as though **you personally** are the subcontractor: it names `Brian Kenji Takita` as the `Subcontractor` in the opening paragraph and again in Exhibit A / the SOW.

The restrictive clause is [Test_Double_Subcontractor_Agreement_Brian_Takita.md](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:207): `The Subcontractor may not use further subcontractors or subconsultants to perform any of the Services without prior written consent of the Company.` Read together with the engagement language in [the same file](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:7) and the independent-contractor section at [lines 147-165](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:147), the practical reading is:

- You can perform the services yourself.
- You cannot delegate the work to another person, subconsultant, or downstream contractor unless Test Double consents in writing.
- If by `subcontract myself` you mean doing the work through your own LLC/corporation rather than as Brian individually, the agreement does not clearly authorize that. Since the contract is with you by name, the safer reading is that Test Double hired **you personally**, not an interchangeable entity or substitute.

So the narrow answer is: **no clause forbids you from doing the work yourself; the clause forbids you from having someone else do it without written consent.** If you want to invoice or perform through an entity, I would get that confirmed in writing before relying on it.
❯ Please review the contract. Anything to note that I should be aware of?
### Re: contract review — gpt-5

Findings, highest risk first:

1. **You can lose already-earned money if you terminate early or breach.** If you terminate within the first 60 days, you forfeit any outstanding invoices ([Test_Double_Subcontractor_Agreement_Brian_Takita.md](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:103)). Separately, if you breach and do not cure within 10 days, you forfeit rights to pending or future payments ([same file](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:31)). That is a very contractor-unfriendly remedy because it is not limited to the value of the actual damage.

2. **There is no liability cap, but there is broad indemnity against you.** You indemnify the company and clients for claims and liabilities, including consequential or indirect damages, tied to your negligence, willful misconduct, or breach ([same file](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:217)). There is no matching cap on your aggregate liability anywhere in the agreement. That leaves your downside open-ended.

3. **The IP assignment is broad and appears to give ownership to the company or client by default.** Section 5 says any work prepared under the agreement is work-for-hire and, if not, is assigned to the company or client ([same file](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:120)). If you use preexisting code, templates, utilities, or know-how, there is no explicit carveout preserving those as yours.

4. **The non-solicitation / non-circumvention clause is broad and lasts 12 months.** For a year after termination, you cannot perform services directly for a client in a way that bypasses Test Double, and you also cannot interfere with company/client relationships ([same file](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:55)). If you meet a client through this engagement, assume direct follow-on work is restricted unless Test Double consents.

5. **Termination is asymmetric.** You owe 30 days' notice after the first 60 days if there is no clear end date, but the company may terminate on written notice without a matching notice period ([same file](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:103), [same file](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:115)). Combined with the invoice-forfeiture language, that gives them materially more leverage.

6. **The SOW is loose on duration and work commitment.** Start date and end date are both `TBD`, while the pricing line says `$100/hr @ 40hrs week` and the budget says no more than 40 hours/week without consent ([same file](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:308)). I would not read that as a guaranteed 40 hours per week. It looks more like a rate plus a cap.

7. **Venue and governing law are fixed in Ohio, with prevailing-party fees.** Any dispute points to Columbus, Ohio, and the loser pays attorneys' fees ([same file](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:229)). That materially raises the cost of fighting even a modest dispute.

A few lower-signal notes:

- The document quality is a bit rough in places: numbering artifacts, formatting noise, and some awkward wording. That does not automatically make it unenforceable, but it does mean I would want any negotiated points confirmed explicitly in writing rather than relying on implication.
- The insurance section is vague (`as required for specific engagements`) rather than setting concrete requirements up front ([same file](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:133)). That leaves room for later requests depending on the client engagement.

What I would push on first:

1. Remove the forfeiture of outstanding invoices and replace it with payment for all approved work performed through termination.
2. Add a reasonable liability cap, and at minimum exclude consequential / indirect damages from your exposure.
3. Add an IP carveout for preexisting materials, generic tools, libraries, and know-how.
4. If relevant, get written confirmation on whether you may perform/invoice through your LLC and whether any client-specific insurance requirements apply.

Residual risk: I am reading this as a business-risk review, not giving a legal opinion on enforceability under Ohio law. If you expect meaningful revenue here or any chance of dispute, this is worth a quick attorney pass because the biggest risks are economic rather than technical.
<!-- agent:boundary:247a6265 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#td-payment-termination] [recommended] Ask Test Double to revise the payment/termination terms so you do not forfeit outstanding invoices for early termination or unrelated breach after cure failure.
- [ ] [#td-liability-cap] [recommended] Ask for a reasonable liability cap and narrower indemnity, since the agreement currently has uncapped indemnity and no contractor-side cap.
- [ ] [#td-ip-carveout] [recommended] If you plan to reuse preexisting tools, libraries, or templates, get a written carveout so §5 does not sweep them into work-for-hire ownership.
- [ ] [#td-subcontract-consent] [recommended] Get written confirmation from Test Double before performing through an LLC or using any substitute, because §10.1 requires prior written consent for further subcontractors/subconsultants.
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
