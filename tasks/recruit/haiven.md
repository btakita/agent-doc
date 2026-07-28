---
agent_doc_session: 7a3dfe1a-875b-4b93-9e8e-12d156448f63
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
codex_args: -s danger-full-access
queue: go
---

## Status

<!-- agent:status patch=replace -->


<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/e715e181b73b2efd56586b6dc037817620083b669835f8f4d448e1adedf65952-20260728-171529.md`*

Compacted content:
- Prior summary/context: prior compacted content: Archived 20 response topic(s): simplify LOADTEST.md + DESIGN.md + README.md; why a friend-fr...
Please read through:
- [haiven-offer-letter-brian-takita-jul-2026.md](../professional/haiven/haiven-offer-letter-brian-takita-jul-2026.md)
- [haiven-at-will-employee-contractor-agreement.md](../professional/haiven/haiven-at-will-employee-contractor-agreement.md)

Does everything look good? Should I ask for the fully-diluted 1% equity in writing?

### Re: Haiven offer and equity — gpt-5

Short answer: **I would not sign these as written, and yes—absolutely get the fully diluted 1% in writing before accepting.** The salary, bonus, and four-year vesting schedule are plausible, but several clauses are ambiguous or directly contradictory. The equity could be a major part of the economics, so “eligible to receive one percent” is not precise enough.

The offer’s entire-agreement clause is especially important: it says the final written documents supersede prior oral and written understandings. If “1% fully diluted” was promised verbally, assume it does not survive unless the signed documents state it.

#### Must fix before signing

1. **Define and approve the 1%.** Ask for the exact number of options, the fully diluted share count used as the denominator, the measurement date, and the treatment of preferred stock, the granted and ungranted option pool, warrants, SAFEs, and convertible notes. Replace “eligible to receive” with a binding commitment subject only to formal Board approval, require Board action within a stated period, and make the vesting commencement date your first day—not the later grant date. Also obtain the equity plan and proposed option agreement before signing.

   Suggested language:

   > Subject to Board approval, the Company will grant Employee an option to purchase **[N] shares**, representing **1.00% of the Company’s Fully Diluted Capitalization immediately following the grant as of [date]**. The Company will provide the numerator, denominator, and treatment of all outstanding or reserved equity and convertible securities in writing. The Board will consider the grant no later than [30] days after Employee’s start date, and vesting will commence on July 29, 2026.

   Also confirm ISO versus NSO treatment, exercise price/current 409A value, option term, post-termination exercise window, and any change-of-control treatment. A grant equal to 1% on the grant date will normally dilute in later financings; if the actual promise was to *maintain* 1%, a top-up right must also be written, though that is unusual for an employee grant.

2. **Make the pre-cliff protection operative.** “Prorated vesting credit” is undefined. It should say that termination without cause before the cliff causes actual vesting of 1/48 of the grant for each completed month of service (or whatever exact formula you agree), with “Cause” narrowly defined. A vague “credit” may not produce exercisable shares.

3. **Delete the resurrected noncompete.** Section 10 is struck, but Exhibit C says you must observe a two-year noncompete and references a nonexistent Section 10.2 and “Geographic Area.” Section 9 also obligates you to sign that certificate later. Delete that paragraph from Exhibit C and make Section 9 subject only to a corrected certificate. Massachusetts’ statute imposes specific notice, counsel, duration, consideration/garden-leave, choice-of-law, and venue requirements; foreign law cannot be used to avoid those protections for a qualifying Massachusetts employee. [Massachusetts General Laws c. 149, § 24L](https://malegislature.gov/Laws/GeneralLaws/PartI/TitleXXI/Chapter149/Section24L)

4. **Delete or radically narrow Section 11.** It currently says *all* compensation from third-party services—including consulting fees and equity—belongs to Haiven unless the Board unanimously approves otherwise. That conflicts with the offer’s express permission for disclosed external contracting. Replace it with a conflicts-only rule and attach written approval for your existing contracting, advisory work, and open-source activity.

5. **Protect prior IP explicitly.** Section 3.2 broadly assigns inventions created during employment, relies on a Washington statutory carve-out, and Section 3.4 says that an empty Exhibit A means you have no prior inventions. The offer uses a different carve-out, while you will work from Massachusetts. Attach a detailed Exhibit A covering your existing repositories, organizations, products, libraries, writings, trademarks, and active side projects, and obtain a countersigned acknowledgment that they remain yours. Do not incorporate any of them into Haiven products without a separate written license or approval.

6. **Unify governing law, venue, and arbitration.** The offer selects Wyoming law and Laramie County courts; the other agreement selects Washington law, Spokane courts, and Spokane arbitration. Ask for Massachusetts law and a Massachusetts or remote forum. If arbitration remains, require the Company to pay the arbitration and arbitrator costs, preserve statutory remedies and fee-shifting, and permit remote proceedings. The current 50/50 cost clause is materially worse than the AAA’s current employment framework, under which employee filing fees are capped and the employer ordinarily pays the remaining administrative fees and arbitrator compensation. [AAA Employment/Workplace rules and fees](https://adr.org/rules-forms-and-fees/employment/)

#### Also clarify in writing

- **Signing bonus:** Define “Cause,” the exact prorated-repayment formula, that repayment applies only to amounts actually paid, and that nothing is owed after termination without cause, layoff, death/disability, or resignation for agreed “Good Reason.” Make the second $10,000 payable on a date certain—regardless of whether “official launch” is declared—and address what happens if the Company terminates you first.

- **Home-office “pre-tax offset”:** Do not elect this as currently described. The IRS says an amount labeled reimbursement is treated as wages under a nonaccountable plan when it replaces wages the employer otherwise would have paid. Ask for a separate, written accountable-plan reimbursement reviewed by payroll/tax counsel; it should not reduce the stated $155,000 salary. [IRS Publication 15 (2026)](https://www.irs.gov/publications/p15)

- **Benefits and Massachusetts employment setup:** Confirm W-2 status, Massachusetts payroll registration, unemployment/workers’ compensation/PFML coverage, health-plan effective date, employer premium contribution, dependent costs, deductible, and vacation accrual/carryover. Massachusetts treats promised vacation pay as wages, including accrued vacation due at separation. [M.G.L. c. 149, § 148](https://malegislature.gov/Laws/GeneralLaws/PartI/TitleXXI/Chapter149/Section148)

- **Background check:** Change “verify no prior criminal record” to a lawful, individualized background-check condition with written consent and required pre-adverse-action procedures. Massachusetts limits what criminal-history information employers may request or use. [Massachusetts CORI employment guide](https://www.mass.gov/guides/guide-to-criminal-records-in-employment-and-housing)

- **Confidentiality carve-outs:** Add standard exceptions for independently developed information, lawful third-party receipt, discussing wages and working conditions, cooperating with government investigations, protected whistleblowing, and the federal trade-secret immunity notice required in employee confidentiality agreements. [18 U.S.C. § 1833(b)](https://uscode.house.gov/view.xhtml?req=%28title%3A18+section%3A1833+edition%3Aprelim%29)

- **Execution:** Ensure the Company’s legal entity, authorized signer’s printed name/title, signature, and date appear on every final document. The stock grant itself must still receive the required Board approval.

#### Suggested message

> I’m excited about the role and would like to move forward. Before I sign, I need the documents revised—or a countersigned side letter—to confirm that the option grant is for an exact number of shares equal to 1.00% of fully diluted capitalization immediately after grant, with the numerator, denominator, convertible-security treatment, Board-approval deadline, and July 29 vesting commencement stated in writing. I also need the Section 11 outside-compensation assignment narrowed, my existing open-source and side projects acknowledged on Exhibit A, the noncompete paragraph removed from Exhibit C, the pre-cliff vesting protection made explicit, and the conflicting Wyoming/Washington forum provisions reconciled for a Massachusetts employee. Please also send the equity plan and proposed option agreement. Because these materials are part of the deal, please extend the acceptance deadline until one business day after I receive the revised documents.

This is issue-spotting, not legal advice. Given the three-state conflict and the value of a genuine 1% grant, a one-hour review by a Massachusetts startup/employment lawyer would be money well spent.
❯ I'm working in a Connecticut home office. Acadian, my other contract, is in Massachusetts. Does that change anything?
### Re: Connecticut work location and Acadian — gpt-5

Yes—it changes the **state-law and payroll analysis**, but not the main conclusion about the equity or the problematic clauses.

The key fact for Haiven is where you physically perform the Haiven work. If that is your Connecticut home office, Connecticut—not Massachusetts—is the principal employee-location state. Acadian’s Massachusetts location does not, by itself, make you a Massachusetts employee of Haiven.

#### Corrections to my prior review

- Replace my recommendation for **Massachusetts law/venue** with **Connecticut law and a Connecticut or remote forum**. The Wyoming-versus-Washington conflict still needs to be fixed. Do not rely on the Massachusetts noncompete statute I cited earlier unless you actually perform Haiven work in Massachusetts or counsel identifies another Massachusetts nexus. The stray two-year noncompete in Exhibit C remains internally contradictory and should still be deleted.

- Replace the Massachusetts payroll, leave, vacation, and CORI checklist with a **Connecticut employment-compliance checklist**. Have the offer show your actual Connecticut work address instead of “Greater Boston, MA.” Confirm that Haiven—or its PEO/employer-of-record—is registered for Connecticut withholding, unemployment, workers’ compensation, and Connecticut Paid Leave. Connecticut requires employers with Connecticut workers to register for withholding, and Connecticut residents are taxed on all wages. [Connecticut DRS employer registration guidance](https://portal.ct.gov/drs/businesses/new-business-resource-center/registering-with-drs)

- The offer’s twice-monthly “15th and 31st” payroll is now a specific issue. Connecticut generally requires weekly or biweekly pay; an employer needs a Labor Commissioner waiver to use a less-frequent regular schedule. Ask Haiven to change this to biweekly or confirm that it has the required waiver. [Conn. Gen. Stat. §§ 31-71b and 31-71i](https://www.cga.ct.gov/current/pub/chap_558.htm)

- The federal concern about replacing salary with a purportedly “pre-tax” home-office rent payment is unchanged. Connecticut residency does not cure that accountable-plan issue.

#### Acadian makes the outside-work conflict more concrete

The Acadian engagement should be disclosed and approved **by name, in writing, before you sign**. Merely telling a manager is not enough because:

- the offer permits limited disclosed contracting;
- Section 4 of the second agreement prohibits consulting related to Haiven’s present or future business; and
- Section 11 purports to give Haiven *all compensation* from third-party services unless its Board unanimously approves otherwise.

As written, Section 11 could be asserted against your Acadian fees or equity even though Acadian is in another state. Its wording is not geographically limited. You need Section 11 deleted or an express contractual carve-out—not an informal assurance that they would never enforce it.

Suggested side-letter language:

> Notwithstanding Sections 3, 4, 8, and 11 of the At-Will Employee and Contracting Agreement, the Company acknowledges Employee’s pre-existing engagement with Acadian under the agreement dated [date], approves its continuation, and disclaims any right to compensation, equity, work product, inventions, or confidential information arising from that engagement, provided Employee performs it on separate time, equipment, systems, and accounts; does not use or disclose Haiven Confidential Information; and it does not materially interfere with Employee’s Haiven duties. This approval may not be revoked retroactively.

Also have Haiven acknowledge that:

- Acadian’s confidential information and work product are not Haiven property;
- Haiven will not require you to disclose Acadian material;
- your Acadian work and all pre-existing tools/projects used in it are listed or cross-referenced in Exhibit A; and
- any future claim of conflict will be handled prospectively, rather than forfeiting compensation already earned.

You should separately review the Acadian agreement for IP assignment, confidentiality, availability, and outside-employment provisions. Two broad invention-assignment agreements can create competing ownership claims even when the businesses are unrelated. Keep separate devices, accounts, repositories, credentials, calendars, and working hours, and never move code, prompts, data, or architecture between them.

#### Tax distinction

If you are a Connecticut resident and physically perform all Acadian services from Connecticut, Acadian’s Massachusetts address alone does not necessarily make the compensation Massachusetts-source. Massachusetts generally sources personal-service compensation to services actually performed in Massachusetts; days physically worked there may require allocation. [Massachusetts nonresident-income guidance](https://www.mass.gov/info-details/ma-gross-adjusted-gross-and-taxable-income-for-nonresidents)

The answer can differ if you travel to Massachusetts, maintain a business location there, work through an entity, receive partnership/pass-through income, or Acadian treats you as an employee rather than an independent contractor. Connecticut taxes residents on all income and generally provides a credit mechanism for qualifying tax paid to another state. [Connecticut 2026 withholding guidance](https://portal.ct.gov/DRS/Withholding-Taxes/Tax-Information) Have a CT/MA tax preparer confirm the classification and sourcing rather than letting either payer guess.

#### What does not change

Still insist on the fully diluted 1% in signed writing with the exact option count, denominator, measurement date, Board-approval deadline, and vesting commencement date. Still fix the pre-cliff vesting language, Section 11, Exhibit A/IP scope, the noncompete paragraph, signing-bonus definitions, arbitration costs, and the missing stock-plan documents.

For counsel, the best fit is now a **Connecticut employment/startup lawyer comfortable with multistate remote work**, with the Acadian agreement included in the review. The most urgent practical request to Haiven is: correct the work location to Connecticut, confirm CT payroll compliance, and countersign the Acadian carve-out alongside the fully diluted equity confirmation.
❯ Please draft an email clearing explaining what needs to be changed and why. Disclose my other contracts (Acadian Financial Services, monsterrodholders.com, Equity Funding Source) to be in compliance with the contract.

### Re: Response

The `agent-doc` cycle cannot start because the prior committed projection is still awaiting JetBrains delivery acknowledgment. Your new email-drafting prompt is detected and preserved.

Please reload or close/reopen `haiven.md` in JetBrains, then invoke the same `agent-doc` command again. No replacement or forced write was attempted.
Replace the blocker response with the actual email draft. My Acadian engagement is contracted through Apollo Solutions; Acadian Financial Services is the end client. Also disclose MonsterRodHolders.com and Equity Funding Source.

### Re: Email requesting revisions and disclosing outside engagements — gpt-5

Here is a sendable draft. Replace the bracketed items with the exact legal entity names, approximate weekly hours, and expected duration before sending.

```text
Subject: Haiven offer documents — requested clarifications and outside-engagement disclosure

