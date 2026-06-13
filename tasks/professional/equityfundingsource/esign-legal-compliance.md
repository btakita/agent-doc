# EFS Borrower Application — E-Sign Legal Compliance

**Status:** Open — awaiting legal review
**Owner:** Brian (engineering) + Judith Krug (business/legal decision)
**Last updated:** 2026-06-13

Ongoing tracking doc for the electronic-signature compliance of the borrower
loan-application "Review & E-Sign" step. Engineering assessment is below; the
compliance determination needs an attorney. A draft email to Judith is at the
bottom.

---

## Where the e-signature stands today

The Review & E-Sign step already captures the core elements of a valid e-signature
under the federal ESIGN Act (15 USC 7001) and California's UETA:

- **Intent to sign** — explicit "E-sign application" action, typed initials, printed name.
- **Consent to transact electronically** — a dedicated ESIGN/UETA consent checkbox.
- **Association with the record** — the signature is stored against the specific application.
- **Attribution / audit trail** — signer's IP address, browser user-agent, and a timestamp.

That is a reasonable, good-faith e-signature. It is not a substitute for a lawyer's
sign-off on the specific disclosures CA hard-money lending requires.

## Open items before launch sign-off

1. **Consumer e-sign disclosures (ESIGN §101(c)).** Before consent, consumers should
   be shown: the hardware/software needed to access records, the right to a paper copy,
   and the right to withdraw consent. Today the checkbox asserts consent but does not
   present these disclosures.
2. **Retain what was signed.** Store a tamper-evident snapshot or hash of the exact
   application content at the moment of signing, so EFS can prove *what* the borrower
   agreed to, not only *that* they signed.
3. **Distinct consent timestamp** separate from the signing timestamp.
4. **CA hard-money disclosures.** Confirm any state-specific disclosure/signature
   requirements (e.g. RE 885 for interest-only 1-4 unit residential) are satisfied within
   the electronic flow.
5. **Printed-name handling** (related work, `#gz9h`): soft match against the applicant's
   legal name with a non-blocking confirm — not a hard block — to avoid locking out
   co-borrowers, trusts/LLCs, and name changes.

Engineering can implement whatever the attorney specifies; the compliance call is theirs.

## Decisions / log

- 2026-06-13 — Created this doc. Drafted the email to Judith below. Backlog items
  `#fxsv` (ESIGN/UETA hardening) and `#gz9h` (printed-name match) track the engineering side.

---

## Draft email to Judith Krug

**To:** Judithkrug@gmail.com
**Subject:** Quick legal check on the application e-signature

Hi Judith,

The borrower loan application now has an electronic signature step, and I built it to
the standard ESIGN/UETA requirements: the borrower consents to sign electronically, types
their name, and we keep a timestamped record with their IP for the audit trail.

Before we go live, I'd recommend having your attorney review the e-signature and its
disclosures. There are a few consumer-disclosure and record-keeping details (and any
California hard-money specifics) that should come from a lawyer rather than from me. I've
listed what I think needs review and can implement whatever they ask for.

Want me to put together a short summary your attorney can work from?

Best,
Brian
