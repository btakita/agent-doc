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
queue: start
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

### Re: Haiven SDK agent harness assistance — gpt-5

> **Queue prompt:** Please include agentic harness assistance with a mcp server for documentation lookups + opinionated guidance + possible cli tooling for verification + eval test suite to verify agentic requests.

Updated both presentation artifacts with a concrete, phased agent-harness lane:

- [`tasks/recruit/haiven/sdk-proposal-2026-07-26.md`](../recruit/haiven/sdk-proposal-2026-07-26.md) now presents a Haiven-owned, read-only MCP server for version-scoped documentation and opinionated guidance; a thin deterministic verification/evidence CLI; and an agent-request eval suite. It keeps the harness separate from the game runtime, places contract/design work in Milestone 0, and makes the narrow Milestone 1 prototype subject to discovery-confirmed scope and pricing.
- [`tasks/recruit/haiven/sdk-implementation-plan-2026-07-26.md`](../recruit/haiven/sdk-implementation-plan-2026-07-26.md) now specifies the architecture, repository shape (`tools/mcp`, `tools/cli`, and `evals/agent-requests`), MCP resources and citation rules, CLI behavior, eval scenarios and rubrics, security/access boundaries, metrics, milestone outputs, CI evidence, and acceptance checks.

The opinionated guidance contract distinguishes **required contract**, **recommended paved road**, **supported alternative**, and **open decision**. It requires version/source citations, reports stale or conflicting content explicitly, and keeps write-capable tools, production credentials, autonomous releases, and protected console material outside the first version.

The eval suite verifies the request and its evidence as a system: expected and forbidden file changes, required commands and fixture execution, grounding, API correctness, uncertainty handling, write-scope compliance, and safety are scored separately. Initial cases cover Unity scaffolding, reconnect/resume, a chat/presence/moderation slice, version mismatch diagnosis, SDK migration, and refusal or safe redirection of disallowed production/console requests. A fluent response without the required artifacts and executed fixtures fails.

**Changed paths:** the two Haiven proposal/plan artifacts above. The active session document is reserved for binary-owned closeout.

**Verification:** both documents parse successfully through the Markdown AST (`79` nodes / `264` lines for the proposal and `240` nodes / `485` lines for the implementation plan before the final metadata-spacing cleanup), and whitespace validation passes. The stale `agent-doc` install was rebuilt successfully as `0.35.30`, including the matching editor package. Workspace `make check` then ran 955 of 8,181 tests: 952 passed and three current-tree `agent-doc::run_integration` owner-pane preflight tests failed (`preflight_emits_owned_pane_self_invocation_for_active_queue_head`, `preflight_emits_owned_pane_self_invocation_for_unresolved_prompt`, and `preflight_owned_pane_self_invocation_absent_for_non_owner_pane`). The Haiven changes are Markdown-only and do not touch that workstream, but the workspace is not green on this evidence. The required CI lookup was unavailable because this repository has no workflow named `CI`.

**Confidence:** high that the requested MCP, guidance, CLI, and eval concepts are now presentation-ready and technically actionable. No additional backlog follow-up is needed because the requested artifact update is complete; remaining decisions are Milestone 0 commercial and product approvals.

### Re: Haiven SDK agent harness assistance — gpt-5

> **Queue prompt:** Please include agentic harness assistance with a mcp server for documentation lookups + opinionated guidance + possible cli tooling for verification + eval test suite to verify agentic requests.

Updated both presentation artifacts with a concrete, phased agent-harness lane:

- [`tasks/recruit/haiven/sdk-proposal-2026-07-26.md`](../recruit/haiven/sdk-proposal-2026-07-26.md) now presents a Haiven-owned, read-only MCP server for version-scoped documentation and opinionated guidance; a thin deterministic verification/evidence CLI; and an agent-request eval suite. It keeps the harness separate from the game runtime, places contract/design work in Milestone 0, and makes the narrow Milestone 1 prototype subject to discovery-confirmed scope and pricing.
- [`tasks/recruit/haiven/sdk-implementation-plan-2026-07-26.md`](../recruit/haiven/sdk-implementation-plan-2026-07-26.md) now specifies the architecture, repository shape (`tools/mcp`, `tools/cli`, and `evals/agent-requests`), MCP resources and citation rules, CLI behavior, eval scenarios and rubrics, security/access boundaries, metrics, milestone outputs, CI evidence, and acceptance checks.

The opinionated guidance contract distinguishes **required contract**, **recommended paved road**, **supported alternative**, and **open decision**. It requires version/source citations, reports stale or conflicting content explicitly, and keeps write-capable tools, production credentials, autonomous releases, and protected console material outside the first version.

