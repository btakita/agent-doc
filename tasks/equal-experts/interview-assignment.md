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
queue: start
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

### Re: Python stack — drop FastAPI, single Python package — opus-4-8

Python is a good pick, but I'd **drop FastAPI entirely** and **not introduce a JS client**. Both add exactly the kind of layer the brief warns against.

#### Why no FastAPI / no "service"

- `❌ Submit any form of app, such as web APIs` — a FastAPI service *is* a web API. Building one is the one deliverable explicitly disallowed.
- There is nothing for us to serve. The **Price API already exists** and is hosted externally (`https://equalexperts.github.io/backend-take-home-test-data/{product}.json`). We are a *consumer* of it, not its author. We don't build it, and we don't stand up a local copy.
- In tests we don't need a running server either — we stub the HTTP call **in-process** (no socket, no port).

#### Why no separate JS client

- A second language/runtime is added complexity for zero benefit and splits "client + logic" across two toolchains. `❌ Add unnecessary layers`.
- "Client" here = the small **Price API HTTP client** *inside* the Python package — not a separate app or front-end. Keep it in the same language as the cart.

#### Recommended shape — one Python package, library only

```
shopping_cart/
  __init__.py
  price_client.py   # PriceClient: get_price(name) -> Decimal; wraps httpx, parses JSON
  cart.py           # Cart: add(name, qty), subtotal, tax, total (Decimal, ROUND_UP, 2dp)
tests/
  test_price_client.py   # respx/responses-mocked HTTP: URL, parse, 404/non-200
  test_cart.py           # pure logic incl. the worked example -> 15.02 / 1.88 / 16.90
pyproject.toml
README.md
```