Hi Eddie,

Thank you again for the offer. I’m excited about the role and would like to move forward. I reviewed the offer letter and At-Will Employee and Contracting Agreement carefully. Before I sign, I’d like the final documents—or a countersigned addendum—to address the items below so they accurately reflect our discussions, my Connecticut work location, and my existing outside activities.

1. Equity grant

Please revise the equity provision to confirm that, subject to Board approval, Haiven will grant me an option for an exact number of shares equal to 1.00% of the Company’s fully diluted capitalization immediately following the grant. Please state:

- the number of option shares;
- the fully diluted denominator and measurement date;
- how outstanding preferred shares, granted and ungranted option-pool shares, warrants, SAFEs, and convertible notes are treated;
- that the Board will consider/approve the grant within 30 days after my start date;
- that vesting begins on my July 29, 2026 start date, regardless of the later paperwork or approval date; and
- whether the grant will be an ISO or NSO, its exercise price/current 409A value, option term, post-termination exercise period, and change-of-control treatment.

The current phrase “eligible to receive one percent” does not identify the shares or capitalization denominator, and the entire-agreement language would supersede any prior oral understanding. I want to ensure the agreed 1% is unambiguous in the signed documents. Please also send me the equity incentive plan and proposed stock-option agreement before I sign.

2. Formal disclosure and approval of existing outside engagements

