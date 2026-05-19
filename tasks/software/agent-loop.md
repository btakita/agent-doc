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
<!-- agent:boundary:fd41527d -->
<!-- /agent:exchange -->
###
<!--
-->

<!-- agent:pending -->
- [ ] [#e92b] [recommended] Move compose-skills toward skill-harness/agent-skills with Rust validation and Claude/Codex installer coverage
- [ ] [#bx57] Disable unused MCP servers — currently managed via Claude Code UI, not settings.json
- [ ] [#wr52] Run litellm proxy pointing at vllm
- [ ] [#04hc] Test `claude` against local model via `ANTHROPIC_BASE_URL`
<!-- /agent:pending -->

## Completed / Reaped

<!-- agent:done -->
- 2026-05-11 [#aaj9] Transfer snapshot GC feature to agent-doc-feat.md
- 2026-05-11 [#bamc] Evaluate graphify as graph layer for code RAG (extend extractors with tree-sitter code parsing)
- 2026-05-19 [#0b3x] [recommended] Create a composable-skill-design skill or extend skill-creator with a composability audit workflow, backed by examples and validation checks
<!-- /agent:done -->
