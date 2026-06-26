---
agent_doc_session: b4e9210c-5479-4031-a07e-44fb5ae046f0
model: opus
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

❯ Create a contact for alex-lee https://www.linkedin.com/in/leehosang/
❯ Tell me more about his company https://governingdocs.dev/.

❯ We will be having a chat. He has consistently liked my LinkedIn posts so I want to meet him. We both work with real estate + ai.

### Re: Contact for Alex Lee + his company — opus-4-8

**Contact created:** `mail/contacts/alex-lee/` (`AGENTS.md` + `CLAUDE.md` symlink, kebab-cased to match the other contacts, and registered in `.corky.toml`). Captured: LinkedIn `leehosang`, company GoverningDocs, the real-estate + AI shared thread, and the fact that he consistently likes your posts. No email yet — add it once you have it (`corky contact add` or edit the file).

### Re: His company — GoverningDocs (https://governingdocs.dev/) — opus-4-8

**What it is:** an AI platform that analyzes **HOA governing documents** for residential real-estate transactions. It takes the 200–400-page document packet a buyer receives and turns it into actionable reports within days.

- **Documents ingested:** CC&Rs, bylaws, budgets, reserve studies, meeting minutes.
- **Product:** 25+ automated reports (financial health, rental restrictions, governance); an **HOA Health Grade** (A–F from five financial metrics); **AI Q&A chat with page-level source citations**; free standalone analyzers; paid full report at $39 (first one free).
- **Mission:** help buyers "know what's in the HOA documents before you buy."
- **Customers:** homebuyers, RE investors (rental-restriction/reserve due diligence), agents, brokerages.
- **Founder/team:** not published on the site — worth confirming his exact role and the founding story on the call.

**For your chat:** strong, genuine overlap — you're both at the **real-estate × AI** intersection. His source-cited Q&A over dense governing docs sits right next to your work on graph-salience / constraint-shaped retrieval, so that's a real technical conversation, not small talk. Good openers: is he founder/CEO, and what stage is GoverningDocs at (team, funding, traction)?

### Re: Would Alex care about real-time? — opus-4-8

> **Queue prompt:** Do you think Alex would be interested in any real-time systems? It seems his company is more batch processing. Are there any benefits to real-time? Perhaps with CRM, sales, and communication?

Good instinct, and worth splitting "real-time" into three different things first — they have very different relevance to GoverningDocs:

1. **Real-time *data/stream processing*** (Kafka-style continuous ingest). **Not a fit for their core.** Analyzing a 200–400-page HOA packet is inherently a deep, async, batch job — you want the model reasoning over the whole corpus, not a stream. Forcing it real-time adds cost and fragility for no benefit. You're right that the core is, and should stay, batch.
2. **Real-time *inference UX*** (token streaming, low-latency interaction). **Already relevant.** Their AI Q&A-with-citations is interactive; streaming responses and fast re-ranking is "real-time" in the way that matters for an LLM product — perceived latency, not stream processing.
3. **Real-time *coordination / event-driven*** (notifications, webhooks, live collaboration). **This is the real opportunity, and it's your CRM/sales/communication angle:**
   - **Deadline clocks.** HOA-document review is *time-boxed* in a transaction — in many states the buyer gets a fixed window (often ~3–7 days) after receiving docs to review and keep/waive the contingency. A real-time alert ("report ready," "red flag found, 2 days left on your review") is high-value and time-sensitive — a natural event-driven layer over a batch analysis.
   - **CRM / transaction-system webhooks.** Push "report complete / grade = D / rental cap hit" events straight into the agent's pipeline (Dotloop, SkySlope, Salesforce) so their CRM updates without polling — "real-time" to the agent even though the analysis was batch.
   - **Live collaborative review.** Agent + buyer + lender on the *same* report with live annotations — this is **your** wheelhouse (CRDT, collaborative editing in lazily/agent-doc). The cleanest bridge between your tech and his product.
   - **Brokerage pipeline dashboards.** Teams running many concurrent deals want a live status board, not a batch report list.

**For the chat:** don't pitch "make the analysis real-time" — agree the heavy analysis is correctly batch, then steer to the *coordination layer*: event-driven deadline/CRM notifications and live collaborative review. That connects your real-time + CRDT work to his batch product without implying he built it wrong.

❯ I also have experience with Rubrics, which can be directly useful.
❯ deepeval

### Re: Your eval experience (Rubrics + deepeval) is your strongest angle — opus-4-8

This is probably the **best card to lead with**, because it targets GoverningDocs' single biggest risk: their whole value prop is *trustworthy* AI output over legal/financial docs. A wrong red flag or a hallucinated citation in a $39 report that someone uses to decide on a home purchase is a real liability — so output quality/eval isn't a nice-to-have for them, it's existential.

- **Rubrics** → rubric-based scoring of LLM outputs: define criteria (citation accuracy, factual grounding, grade correctness, completeness, no-hallucinated-clauses) and score generated reports against them. Maps directly onto "is this HOA report correct and complete?"
- **deepeval** → open-source LLM eval framework (G-Eval, faithfulness / answer-relevancy / hallucination metrics, test cases, CI integration). Lets them build a **regression suite over report generation + the cited Q&A** so a model or prompt change can't silently degrade accuracy. Faithfulness / citation-grounding metrics are *exactly* what a source-cited product needs.

Why it lands:
- It de-risks their core product, framed as a concrete deliverable: "I can help you build an eval harness that scores report accuracy + citation faithfulness and runs in CI."
- It pairs naturally with the retrieval / graph-salience angle — eval + retrieval quality are the two halves of making RAG-over-documents reliable.

**Suggested chat arc:** open on the shared real-estate × AI interest → offer two concrete value props: **(1) reliability/eval** (Rubrics + deepeval: faithfulness, citation-grounding, CI regression tests) as the lead, and **(2)** the real-time/collaborative coordination layer as the follow-up. Eval first — it speaks to their core risk and is the most ownable contribution.
<!-- agent:boundary:2f3bd2eb -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
