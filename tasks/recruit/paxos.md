---
agent_doc_session: b36e9ba4-efa2-4955-817d-4041f42e2f64
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Video compression running in background
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ Please help me prepare for an interview for Paxos:
❯ - https://www.paxos.com/
❯ - https://www.linkedin.com/company/paxos/

❯ Call will be with bryan-boyer. Please add contact:
❯ - https://www.linkedin.com/in/bboy2448/
### Re: Interview prep for Paxos

Contact added: `mail/contacts/bryan-boyer/AGENTS.md`

**Bryan Boyer** — engineering leadership at Paxos. Prior roles: Senior Engineering Manager at Wayfair (Supply Chain), engineering leadership at Evolent Health, Relativity, Flowhub. B.S. Industrial & Systems Engineering, Georgia Tech. LinkedIn: https://www.linkedin.com/in/bboy2448/

---

## Paxos Overview

**Mission:** Build the open financial system — regulated blockchain infrastructure for instant movement of any asset.

**Key differentiator:** Most regulated U.S. blockchain infrastructure provider.
- First NYDFS limited purpose trust charter for digital assets (2015)
- OCC conditional approval to convert to federally regulated national trust bank charter (Dec 2025) — enables serving institutional clients across all 50 states

**Scale:** $180B+ in tokenization activity; 10M+ active digital wallets globally

---

## Products

| Product | Description |
|---------|-------------|
| **PayPal USD (PYUSD)** | $3.7B stablecoin — largest product on platform |
| **USDP** | USD-backed stablecoin, issued since 2018 |
| **PAXG** | Tokenized physical gold |
| **Global Dollar (USDG)** | Consortium stablecoin ($975M) — Robinhood, Kraken, Galaxy, Anchorage |
| **Bankchain** | Post-trade securities settlement |
| **Fordefi** | Acquired Nov 2025 (~$100M) — MPC wallet platform, $120B/mo transaction volume, DeFi integrations |

**Customers:** PayPal, Mastercard, Interactive Brokers, Bank of America, Mercado Libre, Nubank, Robinhood

---

## Tech Stack

- **Languages:** Kotlin, Golang, **Rust** (recently adopted in trading/performance-critical systems)
- Distributed systems, consensus algorithms, cryptography (ECC, MPC)
- Strong emphasis on correctness, uptime, security
- Engineering culture: "Ask forgiveness, not permission" — engineers take initiative

---

## Recent Milestones

- **Nov 2025:** Acquired Fordefi (MPC wallet platform) — adds full custody + DeFi stack
- **Dec 2025:** OCC national trust bank charter conditional approval
- **Jun 2025:** Launched Paxos Labs (DeFi/on-chain institution incubator)
- **Feb 2024:** Acquired Membrane Finance (EU/MiCA-compliant stablecoin issuer)

---

## Questions to Ask

1. "What drove the OCC charter conversion, and how does it change what engineering can build?"
2. "Where is Rust being adopted beyond trading? Plans for custody/settlement infrastructure?"
3. "How is Fordefi integration going — what's the near-term engineering priority?"
4. "How do you structure teams around blockchain domains vs. application-layer products?"
5. "Are you exploring AI agents or LLM tooling for developer productivity or compliance automation?"
6. "What's the biggest technical challenge you're solving this year?"

---

## Talking Points for Brian

- **Rust expertise** — Paxos actively expanding Rust in security/performance-critical systems
- **Agent tooling** — relevant to Paxos Labs and potential internal dev tools
- **Distributed systems** — settlement infrastructure is inherently distributed; consensus experience lands well
- Emphasize correctness + safety-first engineering under regulatory constraints
- Paxos values engineers who understand business impact, not just tech

❯ Should I look into anything before the call? How open is Paxos to agent harnesses to develop Rust code? Is manual code review very important?

❯ Please genera a 1 page summary that I can look at during the call on Zoom.
### Re: Pre-call prep, agent harnesses, code review, and 1-page summary

