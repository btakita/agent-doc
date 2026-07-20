---
agent_doc_session: 4272fa95-d314-4843-bd18-e21c8c99823a
resume: ec539e35-173e-4b72-bce4-6d9af2f812cf
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
pending_done_guard: warn
agent_doc_lint_dialect: off
queue: go
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/5bb29c8c060c6614c2a73531336daae9d157ea8fa3f792f46e1ab079ff83813d-20260720-183115.md`*

Compacted content:
- Archived 11 response topic(s): PubNub call prep; The role (jid 7988212; What PubNub actually builds; 8 more
- Prior summary/context: ❯ Please create a profile for: ❯ - PubNub https://www.pubnub.com/ ❯ - rama-sudha https://www.linkedin.com/in/rama-sud...
Help me to create a simple pubnub app. Create a gh repo for it...btakita/pubnub-example?
Make a simple chat application.
See https://admin.pubnub.com/account/198533/functions/packages

### Re: PubNub chat example in Rust — opus-4-8

> **Queue prompt:** Help me to create a simple pubnub app. Create a gh repo for it...btakita/pubnub-example?

Shipped: **https://github.com/btakita/pubnub-example** (public, MIT).

A real-time terminal chat on the PubNub Rust SDK. Two terminals, one channel, live messages + presence. Runs with zero setup on PubNub's public `demo` keyset.

**What it does**
- Subscribe stream renders messages, typing events, and presence join/leave as they arrive
- stdin reader publishes what you type; a failed publish warns instead of killing the session
- `/who` lists occupants via the presence API; `/quit` exits; `status_stream()` surfaces connection state
- Config precedence: CLI flags > env > defaults

**Structure** — network calls confined to `main.rs`; the decision logic (wire format, event decoding, flag/env precedence) is pure functions in `chat.rs` / `config.rs` with 23 unit tests.

**Verification** — `cargo fmt --check`, `clippy -D warnings`, 23 tests, and `--release` all green locally. Verified live end-to-end: two clients on one channel, bidirectional messages plus presence join observed. CI run 29768829967 was still in progress at closeout.

**Two things the live run caught that tests alone would not have:**

1. **Message types are keyset-gated.** `publish_message(..).r#type("text")` is only echoed back on subscribe when the keyset has the message-type feature enabled. On `demo` it arrives as `None`, so routing on the type alone rendered every typing event as `Unknown` noise in the transcript. `Event::parse` now treats the type as a hint and discriminates on payload shape; regression test pins it.
2. **Presence needs heartbeats.** Without `.with_heartbeat_value()` / `.with_heartbeat_interval()` on the builder, join/leave never fire and `/who` reports an empty channel.

**On the docs you linked** — there is no Rust Chat SDK. The Chat SDK covers JavaScript, Swift, Kotlin, Unity, and Unreal only, so the chat layer here is built directly on the core client. And the Rust core SDK at `0.8` is publish/subscribe/presence/access/parse_token — **no Message Persistence, App Context, or Message Actions**. That is why a late joiner starts with an empty transcript instead of backfilling, and why typing indicators ride as typed messages rather than true signals. Both gaps are documented honestly in the README.

That gap is worth carrying into the recruiter conversation: it is a concrete, specific observation about their product surface, made from having actually built on it.

I did not touch the PubNub Functions package you linked (`admin.pubnub.com/.../functions/packages`) — that needs your account session. Say the word if you want a Function added to this repo.
<!-- agent:boundary:d935c6f9 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue go -->
- ~~📌 Help me to create a simple pubnub app. Create a gh repo for it...btakita/pubnub-example?~~ — auto-struck: answered this cycle (#ftstrike)
- /goal Implement backlog item(s): #bt09
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#pubnubpoc] [recommended] IF advancing past the PubNub recruiter screen to a technical round: build a tight patchboard-io PubNub-transport POC — one channel ingress (PubNub subscribe -> signal) + one channel egress (signal delta -> PubNub publish) on their free tier, reusing the #spb3 IoBinding/WebSocket spine. Weekend-sized; high-ROI technical-round differentiator. Do NOT build before the recruiter call or before role/timeline is confirmed.
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
