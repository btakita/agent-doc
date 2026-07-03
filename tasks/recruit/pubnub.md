---
agent_doc_session: 4272fa95-d314-4843-bd18-e21c8c99823a
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
agent_doc_lint_dialect: off
queue: start
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ Please create a profile for:
❯ - PubNub https://www.pubnub.com/
❯ - rama-sudha https://www.linkedin.com/in/rama-sudha-a328632b/

❯ The role is https://www.pubnub.com/company/careers/?gh_jid=7988212

❯ Look at their technology:
❯ - https://www.pubnub.com/real-time-messaging-and-events/
❯ - https://www.pubnub.com/decision-intelligence-software/
❯ - https://www.pubnub.com/global-network-infrastructure/
❯ - https://www.pubnub.com/networking-mcp-server/
❯ - https://www.pubnub.com/docs/general/concepts
❯ - https://www.pubnub.com/illuminate/

❯ Please help match this experience with this role. I will be having a 45 minute call with Rama.


### Re: PubNub call prep — who Rama is & what this call is — opus-4-8

**Before anything else: Rama Sudha is a recruiter, not an engineer.** Her LinkedIn/ZoomInfo confirm she's **Senior Talent Partner at PubNub (APAC / Singapore)** — prior Talent Acquisition roles at CoachHub and Delfers. So this 45-minute call is a **recruiter screen**, not a technical or hiring-manager interview. Optimize for: clear motivation, culture/fit signal, communication, logistics (comp, location, timezone), and a crisp 2-minute experience story. Save the deep architecture dives for round 2 — but be ready to prove depth in one or two sentences if she probes.

### Re: The role (jid 7988212 — most likely match) — opus-4-8

The Greenhouse embed (`gh_jid=7988212`) doesn't render its body to a fetch, but PubNub's **only actively-posted engineering role** right now is:

**Senior Software Engineer, Core Systems (Rust)** — remote / 6 locations
> "Design and build distributed systems in **Rust** that power messaging, storage, and eventing across PubNub's real-time platform" — reliability, performance, **observability**, incident response, capacity planning, running services in **Kubernetes** production.

