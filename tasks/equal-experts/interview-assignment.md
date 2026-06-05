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

### Re: venv + manual testing — opus-4-8

> **Queue prompt:**
>
> Where is the venv? Does the README stipulate that I should manually test the app or are the tests sufficient?


#### Where is the venv?
There isn't one. I ran the suite against your **system Python 3.12**, which already had `httpx` and `pytest` installed, so `python -m pytest` worked without an isolated environment. I deliberately didn't create one (and `.gitignore` excludes `.venv/`, so none would be committed).

That's fine for *my* run, but a reviewer cloning fresh would need the deps. Two options:

- **Minimal / stdlib:** `python -m venv .venv && source .venv/bin/activate && pip install -e ".[test]" && pytest`
- **uv (you have it):** `uv venv && uv pip install -e ".[test]" && uv run pytest`

Either way the only runtime dep is `httpx`; tests add `pytest`. Say the word and I'll add a short **"Development setup"** block to the README with those commands (committed local, no push) so the suite is reproducible on a clean checkout.

#### Manual testing — required?
**No.** The brief does not ask for manual testing, and there is nothing to manually run:

- The DO list is *"Include unit tests"* and *"Test both any client and logic"* — that's the 22-test suite (client mocked, cart logic, worked example).
- The DO-NOT list explicitly forbids submitting *"any form of app… web APIs, browser, desktop, or command-line."* There is no executable/app to exercise by hand — by design it's a library.

So the automated tests are the intended and sufficient verification. If you personally want a sanity check, you can drop into a REPL and hit the live API:

```python
from shopping_cart import Cart, PriceClient
c = Cart(PriceClient()); c.add("cornflakes"); c.add("cornflakes"); c.add("weetabix")
(c.subtotal, c.tax, c.total)   # (Decimal('15.02'), Decimal('1.88'), Decimal('16.90'))
```

That's optional — purely for your confidence, not a submission requirement.

### Re: integration tests with real API calls — opus-4-8

> **Queue prompt:**
>
> Are there any integration tests that test the full stack including the real API calls?


**No — there are no tests that hit the real Price API.** Every test mocks the HTTP boundary:
- `test_price_client.py` uses `httpx.MockTransport` (in-process fake responses).
- `test_cart.py` injects a stub client, so no HTTP at all.

#### Why I left them out
- **Determinism.** The brief asks for simple, well-tested solutions and forbids unnecessary complexity. Tests that call `https://equalexperts.github.io/...` are non-deterministic: they fail on no network, on a slow/blocked CI runner, or if EE changes the hosted data — none of which reflect a bug in this code.
- **The contract is what we test.** Mocking lets the client tests assert the exact URL, parsing, and error handling. A live call mostly re-tests EE's hosting, not our logic.

#### If you'd like one anyway (recommended pattern)
A single **opt-in smoke test**, skipped by default so the normal suite stays offline and green, run only on demand:

```python
# tests/test_integration.py
import os, pytest
from decimal import Decimal
from shopping_cart import PriceClient

@pytest.mark.skipif(not os.getenv("RUN_INTEGRATION"), reason="hits the live Price API")
def test_real_api_returns_a_price():
    assert PriceClient().get_price("cornflakes") > Decimal("0")
```

Run with `RUN_INTEGRATION=1 pytest`. It proves the real end-to-end wiring without making the default suite flaky. Say the word and I'll add it (committed local, no push).

---
*Housekeeping: the exchange is now ~175 lines / 7 sections and agent-doc is flagging it as large. Whenever you're done reviewing, I can run `agent-doc compact` to condense it — just say so; I won't compact mid-review.*

### Re: Real-API smoke suite added — opus-4-8

Done. `tests/test_integration.py` added, committed in the submodule (`solution`, `ahead 3`, **not pushed**).

- Skipped by default; runs with `RUN_INTEGRATION=1 python -m pytest tests/test_integration.py`.
- Covers **all five products** (`cheerios, cornflakes, frosties, shreddies, weetabix`) against the live API, plus an end-to-end cart that builds totals from real prices.
- Verified just now: default suite **22 passed, 6 skipped**; live suite **6 passed** (real API responded for every product). README updated with how to run it.

### Re: Fake API vs mocks — opus-4-8

> **Queue prompt:**
>
> I would like a smoke test for the real API calls. This should include all products hitting the real api.


Good instinct, and you've named the real tradeoff. My recommendation: **a lightweight Fake at the transport seam — yes; a full Fake API service — no.**