To comply with the offer’s disclosure requirement, I am formally disclosing the following pre-existing activities:

- Apollo Solutions [exact contracting entity]: my contract/freelance counterparty for a technology engagement in which Acadian Financial Services is the current end client; approximately [hours] per week through [date or “ongoing”]. I do not contract directly with Acadian.
- MonsterRodHolders.com [exact legal entity, if any]: my pre-existing independently owned business/activity and its associated website, products, software, content, operations, compensation, and intellectual property; approximately [hours] per week.
- Equity Funding Source [exact legal entity]: my pre-existing independent business/engagement and its associated website, software, content, operations, compensation, and intellectual property; approximately [hours] per week.

These activities will be performed outside Haiven working time using separate equipment, accounts, repositories, credentials, and systems. I will not use or disclose Haiven confidential information in them, disclose their confidential information to Haiven, or allow them to materially interfere with my Haiven duties.

Please confirm in the signed documents that Haiven approves their continuation and that, notwithstanding Sections 3, 4, 8, and 11 of the At-Will Employee and Contracting Agreement:

- all compensation, fees, equity, work product, inventions, and confidential information arising from these activities remain with me or the applicable third party, as their governing agreements require;
- Haiven acquires no ownership or license merely because the activity continues during my employment;
- Haiven will not require disclosure of another party’s confidential information;
- the approval cannot be revoked retroactively or used to claim compensation already earned; and
- any later conflict concern will be addressed prospectively and in good faith.

