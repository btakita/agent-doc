---
agent_doc_session: be20dd6e-31ce-4f16-93ed-42c9c2a21b98
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_model: claude-opus-4-6
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': ''
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push

---

# agent-loop operations

Operational task document for agent-loop workspace management.

**History** (archived in git, commits up to `f80f524`):
- Submodule conversions (briantakita.me-dev, ctx-core-dev, mail, python-intentions)
- mail/mailboxes/lucas nested submodule init + push
- JetBrains module creation (20 .iml files)
- IDEA Cargo project path fixes (workspace.xml)
- Recursive `.agent-doc/` gitignore additions (20+ submodules)
- `.sync-state.json` gitignore for corky/mail
- Nested submodule fixes (ctx-core-dev/lib/relysjs, rappstack-dev/lib/ui--server--blog)

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/18c3ca3007b9c6c5bc21d81a0b9ff27f894487e97141f4cdafe2a9d12a4a19b1-20260519-035814.md`*

Compacted content:
- Archived 1 response topic(s): spec created & items transferred
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/18c3ca30...
❯ Read transcript from https://www.youtube.com/watch?v=qOvc9IUKEIc. Provide a plan on creating composable skills. Should we create a skill on creating composable skills?
### Re: composable skills plan — gpt-5

I read the transcript for `https://www.youtube.com/watch?v=qOvc9IUKEIc` using native captions. The video's core advice is useful, but it should be translated into our skill system with some discipline: prompt skills instead of ad hoc prompts, treat skills as instructions plus tools/resources, split large workflows into small reusable capabilities, and update skills after real usage. The key composability section is around `04:37-06:18`, and the deterministic-script/tooling section is around `06:29-07:34`.

Yes, we should create a skill for creating composable skills if this is going to be a repeated activity. But it should not duplicate the existing `skill-creator` skill. The better shape is either a small companion skill, tentatively `compose-skills` / `skill-architect`, or a new section in `skill-creator` if we want fewer moving parts. I lean toward a companion skill only if it owns a distinct pre-creation workflow: decomposing a domain into multiple cooperating skills before implementation begins.

Plan:

1. Define the decision boundary. The skill should trigger when the user asks to design a skill ecosystem, split an oversized skill, audit skill composability, or decide whether a workflow deserves one skill or several. It should not trigger for ordinary single-skill creation; `skill-creator` already owns that.

2. Create a composability rubric. Score candidate skills on focus, reuse potential, stable inputs/outputs, hidden tool needs, safety/invocation policy, and overlap with existing skills. The output should be a short architecture plan: skill names, responsibilities, what each skill may call or rely on, and what should remain ordinary prompt text.

3. Add decomposition patterns. Include examples such as replacing one broad `content-creation` skill with `research-topic`, `write-script`, and `draft-social-post`; or splitting agent-doc work into session-cycle, routing-diagnostics, commit-closeout, and repair workflows. The examples should show when splitting improves debugging and reuse, and when it adds ceremony.

4. Require a resource inventory. For each proposed skill, identify whether it needs only `SKILL.md`, a `references/` file, a deterministic `scripts/` helper, or reusable `assets/`. This follows the video's point that the leverage is often in tools, not prettier prompt text.

5. Add invocation policy. Decide which skills should be user-invocable, agent-only, or user-only/high-risk. For this codebase, anything that deploys, sends messages, publishes packages, or mutates remote systems should have explicit invocation boundaries.

6. Validate with forward tests. Before treating a composed skill set as good, run realistic prompts that require one skill, multiple skills, and no skill. Check whether the intended skill triggers, whether the agent overuses it, and whether the split actually reduces repeated reasoning.

7. Build an improvement loop. After each real use, ask whether the correction was one-off or should become permanent skill knowledge. Permanent fixes become edits to `SKILL.md`, `references/`, or scripts, followed by validation.