#### Where I agree
A Fake (one in-memory implementation of a contract) beats scattered mocks when there are *many* call sites or response shapes: it consolidates the "what the API looks like" assumption in one place, so a contract change is a one-line edit, not a shotgun surgery across tests. Repeated inline mocks are where brittleness creeps in.

#### Where it applies here (and where it doesn't)
- The surface is tiny: **one** operation (`get_price`), one response shape, a few error cases. The assumptions are already small.
- You actually already have a Fake on one seam: `test_cart.py`'s `StubClient` *is* an in-memory fake of the `PriceClient` contract. That's the right tool there.
- The brittleness you're pointing at is on the **client** seam: `test_price_client.py` repeats the `{"title":…, "price":…}` shape in several inline handlers.

#### Concrete recommendation
Keep `httpx.MockTransport` (no new dependency), but **consolidate the client tests behind one shared fake-endpoint handler** — a single `product -> payload` dict + handler (returns 404 for unknown, the payload otherwise), exposed as a pytest fixture. That captures the API's shape **once**, kills the repetition, and stays well within the brief's simplicity bar. A standalone Fake API *class* or a spun-up local server would be over-engineering for a single GET endpoint — more moving parts than the thing under test.

Net: a small fake *fixture*, not a fake *service*. Want me to refactor `test_price_client.py` to that shared fixture now (committed local, no push)? It's ~15 lines net and makes the client tests express one contract instead of five.

### Re: Fake API — decision pending (answered above) — opus-4-8

> **Queue prompt:**
>
> Should we also consider a Fake API for testing instead of mocks? Mocks are simple but brittle and repeat testing assumptions. A Fake would consolidate the assumptions. This is not as simple as it could possibly be but is more maintainable than mocks.


Consolidating this queue item with the full answer in **Re: Fake API vs mocks** just above (both items landed in one cycle; the queue carries one head per turn, so this drains the second).

**TL;DR:** a small fake *fixture* — yes; a fake *service* — no. Keep `httpx.MockTransport`, consolidate `test_price_client.py` behind one shared `product -> payload` fake-endpoint fixture so the API shape is expressed once instead of five times. A standalone Fake API class/server is over-engineering for a single GET endpoint.

This needs your call — say **"refactor to the shared fake"** and I'll apply it (committed local, no push), or leave the mocks as-is. Either way `#next-steps-2` stays gated for your review.

### Re: Does the brief expect a client for the Cart? — opus-4-8

> **Queue prompt:**
>
> I notice that `PriceClient` is a client to the API. Does the README expect a client for the `Cart`?


No — there's no separate "Cart client" expected, and adding one would be an unnecessary layer.

#### Reading the brief's wording
The DO line is *"Test both any client and logic."* In a layered design that splits into:
- **client** = the code that talks to an external system → our `PriceClient` (the Price API client). The README devotes a whole section to the Price API, which anchors this reading.
- **logic** = the domain → our `Cart`.

So "test both client and logic" maps cleanly onto the two pieces we already have. The only "client" in the system is the one to the Price API.

#### Is the `Cart` supposed to have its own client?
No. The `Cart` *is* the library's public API — the thing a caller uses directly (`cart.add(...)`, `cart.subtotal`). Its "client" is simply the **calling code**, and in a library-only deliverable that caller is the test suite, which drives `Cart` end-to-end. Wrapping `Cart` in a dedicated client class would be exactly the *"unnecessary layer of abstraction"* the brief tells us to avoid (❌).

#### So is the requirement met?
Yes, on both readings:
- Strict reading (client = API client): `test_price_client.py` covers the client, `test_cart.py` covers the logic.
- Loose reading (client = consumer of your public API): `test_cart.py` already exercises `Cart` from a caller's perspective, asserting the worked example through its public surface.

If you wanted to make the "consumer" angle explicit, the lightest touch would be a tiny usage example in the README (already present in the Usage block) rather than a new class. I'd not add a `Cart` client type.

