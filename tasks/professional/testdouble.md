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
❯ Please help me with these 4 items?
### Re: help with the 4 items — gpt-5

Yes. The practical way to handle these is to send one short markup / business-points note rather than a long legal memo. I would keep it direct and framed as cleanup, not confrontation.

1. Payment / termination

Ask for: payment for all approved work performed through the termination date, even if the relationship ends early.

Fallback: if they will not delete the forfeiture concept entirely, limit it to a narrow, material, uncured breach by you, and never to already-earned approved invoices.

Suggested language:

> On payment/termination, I’d like to clarify that any work performed and approved through the termination date remains payable in the ordinary course. I’m not comfortable with forfeiting outstanding earned invoices based solely on early termination or a breach provision that is broader than actual unpaid damages.

Redline concept:

> Replace any forfeiture of outstanding invoices with: “Company will pay Subcontractor for all Services properly performed through the effective date of termination, subject to ordinary invoice review and any specific offset required by law or finally determined damages.”

2. Liability cap / indemnity

Ask for: an aggregate liability cap, and exclusion of indirect, incidental, special, and consequential damages.

Good target: total fees paid or payable under the SOW. Moderate fallback: 2x fees paid under the SOW. Also narrow indemnity to third-party claims only.

Suggested language:

> I’d also like a standard contractor-side liability structure: a reasonable aggregate cap on my liability, no consequential or indirect damages, and indemnity limited to third-party claims arising from my negligence, willful misconduct, or material breach.

Redline concept:

> “Except for fraud, willful misconduct, confidentiality breach, or infringement arising from Subcontractor materials provided in breach of this Agreement, each party’s aggregate liability under this Agreement will not exceed the fees paid or payable under the applicable Statement of Work. Neither party will be liable for consequential, incidental, indirect, special, exemplary, or punitive damages.”

3. IP carveout

Ask for: explicit retention of ownership in your preexisting materials, generic tools, libraries, templates, know-how, and improvements to those materials, while granting them a license only as needed to use the deliverables.

Suggested language:

> On IP, I want to make sure the assignment covers the deliverables created for the engagement, but not my preexisting materials, reusable libraries, templates, tools, or general know-how that I may use while performing the work.

Redline concept:

> “Notwithstanding anything to the contrary, Subcontractor retains all right, title, and interest in and to any preexisting materials, generic tools, libraries, templates, routines, processes, ideas, concepts, know-how, and other technology developed or owned by Subcontractor outside the scope of this Agreement (‘Background Technology’). To the extent any Background Technology is incorporated into a deliverable, Subcontractor grants Company and its client a non-exclusive, perpetual, worldwide license to use such Background Technology solely as incorporated in the deliverable.”

4. LLC / insurance / engagement logistics

Ask for two written confirmations:

- Whether they are comfortable with you invoicing / performing through your LLC, while you remain the named individual actually doing the work.
- Whether this engagement has any client-specific insurance requirements before you start.

Suggested language:

> I’d also like written confirmation on two operational points: first, whether I may invoice and perform through my LLC while remaining the individual providing the services; second, whether there are any client-specific insurance requirements for this engagement beyond what is already stated in the agreement.

If you want one combined note, I would send this:

> Thanks — I reviewed the agreement and had four cleanup items I’d like to confirm before signing:
>
> 1. Payment on termination: I want to confirm that all approved work performed through termination remains payable in the ordinary course, and that I do not forfeit already-earned outstanding invoices.
> 2. Liability: I’d like a standard liability cap tied to fees under the SOW, no consequential or indirect damages, and indemnity limited to third-party claims arising from my negligence, willful misconduct, or material breach.
> 3. IP: I’d like an express carveout confirming I retain ownership of my preexisting materials, reusable tools, libraries, templates, and general know-how, with a license only to the extent incorporated into deliverables.
> 4. Logistics: Please confirm whether I may invoice/perform through my LLC while remaining the individual service provider, and whether there are any engagement-specific insurance requirements.
>
> If helpful, I can mark these as redlines directly in the agreement language.

Negotiation priority, if they push back:

1. Do not concede on already-earned invoice forfeiture.
2. Push hard for the liability cap and consequential-damages exclusion.
3. Get the IP carveout if you use any of your own reusable code/tools.
4. Get the LLC / insurance answer in writing even if they refuse to amend the main form.