- **HTTP lib:** `httpx` (or `requests` — either is fine; `httpx` is a touch more modern). One dependency.
- **Money math:** `decimal.Decimal` with `ROUND_UP` / quantize to `0.01`. The brief says "rounded up", so use `ROUND_UP` (not bankers'/half-up) and decide whether tax rounds before or after summing — I'd round the final tax and total only, and note the assumption in the README.
- **Tests:** `pytest` + `respx` (httpx) or `responses` (requests) to mock the price endpoint so tests are offline and deterministic. Test the client and the cart logic separately, per `✅ Test both`.
- **No** `main()`, CLI, server, persistence, or DI framework. The "interface" is the `Cart` class API, exercised by tests.

#### Net answer to your question

Neither a JS client nor a separate Python *service*. **One Python library**: `PriceClient` (httpx) + `Cart`, with `pytest` + `respx`. That is the simplest thing that satisfies "test both client and logic" while staying inside the ❌ no-app rule.

If you'd still like FastAPI in the picture for your own learning, we could keep it as a throwaay *outside* the submission, but I wouldn't include it in what we push. Want me to update `#next-steps-2` to pin this Python/httpx/pytest decision and scaffold the package?

### Re: Plan written into the repo — Python library, commits stay in agent-loop — opus-4-8

Done. Plan document written to **`src/equal-experts-neat-exhilarating-friendly-inspiration/PLAN.md`**.

#### Stack decision (recap)
Single **Python** package — `PriceClient` (httpx) + `Cart` (Decimal/ROUND_UP) + `pytest`/`respx`. **No FastAPI, no "service", no JS client.** The Price API is external and already hosted; we only consume it, and a web API is the one deliverable the brief forbids. "Another Python client" or "a JS client" would both be unnecessary layers — keep client + logic in one language.

#### Commit handling (your "commits in agent-loop" note)
The submodule is its own git repo whose remote is the EqualExperts assignment. A file under `src/equal-experts.../` belongs to *that* repo, not agent-loop — git can't commit it into agent-loop while it lives there. So:

- **`PLAN.md` is written but left UNCOMMITTED inside the submodule.** No EqualExperts-repo commit is created.
- **The only commits this session makes are in agent-loop** (this session-doc cycle) — matching your instruction and the README (push to EqualExperts only when the assignment is *complete*; access is revoked right after, so we don't push prematurely).
- When you're ready to submit, you (or I, on your go) `cd` into the submodule, commit `PLAN.md` + the implementation, and push to EqualExperts — then click the completion link.

If you actually wanted `PLAN.md` committed into the submodule now, say so and I'll make that submodule commit instead. Likewise, when you move this session doc into the submodule later, that physically removes it from agent-loop's tree — happy to help sequence that so history stays clean.

#### Next
`#next-steps-2` proceeds on this plan. Want me to scaffold the package (`shopping_cart/`, `tests/`, `pyproject.toml`) with the `PriceClient` + `Cart` skeleton and the first failing tests?

### Re: SPEC.md created + committed (no push) — opus-4-8

Done both parts of your queue item.

- **`SPEC.md` written** to `src/equal-experts-neat-exhilarating-friendly-inspiration/SPEC.md`, derived from `PLAN.md` — public API (`PriceClient`, `Cart`), error table, the 12.5%/ROUND_UP calculation rules, the worked-example acceptance criteria (15.02 / 1.88 / 16.90), and the per-file test plan.
- **Committed, not pushed.** Given your "commit. DO NOT PUSH", I committed `PLAN.md` + `SPEC.md` **inside the submodule** (branch `solution`) as a local commit — nothing pushed to the EqualExperts remote. (Updates my earlier "left uncommitted" note: they're committed now, just local.)
- **Session commits stay in agent-loop.** This session-doc cycle commits to agent-loop, and I'll record the submodule pointer bump here too. No `git push` anywhere.

This honours both rules: the deliverable history lives in the submodule (local only), while the agent-doc session machinery stays in agent-loop. Say "scaffold" when you want the `shopping_cart/` package + first failing tests, same commit-local-no-push handling.

### Re: #next-steps-2 implemented — committed local, not pushed, awaiting your review — opus-4-8

> **Queue prompt:**
>
> [#next-steps-2]. Do not push or mark as complete. I need to review.


Shopping cart implemented per `PLAN.md` / `SPEC.md`. **Committed in the submodule locally (branch `solution`, `ahead 2` of origin), nothing pushed, not marked complete.** Gated `#next-steps-2` for your review.

#### What landed (`src/equal-experts-neat-exhilarating-friendly-inspiration/`)
- `shopping_cart/price_client.py` — `PriceClient.get_price(name) -> Decimal`; httpx; `ProductNotFound` (404) / `PriceApiError` (non-200, malformed body); base URL injectable.
- `shopping_cart/cart.py` — `Cart.add(name, qty)`, accumulates per product; `subtotal` / `tax` (12.5%) / `total`, all `Decimal` rounded **up** to 2dp.
- `tests/` — **22 tests, all green**. Client tests mock HTTP in-process via `httpx.MockTransport` (no network, no respx); cart tests stub the client. Includes the brief's worked example as an explicit assertion (15.02 / 1.88 / 16.90).
- `pyproject.toml`, `.gitignore`; `README.md` updated with usage, assumptions/tradeoffs, and the AI-tool-usage disclosure.

#### Verification
```
python -m pytest    # 22 passed
```

#### Deliberately NOT done (per your instruction)
- **No `git push`** — neither the submodule nor agent-loop is pushed.
- **Not marked complete** — the SnapCode completion link is untouched.

#### Review pointers
- Rounding rule (subtotal/tax/total each `ROUND_UP` to 2dp; tax from rounded subtotal) — see README "Assumptions". Easy to switch to line-item rounding if you prefer.
- Stub prices for non-example products in `test_cart.py` are arbitrary (only cornflakes 2.52 / weetabix 9.98 must match the brief).
- AI-usage disclosure estimates ~95% AI-generated — adjust to your comfort before submitting.

Tell me what to change, or say "submit" when you're ready and I'll push to the EqualExperts remote + (only on your explicit go) mark complete.
<!-- agent:boundary:0be74f3b -->
<!-- /agent:exchange -->

## Queue
<!-- agent:queue auto go -->
- Where is the venv? Does the README stipulate that I should manually test the app or are the tests sufficient?
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#next-steps-2] Implement Equal Experts shopping cart: (1) choose language/framework, (2) add-product with Price API integration (GET https://equalexperts.github.io/backend-take-home-test-data/{product}.json), (3) cart state: subtotal / tax 12.5% / total, rounded up to 2dp, (4) unit tests for client + logic, (5) update README with assumptions + AI usage disclosure
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
