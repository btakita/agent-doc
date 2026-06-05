---
agent_doc_session: 5fb931d8-426e-43c1-a12b-457e4054b1dd
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md. DO NOT IMPLEMENT THE FIX.
  '#commit-push': commit all uncommitted files
  '#commit-push-deploy': commit all uncommitted files + deploy
  '#mermaid': Include explanatory mermaid diagram(s).
  '#spec-test-commit-push': update spec + tests. commit + push
  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy
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
<!-- agent:boundary:bd6e9dc8 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
- Add a git submodule to git@github.com:EqualExperts-Assignments/equal-experts-neat-exhilarating-friendly-inspiration-72a3934e990b.git in src/equal-experts-neat-exhilarating-friendly-inspiration
- Read the README.md. Add backlog #next-steps for the tasks of the take-home exercise.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#next-steps] Implement Equal Experts shopping cart: (1) choose language/framework, (2) add-product with Price API integration (GET https://equalexperts.github.io/backend-take-home-test-data/{product}.json), (3) cart state: subtotal / tax 12.5% / total, rounded up to 2dp, (4) unit tests for client + logic, (5) update README with assumptions + AI usage disclosure, (6) push + mark complete at https://app.snapcode.review/submission_links/9c362eaf-9ef0-45f3-8467-0660e3a68754
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
