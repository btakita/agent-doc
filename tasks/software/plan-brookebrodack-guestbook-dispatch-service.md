# Plan: local moderation dispatch service for brookebrodack.net (#gbdispatch1–5)

A service on Brian's machine holds a WebSocket to the brookebrodack.net backend,
authenticated by API key. The backend dispatches guestbook moderation requests
over it; local **actor plugins** judge each one and answer approve / deny.

`#gbdispatch1` = this design. `#gbdispatch2` = Durable Object + wire protocol.
`#gbdispatch3` = local service + plugin host. `#gbdispatch4` = edge integration
and fail-closed behaviour. `#gbdispatch5` = tests.

> **Prompt was truncated.** The queued request ends mid-sentence — "It will
> automatically". Everything after that is unspecified, and the most likely
> continuations (auto-approve on plugin consensus? auto-fall-back to OpenAI?
> auto-reconnect? auto-start on boot?) change §6 materially. Confirm before
> `#gbdispatch2` starts.

## Why this is worth building here

It inverts the current dependency in a way that solves a live problem. Today the
edge calls OpenAI and needs paid credit — which is the exact blocker the
guestbook is stuck on (`insufficient_quota`, backlog `#y457`). With dispatch, the
**edge asks; the laptop decides.** Judgement runs on hermes/Gemini or any local
model, at no per-call cost, and the API key never leaves the machine.

It also matches hardware to the job: local GPU, local models, no token bill.

## What already exists (don't rebuild)

- **The moderation core** — `scripts/_lib/guestbook-moderation.ts` already has
  the provider abstraction (`provider_T`, `batch__judge`), the prompt, the JSON
  schema, and the fail-closed decision mapping. A dispatch plugin is **another
  provider**, not a new pipeline.
- **The fail-closed contract** — unparseable, missing, unknown, or absent
  verdicts all resolve to `escalate` and leave the row `pending`. Dispatch must
  inherit this, not reinvent it.
- **Both call sites** — inline judging in `functions/guestbook/api/entries.ts`
  and the cron sweeper in `workers/guestbook-moderate/`. Dispatch slots in at the
  same seam.
- **hermes** — already the primary provider for the local CLI path, already
  configured against Google AI Studio. It is the obvious first actor plugin.
  (Blocked on `#2amm`: `hermes -z` crashes with an httpx `ResponseNotRead`.)
- **lazily-js** — now a dependency of the browser package (`0.24.0`). Its
  `/relay`, `/presence`, `/membership`, `/coordination`, and `/queue` entrypoints
  are directly relevant below.

## 1. Connection direction (forced)

The laptop is behind NAT with no stable inbound address. **The local service is
the WebSocket client**; it dials out to `wss://brookebrodack.net/...`. The
backend can never initiate. Every mechanism below follows from this.

## 2. The edge needs a Durable Object

This is the main architectural consequence, and it is not optional:

- **Pages Functions cannot hold a WebSocket.** They are request-scoped. There is
  no place in the current stack to park a long-lived connection.
- A **Durable Object** is the correct primitive: single-instance, addressable,
  holds the socket and in-flight request state. Use **WebSocket Hibernation** so
  an idle connection does not burn duration billing.
- So the topology grows one component:

```
visitor ──POST /guestbook/api/entries──▶ Pages Function
                                             │ (dispatch RPC)
                                             ▼
                                     Durable Object  ◀──wss (client-initiated)── local service
                                             │                                        │
                                             ▼                                   actor plugins
                                        D1 (verdict)                         (hermes, rules, human)
```

The cron Worker (`workers/guestbook-moderate/`) talks to the same DO, so both
paths share one connection.

## 3. Auth

- Local service presents `Authorization: Bearer <key>` on the upgrade request.
- The DO compares against a Worker secret with a **constant-time** compare —
  reuse `timing_safe_equal` from `functions/_lib/guestbook.ts` rather than `===`.
- Key lives in `passage brookebrodack/guestbook/DISPATCH_API_KEY` locally and as
  a Worker secret on the edge. Distinct from the OpenAI key; different blast
  radius.
- **Single-holder rule.** One connection at a time. A second authenticated
  connect supersedes the first (last-writer-wins) rather than fanning out — two
  laptops answering the same request is a correctness problem, not redundancy.

