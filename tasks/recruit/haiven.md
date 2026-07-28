---
agent_doc_session: 7a3dfe1a-875b-4b93-9e8e-12d156448f63
agent: codex
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
codex_args: -s danger-full-access
queue: stop
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
<!-- agent:boundary:18f5eefa -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#haiven-paper] Obtain and review revised Haiven offer, equity grant terms, and employment/IP agreement before signing; correct the work location to Connecticut, confirm the fully diluted 1% numerator/denominator and Board approval, and obtain an express Acadian compensation/IP/outside-work carve-out.
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/recruit/recruit.done.md -->
<!-- /agent:done -->