*(Aside: exchange is now 252 lines / 11 sections — happy to `agent-doc compact` once you're done reviewing.)*

### Re: Cart docstring — interface + example added — opus-4-8

> **Queue prompt:**
>
> The Cart pydoc should include the interface and example usage.


Done. Added a full class docstring to `Cart` (committed in the submodule, `solution` `ahead 4`, **not pushed**).

It now documents:
- **Interface** — every method/property with signature and contract: `add(product, quantity=1)`, `items`, `subtotal`, `tax`, `total`, plus the Decimal/ROUND_UP/2dp rule.
- **Example** — the brief's worked example as a runnable-style block (`cornflakes ×2 + weetabix` → `15.02 / 1.88 / 16.90`).
- A note that any object with `get_price(product) -> Decimal` can be injected (e.g. a test stub), keeping the cart network-independent.

`help(Cart)` / IDE hovers now show the interface and usage. Verified the suite stays green (**22 passed, 6 skipped**); the `>>>` block is documentation only — doctests aren't collected (no `--doctest-modules`), so it won't make the suite hit the network.

`#next-steps-2` stays gated for your review.

### Re: PriceClient docstring — interface + example added — opus-4-8

> **Queue prompt:**
>
> The PriceClient pydoc should include the interface and example usage.


Done. Expanded the `PriceClient` class docstring (committed in the submodule, `solution` `ahead 5`, **not pushed**).

It now documents:
- **Interface** — `get_price(product) -> Decimal`, the URL it issues, and the error contract (`ProductNotFound` on 404, `PriceApiError` on other non-200 / malformed payload).
- **Known products** — the five valid names.
- **Examples** — live usage (`get_price("cornflakes") -> Decimal('2.52')`) and the offline `httpx.MockTransport` injection pattern used by the tests.

Suite still green (**22 passed, 6 skipped**); the `>>>` blocks are documentation only (doctests not collected). Both `Cart` and `PriceClient` now carry full interface + example docstrings.

`#next-steps-2` stays gated for your review.

### Re: "always 2.52" — no production bug; the mock + a weak test fooled you — opus-4-8

> **Queue prompt:**
>
> I manually tested the products with the api and the prices are incorrect. Always 2.52 for all products. cheerios should be 8.43. Are we using mock data? Why did the test pass?


Good catch, and your instinct to distrust the green suite was right. I investigated rather than assumed. Findings:

#### The real API and our code are correct
I curled all five endpoints and ran the actual `PriceClient` against the live API:

| product | price |
|---|---|
| cheerios | **8.43** |
| cornflakes | 2.52 |
| frosties | 4.99 |
| shreddies | 4.68 |
| weetabix | 9.98 |

`PriceClient().get_price("cheerios")` returns `8.43`. So production code hits the real API and returns correct per-product prices. **No bug in the shipping code.**

#### Where "always 2.52" came from — mock data
Yes, the *tests* use mock data (by design, for offline determinism). The problem: the old mock handler returned a **fixed** `{"price": 2.52}` and ignored the requested product. If you ran that snippet — e.g. the offline example that was in the `PriceClient` docstring — across all products, every one comes back `2.52`. That's the fake ignoring the product, not the API.

#### Why the test passed (the real defect — in the test, not the code)
- The **unit** tests mock HTTP, so they validate parsing/logic, never real prices.
- The **integration** smoke test *did* hit the real API, but I only asserted `price > 0` and internal consistency — so it passed whether cheerios was 8.43 or 2.52. **That was too weak.** That's the honest answer to "why did the test pass?".

#### What I fixed (committed in the submodule, `ahead 6`, not pushed)
1. **Integration test now asserts exact live prices** per product **and** that the five are distinct — it would now fail loudly on an "always 2.52". Verified: `RUN_INTEGRATION=1 pytest` → 7 passed against the real API.
2. **Mocks are now product-aware** via a shared `fake_price_api` fixture (`tests/conftest.py`) keyed by a real catalogue — cheerios returns 8.43, not a blanket 2.52. This also lands the "consolidate the fake" idea you raised earlier: the response shape lives in one place.
3. **Fixed the misleading docstring example** to be product-aware so copying it no longer yields 2.52 for everything.

Offline suite: **27 passed, 7 skipped**. `#next-steps-2` stays gated for your review.

### Re: MockTransport now provides per-product price variation — opus-4-8

> **Queue prompt:**
>
> I see what happened, I used the MockTransport in the tests, which always returns 2.52 for every product. Can we have the MockTransport provide some variation in prices to ensure our tests are robust against product price differences?


Yes — and it's done. Exactly that fix landed in the previous commit, and I've now consolidated it so the variation lives in one place.

#### The fake already varies by product
`tests/conftest.py` exposes `fake_price_api`, a `MockTransport`-backed fake that reads the requested product from the URL and returns **its own** price (cheerios 8.43, cornflakes 2.52, frosties 4.99, shreddies 4.68, weetabix 9.98), and 404s unknown products. No more blanket 2.52.

Two tests enforce the variation so a fixed-value regression can't sneak back:
- `test_get_price_returns_per_product_decimal` — parametrized over every product, asserts each distinct price.
- `test_prices_are_not_all_identical` — asserts the set of returned prices has more than one value.

#### New this cycle: one catalogue, not three
The price list had drifted into three copies (`conftest`, `test_cart`, `test_integration`). I made **`conftest.CATALOG` / `PRICES` the single source of truth**; the cart and integration tests now import from it. So the variation is defined once and the files can't disagree.

Verified: offline **27 passed, 7 skipped**; `RUN_INTEGRATION=1` **7 passed** against the real API. Committed in the submodule (`solution`, `ahead 7`, **not pushed**).

`#next-steps-2` stays gated for your review.

### Re: total rounding semantics — correct, and now pinned by tests — opus-4-8

Great question. Let me be precise about what the code does, then the subtle reason it's correct, then the new tests.

#### What the code actually does
```python
subtotal = _round_up(raw_sum)              # raw_sum of price*qty
tax      = _round_up(subtotal * 0.125)     # tax on the (rounded) subtotal
total    = _round_up(subtotal + tax)       # subtotal & tax are already 2dp
```
So `total` is built from the **already-rounded** `subtotal` and `tax`. The outer `_round_up` on total is a **no-op** (2dp + 2dp is already 2dp), so effectively `total == subtotal + tax`. This matches the brief's worked example arithmetic literally: `15.02 + 1.88 = 16.90`.

#### Is that the "double rounding" you were worried about?
It's the `subtotal_rounded + tax_rounded` form — but here that's **provably identical** to "sum then round once", for this domain:

- Prices are 2dp and quantities are integers, so `raw_sum` is **always exactly 2dp** → `_round_up(subtotal)` is a no-op → tax-on-rounded-subtotal == tax-on-raw-subtotal.
- For an exact-2dp `subtotal`, `ROUND_UP(subtotal + tax_raw) == subtotal + ROUND_UP(tax_raw)`. Adding an exact-2dp number can't change the rounding of the fractional tail.

I verified it: `round-each = 16.90`, `round-once = 16.90` — equal. The only operation that ever actually rounds is the 12.5% tax (which can exceed 2dp). So there's **no divergence and no bug**; `total = subtotal + tax` holds exactly. (The one way to make them differ would be sub-cent unit prices — not possible from this API's 2dp data.)