## 4. Wire protocol

JSON frames, one in-flight map keyed by `request_id`:

| Frame | Direction | Payload |
|---|---|---|
| `hello` | local → edge | service version, plugin ids, max concurrency |
| `ready` | edge → local | accepted; resume token |
| `moderate` | edge → local | `request_id`, entry `{id, name, message, created_at}` |
| `verdict` | local → edge | `request_id`, `approve\|reject\|escalate`, reason, plugin id |
| `ping`/`pong` | both | liveness |

Rules:
- **Every request carries a deadline.** The edge stops waiting at the deadline
  and escalates; a late `verdict` for an expired `request_id` is dropped, not
  applied. Otherwise a slow laptop silently publishes minutes later.
- `verdict` values are the **existing** three. No new vocabulary.
- Unknown frame types are ignored, so the protocol can grow.

## 5. Local service and actor plugins

- Bun process. Plugin = a module exporting `judge(entry) → verdict | null`,
  where `null` means "no opinion" and is **not** an approval.
- Concurrency and backpressure via lazily `/relay` (RelayCell) — a **bounded**
  queue. A submission flood must shed load or park it, never OOM the laptop. On
  overflow, decline explicitly so the edge escalates immediately instead of
  waiting out the deadline.
- Plugin presence/liveness via lazily `/presence` + `/membership`; multi-plugin
  agreement via `/coordination`.
- **Aggregation policy across plugins is a decision, not a default.** Options:
  first-answer-wins (fastest), unanimous-approve (safest), or majority. Given the
  fail-closed posture everywhere else, **unanimous-to-approve, any-reject-rejects,
  otherwise escalate** is the consistent choice. Confirm — this may be what the
  truncated sentence was specifying.
- First plugins: (a) hermes/Gemini, (b) a cheap deterministic rules pass for
  obvious spam, (c) a human-prompt plugin that pushes to a terminal or phone.

## 6. Edge integration and fail-closed behaviour

Add a `dispatch__provider` to the existing provider list. Order becomes:

```
dispatch (local, free) → openai (paid, if credentialed) → escalate
```

Non-negotiable invariants, all inherited from the current system:

- **Laptop offline ⇒ entries stay `pending`.** Never a failed submission, never
  an auto-approve. The visitor sees the normal "will appear once reviewed".
- **Inline path keeps a short deadline** (~3s, tighter than the current 8s
  OpenAI timeout — a local model on the LAN should be fast, and the visitor is
  waiting). On timeout: escalate, let the cron sweeper retry later.
- **The cron sweeper is the safety net**, exactly as it is now. It can afford a
  much longer deadline than the inline path.

## 7. Risks

- **A Durable Object is real new infrastructure** — new billing surface, new
  failure mode, and the first stateful component in this stack. Weigh it against
  simply putting $5 of credit on OpenAI, which solves today's blocker with zero
  new components. This design is worth it for the *plugin/actor* capability and
  local-model routing, not as a way to dodge a small bill.
- **Home uptime becomes site behaviour.** If the laptop is the only judge, the
  queue stalls whenever it sleeps. Keeping OpenAI as the second provider removes
  that coupling — but then the credit is needed anyway.
- **The single-holder rule needs teeth.** Reconnect storms and duplicate
  connections are where this class of design usually breaks.

## 8. Sequence

1. `#gbdispatch2` — DO + wire protocol + auth, with a stub judge that always
   escalates. Proves the transport without touching moderation behaviour.
2. `#gbdispatch3` — local service, plugin host, RelayCell backpressure, hermes
   plugin.
3. `#gbdispatch4` — `dispatch__provider` in the provider list; deadlines and
   fail-closed paths in both call sites.
4. `#gbdispatch5` — tests: offline, timeout, late verdict, duplicate connection,
   plugin disagreement, overflow. All must assert **nothing gets published**.

## Open questions

1. **What does the truncated sentence say?** Blocks §6 aggregation/automation.
2. Should the human-prompt plugin be in scope for v1, or is it purely automated?
3. Is a DO acceptable, given it's the first stateful piece of this stack?
4. Should dispatch also serve the *admin* dashboard (bulk re-judge), or only new
   submissions?