Section 11 currently states that all third-party compensation belongs to Haiven unless unanimously approved by the Board. That conflicts with the offer’s permission for disclosed external contracting and is much broader than necessary to prevent an actual conflict. My preference is to replace Section 11 with a conflicts-only provision; at minimum, I need a specific written carve-out and whatever Board approval the present language requires for the three activities above.

3. Prior intellectual property

Please incorporate a completed Exhibit A identifying my pre-existing repositories, software libraries, websites, products, writings, trademarks, and other projects, including those associated with the activities above. The agreement should confirm that this prior IP remains excluded from Company Inventions and Work Product. If Haiven later wants to use any excluded project, we can document that separately in writing.

This is important because the current agreement broadly assigns inventions created during employment, uses a Washington-law exception even though I will work in Connecticut, and treats an empty Exhibit A as a representation that I have no prior inventions.

4. Internal inconsistencies and termination terms

Please make the following corrections:

- Delete the final noncompetition paragraph in Exhibit C. Section 10 is intentionally struck, but Exhibit C reintroduces a two-year noncompete and refers to a nonexistent Section 10.2 and “Geographic Area.”
- Replace “prorated vesting credit” upon termination without cause before the cliff with actual vesting under a stated formula—preferably 1/48 of the option grant for each completed month of service—and define “Cause.”
- Define the signing-bonus repayment formula and “Cause”; limit repayment to amounts actually paid; and confirm that no repayment is due after termination without cause, layoff, death/disability, or agreed Good Reason. Please make the second $10,000 payable on a date certain no later than December 31, 2026, regardless of whether an “official launch” is declared.