Recommended first artifact: a short `compose-skills` spec or plan, not implementation yet. It should include the trigger description, the rubric, two worked examples from our repo, and a validation checklist. If that plan feels thin after one or two uses, fold it into `skill-creator`; if it proves reusable, promote it into a standalone skill.
❯ do [#0b3x]. #spec-test-build-install-commit-push
### Re: #0b3x compose-skills implementation — gpt-5

Implemented `#0b3x` as a standalone Codex skill rather than extending `skill-creator`, because the reusable boundary is pre-implementation architecture: deciding whether a workflow should be one skill, several cooperating skills, or ordinary prompt guidance. `skill-creator` remains the implementation path for each individual skill after the composition plan is validated.

Changed files:

- `.codex/skills/compose-skills/SKILL.md` — workflow, decision boundary, resource inventory, invocation policy, and validation instructions.
- `.codex/skills/compose-skills/SPEC.md` — functional contract for the skill architecture plan.
- `.codex/skills/compose-skills/references/examples.md` — worked examples for content workflows, agent-doc workflows, and browser automation.
- `.codex/skills/compose-skills/references/example-plan.md` — valid sample plan fixture.
- `.codex/skills/compose-skills/scripts/validate_composition_plan.py` — deterministic validator for required plan sections and candidate skill entries.
- `.codex/skills/compose-skills/agents/openai.yaml` — UI metadata.

Validation:

- `python3 /home/brian/.codex/skills/.system/skill-creator/scripts/quick_validate.py .codex/skills/compose-skills` passed.
- `python3 .codex/skills/compose-skills/scripts/validate_composition_plan.py .codex/skills/compose-skills/references/example-plan.md` passed.
- Installed locally to `/home/brian/.codex/skills/compose-skills` and re-ran both validation checks there; both passed.
- `make check` passed at the workspace root, covering `instruction-files`, `src/agent-doc`, and `src/corky`.

CI/tmux review:

- Latest root `Agent Doc` GitHub Actions run `26075147189` had empty-step failures for both `check` and `tmux-ci`, with no job steps started. I classified this as an external CI-start blocker, not a tmux regression. Local verification evidence above is clean.

Repo commit:

- `ba4734c97 feat: add compose-skills planning skill`

The pending item is complete.
❯ Should composable-skills be part of one of my packages or its own package? I want composable-skills to work with Claude code as well. Ideally, the deterministic logic would be written in Rust to ensure installability without a runtime.
### Re: composable-skills packaging — gpt-5

I would not make `composable-skills` its own package yet. The best initial home is the skill lifecycle package: either the existing `skill-harness` crate, or a renamed/split `agent-skills` crate if that is where the portable skill standard is heading. The reason is that composition is not a separate product surface yet; it is part of installing, validating, auditing, and distributing portable skills across harnesses.

I would keep `agent-doc` out of the ownership path. `agent-doc` can consume the skill and may have agent-doc-specific examples, but generic skill composition should not be coupled to interactive markdown sessions.

Recommended shape:

- Canonical skill content lives in a portable source tree, probably `.agent/skills/compose-skills/`, not only `.codex/skills/compose-skills/`.
- `skill-harness` owns install/sync into Claude Code, Codex, OpenCode, and generic layouts.
- The deterministic checks move from the current Python validator into Rust, exposed as a library plus CLI command, for example `skill-harness compose validate <plan.md>` or `skill-harness validate-composition <plan.md>`.
- Claude Code support comes from installing the same portable skill into `.claude/skills/compose-skills/SKILL.md`; Codex support can continue to use the Codex-local skill layout or generated AGENTS adapter.
- A separate package only becomes justified if composition gets a distinct CLI/API surface used independently of skill installation, such as a planner that reads a workflow spec and emits multiple skill directories.

So the packaging path I would take is: keep the current Codex skill as the prototype, move the portable source into `skill-harness` / `agent-skills`, replace the validator with Rust, then add installer tests proving the same skill works for Claude Code and Codex. If that starts to outgrow lifecycle management, split it later as something like `agent-skill-compose`, but make that an extraction after the API stabilizes rather than the first move.
❯ Would you recommend using `skill-harness` or creating a `agent-skills` package?
### Re: agent-skills vs skill-harness — gpt-5

I would create `agent-skills` as the durable package boundary, but I would not start it as unrelated new code. Use `skill-harness` as the seed and migrate/rename/extract from there.

The reason is naming and ownership. `skill-harness` sounds like a test harness or lifecycle utility. But the thing you are describing is broader: a portable skill format, installation across Claude Code/Codex/OpenCode, deterministic validation, composition planning, and eventually maybe generation. That domain wants a first-class package name: `agent-skills`.

The clean shape is:

- `agent-skills`: Rust library plus CLI for the portable skill model, validation, composition checks, install/sync, and harness adapters.
- `agent-kit`: low-level shared agent environment detection and generic utilities.
- `agent-doc`: consumer of skills, not owner of the generic skill system.
- `skill-harness`: either retired, renamed to `agent-skills`, or kept temporarily as a compatibility wrapper around `agent-skills` commands.

So my recommendation is: make `agent-skills` the target package, but implement it by moving the relevant `skill-harness` code and specs forward. Do not maintain both as peer packages long term unless they have a sharp split, such as `agent-skills` owning the library/domain and `skill-harness` only owning eval/test harness behavior. If there is no separate eval harness product, the `skill-harness` name should go away.

For `compose-skills`, that means the Rust validator and portable source layout should land under `agent-skills`, with tests proving installs into `.claude/skills/compose-skills/SKILL.md` and the Codex skill layout. That gives you the no-runtime installability path and avoids baking a generic skill design system into a Codex-only directory.
❯ agent-skills is taken at https://crates.io/crates/agent-skills
❯ Please update your recommendation.
### Re: agent-skills name collision — gpt-5

Updated recommendation: do not create or rename the Rust crate to `agent-skills` if it needs to publish on crates.io. The name is already occupied at [crates.io/crates/agent-skills](https://crates.io/crates/agent-skills), and docs.rs indexes it as an existing crate, currently visible as [`agent-skills 0.2.0`](https://docs.rs/crate/agent-skills/0.2.0/source/Cargo.lock).

That changes the packaging recommendation from "make `agent-skills` the target package" to this:

- Keep `skill-harness` as the immediate Rust crate/package boundary if it already owns the relevant code and can publish today.
- Use `agent skills` as the domain/product language in docs, specs, commands, and possibly repository naming, but not as the crates.io package name.
- If the package name should become more durable than `skill-harness`, choose a precise available crate name after registry checks, such as `agent-skillkit`, `agent-skill-tools`, `portable-agent-skills`, or `skill-compose`. I would avoid a rename until the API boundary is clearer.
- Keep `skill-harness` only if it means lifecycle tooling: install, validate, audit, compose, and test skills across Claude Code, Codex, OpenCode, and generic layouts. If it grows beyond "harness" semantics, rename later to an available name, but do that as a deliberate crate-name migration rather than assuming `agent-skills` is available.

For `compose-skills`, the revised path is: move the portable source layout and Rust validator into `skill-harness` first, add Claude Code and Codex installer tests, and expose commands under whatever CLI name you control. If a future public package split is justified, extract to an available focused crate like `agent-skill-compose` rather than the taken `agent-skills` name.

I also updated the pending item that still pointed at `agent-skills` as the target crate name, and added a follow-up to check registry availability before any rename.
❯ Why not just keep the name skill-harness?
### Re: keeping skill-harness — gpt-5

That is probably the right answer now: keep `skill-harness`.

My earlier push toward a broader name was based on the package owning more than test harness behavior. But given `agent-skills` is unavailable, the bar for renaming should be higher. `skill-harness` is already yours, already connected to the code lineage, and it can plausibly mean the harness that installs, validates, tests, and composes portable skills across agent runtimes.

I would keep the name if these remain true:

- The crate is primarily lifecycle tooling: validation, installation, sync, fixtures, adapters, tests, and composition checks.
- The public CLI can make the domain clear through commands, for example `skill-harness compose validate`, `skill-harness install --harness claude`, and `skill-harness audit`.
- The docs define "harness" explicitly as the cross-runtime skill lifecycle harness, not only an eval harness.
- You avoid creating a second peer crate unless there is a real split between reusable library APIs and test/eval harness behavior.

The main risk is only naming perception: people may assume `skill-harness` is test-only. That is fixable with a crisp README, command names, and crate description. It is less costly than forcing a premature rename to a weaker available name.

So my revised recommendation is stronger: keep `skill-harness` as the crate/package name for now. Rename later only if usage proves the name is actively misleading, or if a clean extraction emerges with a much sharper name like `agent-skill-compose` for the composition-only layer.
❯ do [#e92b]. Keep `skill-harness` name. #spec-test-build-install-commit-push
### Re: #e92b skill-harness compose-skills — gpt-5

Completed `#e92b` while keeping the crate/package name `skill-harness`.

Implemented in `src/skill-harness`:

- Added portable `skills/compose-skills/` source with `SKILL.md`, `SPEC.md`, and references.
- Added Rust composition-plan validation as `skill-harness compose validate <plan.md>`.
- Added `install-dir` plus explicit `--harness` target support, including Claude Code `.claude/skills/<name>/SKILL.md` and Codex `.codex/skills/<name>/SKILL.md` directory installs.
- Added tests for the validator and Claude/Codex full-directory installer coverage.
- Updated README, SPEC, and the bundled `skill-harness` skill docs to use `skill-harness` as the durable package name.

Validation completed:

- `cargo fmt`
- `cargo test`
- `cargo clippy -- -D warnings`
- `cargo build`
- `cargo install --path . --force`
- `skill-harness compose validate src/skill-harness/skills/compose-skills/references/example-plan.md`
- Installed binary smoke test for `install-dir` into temporary Codex and Claude roots.
- Standalone temp-copy verification outside the parent workspace Cargo patch: `cargo test --locked`
- Root workspace `make check`

CI/tmux evidence:

- `btakita/skill-harness` CI run `26076744803` passed, including `cargo clippy -- -D warnings` and `cargo test`.
- Root `Agent Doc` run `26076757906` failed before job steps started: both `check` and `tmux-ci` had empty step lists. I classified this as the existing external CI-start blocker, not a code or tmux regression. Local root `make check` passed.

Commits pushed:

- `842c576 feat: add compose-skills validation` to `btakita/skill-harness main`
- `602c1a838 chore: update skill-harness submodule` to `btakita/agent-loop main`
<!-- agent:boundary:d1db3829 -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:pending -->
- [ ] [#bx57] Disable unused MCP servers — currently managed via Claude Code UI, not settings.json
- [ ] [#wr52] Run litellm proxy pointing at vllm
- [ ] [#04hc] Test `claude` against local model via `ANTHROPIC_BASE_URL`
<!-- /agent:pending -->

## Completed / Reaped

<!-- agent:done -->
- 2026-05-11 [#aaj9] Transfer snapshot GC feature to agent-doc-feat.md
- 2026-05-11 [#bamc] Evaluate graphify as graph layer for code RAG (extend extractors with tree-sitter code parsing)
- 2026-05-19 [#0b3x] [recommended] Create a composable-skill-design skill or extend skill-creator with a composability audit workflow, backed by examples and validation checks
- 2026-05-19 [#cdvx] [recommended] Check crates.io availability for candidate public crate names before renaming skill-harness
- 2026-05-19 [#25k6] [recommended] Keep skill-harness as the current Rust crate/package boundary, or rename only to an available precise crate name such as agent-skillkit/agent-skill-tools after checking registry availability; do not target the taken agent-skills crate name
- 2026-05-19 [#e92b] [recommended] Move compose-skills toward skill-harness/agent-skills with Rust validation and Claude/Codex installer coverage
<!-- /agent:done -->