The eval suite verifies the request and its evidence as a system: expected and forbidden file changes, required commands and fixture execution, grounding, API correctness, uncertainty handling, write-scope compliance, and safety are scored separately. Initial cases cover Unity scaffolding, reconnect/resume, a chat/presence/moderation slice, version mismatch diagnosis, SDK migration, and refusal or safe redirection of disallowed production/console requests. A fluent response without the required artifacts and executed fixtures fails.

**Changed paths:** the two Haiven proposal/plan artifacts above, committed as `736f337a0`. The active session document is reserved for binary-owned closeout.

**Verification:** both documents parse successfully through the Markdown AST (`79` nodes / `264` lines for the proposal and `240` nodes / `485` lines for the implementation plan), and whitespace validation passes. The stale `agent-doc` install was rebuilt successfully as `0.35.30`, including the matching editor package. Workspace `make check` then ran 955 of 8,181 tests: 952 passed and three current-tree `agent-doc::run_integration` owner-pane preflight tests failed (`preflight_emits_owned_pane_self_invocation_for_active_queue_head`, `preflight_emits_owned_pane_self_invocation_for_unresolved_prompt`, and `preflight_owned_pane_self_invocation_absent_for_non_owner_pane`). The Haiven changes are Markdown-only and do not touch that workstream, but the workspace is not green on this evidence. The required CI lookup was unavailable because this repository has no workflow named `CI`.

**Confidence:** high that the requested MCP, guidance, CLI, and eval concepts are now presentation-ready and technically actionable. No additional backlog follow-up is needed because the requested artifact update is complete; remaining decisions are Milestone 0 commercial and product approvals.

### Re: Wave C engine market balance — gpt-5

Agreed—Bevy is useful as a technically aligned Rust integration, but its present reach is too small for it to anchor the market-coverage wave. I updated both Haiven artifacts so Wave C now distinguishes broader-reach candidates from pilot-driven specialist adapters.

The revised priority order is:

- **GameMaker** is a Priority 3 market-balancing candidate for commercial 2D indie studios. The proposal calls for a GML-facing package plus platform extensions, with desktop/mobile/web first and consoles kept access-gated. GameMaker's official extension model supports third-party SDK and platform-specific integration, and its export surface spans desktop, mobile, web, and approved console lanes ([extensions](https://manual.gamemaker.io/lts/en/The_Asset_Editors/Extensions.htm), [exports](https://gamemaker.io/en/blog/export-with-gamemaker)).
- **Cocos Creator** is a Priority 3 TypeScript-oriented mobile/web candidate. The adapter shape is a TypeScript client/component package with native bridges only where required; Cocos officially supports TypeScript/JavaScript and project extension packages ([language support](https://docs.cocos.com/creator/3.7/manual/en/scripting/language-support.html), [extensions](https://docs.cocos.com/creator/3.8/manual/en/editor/extension/create-extension.html)).
- **Construct 3** is a Priority 3 low-code/small-studio candidate, represented as a JavaScript/TypeScript addon with actions, conditions, expressions, events, and a sample project. Its official Addon SDK is designed for third-party service plugins, and its publishing targets cover web, mobile, and desktop wrappers ([Addon SDK](https://www.construct.net/en/make-games/manuals/addon-sdk), [publishing](https://www.construct.net/en/make-games/manuals/construct-3/overview/publishing-projects)).
- **Bevy** moves to Priority 4 and the pilot-driven specialist group. MonoGame/FNA, Clickteam Fusion, and Source remain there; Defold, Phaser, and GDevelop are explicit pipeline- or reuse-driven evaluation options.

The implementation plan's Wave C is now split into **Market-balancing candidates** and **Pilot-driven specialist candidates**. It also requires Milestone 0 to score named studios, target platforms, launch timing, integration cost, support burden, and protocol-core reuse before authorizing any adapter. That keeps the wider engine list commercially disciplined rather than turning it into an unsupported-platform promise.

**Changed paths:** `tasks/recruit/haiven/sdk-proposal-2026-07-26.md` and `tasks/recruit/haiven/sdk-implementation-plan-2026-07-26.md`, committed as `383138ada`.

**Verification:** both updated documents parse through the Markdown AST (`82` nodes / `279` lines and `246` nodes / `511` lines), whitespace validation passes, and the full workspace `make check` passed all `8,181` tests (`241` skipped). The required GitHub lookup remains unavailable because this repository has no workflow named `CI`.

**Confidence:** high that Wave C now balances market reach much better without overcommitting to unsupported engines. No additional backlog follow-up is needed because the requested prioritization and artifact update are complete. The session document is large, but I left it un-compacted because auto-compaction is off and no compaction approval was given.
Bevy has extremely small marketshare. Please include other engines in Wave C to balance the marketshare.

### Re: Haiven SDK superrepo design — gpt-5

Designed and committed a superrepo architecture that keeps every SDK, the shared foundations, and the agent harness in dedicated repositories.

**Architecture decision:** use a thin `haiven-sdk-superrepo` as the integration control plane. A versioned `components.yaml` declares repositories, lifecycle, owners, compatibility ranges, and standard commands; `workspace.lock.json` pins exact commits and artifact digests. A workspace CLI materializes the locked repositories without treating sibling source checkouts as package dependencies. Shipping SDKs consume released contract/conformance artifacts and retain independent versions, CI, permissions, and release histories.

The design establishes:

- dedicated repositories for contracts, conformance, the agent harness, Unity, Unreal, Godot, Android, iOS, GameMaker, Cocos Creator, Construct 3, and pilot-gated Bevy;
- active/planned/incubator lifecycle gates so empty or unsupported SDK repos are not created prematurely;
- repository-local native CI plus superrepo integration CI;
- an explicit multi-repository contract-change train using immutable candidates, repository-local pull requests, and a tested lock update;
- independent Semantic Versioning with a compatibility dashboard derived from CI evidence;
- least-privilege ownership and publishing boundaries;
- a dedicated `haiven-sdk-agent-harness` repository for the read-only MCP server, verification CLI, and agent-request eval suite, with no merge, publish, deploy, or production-secret authority;
- Milestone 0/1 bootstrap steps, handoff/rollback drills, and implementation acceptance criteria.

**Changed paths:**

- [Superrepo design](../recruit/haiven/sdk-superrepo-design-2026-07-26.md)
- [Milestone proposal](../recruit/haiven/sdk-proposal-2026-07-26.md)
- [Implementation plan](../recruit/haiven/sdk-implementation-plan-2026-07-26.md)

**Commit:** `512344c07` (`haiven sdk: design dedicated-repo superrepo`)

**Commands and verification:** all three documents parse through the Markdown AST (superrepo: `124` nodes / `408` lines; proposal: `83` / `289`; plan: `251` / `525`), whitespace validation passed, and the required full workspace `make check` completed with exit code 0 and no detected failures. The required one-time GitHub lookup could not find a workflow named `CI`, so no CI run exists to inspect.

**Confidence:** high that the design reconciles a unified development experience with truly independent SDK repositories and gives the agent harness a clear product and security boundary. No additional backlog follow-up is needed.

**Escalation:** none for this task. Preflight did warn that the installed `agent-doc` binary is one committed change behind its local source; that is unrelated to the Haiven artifacts and did not block the cycle.
Please design a superrepo with dedicated repos for each SDK. This would include agent harness.

### Re: RPG Maker, Ren'Py, and long-tail framing — gpt-5

> **Queue prompt:**
>
> do [#rpgmakerren]


Added RPG Maker and Ren'Py across the SDK proposal, implementation roadmap, and superrepo design, and corrected Wave C so none of its adapters is presented as a market-balancing anchor.

The [2025 Video Game Insights report](https://app.sensortower.com/vgi/assets/reports/The_Big_Game_Engines_Report_of_2025.pdf) supports the long-tail framing: it describes public engines outside Unity and Unreal as significantly smaller; its 2024 Steam view groups Godot, RPG Maker, GameMaker, and similar engines at just over 10% of qualifying releases but about 2% of units sold, and its smaller-engine analysis includes Ren'Py. I recorded the important limits as well: this is directional Steam evidence, the highlighted mix excludes games below 1,000 lifetime units, and the report does not separately quantify Cocos Creator or Construct 3.

The roadmap now specifies:

- **RPG Maker:** a dedicated `haiven-sdk-rpg-maker` repository, with one target generation chosen first—prefer RPG Maker MZ's [JavaScript plugin surface](https://rpgmakerofficial.com/product/mz/plugin/start/whta-plugin.html) unless a pilot requires something else—and an upfront networking, save/load, deployment, and distribution spike.
- **Ren'Py:** a dedicated `haiven-sdk-renpy` repository, using a Python package plus thin `.rpy` integration. Native targets come first; save/rollback behavior and the web transport need explicit validation because Ren'Py's [Python integration](https://www.renpy.org/doc/html/python.html) and [web networking constraints](https://www.renpy.org/doc/html/web.html) differ by target.
- **Wave C:** renamed to “long-tail and specialist ecosystems”; GameMaker, RPG Maker, Ren'Py, Cocos Creator, and Construct 3 are all pilot-gated. Selection is based on Haiven's qualified pipeline, integration cost, platform fit, reuse, and support burden—not generalized market-share claims.

**Changed paths:**

- [Milestone proposal](../recruit/haiven/sdk-proposal-2026-07-26.md)
- [Implementation plan](../recruit/haiven/sdk-implementation-plan-2026-07-26.md)
- [Superrepo design](../recruit/haiven/sdk-superrepo-design-2026-07-26.md)

**Commit:** `e39a5dac7` (`haiven sdk: add RPG Maker and Ren'Py roadmap`)

**Commands and verification:** the three documents parse through the Markdown AST (proposal: `86` nodes / `304` lines; plan: `253` / `540`; superrepo: `124` / `411`), whitespace validation passed, and full workspace `make check` completed with exit code 0 and no detected failures. The required one-time GitHub lookup found no workflow named `CI`.

**Confidence:** high. The documents now reflect the report without overstating what its Steam-only estimates establish, and both new SDKs have concrete technical and repository boundaries.

**Escalation:** none. The requested work is complete.

The [2025 Video Game Insights report](https://app.sensortower.com/vgi/assets/reports/The_Big_Game_Engines_Report_of_2025.pdf) supports the long-tail framing: it describes public engines outside Unity and Unreal as significantly smaller; its 2024 Steam view groups Godot, RPG Maker, GameMaker, and similar engines at just over 10% of qualifying releases but about 2% of units sold, and its smaller-engine analysis includes Ren'Py. I recorded the important limits as well: this is directional Steam evidence, the highlighted mix excludes games below 1,000 lifetime units, and the report does not separately quantify Cocos Creator or Construct 3.

The roadmap now specifies:

- **RPG Maker:** a dedicated `haiven-sdk-rpg-maker` repository, with one target generation chosen first—prefer RPG Maker MZ's [JavaScript plugin surface](https://rpgmakerofficial.com/product/mz/plugin/start/whta-plugin.html) unless a pilot requires something else—and an upfront networking, save/load, deployment, and distribution spike.
- **Ren'Py:** a dedicated `haiven-sdk-renpy` repository, using a Python package plus thin `.rpy` integration. Native targets come first; save/rollback behavior and the web transport need explicit validation because Ren'Py's [Python integration](https://www.renpy.org/doc/html/python.html) and [web networking constraints](https://www.renpy.org/doc/html/web.html) differ by target.
- **Wave C:** renamed to “long-tail and specialist ecosystems”; GameMaker, RPG Maker, Ren'Py, Cocos Creator, and Construct 3 are all pilot-gated. Selection is based on Haiven's qualified pipeline, integration cost, platform fit, reuse, and support burden—not generalized market-share claims.

**Changed paths:**

- [Milestone proposal](../recruit/haiven/sdk-proposal-2026-07-26.md)
- [Implementation plan](../recruit/haiven/sdk-implementation-plan-2026-07-26.md)
- [Superrepo design](../recruit/haiven/sdk-superrepo-design-2026-07-26.md)

**Commit:** `e39a5dac7` (`haiven sdk: add RPG Maker and Ren'Py roadmap`)

**Commands and verification:** the three documents parse through the Markdown AST (proposal: `86` nodes / `304` lines; plan: `253` / `540`; superrepo: `124` / `411`), whitespace validation passed, and full workspace `make check` completed with exit code 0 and no detected failures. The required one-time GitHub lookup found no workflow named `CI`.

**Confidence:** high. The documents now reflect the report without overstating what its Steam-only estimates establish, and both new SDKs have concrete technical and repository boundaries.

**Escalation:** none. The requested work is complete.
### Re: RPG Maker, Ren'Py, and long-tail framing — gpt-5

Added RPG Maker and Ren'Py across the SDK proposal, implementation roadmap, and superrepo design, and corrected Wave C so none of its adapters is presented as a market-balancing anchor.
<!-- agent:boundary:7083b799 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority -->
- 🚧 do [#authenticationhandledsdk]
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-07-26 [#rpgmakerren] Add RPG Maker + Ren'Py. These are all long tail. See https://app.sensortower.com/vgi/assets/reports/The_Big_Game_Engines_Report_of_2025.pdf
<!-- /agent:done -->