These changes remove contradictory or undefined terms that otherwise make the promised protections difficult to apply.

5. Connecticut work location, payroll, and dispute provisions

I will perform the role remotely from Connecticut. Please update the offer’s “Greater Boston, MA” location to my Connecticut address and confirm that Haiven or its payroll provider is set up for Connecticut withholding, unemployment, workers’ compensation, and Connecticut Paid Leave.

The current twice-monthly pay schedule should either be changed to weekly/biweekly or accompanied by confirmation that Haiven has the Connecticut Labor Commissioner waiver required for a less-frequent schedule.

The documents also conflict: the offer selects Wyoming law and Laramie County courts, while the other agreement selects Washington law, Spokane courts, and Spokane arbitration. Please use Connecticut law and a Connecticut or remote forum. If arbitration remains, it should proceed remotely under the AAA Employment/Workplace Rules, preserve all statutory remedies and fee-shifting, and require Haiven to pay the arbitration and arbitrator costs other than a filing fee no greater than the fee for filing the same claim in court.

6. Home-office payment and remaining documents

Please remove the suggestion that part of my $155,000 salary can be converted into a “pre-tax” rent payment. If Haiven offers home-office reimbursement, it should be a separate accountable-plan benefit that does not reduce base salary and has been approved by Haiven’s payroll/tax advisers.

Please also provide the employee handbook, benefit summaries and employee/dependent premium costs, health-coverage effective date, vacation accrual/carryover policy, equity plan, and proposed option agreement before acceptance, since the offer incorporates several of those materials into the employment relationship.

I’m raising these items to make the written documents consistent with the deal we discussed and to avoid ambiguity for either side. I’m happy to discuss them by phone, but I need the final terms and outside-engagement approvals in signed writing. Please extend the acceptance deadline until one business day after I receive the revised documents or countersigned addendum.

