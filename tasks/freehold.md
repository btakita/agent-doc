---
agent_doc_session: 6d45cd6f-0051-481b-8aca-02649afc740c
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
agent_doc_model_tier: high
queue: stop
---

# freehold — Session

**Repo:** [freehold-hub/freehold](https://github.com/freehold-hub/freehold) ·
submodule at `src/freehold` · MIT · currently **private**

> **Cloud compute is leasehold. This is freehold.**

A local-first host service in Rust. It runs on hardware you own, dials **out** to
the websites it serves, and runs their AI work in **sandboxed one-shot plugins**.
No per-call bill, no model key on the edge, nothing about your users leaves the
machine.

Split out of [`brookebrodack-dev.md`](brookebrodack-dev.md) on 2026-07-18 —
brookebrodack.net is the first *consumer*, not the project. Guestbook, D1,
wrangler, and OpenAI-billing work stays in that document.

## Design documents

- [`plan-brookebrodack-guestbook-dispatch-service.md`](software/plan-brookebrodack-guestbook-dispatch-service.md)
  — `#gbdispatch1`–`5`: transport, auth, wire protocol, edge integration
- [`plan-dispatch-plugin-ai-stack.md`](software/plan-dispatch-plugin-ai-stack.md)
  — `#gbdispatch6`: which AI runs in a plugin, and where
- `docs/manifesto.md` in-repo — **DRAFT** for later publication
- `docs/plugin-isolation.md` in-repo — threat model

<!-- agent:status mode=replace -->
- [x] Repo created, renamed `brookebrodack-dispatch` → `freehold`, transferred to `freehold-hub`
- [x] Rust workspace: `freehold-protocol`, `freehold-tribunal`, `freehold-host`, `freehold-rules`
- [x] Sandbox runner — bwrap (default) / podman / docker, one sandbox per request
- [x] Strict verdict schema + unanimous-to-approve aggregation
- [x] Protocol generalized: `Job {id, kind, payload, deadline}`; moderation is one task kind
- [x] Single-holder rule replaced by lease-based work claiming
- [x] License Apache-2.0 → MIT, matching the rest of the workspace
- [x] 22 tests, `make check` clean; live `bwrap` run + end-to-end Docker round-trip
- [x] Network denial verified empirically (no `--share-net` ⇒ no resolution)
- [x] Manifesto drafted with verified attributions
- [ ] WebSocket transport + Durable Object (`#gbdispatch2`) — not written, not stubbed
- [ ] seccomp filter — the default bwrap profile has **no syscall filtering**
- [ ] Egress proxy — model plugins are routed to a network that does not exist yet
- [ ] Decide public vs private for the repo
<!-- /agent:status -->

<!-- agent:exchange mode=append -->
### Session origin — opus-4-8

Extracted from `brookebrodack-dev.md`, which holds the full exchange history for
the design conversation. This document starts from the state above.

Three things worth carrying forward, because each was arrived at by correcting an
earlier wrong answer:

**Containers do not solve prompt injection.** They bound the blast radius of a
compromised plugin and do nothing about one steered into voting `approve`. The
actual firewall is the strict verdict schema — a plugin's whole output surface is
`{id, decision, reason}`, so a fully compromised plugin can only vote, in a
vocabulary it does not control. Unanimous-to-approve is the second control:
owning one plugin must never be enough.

**WebSockets were kept, against an earlier recommendation to switch to
queue-polling.** That argument blamed the transport for two design choices — the
single-holder rule and an unsharded Durable Object. Both were fixed directly
instead: lease-based claiming, and one DO per site.

**Inference belongs on the host, not in the sandbox**, and no agent framework
belongs inside a plugin. A one-shot classifier is one HTTP POST and one JSON
parse; a tool-using agent reading attacker-controlled text inverts the threat
model the sandbox exists for.
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Notes

<!-- agent:notes -->
**Naming.** Rejected `dispatch-host` (SEO, unbrandable), `lazily-dispatch`
(semantic conflict — deferred vs realtime), `outrider` (outrider.ai),
`purview` (Microsoft Purview), `inrider` (one letter from "insider"),
`bailiff` (UK debt-collection connotation), `freeholder` (NJ Board of Chosen
Freeholders, exclusionary history), `allodial` (sovereign-citizen pseudolaw),
`freebird` (crowded, plus the song).

The recurring test: a name must not argue against the pitch. Three candidates
died on exactly that.

**Federation** is an intention, not a feature — freeholds interoperating, sharing
plugin reputation, cross-signing verdicts. Deliberately undesigned. Autonomy
first: a freehold must work completely alone, so federation only ever adds.
<!-- /agent:notes -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
