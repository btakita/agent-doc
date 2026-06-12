---
agent_doc_session: 585ce274-fe94-4f3b-94ed-6cbb09b0f726
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. DO NOT IMPLEMENT THE FIX.
  '#commit-push': commit all uncommitted files
  '#commit-push-deploy': commit all uncommitted files + deploy
  '#mermaid': Include explanatory mermaid diagram(s).
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ I'm preparing for a 90 minute technical interview to go over the interview-assignment.md solution. Please help me prepare.
❯ - See https://mail.google.com/mail/u/1/#search/equal+experts/FMfcgzQgMMHLwmmbjrNMgZngVjcTZLrL with btak.dev@gmail.com
❯ - https://www.linkedin.com/in/balazs-gerendai-859390a3/
❯ - https://www.linkedin.com/in/adamsurgenor/


### Re: 90-minute EE technical interview prep — solution walkthrough + likely deep-dives — opus-4-8

Here's a complete prep pack, grounded in the solution you actually shipped (`PriceClient` + `Cart`, Python/httpx/pytest, round-**half-up** tax, opt-in integration tests, GitHub Actions CI). The single most important framing: Equal Experts is a **consultancy**, so this round tests *how you reason and communicate tradeoffs to a client* at least as much as the code. Own every decision — including the AI-assisted ones.

> On the two links you gave me: I can't reach the Gmail thread (needs auth on btak.dev@gmail.com) or the LinkedIn profiles directly (LinkedIn blocks fetches). I can pull public info on **Balazs Gerendai** and **Adam Surgenor** via `chromium-bridge` if you want — queued as `#interviewer-research`. Tell me to run it.

---

#### 1. Likely 90-minute shape (EE pairing/walkthrough round)

| Phase | ~mins | What they assess |
|---|---|---|
| Intro / rapport | 5–10 | Communication, your framing of the problem |
| **You walk the solution** | 15–20 | Can you explain design & tradeoffs concisely |
| **Deep-dive Q&A** | 20–30 | Depth behind each decision; do you *own* it |
| **Live extension / pairing** | 20–30 | Collaboration, TDD instinct, thinking aloud |
| Your questions + behavioral | 10 | Curiosity, consultancy fit |

EE explicitly values **simplicity, pragmatism, testing discipline, and "you build it you run it."** Map every answer back to those.

#### 2. Your 3-minute walkthrough script (rehearse this)

> "The brief is a library, not an app — so I built exactly that: no CLI, no web API, no `main()`. Two seams. `PriceClient` is the only thing that talks to the external Price API — get a product, parse JSON, return a `Decimal`, raise typed errors on 404 / malformed. `Cart` is pure domain logic — accumulate quantities, subtotal, 12.5% tax, total. The client is injected into the cart, so the cart is network-free and trivially testable. Money is `Decimal` throughout. Tests split the same way: client tests mock the HTTP boundary with `httpx.MockTransport`, cart tests inject a stub client; one shared fake catalogue in `conftest.py` is the single source of truth for prices. There's an opt-in live-API smoke suite and a GitHub Actions workflow that runs the full suite incl. integration on 3.11 and 3.12."

```mermaid
flowchart LR
    Tests[pytest suite] -->|inject stub| Cart
    Cart -->|get_price| PriceClient
    PriceClient -->|GET .../{product}.json| API[(EE Price API)]
    Tests -.->|MockTransport / conftest fake| PriceClient
    Cart --> Money[Decimal: subtotal / tax 12.5% / total]
```

#### 3. Deep-dive Q&A — anticipated questions + your answers

**"Why a library and not an app/CLI?"** — The DO-NOT list forbids "any form of app — web APIs, browser, desktop, or command-line." The deliverable *is* the `Cart` public API, exercised by tests. Adding a CLI would violate the brief and add an untested layer.

**"Why the `PriceClient` / `Cart` split?"** — "Test both any client and logic" maps onto exactly these two seams. The split lets the domain logic be tested with zero network and the HTTP concerns be tested in isolation. It's the natural boundary, not an invented abstraction.

**"Why `Decimal`?"** — Money in binary float silently corrupts (`0.1 + 0.2`). `Decimal` gives exact base-10 arithmetic and explicit rounding control — non-negotiable for currency.