I’m looking forward to working together.

Best,
Brian
```

Before sending:

- Use Apollo’s exact legal entity from your signed agreement; Apollo Solutions is the contracting intermediary and Acadian Financial Services is the end client.
- Supply truthful hours and duration. Do not state that the activities occur outside Haiven time or do not interfere unless you can operationally maintain that separation.
- Use the registered entity names for MonsterRodHolders.com and Equity Funding Source, if they have them; otherwise describe whether each is an owned business, advisory role, or client engagement.
- Attach a proposed Exhibit A or at least say it will follow before signature. A list of repository organizations, domains, products, and active projects is more useful than a vague reference to “all prior IP.”
- Send the request to the authorized signer and ask that any approval required by Section 11 be reflected in Board consent or a countersigned amendment—not merely an email from a manager.
It is Craig + Luke, not Eddie. Please write draft into a md file so we can revise.

### Re: Revision-ready email draft — gpt-5

Created [haiven-offer-revision-email-draft.md](../professional/haiven/haiven-offer-revision-email-draft.md), addressed to **Craig and Luke**. The draft consolidates the proposed equity clarification, Apollo Solutions/Acadian disclosure and approval, outside-project carve-outs, prior-IP protection, noncompete removal, vesting and bonus clarification, Connecticut employment terms, and remaining-document requests.

I left the date, recipient addresses, and signature as explicit placeholders for revision. The draft is committed as `e3cf6c0ae` (`docs: draft Haiven offer revision email`).

Verification: the new file has no uncommitted changes; the document and agent-doc test suites passed. The repository-wide `make check` later encountered four Corky loopback tests competing for a local port, and all four passed on a serial targeted rerun. This repository has no GitHub Actions workflow named `CI`, so no matching run was available to inspect.

### Re: Where the disclosure requirement appears — gpt-5

The direct disclosure requirement is in the offer letter under **“External Contracting”**:

> “Limited external contracting is permissible, provided that such activities do not interfere with your responsibilities at Haiven and are disclosed to the Company.”

See [haiven-offer-letter-brian-takita-jul-2026.md, lines 82–83](../professional/haiven/haiven-offer-letter-brian-takita-jul-2026.md#L82-L83).

Two provisions in the separate At-Will Employee and Contracting Agreement make a specific written disclosure and approval especially important:

- **Section 4, “No Conflicting Employment”** prohibits other employment, consulting, or business activity directly related to Haiven’s current or future business, plus any activity that conflicts with your Haiven obligations. See [lines 142–147](../professional/haiven/haiven-at-will-employee-contractor-agreement.md#L142-L147).
- **Section 11, “Compensation”** says all compensation for third-party services during your Haiven employment—including equity and consulting or advisory fees—must be paid to Haiven unless Haiven’s Board unanimously approves otherwise in writing. See [lines 191–195](../professional/haiven/haiven-at-will-employee-contractor-agreement.md#L191-L195).

So, strictly speaking, **the documents do not say that you must list every “other party” by name**. The offer says the *external contracting activities* must be disclosed. I recommended naming Apollo Solutions, identifying Acadian Financial Services as its end client, and describing MonsterRodHolders.com and Equity Funding Source so that:

1. Haiven cannot later argue that the disclosure was too vague;
2. the Section 11 written Board approval unmistakably covers the income and IP associated with each activity; and
3. you have a clear written record that Haiven knew about the engagements before you signed.

The draft’s wording can be softened for precision. Instead of saying, “To comply with the offer’s disclosure requirement, I am formally disclosing the following pre-existing activities,” it could say:

> “The offer requires external contracting activities to be disclosed to Haiven. To satisfy that requirement and obtain the written approval contemplated by Section 11, I am disclosing the following pre-existing activities.”

That accurately separates the offer’s express disclosure requirement from our recommendation to identify the parties sufficiently for a reliable carve-out. This is contract issue-spotting, not a legal conclusion about the minimum disclosure a court would require.

### Re: Please let me know where they stated I need to disclose my other parties.

The express requirement is in the offer letter’s **“External Contracting”** paragraph: “Limited external contracting is permissible, provided that such activities do not interfere with your responsibilities at Haiven and are disclosed to the Company.” See [lines 82–83](../professional/haiven/haiven-offer-letter-brian-takita-jul-2026.md#L82-L83).

It requires disclosure of the **activities**, not literally a list of every party by name. Naming the parties is my risk-reduction recommendation because Section 11 of the other agreement says compensation from third-party services goes to Haiven unless its Board unanimously approves otherwise in writing. See [lines 191–195](../professional/haiven/haiven-at-will-employee-contractor-agreement.md#L191-L195). A named disclosure makes it much harder to dispute what the approval covers.

Section 4 also prohibits consulting or business activity related to Haiven’s business or conflicting with your obligations, but does not itself establish a disclosure procedure. See [lines 142–147](../professional/haiven/haiven-at-will-employee-contractor-agreement.md#L142-L147).

### Re: Please let me know where they stated I need to disclose my other parties.

The express requirement is in the offer letter’s **“External Contracting”** paragraph: “Limited external contracting is permissible, provided that such activities do not interfere with your responsibilities at Haiven and are disclosed to the Company.” See [lines 82–83](../professional/haiven/haiven-offer-letter-brian-takita-jul-2026.md#L82-L83).

It requires disclosure of the **activities**, not literally every party by name. Naming the parties is a risk-reduction recommendation so any written approval clearly covers them. That matters because Section 11 says compensation from third-party services goes to Haiven unless the Board unanimously approves otherwise in writing. See [lines 191–195](../professional/haiven/haiven-at-will-employee-contractor-agreement.md#L191-L195).

Section 4 restricts consulting or business activity related to Haiven’s business or conflicting with your obligations, but does not itself prescribe disclosure by name. See [lines 142–147](../professional/haiven/haiven-at-will-employee-contractor-agreement.md#L142-L147).

### Re: Disclosure requirement

> **Queue prompt:**
>
> re: "These activities will be performed outside Haiven working time using separate equipment, accounts, repositories, credentials, and systems. I will not use or disclose Haiven confidential information in them, disclose their confidential information to Haiven, or allow them to materially interfere with my Haiven duties.". I will use my same machine for all contract work. I will also be running AI with brief task switching during my Haiven hours. Should I disclose that or not? It will not interfere with fulfilling my Haiven work.


> **Queue prompt:**
>
> Please do not over-disclose. If Haiven is not asking for the number of hours worked for my other contracts, please don't include the number.


> **Queue prompt:** Please let me know where they stated I need to disclose my other parties.

The direct requirement is in the offer letter’s **“External Contracting”** paragraph: “Limited external contracting is permissible, provided that such activities do not interfere with your responsibilities at Haiven and are disclosed to the Company.” See [lines 82–83](../professional/haiven/haiven-offer-letter-brian-takita-jul-2026.md#L82-L83).

That language requires disclosure of the **activities**, not literally every party by name. Naming the parties is a risk-reduction recommendation so the written approval clearly identifies what it covers. This matters because Section 11 says compensation from third-party services goes to Haiven unless its Board unanimously approves otherwise in writing. See [lines 191–195](../professional/haiven/haiven-at-will-employee-contractor-agreement.md#L191-L195).

Section 4 restricts consulting or business activity related to Haiven’s business or conflicting with your obligations, but does not itself prescribe disclosure by name. See [lines 142–147](../professional/haiven/haiven-at-will-employee-contractor-agreement.md#L142-L147).
<!-- agent:boundary:fc3af24d -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue go -->
- do [#haiven-paper]
- The email I'm replying to is at https://mail.google.com/mail/u/0/#inbox/FMfcgzQhVXGNlfntxSpmgZRhHwWRslzp. Use chromium-bridge to draft the reply. Do not send. I will look it over.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#haiven-paper] Revise and send tasks/professional/haiven/haiven-offer-revision-email-draft.md, then obtain revised, countersigned Haiven documents before signing: Connecticut work/payroll terms; fully diluted 1% numerator, denominator, and Board approval; and express compensation/IP/outside-work carve-outs for the Apollo Solutions engagement serving Acadian Financial Services, MonsterRodHolders.com, and Equity Funding Source.
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/recruit/haiven.done.md -->
<!-- /agent:done -->
