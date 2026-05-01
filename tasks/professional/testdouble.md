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
Reviewed converted agreement. No clause bars Brian personally performing the work, but §10.1 prohibits using further subcontractors or subconsultants without prior written consent.
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
<!-- agent:boundary:b88a7c45 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#td-subcontract-consent] [recommended] Get written confirmation from Test Double before performing through an LLC or using any substitute, because §10.1 requires prior written consent for further subcontractors/subconsultants.
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