**⚠️ "The brief says rounded *up*. Your code rounds half-up. Why?"** — *This is your highest-risk question.* Your shipped solution deviates from the brief's literal wording. Be ready with the crisp version: "The worked example (`15.02 / 1.88 / 16.90`) passes under either rule, so the acceptance test doesn't distinguish them. I made a *documented, sourced* choice to use real-world nearest-cent rounding (SSUTA §324, EU/UK VAT), isolated to one helper, with a README note and a one-line revert to strict ceiling. In a real client engagement I'd flag the ambiguity and confirm the requirement rather than assume." **Decision point before the interview** — see Risk #1 below; you may want to flip back to literal `ROUND_UP` to match the graded brief. Queued as `#rounding-decision`.

**"Mocks vs fakes — what did you do and why?"** — Started with inline `MockTransport` mocks, hit the classic failure mode (a fixed `2.52` mock that ignored the product), and consolidated into one product-aware fake in `conftest.py`. That's the textbook "Fake over scattered mocks when assumptions repeat" call — but kept it a *fixture*, not a standalone fake *service*, because the surface is one GET endpoint. Pragmatic, not dogmatic.

**"Walk me through a bug you caught."** — *Use the "always 2.52" story — it's gold.* You manually hit the live API, noticed every product returned 2.52, distrusted the green suite, and found the defect was in the **test** (a fixed mock + a too-weak integration assertion of `price > 0`), not production code. Fix: product-aware fake + exact-per-product live assertions + distinctness check. This demonstrates exactly the skepticism EE wants — you didn't trust green.

**"Why are integration tests opt-in?"** — Dev runs must be offline/deterministic, so unit-only by default (`pytest` → 33 passed); the live suite runs via `RUN_INTEGRATION=1` and **always in CI** (GitHub Actions, Py 3.11/3.12). Best of both: fast local loop, real-wiring coverage in CI.

**"Error handling?"** — Typed: `ProductNotFound` (404) vs `PriceApiError` (other non-200 / malformed body). Callers can distinguish "no such product" from "API is broken."

**"How would you scale / productionize this?"** — Price caching with TTL, retries/timeouts/circuit-breaker on the client, configurable tax rate & rounding policy (inject it), structured logging/metrics, and confirming the *real* rounding rule with the business.

#### 4. The AI-usage conversation (EE asks for disclosure — own it)

You used agent-doc + Claude heavily (~95% generated). Don't hide it; frame it as **engineering judgment over an AI pair**:
- "I drove the design decisions and reviewed every change. The AI accelerated typing and research; I owned the architecture, caught the 2.52 bug by manual testing, and made the rounding call."
- Be ready to **explain any line cold** — they may point at `_round_money` or the conftest fake and ask "why this?" Treat the codebase as fully yours.
- The agent-doc session itself (`AGENT_SESSION.md` in the repo) is your audit trail — a *strength*: full traceability of the reasoning. You can offer to show it.

#### 5. Likely live-coding extensions (think aloud, TDD-first)

Rehearse a red→green rhythm for: **discount codes / promotions**, **currency formatting or multi-currency**, **a quantity cap or remove-item**, **caching prices within a cart**, **bulk `add_products`**. For each: write the failing test first, narrate the seam it touches, keep it in `Decimal`.

#### 6. Risks to neutralize before the interview

1. **Rounding deviation (highest).** Shipped code uses `ROUND_HALF_UP`; brief says "rounded up." Worked example hides it. Either defend confidently *or* flip to `ROUND_UP` to match the graded brief. Decide deliberately — `#rounding-decision`.
2. **~95% AI-generated disclosure.** Neutralized by owning every decision and the manual-testing catch.
3. **Don't over-claim.** If asked something the code doesn't do (e.g., concurrency, retries), say "not implemented — here's how I'd add it," don't invent.

#### 7. Questions to ask them (consultancy signal)

- "How does EE balance simplicity against future-proofing on real client engagements?"
- "What does 'you build it you run it' look like day-to-day here?"
- "How do EE teams currently fold AI tooling into delivery — and where do you draw the review line?"
- "What did you want to see in this exercise that candidates most often miss?"

---

Want me to **(a)** run `#interviewer-research` on Balazs & Adam, **(b)** decide `#rounding-decision` (keep half-up vs revert to brief-literal), or **(c)** run a mock interview where I play the EE interviewer and grill you on the above? Say the word.
<!-- agent:boundary:52f477a9 -->
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
