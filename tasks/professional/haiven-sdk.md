---
agent_doc_session: 2516b91c-5d35-4217-9e18-8339f7489794
resume: 15f2a2f8-4f47-4511-a52f-6498d5ced7ce
agent: codex
claude_model: fable
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
agent_doc_model_tier: high
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Re: Haiven SDK proposal, delivery plan, and contacts — gpt-5

Created two presentation-ready artifacts:

- [`tasks/recruit/haiven/sdk-proposal-2026-07-26.md`](../recruit/haiven/sdk-proposal-2026-07-26.md) — external proposal for Luke and Craig, with a 7–11 week Unity-first pilot, milestone gates, budgetary ranges, commercial terms, success measures, mobile coverage, and an access-gated console lane.
- [`tasks/recruit/haiven/sdk-implementation-plan-2026-07-26.md`](../recruit/haiven/sdk-implementation-plan-2026-07-26.md) — technical architecture, conformance strategy, milestone work breakdown, engine/mobile/console expansion plan, quality policy, risks, and first-review agenda.

The proposal reconciles the job-offer context with your current direction: take the stable primary engagement if that is the family-financial decision, then offer Haiven a bounded 10–15 hour/week contract rather than another open-ended founding role. The first authorization is deliberately only Milestone 0 (contract, protocol, conformance, Unity design); Haiven can stop, redirect, or authorize implementation after buying clarity.

I separated engines/frameworks from runtime platforms. Unity is first; Godot or Unreal follows named-pilot evidence; native Swift/Kotlin mobile is a separate product lane; Bevy and MonoGame/FNA serve smaller technical indie ecosystems; Clickteam Fusion 2.5 replaces the old Multimedia Fusion name; Source is project-specific; and Nintendo/PlayStation/Xbox work waits for approved developer access, protected SDKs, hardware, and NDA-compliant CI. This matches the access constraints documented by [Unity](https://unity.com/solutions/console), [Godot](https://docs.godotengine.org/en/stable/about/faq.html#which-platforms-are-supported-by-godot), [MonoGame](https://docs.monogame.net/articles/console_access.html), [Nintendo](https://developer.nintendo.com/the-process), and [PlayStation](https://partners.playstation.net/).

Your realtime addition about agentic development is included in both documents. The method uses Lazily as evidence, not as a required dependency: human-approved semantics and architecture; a reference implementation; bounded agent work packets; shared conformance fixtures; an independent adversarial review pass; proof that the intended corpus actually executed; human merge/release approval; and no production credentials or autonomous deploys. Later adapters parallelize only after the Unity reference and protocol contract are stable.

Corky now resolves both contacts:

- `craig-fryar`: registered with the verified correspondence address `craig@haiven.gg`, canonical `AGENTS.md`, and proposal context.
- `luke-richey`: registered as a social-only contact with [his supplied LinkedIn profile](https://www.linkedin.com/in/lukerichey/), canonical `AGENTS.md`, and proposal context. No email was guessed; the address remains empty until verified. Public profile and Haiven company information support the founder/mission context used in the contact notes ([Luke Richey](https://www.linkedin.com/in/lukerichey), [Haiven](https://www.linkedin.com/company/haiven-inc)).

**Changed paths:** the two proposal/plan files above; `mail/.corky.toml`; `mail/contacts/craig-fryar/{AGENTS.md,CLAUDE.md}`; and `mail/contacts/luke-richey/{AGENTS.md,CLAUDE.md}`. The `CLAUDE.md` entries are compatibility symlinks to canonical `AGENTS.md` files.

**Verification:** both `corky contact info` queries pass and parse the updated configuration. Markdown structure checks pass. The workspace-wide `make check` executed 8,176 tests successfully, then failed while compiling the concurrently modified `src/agent-doc` tree: three `CurrentText` usages omit its new `delivery_version` field (`rpc.rs:5420`, `5444`, `5491`). This cycle did not modify that workstream, so the wider workspace is not CI-ready on current-tree evidence. The required `gh run list --workflow CI --limit 1` lookup was unavailable because this repository has no workflow named `CI`.

**Confidence:** high in the proposal structure, platform sequencing, contact registration, and agentic process; medium in schedule/pricing until Milestone 0 inspects Haiven's live API and names the pilot studio.

**Escalation before signing:** obtain written conflict-of-interest, moonlighting, and invention-assignment clearance from the primary engagement; use separate equipment/accounts/time; and do not promise closed-console delivery before Haiven or a pilot provides approved access. No additional backlog follow-up is needed because the requested artifacts and contacts are complete; remaining choices are commercial approval decisions.
Please reference job-offer.md. I'm thinking of taking the Acadian job for financial stability.
I will propose contracting to build the prototype + drive implementation of the Haiven SDK.
This will target major and possibly minor gaming platform. Haiven caters to Indie game studios so some small platform would be good. Think games that would want chat and social features. Include mobile platforms. Some include:
- Unity
- Unreal Engine
- Godot
- Bevy
- Multimedia Fusion
- Source
- XNA
- Nintendo Switch
- PlayStation 5
- Xbox Series X

Please create a proposal + plan that I can present to luke-richie (https://www.linkedin.com/in/lukerichey/) + craig-fryar (https://www.linkedin.com/in/craigfryar/). Add both as corky contacts.

I want to use my experience with Lazily to propose an agentic development process.
<!-- agent:boundary:eaee4d3b -->
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