#### "Do we have a test?" — we did not isolate it; now we do
You were right that the worked-example test alone didn't pin the *semantics* (every case happened to agree). Added two explicit tests (committed in the submodule, `solution` `ahead 8`, **not pushed**):
- `test_total_equals_subtotal_plus_tax` — asserts `total == subtotal + tax` across four carts, pinning that total is the sum of the two reported figures.
- `test_tax_is_twelve_point_five_percent_of_subtotal_rounded_up` — asserts `tax == ROUND_UP(subtotal * 0.125)` and that `8.43 → 1.06` (round-**up**, not half-up which would give 1.05).

Suite: **32 passed, 7 skipped**. `#next-steps-2` stays gated for your review.

### Re: How tax rounding actually works (industry + tax authorities) — opus-4-8

I researched this against US Streamlined Sales Tax and general VAT practice. Short version: **tax is computed on the (un-pre-rounded) subtotal, then the tax is rounded — and the real-world rounding is round-to-nearest-cent (half-up), not ceiling.** Neither real option "rounds the subtotal up first."

#### The authoritative US rule (SSUTA §324)
The Streamlined Sales and Use Tax Agreement — the closest thing to a multi-state standard — specifies:
1. Compute tax = `taxable_amount × rate`, carried to **three decimal places**.
2. Round to a whole cent: **round up only when the third decimal is ≥ 5** (i.e., ≥ half a cent), else round down. That's **round-half-up to the nearest cent**, not "always up".
3. The seller may **elect item-basis or invoice/aggregate-basis**. Aggregate = compute the tax on the **whole taxable subtotal once**, then round once. This is allowed and generally preferred because it minimises accumulated rounding error.

So the subtotal is just the exact sum of line amounts (already whole cents); it is **not** "rounded up and then taxed." Tax is taken on that subtotal and the **tax** is what gets rounded.

