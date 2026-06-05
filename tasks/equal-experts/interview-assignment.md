---
agent_doc_session: 5fb931d8-426e-43c1-a12b-457e4054b1dd
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
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

### Re: Add submodule + read README — sonnet-4-6

Submodule added at `src/equal-experts-neat-exhilarating-friendly-inspiration`. README summarises the assignment:

**Shopping Cart** — no app/web API, just a library with:
1. `add_product(name, qty)` — fetches price from `GET https://equalexperts.github.io/backend-take-home-test-data/{product}.json`, accumulates qty
2. Cart state: subtotal (sum of price × qty), tax (12.5%), total (subtotal + tax) — all rounded **up** to 2dp
3. Five products: cheerios, cornflakes, frosties, shreddies, weetabix

**Acceptance example:** 1×cornflakes + 1×cornflakes + 1×weetabix → subtotal 15.02, tax 1.88, total 16.90

**Requirements:** unit tests for client + logic; no persistence; update README with assumptions + AI usage disclosure; push then mark complete via the SnapCode link.

Added backlog `#next-steps` covering all implementation steps.

### Re: What is a valid "client" here? — opus-4-8

You're right that the wording is ambiguous, but the two rules resolve cleanly once you see that "client" is used in the **API-consumer** sense, not the **application/UI** sense.

#### The "client" = the Price API client

The README has a dedicated **Price API** section: the cart must call `GET https://equalexperts.github.io/backend-take-home-test-data/{product}.json` to look up prices. The natural seam in the design is a small component whose only job is to talk to that remote service — fetch a product, parse the JSON, return a price (or an error/not-found). *That* is the client they mean: an **HTTP/API client** that consumes the Price API.

So `✅ Test both any client and logic` is asking for two test surfaces:

1. **The client** — the Price API gateway. Test it with the HTTP layer stubbed/mocked so tests are deterministic and offline: correct URL construction per product, JSON parsing, and failure handling (unknown product, non-200, malformed body).
2. **The logic** — the pure cart domain: `add_product`, quantity accumulation, subtotal, 12.5% tax, total, and round-**up**-to-2dp. No network, no I/O — fast unit tests against the worked example (2×cornflakes + 1×weetabix → 15.02 / 1.88 / 16.90).

#### Why it is *not* a UI/app client

`❌ Submit any form of app — web APIs, browser, desktop, or command-line` rules out building a *front-end* or *delivery mechanism* around the cart. There's no REPL, no `main()`, no HTTP server, no CLI parser. The deliverable is a **library**: a `Cart` (and a `PriceClient`) exposed as a programmatic public API, exercised entirely through unit/integration tests. The "client" you write and test is internal infrastructure (the price fetcher), not a user-facing program.

#### Net interpretation

- **Client** = the code that *consumes the Price API* (an outbound HTTP client/gateway).
- **Logic** = the cart math and state.
- Test both, in isolation, with the network mocked.
- Ship it as a tested library — no executable app of any kind.

A secondary, weaker reading of "client" is "the caller of your library's public API" (i.e., write integration-style tests from a consumer's viewpoint). That's worth doing too, but the primary intent — given the explicit Price API section — is the HTTP client. I'd structure the solution around a `PriceClient` + `Cart` split and let `#next-steps` proceed on that basis.
<!-- agent:boundary:3032eacc -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#next-steps-2] Implement Equal Experts shopping cart: (1) choose language/framework, (2) add-product with Price API integration (GET https://equalexperts.github.io/backend-take-home-test-data/{product}.json), (3) cart state: subtotal / tax 12.5% / total, rounded up to 2dp, (4) unit tests for client + logic, (5) update README with assumptions + AI usage disclosure, (6) push + mark complete at https://app.snapcode.review/submission_links/9c362eaf-9ef0-45f3-8467-0660e3a68754
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
