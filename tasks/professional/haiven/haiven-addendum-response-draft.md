# Haiven addendum response draft

Status: **Draft only — do not send or place in Gmail without Brian's explicit instruction containing “send.”**

Thread: <https://mail.google.com/mail/u/0/#inbox/FMfcgzQhVhWChVfCmrxpKKxzwmJtNMxH>

## Review notes

- The newly downloaded `Employment Addendum - Brian Takita v1.0.pdf` still has the visible page-one corruption: `Haiven, Inc. (“Comp`, followed by the standalone text `Employee will attach`, followed by `any”)`. It needs to be regenerated and re-signed.
- Section 1 says the exercise price will be the common-stock fair market value on the grant date under the latest 409A valuation, but it does not state the current 409A value or estimated exercise cost.
- If 16,053 shares are exactly 1.00% of fully diluted capitalization immediately after grant, the implied denominator is 1,605,300 shares. Haiven should confirm that denominator and its measurement date.
- The 409A common-stock value that sets the option strike price is distinct from Haiven's preferred-share financing price or headline/post-money valuation. Asking for both makes the economics understandable without conflating them.
- The completed Exhibit A remains local and must be integrated into, and expressly acknowledged as part of, the signed package.
- `src/haiven-websocket-hub` is a separate Git repository linked into `agent-loop` as a submodule, so the parent repository stores a gitlink rather than the submodule's source blobs. That boundary alone is not enough if root-level search, AI context, caches, logs, credentials, or build artifacts ingest Haiven material; the current root `tsift` index includes the Haiven submodule as a searchable scope. Use isolated tooling or exclude Haiven content from superproject-wide indexes, and obtain written confirmation that a metadata-only submodule arrangement satisfies the contract.
- No email, attachment, signature, or other external action is authorized by this draft.

## Email draft

**Subject:** Re: Addendum to Offer Letter :)

Hi Luke and Craig,

Thank you for sending this over. I reviewed the attached addendum. It is a strong improvement and I appreciate the work you put into incorporating the requested protections.

Before I sign, I noticed a formatting issue that still appears in the attached PDF. On page 1, the opening sentence renders as `Haiven, Inc. (“Comp`, followed by a standalone `Employee will attach`, and then `any”)`. Could you please send a clean regenerated and re-signed PDF that reads `Haiven, Inc. (“Company”)` and places the Exhibit A sentence in Section 3?

I also have a few remaining points to confirm:

1. **Option price and capitalization.** Please provide the current 409A common-stock fair market value per share and its effective date. If the grant were approved under that valuation, please also confirm the expected exercise price per share and estimated total exercise cost for all 16,053 options. I understand that the final exercise price will be fixed on the actual grant date.

   Please also confirm the fully diluted capitalization and measurement date supporting the 1.00% figure. The stated numbers imply a fully diluted denominator of 1,605,300 shares immediately after the grant. If available, please provide the most recent preferred financing price per share and post-money valuation as well, so I can understand how the 409A common-stock value and option cost relate to the Company's current financing valuation.

   Because Section 1 remains subject to Board approval, please also clarify what protection or equivalent remedy applies if the Board does not approve the promised grant. Please send the current equity incentive plan and proposed option agreement early enough for review before I sign.

2. **Exhibit A.** I have completed a proposed Exhibit A. I would like it attached to the addendum as part of one integrated document, with Haiven expressly acknowledging that the attached schedule—including its Effect of Listing, Incorporation, and Priority provisions—is approved and incorporated into the Agreement.

3. **Signing bonus.** Please state the exact remittance date and payment method for each $10,000 installment, including whether each will be paid through regular payroll or separately. Please also clarify whether any repayment would be calculated on the gross or net amount after withholding and how payroll-tax adjustments would be handled.

4. **Termination definitions.** Please define Good Reason in this addendum rather than leaving it to a possible later agreement. I would also like the curable portions of the Cause definition to use a definite cure period. Please replace the broad “crime involving moral turpitude” phrase with an objective, enumerated definition covering convictions or guilty/no-contest pleas for serious offenses involving fraud, theft, embezzlement, bribery, intentional violence, or sexual abuse or exploitation, together with a material nexus to my duties or demonstrable material harm to the Company. Arrests, pending charges, civil violations, traffic infractions, and offenses based solely on negligence or strict liability should not constitute Cause.

5. **Future outside work and separation.** Please confirm that future outside-activity approvals will not be unreasonably withheld, conditioned, or delayed; that a specific written objection will be provided within a reasonable response period; and that CEO or Company written approval satisfies the requirement without unanimous Board approval. Please also confirm that “logical and physical separation” means reasonable separation of confidential information, accounts, credentials, repositories, work product, and access controls, and does not require separate physical devices unless specifically agreed. A dedicated private Haiven Git repository should satisfy the repository-separation requirement even if referenced as a submodule from my private, employee-controlled superproject, provided that the superproject contains only standard submodule metadata and the referenced commit—not Haiven source code, credentials, Confidential Information, or work product.

6. **Supporting materials, payroll, and timing.** Please provide the handbook, vacation policy, equity plan, option form, health-coverage effective date, and contribution rates before my signature rather than concurrently with execution or later during onboarding. Please also confirm that any Connecticut waiver required for the semimonthly payroll schedule will be in place before that schedule is used.

Given these remaining items, please extend the acceptance deadline until at least the first business day after I receive the clean re-signed addendum, the integrated and acknowledged Exhibit A, and the supporting materials.

I remain excited about the role and would like to get the documents finalized promptly. Thank you again.

Best,

Brian