#### VAT (EU/UK) — same shape, details vary
VAT rules leave rounding to member states: traders may round **per line or per invoice**, and typically round to the nearest unit (UK HMRC also historically allowed rounding *down* per line for invoice traders). Again: VAT is computed on the net total, and it's the VAT figure that's rounded — not a pre-rounded subtotal.

#### Answer to your exact question
- "Subtotal rounded up, then tax on that" — **not** how it's done.
- "Tax calculated on the (raw, exact) subtotal, then rounded" — **yes**, this is the standard. With the one correction that the rounding is **to the nearest cent (half-up)**, not ceiling.

#### How this maps to our code (and the brief)
- We compute `tax = round(subtotal × 0.125)` on the **exact** subtotal (subtotal is the precise sum) — that matches the aggregate-basis standard. ✓
- We use **`ROUND_UP` (ceiling)** because **the brief explicitly says "rounded up"** — a deliberate simplification that diverges from real authorities' round-half-up. For the worked example it doesn't matter (1.8775 rounds to 1.88 either way), but e.g. a raw tax of 1.881 → **1.89** under our ceiling vs **1.88** under real-world half-up.
- Because prices are whole cents, "round subtotal then tax" ≡ "tax on raw subtotal" anyway (shown last cycle) — so the only meaningful rounding decision is the tax, which we round once. That's aligned with the standard.

**Recommendation:** keep `ROUND_UP` for the submission (the brief is explicit and the assignment is graded against it), and add a one-line note in the README assumptions that production code should use round-half-up to the nearest cent per SSUTA/VAT. Want me to add that note?

**Sources:**
- [Streamlined Sales Tax — Rounding Rule Issue Paper (SSUTA §324)](https://www.streamlinedsalestax.org/docs/default-source/issue-papers/rounding-rule.pdf)
- [Streamlined Sales Tax — Approved Rounding Rules](https://www.streamlinedsalestax.org/docs/default-source/miscellaneous/approved-rounding-rules.pdf)
- [Ohio Dept. of Taxation — ST 2005-05 (calculation & rounding)](https://tax.ohio.gov/business/ohio-business-taxes/sales-and-use/information-releases/st200505)
- [TaxJar — Rounding in sales tax reporting](https://www.taxjar.com/blog/file/rounding-issues-sales-tax-returns)
- [Microsoft Dynamics 365 — Tax calculation rounding rules (line vs total)](https://learn.microsoft.com/en-us/dynamics365/finance/localizations/global/tax-calculation-rounding-rules)

### Re: Round-half-up implemented via TDD + README note — opus-4-8

> **Queue prompt:**
>
> Yes add the note to README and use test first TDD to implement the corrected rounding procedure. The test should fail first then implement to fix the test. 


Done, TDD-style (committed in the submodule, `solution` `ahead 9`, **not pushed**).

#### Red → Green
1. **Red:** rewrote the cart rounding tests to expect nearest-cent (half-up) and ran them — 3 failed against the old ceiling code, exactly as expected:
   - `8.43 → tax 1.05` (1.05375, 3rd decimal `3 < 5` rounds **down**; ceiling gave 1.06)
   - `2.52 → tax 0.32` (0.315, 3rd decimal `5 ≥ 5` rounds **up**)
   - multi-item `21.85 → tax 2.73 / total 24.58`
2. **Green:** changed the one helper `cart._round_money` from `ROUND_UP` to `ROUND_HALF_UP`. All 40 tests pass.

#### The note (README + SPEC + PLAN)
Documented it as a **deliberate, sourced deviation from the brief**: the brief says "rounded up", but real-world sales-tax/VAT (SSUTA §324, EU/UK VAT) rounds tax to the **nearest cent**. The brief's worked example still passes (`15.02 / 1.88 / 16.90`). The mode is isolated to one helper, so reverting to strict ceiling is a one-line change.

#### Heads-up on the tradeoff
This is the one place we now diverge from the brief's literal wording. It's defensible (correct, documented, sourced, worked-example-preserving), but if you'd rather follow the brief to the letter for grading, say the word and I'll flip `_round_money` back to `ROUND_UP` (and revert the test expectations). `#next-steps-2` stays gated.
<!-- agent:boundary:475d1663 -->
<!-- /agent:exchange -->

## Queue
<!-- agent:queue auto go -->
- ~Yes add the note to README and use test first TDD to implement the corrected rounding procedure. The test should fail first then implement to fix the test. ~
- I notice the integration tests are skipped. Each agent turn should run the integration tests. Please enable these tests and make them pass if they fail.
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