If you want, I can turn this into tighter redline-ready contract text clause by clause against the actual markdown agreement.
❯ Yes, please continue with the tighter redline-ready contract.
### Re: tighter redline-ready contract — gpt-5

Below is the tighter redline-ready version against the actual agreement structure. I would propose these as targeted edits rather than rewriting the whole form.

1. Section 2.2 (`Compensation > Breach`)

Current problem: it says that if you fail to terminate under Section 4 or otherwise breach, and do not cure within 10 days, you forfeit any pending or future payments. That is much too broad.

Suggested replacement for Section 2.2:

> **2.2 Breach.** In the event that Subcontractor breaches this Agreement, Company shall provide written notice describing the breach in reasonable detail, and Subcontractor shall have ten (10) days from receipt of such notice to cure the breach, if curable. If the breach remains uncured after the applicable cure period, Company may pursue any remedies available at law or in equity; provided, however, that Company shall remain obligated to pay Subcontractor for all Services properly performed through the effective date of termination. Subcontractor shall not forfeit payment for Services properly performed and invoiced, or properly performable through the date of termination, except to the extent of damages finally determined to be owed to Company by a court of competent jurisdiction or otherwise agreed in writing by the parties.

Shorter fallback version:

> Delete `Subcontractor shall forfeit any rights to any pending or future payments from Company` and replace with `Company shall pay Subcontractor for all Services properly performed through the effective date of termination.`

2. Section 4.2 (`Termination by Subcontractor`)

Current problem: if you terminate within the first 60 days, you automatically forfeit outstanding invoices. That is the worst clause in the form.

Suggested replacement for Section 4.2:

> **4.2 Termination by Subcontractor.** Subcontractor may terminate this Agreement upon written notice to Company. If termination occurs during the first sixty (60) days following the start date, Subcontractor will use commercially reasonable efforts to provide transition assistance during any requested notice period not to exceed ten (10) business days. After the first sixty (60) days, if Subcontractor wishes to terminate this Agreement prior to the end date, or if there is no reasonably clear ending date in the Statement of Work, Subcontractor may terminate the engagement by sending written notice of such termination to Company at least thirty (30) days before ending work. In all events, Company shall pay Subcontractor for all Services satisfactorily performed prior to termination within thirty (30) days of receipt of an invoice, subject only to ordinary invoice review and any rights expressly provided under this Agreement.

If they refuse a full rewrite, the minimum acceptable edit is:

> Replace `they forfeit any outstanding invoices` with `Company shall pay Subcontractor for all Services satisfactorily performed prior to termination in accordance with Section 2.1.`

3. New limitation-of-liability section, ideally after Section 11

Current problem: you have uncapped indemnity and exposure to consequential/indirect damages, with no overall cap.

Suggested new Section 11.1 or new Section 12:

> **Limitation of Liability.** EXCEPT FOR LIABILITY ARISING FROM A PARTY’S FRAUD, WILLFUL MISCONDUCT, BREACH OF CONFIDENTIALITY, OR INFRINGEMENT OR MISAPPROPRIATION OF THE OTHER PARTY’S INTELLECTUAL PROPERTY RIGHTS, EACH PARTY’S TOTAL AGGREGATE LIABILITY ARISING OUT OF OR RELATING TO THIS AGREEMENT SHALL NOT EXCEED THE TOTAL AMOUNTS PAID OR PAYABLE TO SUBCONTRACTOR UNDER THE APPLICABLE STATEMENT OF WORK DURING THE TWELVE (12) MONTHS PRECEDING THE EVENT GIVING RISE TO THE CLAIM. IN NO EVENT SHALL EITHER PARTY BE LIABLE FOR ANY CONSEQUENTIAL, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR PUNITIVE DAMAGES, OR FOR ANY LOST PROFITS, LOST REVENUE, OR LOSS OF BUSINESS OPPORTUNITY, WHETHER IN CONTRACT, TORT, OR OTHERWISE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGES.

If you want a stronger contractor position, change the cap to `the total amounts paid or payable under the applicable Statement of Work`. If you want a concessionary fallback, use `two (2) times the total amounts paid or payable`.

4. Section 11 (`Indemnification`)

Current problem: it covers `all claims and liabilities including consequential or indirect damages` and is not limited to third-party claims.

Suggested replacement for Section 11:

> **11. Indemnification.** Subcontractor agrees to indemnify, defend, and hold harmless Company from any third-party claims, damages, liabilities, costs, and reasonable attorneys’ fees to the extent arising from (a) Subcontractor’s gross negligence or willful misconduct in performing the Services, or (b) a claim that materials or deliverables created by Subcontractor specifically for Company under this Agreement infringe such third party’s intellectual property rights; provided, however, that Subcontractor shall have no obligation to the extent any claim arises from Company specifications, Company-provided materials, Client-provided materials, or modifications made by Company, Client, or third parties. In no event shall Subcontractor be liable under this Section for consequential, indirect, incidental, special, exemplary, or punitive damages.

If they push back on `gross negligence`, you can fall back to `negligence`, but keep the third-party-claim limitation and the exclusion of consequential/indirect damages.

5. Section 5 (`Work For Hire`)

Current problem: it sweeps in anything used in the work, with no carveout for your preexisting code, tools, libraries, templates, or know-how.

Suggested replacement for Section 5:

> **5. Work For Hire.** The Subcontractor expressly acknowledges and agrees that any deliverables specially prepared by Subcontractor for Company or Client under this Agreement, excluding Background Technology as defined below, shall be considered “work made for hire” and the exclusive property of Company or Client unless otherwise specified in writing. To the extent such deliverables may not be deemed a work made for hire under applicable law, Subcontractor hereby assigns to Company or Client all right, title, and interest in and to such deliverables. Notwithstanding the foregoing, Subcontractor retains all right, title, and interest in and to any preexisting materials, software, source code, object code, libraries, frameworks, tools, templates, routines, processes, documentation, concepts, know-how, and other technology or intellectual property owned, developed, or licensed by Subcontractor outside the scope of this Agreement, together with any modifications, enhancements, or derivatives thereof that do not constitute deliverables uniquely created for Company or Client (`Background Technology`). To the extent any Background Technology is incorporated into a deliverable, Subcontractor grants Company and Client a non-exclusive, perpetual, worldwide, fully paid-up license to use, copy, execute, display, and distribute such Background Technology solely as incorporated in, and as necessary to use, the deliverable.

That is the cleanest version. If they want something shorter, keep at least the `Notwithstanding the foregoing... Background Technology` sentence and the license-back sentence.

6. Section 6 (`Insurance`)

Current problem: it is vague and lets them impose unknown later requirements.

Suggested replacement for Section 6:

> **Insurance.** Subcontractor shall maintain any insurance coverage expressly set forth in the applicable Statement of Work or otherwise agreed in writing by the parties prior to commencement of the applicable Services. Except as expressly agreed in writing for a specific engagement, no additional insurance requirements shall apply.

If they do not want to amend the clause, at least get this in email:

> `Please confirm whether there are any engagement-specific insurance requirements for this assignment. If so, please identify them before work begins.`

7. Section 10.1 and Section 7 (`LLC / invoicing through entity`)

The agreement does not clearly allow you to swap the contracting party from you personally to your LLC. I would not try to solve that with implication. Ask for an explicit rider or email confirmation.

Suggested add-on sentence to Section 7.1:

> Notwithstanding the foregoing, Subcontractor may elect to invoice Company through an entity controlled by Subcontractor, provided that Brian Kenji Takita remains personally responsible for performance of the Services and compliance with this Agreement, and such invoicing arrangement shall not be deemed an unauthorized subcontract under Section 10.1.

If they do not want to amend the agreement text, send this confirmation request:

> `Please confirm that I may invoice through my LLC, while I remain the individual personally performing the services, and that doing so is not considered use of a further subcontractor or subconsultant under Section 10.1.`

What I would actually send back first

> Thanks. I reviewed the subcontractor agreement and would like to request a few targeted edits before signing:
>
> 1. Remove the forfeiture language in Section 2.2 and Section 4.2 so that all services properly performed through termination remain payable.
> 2. Add a standard limitation of liability, including a reasonable aggregate cap and exclusion of consequential and indirect damages.
> 3. Narrow Section 11 so indemnity is limited to third-party claims and does not include consequential or indirect damages.
> 4. Revise Section 5 to carve out my preexisting tools, libraries, templates, code, and general know-how, with a license only to the extent incorporated into deliverables.
> 5. Clarify any engagement-specific insurance requirements in advance.
> 6. Confirm that I may invoice through my LLC while remaining the individual performing the services, and that this does not violate Section 10.1.
>
> I’m happy to mark these directly in redline form if useful.