This is an almost bullseye match for you. A close second possibility is **Senior Backend Engineer, AI/LLM Systems** (building PubNub's AI capability layer — LLM inference on real-time streams, evaluation + observability tooling), which maps onto your Presence AI pipeline and agent tooling. Confirm with Rama which req this is early in the call — it changes which stories you lead with.

### Re: What PubNub actually builds — opus-4-8

PubNub is a **real-time messaging / event infrastructure** company (Logitech, athenahealth, PocketGems). Core primitives and numbers worth knowing:

- **Pub/Sub over a global edge network** — messages, channels, **signals** (≤64-byte high-frequency, e.g. typing indicators), **presence** (who's online), **App Context** (metadata/memberships), **Functions** (edge compute), **Access Manager** (token ACLs, AES-256).
- **Ordering** via 17-digit nanosecond **timetokens** (server-assigned, collision-free — no client-side ordering).
- **Scale**: **<100ms delivery** (MCP page cites **<25ms publishes**), **15+ global PoPs**, **99.999% SLA**, **3T+ events/month**, TLS + AES-256, zero-trust.
- **Illuminate** (Decision Intelligence): real-time metric aggregation, anomaly/behavior detection, no-code Decision Rules engine, predictive analytics — mostly gaming/app use cases (matchmaking, in-app economy, fraud/spam).
- **Networking MCP Server**: bridges AI agents (Claude, Cursor, Windsurf) to live PubNub data via Model Context Protocol — streams 12+ SDK docs/API refs to agents, exposes publish/fetchHistory/getPresence as live tools.

### Re: Why you're a strong match (map to their stack) — opus-4-8

Lead with these — every one is real and on your resume:

| PubNub needs | Your evidence |
|---|---|
| **Rust distributed systems** | You build the **lazily** family + **agent-doc**, **tsift**, **corky** in Rust — cross-language reactive signals interoperating over FFI/IPC/WebSockets/WebRTC/WASM. This is your daily language, not a résumé keyword. |
| **Real-time messaging / eventing at low latency** | Presence AI **real-time multimodal avatar pipeline** — text/audio/video → lip-sync on **NVIDIA H100s** over **WebRTC/LiveKit**, sub-second latency; **ZeroMQ**, **event-driven relational algebra**. |
| **Pub/sub + presence + signal graphs** | **signal-space** (live signal-graph surface) and your reactive stack are literally pub/sub + presence semantics; **Harel statechart / Lean 4** formal modeling shows rigor. |
| **Observability + reliability + incident response** | **BDD / spec-driven** discipline (early **RSpec** contributor), **741+ tests**, crash-safe incremental sync, automated verification. |
| **AI/LLM streams** (if the AI role) | **agent-doc** closed-loop agentic dev infra, pluggable LLM backends, eval/observability tooling — exactly the "AI capability layer" they describe. |
| **Kubernetes / cloud production** | Docker, AWS/GCP, multi-service AI monorepo in production. |

The through-line: **"I've spent 20+ years on real-time and reactive systems, and for the last two years I've been building distributed real-time infrastructure in Rust — which is exactly PubNub's core."**

### Re: Likely screening questions + your angles — opus-4-8

- **"Walk me through your background."** → 2-min arc: 20+ years, real-time/reactive thread → Presence AI real-time AI pipeline → now building Rust distributed-systems open source. Land on "PubNub's core systems is the intersection of everything I do."
- **"Why did you leave Presence AI?"** → Presence AI shut down (last month). Neutral, factual — not a red flag.
- **"Why aren't you already placed given the hot AI market?"** → You're mid-process with several roles; deliberately investing this transition period in frontier agent tooling to stay at the forefront; also have a client. (Same framing that worked for Ripple.)
- **"What interests you about PubNub?"** → Real-time infra is your 20-year thread; Rust core systems is where you're strongest and most energized; their MCP/AI layer overlaps your agent-harness work.
- **"Comp / location / timezone?"** → Have your number ready. Note PubNub hubs are **SF, London, Katowice** and remote — Rama is **APAC-based**, so confirm which timezone the team sits in and whether remote-US is in scope for this req.

### Re: Questions to ask Rama — opus-4-8

1. Which exact role is jid 7988212 — **Core Systems (Rust)** or the **AI/LLM backend** role? (Pick your lead stories from her answer.)
2. What does the interview loop look like after this call, and what's the timeline?
3. Is this role US-remote-eligible, or tied to SF/London/Katowice / an APAC timezone?
4. What's the comp band for the level?
5. What's the team's biggest current engineering challenge (scaling, observability, the AI layer)?

### Re: Watch-outs — opus-4-8

- Don't over-index on deep architecture with a recruiter — read her cues; go deep only if invited.
- The role identity is **unconfirmed** (jid didn't render). I inferred it from PubNub's only live engineering posting. Confirm on the call before committing to Rust-vs-AI framing.
- Keep the "why not placed yet" answer confident and brief — it's a fit/confidence check, not a defense.

Sources: [PubNub careers](https://www.pubnub.com/company/careers/), [Rama Sudha (LinkedIn)](https://sg.linkedin.com/in/rama-sudha-a328632b), [Built In — PubNub jobs](https://builtin.com/company/pubnub/jobs), [Real-time messaging](https://www.pubnub.com/real-time-messaging-and-events/), [Global network infra](https://www.pubnub.com/global-network-infrastructure/), [MCP server](https://www.pubnub.com/networking-mcp-server/), [Illuminate](https://www.pubnub.com/decision-intelligence-software/), [Concepts](https://www.pubnub.com/docs/general/concepts).
❯ I would like to make an integration with the Signal Patchboard project. But I have not yet started. I have started with lazily and signal-space. Would this be worth bringing up or should I implement before the technical round if I move on?

### Re: Should you bring up the Signal Patchboard / PubNub integration? — opus-4-8

**Yes — bring it up, briefly, as enthusiasm — but do NOT build it before this call.** Here's the reasoning, because the two halves of your question have different answers.

**Why it's a strong thing to mention (to a recruiter):** it's genuine "why PubNub" signal. Your `patchboard` already has a transport-agnostic realtime I/O layer designed in (`#spb3 patchboard-io`: webhook/WebSocket ingress + egress "to notify other systems in realtime", with `IoBinding` transport descriptors in the spec). **PubNub's pub/sub is a textbook drop-in transport for that layer** — subscribe → signal on ingress, signal-delta → publish on egress. So you're not inventing a reason to like them; you already built the socket their product plugs into. That reads as "this person thinks in our domain," which is exactly the fit signal a recruiter screen is filtering for.

**How to say it (one or two sentences, then stop):**
> "One reason PubNub stands out to me: I'm building a realtime signal patchboard — reactive signal graphs with a transport-agnostic I/O layer — and PubNub's pub/sub is a natural transport for it. I've got the reactive core (lazily, signal-space) working; the patchboard's realtime I/O host is where a transport like PubNub would slot right in."

Be honest about state: **lazily and signal-space are real and working; patchboard is in active development; the PubNub connector specifically is not started.** Say "designing/planning," never "built." Overclaiming a demo you don't have is the one way this backfires.

### Re: Build it before the technical round? — opus-4-8

**Conditional yes — but gate it on advancing, not on this call.** Don't invest build time before you know (a) you've passed the screen and (b) it's actually the Rust core-systems role.

If you *do* advance, a tight PubNub-transport POC is a **high-ROI differentiator** for a technical round — walking in with working code that rides their platform is far stronger than talking about it. And it's cheap because the hard part already exists: `#spb3` already has the WebSocket transport spine, so PubNub is just another connector.

**Scope it ruthlessly** so it's a weekend, not a project:
- One channel **in** (PubNub subscribe → `patchboard-io` ingress → signal) and one channel **out** (signal delta → PubNub publish), on their **free tier**.
- Reuse the existing `IoBinding` / WebSocket path; don't build the Slint UI for this — a headless rack + console trace is enough to demo.
- Keep it as a `patchboard-io` connector module, not a fork.

I've filed this as **`#pubnubpoc` [recommended]** in the backlog with those exact guardrails (don't build before the call or before role/timeline is confirmed).

### Re: Net recommendation — opus-4-8

1. **In the call:** mention it in ~2 sentences as your "why PubNub," framed as designing/planning. It strengthens fit signal at zero risk.
2. **Don't build first.** A recruiter screen doesn't reward a prototype, and you'd be investing before confirming the role/timeline.
3. **After you advance (technical round):** build the tight `#pubnubpoc` connector — that's where working code pays off.

### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc

```text
**IPC proof issue dogfood log**
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.
Issue class: `ipc_proof_insufficient`
Affected component: editor IPC / writeback

ipc_proof_insufficient file=tasks/recruit/pubnub.md source=socket_already_applied patch_id=8fed8071-f3ea-4dbc-9010-e92f718c999c invariant=disk_missing_response_probe recovery=content_ours_snapshot_visible_response_repair response_sha256=1c1130553aaba38ff7d8dccb8d2c621ac22c62db86d161d1abff41e88760c4f5 current_len=3771 current_hash=5d8f2bd05a7f95c9ec50ac9d6775d9c3dedb98a5f761eda663ce8b6b78fbfafa repaired_len=10765 repaired_hash=0bbd69d8b0fd630f1c909b0f66b7f2b66d532ee2310b0e472b2a31feb24f072d doc=pubnub session=4272fa95-d314-4843-bd18-e21c8c99823a turn=cycle-1783037818790
```
<!-- agent:boundary:109a444a -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
/goal Implement backlog item(s): #bt09
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#pubnubpoc] [recommended] IF advancing past the PubNub recruiter screen to a technical round: build a tight patchboard-io PubNub-transport POC — one channel ingress (PubNub subscribe -> signal) + one channel egress (signal delta -> PubNub publish) on their free tier, reusing the #spb3 IoBinding/WebSocket spine. Weekend-sized; high-ROI technical-round differentiator. Do NOT build before the recruiter call or before role/timeline is confirmed.
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