**Pre-call: what to look into**
- Skim the Fordefi acquisition announcement — it's the freshest news and likely to come up
- Read 1-2 posts on [eng.paxos.com](https://eng.paxos.com/) to get a feel for their engineering voice
- Check LinkedIn for Bryan Boyer's recent activity / posts before the call

**Agent harnesses for Rust development**

No public evidence Paxos currently uses AI coding agents/harnesses in their workflow. This is actually an opportunity: position it as something you bring, not something to ask permission for. Useful framing:
- "I use agent harnesses to handle boilerplate and keep review focused on correctness and architecture — which matters even more in regulated systems"
- Ask: *"Are engineers free to use AI coding tools in their workflow, or are there policies around that given the regulatory environment?"*

Given their "ask forgiveness, not permission" culture, they'll likely be receptive — but no guarantees in a regulated fintech context.

**Code review: yes, it's rigorous**

Paxos has a design-first review culture:
- Engineers write design docs *before* coding (approach, alternatives, tradeoffs, dependencies) — reviewed by multiple teams
- PRs are expected to be self-reviewed and "great" before submission
- Strong CI/CD and testing discipline
- Code is "read far more than written" — they care about clarity

This actually pairs well with AI-assisted development: agents handle the mechanical work, you focus review energy on correctness and design. Frame it that way.

---

## 1-Page Call Summary

**PAXOS** | Regulated blockchain infrastructure | Founded 2012, NY

**What they do**
Build the financial plumbing for digital assets — stablecoin issuance, tokenized assets, post-trade settlement, MPC custody. Customers: PayPal, Mastercard, Robinhood, Bank of America, Mercado Libre.

**Scale:** $180B+ tokenization activity · 10M+ wallets · PYUSD ($3.7B) is their flagship

**Recent moves**
- **Fordefi acquired** Nov 2025 (~$100M) — MPC wallets + DeFi integrations
- **OCC national trust bank charter** Dec 2025 — federally regulated, serves all 50 states
- **Paxos Labs** Jun 2025 — DeFi/on-chain incubator for institutions

**Tech stack**
Kotlin · Go · **Rust** (security/perf-critical systems, expanding) · distributed systems · cryptography (ECC, MPC)

**Engineering culture**
Design docs before code · rigorous PR review · "ask forgiveness, not permission" · correctness + safety first

**Bryan Boyer** — engineering leadership, Paxos. Background: Wayfair (Sr Eng Manager, Supply Chain), Evolent Health, Flowhub. Georgia Tech ISyE.

---

**My angle**
- Rust expertise → aligns with their expansion direction
- Agent harnesses → I write less boilerplate, more focused review
- Distributed systems / correctness → settlement infrastructure is my domain
- I understand building under regulatory constraints

---

**Questions to ask**
1. OCC charter — what does it unlock for engineering?
2. Fordefi integration — near-term priorities?
3. Rust expansion roadmap beyond trading?
4. AI tooling policy for engineers?
5. Biggest technical challenge this year?

❯ Please write the summary to a file that I can open in the browser.
### Re: Browser summary

Written to: `file:///home/brian/work/btakita/agent-loop/tmp/paxos-summary.html`

One-page reference card — company overview, products table, Bryan Boyer background, tech stack, recent milestones, questions to ask, and your angle. Sized to fit a single browser tab alongside Zoom.

❯ Does Paxos have a physical reserve of Gold?
❯ Does Paxos have a reserve for other currencies?

### Re: Gold reserves and currency backing

**PAXG — physical gold**

Yes. Each PAXG token is backed 1:1 by one fine troy ounce of a 400 oz London Good Delivery gold bar held in Brink's vaults in London. Paxos Trust Company is the custodian. Token holders can look up their specific bar (serial number, weight, vault location) on-chain. Independent auditors verify the vault regularly.

**Stablecoin reserves (no fractional reserve)**

Paxos is a trust company, not a bank — they cannot lend out funds. Every token is fully collateralized 1:1 at all times.

| Token | Reserve composition |
|-------|---------------------|
| **USDP** | Cash + short-duration U.S. Treasuries, segregated accounts at FDIC-insured banks / Federal Reserve |
| **PYUSD** | Same structure — Paxos issues and manages reserves, PayPal is the brand |
| **USDG** | USD deposits + Treasuries |

Monthly public attestations by independent accountants for USDP and PYUSD.

**No other currency reserves** — all stablecoins are USD-denominated. PAXG is the only non-USD asset they tokenize (gold). No EUR, GBP, or multi-currency stablecoins currently under the Paxos brand (Membrane Finance, their EU acquisition, handles MiCA-compliant EUR stablecoin territory).

❯ compress [2026-04-08 17-00-21.mp4](../../../../../Videos/2026-04-08%2017-00-21.mp4)
### Re: Video compression

Compressing `Videos/2026-04-08 17-00-21.mp4` (2.3 GB, 1080p, ~53 min) in background.

Output: `~/Videos/2026-04-08 17-00-21-compressed.mp4`
Settings: H.264 CRF 28, medium preset, AAC 128k audio

Expected output size: ~300–600 MB depending on content. Will complete in ~10–20 min.
<!-- agent:boundary:69a7ccce -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