Negotiation floor

- Non-negotiable: remove invoice forfeiture for already-earned work.
- Strongly recommended: add liability cap and consequential-damages exclusion.
- Important if you reuse your own assets: add the IP / Background Technology carveout.
- Operationally important: get LLC and insurance confirmation in writing even if the contract form stays mostly intact.

If you want, the next step can be a tighter minimal-markup version that assumes Test Double will only accept a few sentence-level edits rather than full clause replacements.
❯ Is there any language on the frequency of invoices?
### Re: invoice frequency — gpt-5

Yes. The agreement does speak to invoice frequency, and it is fairly specific.

The operative language is in [Test_Double_Subcontractor_Agreement_Brian_Takita.md](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:23): you `shall submit to Company semi-monthly invoices` for the prior invoicing period, defined as `1st to the 15th` and `16th to the last day of the month`. It also says all invoices must be delivered within `ten (10) days` after completion of the invoiced work, and payment is due within `30 days` after receipt of each invoice ([same file](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:25)).

Exhibit A repeats the cadence: `Please invoice Test Double semi-monthly, clearly indicating the client and project name on the invoice` ([same file](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:315)).

So the practical read is:

- Invoice twice per month.
- One invoice covers the `1st-15th` period.
- The next covers the `16th-end of month` period.
- Each invoice should be sent within 10 days after that billing period’s work is completed.
- Test Double then has 30 days from receipt to pay.

The only mild ambiguity is that Section 2.1 says `number of days worked`, while Exhibit A prices the work at `$100/hr @ 40hrs week`. That is not fatal, but it is worth making sure your invoice format matches how they actually expect time to be reported: hours with period totals, not just day counts.
❯ Can I work less than 40 hours/week by choice?
### Re: working less than 40 hours — gpt-5

Not clearly as a contract right.

The agreement does **not** say, in direct terms, `you must work 40 hours per week minimum`. What it does say is:

- In the SOW, `Price: $100/hr @ 40hrs week, prorated to the nearest half day` ([Test_Double_Subcontractor_Agreement_Brian_Takita.md](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:309)).
- Also in the SOW, `Budget constraints: No more than 40hrs/week without prior written consent` ([same file](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:313)).
- In Section 1, you agree to devote `such portion of [your] professional time, attention, energy, loyalty, and skill as may be necessary to fully perform the Services` ([same file](/home/brian/work/btakita/agent-loop/tasks/professional/testdouble/Test_Double_Subcontractor_Agreement_Brian_Takita.md:10)).

My reading is:

- `No more than 40hrs/week without prior written consent` is a **cap**, not a guaranteed minimum.
- `@ 40hrs week` looks like an expected working cadence or planning assumption, not a clean promise by Test Double to give you 40 hours or a clean promise by you to always provide 40 hours.
- But Section 1 gives them an argument that you must put in whatever time is necessary to perform the services properly. So if the work actually requires near-full-time availability, choosing to work materially less could create friction even if there is no explicit `minimum 40` sentence.

So the practical answer is: **the contract does not clearly give you a right to unilaterally decide to work less than 40 hours/week if that would impair performance, but it also does not clearly impose a hard 40-hour minimum as written.**

If your real question is `can I intentionally treat this as part-time`, I would not rely on the current wording. I would get it confirmed in writing with something like:

> `Please confirm whether the 40 hours/week reference is only an upper cap / planning assumption, or whether the engagement requires a minimum weekly time commitment. If there is a minimum expected weekly commitment, please specify it explicitly.`

If you want, I can also draft a short clarification note asking them to define expected weekly availability and whether hours are guaranteed, capped, or just estimated.
<!-- agent:boundary:c71c8ade -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#td-payment-termination] Ask Test Double to revise the payment/termination terms so you do not forfeit outstanding invoices for early termination or unrelated breach after cure failure.
- [ ] [#td-liability-cap] Ask for a reasonable liability cap and narrower indemnity, since the agreement currently has uncapped indemnity and no contractor-side cap.
- [ ] [#td-ip-carveout] If you plan to reuse preexisting tools, libraries, or templates, get a written carveout so §5 does not sweep them into work-for-hire ownership.
- [ ] [#td-subcontract-consent] Get written confirmation from Test Double before performing through an LLC or using any substitute, because §10.1 requires prior written consent for further subcontractors/subconsultants.
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
