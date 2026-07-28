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
<!-- agent:boundary:6ec7ff51 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog queue -->
- [ ] [#haiven-paper] Obtain and review revised Haiven offer, equity grant terms, and employment/IP agreement before signing; confirm the fully diluted 1% numerator/denominator and Board approval in writing.
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/recruit/recruit.done.md -->
<!-- /agent:done -->
