---
agent_doc_session: 0ec9433a-94a8-4bdd-bf78-79a6c8c6af93
agent_doc_mode: template
claude_args: --dangerously-skip-permissions
links:
- ../dreamable/robert-ross.md
---


# Git + Agent Workflow Research

Context: Rob Ross sent Brian GitButler links after Brian mentioned looking for git management solutions in agent workflows. This research explores the landscape and informs agent-doc + ClaudeScore/Dreamable directions.

## Key Tools

### GitButler
- **URL:** https://gitbutler.com/
- **Founder:** Former CEO of GitHub
- **Funding:** $60M
- **Core concept:** Virtual branches — work on multiple things simultaneously without stashing/switching. AI-assisted commit message generation, smarter diffs.
- **Why relevant:** Rob flagged this as directly aligned with Brian's interest in git management for agent workflows. Well-funded, serious team.
- **News:** https://dev.ua/en/news/kolyshnii-ceo-github-zaluchyv-60-mln-dlia-novoho-startapu-entire-shcho-intehruie-shi-ahentiv-u-git-1770805513

### agent-doc (Brian's tool)
- Manages git snapshots per-session — every response committed with selective staging
- Documents are the UI; git gutter provides visual feedback (committed = no gutter, new input = green)
- Current gap: tmux status indicators not captured (stdout/stderr only); workaround is tmux snapshots

### Related Tools
- **Bench** (benchmarks Claude sessions) — Rob uses this; lacks sharing, no OAuth
- **claudebin** — session sharing (Brian built)
- **DeepEval** — eval framework; scores based on ideal human input, ignores practitioner variability

## Brian's Inclinations

From SMS conversation with Rob:
> "I'm looking for a good solution with git management in an agent workflow. I have some thoughts there... Need to look through these projects to see if my inclinations are still relevant."

Key inclinations from earlier context:
- Git as the persistence layer for agent sessions (agent-doc already does this)
- Realtime interactive dashboard over markdown files (token-efficient, editor/web/TUI targets)
- Practitioner variability matters for scoring — ClaudeScore should account for it
- Enterprise use case: local sessions, no cloud upload

## Research Questions

- [ ] How does GitButler's virtual branch model compare to agent-doc's snapshot/commit approach?
- [ ] What's the overlap/gap between GitButler and what Brian has built?
- [ ] Could agent-doc integrate with GitButler's branch model (sessions as virtual branches)?
- [ ] How does ClaudeScore capture git history as a scoring signal? (CLAUDE.md existence, commit cadence, session patterns)
- [ ] What would a "git-native" agent workflow score look like?

## Competitive Landscape

| Tool | Focus | Git Integration | Agent Aware |
|------|-------|----------------|-------------|
| GitButler | Git UX / virtual branches | Native | Partial |
| Bench | Session capture/playback | None | Yes |
| claudebin | Session sharing | None | Yes |
| agent-doc | Document sessions | Native (snapshot commits) | Yes |
| DeepEval | LLM evals | None | Partial |

## Exchange
Curious how the git submodule workflow is supported. It seems the premise of GitButler is that Pull Requests don't scale for monorepos. But using git submodules, you can have the best of both worlds. A focused repo that is PR friendly and a monorepo with the focused repos grouped together.

Managing the submodules does require work and diligence.
<!-- agent:exchange -->
**On GitButler + submodules:**

GitButler's virtual branch model operates within a single repo's working tree — it doesn't have special submodule awareness. The premise you're identifying ("PRs don't scale for monorepos") is more about *diff noise and review fatigue* than a hard technical limit.

Your submodule approach threads the needle well:

- **Focused repos** → small, reviewable PRs with clear context
- **Aggregator monorepo** → unified view, cross-repo builds, single deploy surface
- **Submodule pointers** → explicit versioning of inter-repo dependencies

The real friction is the **update cycle**: when you change `src/corky/` you need to commit there, then update the pointer in `agent-loop/`, then in any other aggregators. This is disciplined but mechanical — exactly the kind of thing an agent workflow can automate.

Where GitButler *could* help within a submodule setup: managing concurrent work *within* a focused repo (e.g., working on two features in `corky` simultaneously without branches). The virtual branch model is well-suited to that focused-repo context.

The **gap GitButler doesn't address**: cross-submodule sessions. If a feature spans `agent-doc` + `corky` + `agent-loop`, you're managing multiple virtual-branch contexts. That's where agent-doc's approach (document as session, git as persistence) has an advantage — the session document can span repos while each commit goes into the right submodule.

So: submodules + focused PRs solves the scale problem GitButler is pitching against, and does so without requiring a new tool. GitButler adds value *within* each focused repo but doesn't solve cross-repo coordination.

❯ How about entire?
❯ - https://entire.io/
❯ - https://entire.io/blog/hello-entire-world/
❯ - https://dev.ua/en/news/kolyshnii-ceo-github-zaluchyv-60-mln-dlia-novoho-startapu-entire-shcho-intehruie-shi-ahentiv-u-git-1770805513
**On GitButler + submodules:**

GitButler's virtual branch model operates within a single repo's working tree — it doesn't have special submodule awareness. The premise you're identifying ("PRs don't scale for monorepos") is more about *diff noise and review fatigue* than a hard technical limit.

Your submodule approach threads the needle well:

- **Focused repos** → small, reviewable PRs with clear context
- **Aggregator monorepo** → unified view, cross-repo builds, single deploy surface
- **Submodule pointers** → explicit versioning of inter-repo dependencies

The real friction is the **update cycle**: when you change `src/corky/` you need to commit there, then update the pointer in `agent-loop/`, then in any other aggregators. This is disciplined but mechanical — exactly the kind of thing an agent workflow can automate.

Where GitButler *could* help within a submodule setup: managing concurrent work *within* a focused repo (e.g., working on two features in `corky` simultaneously without branches). The virtual branch model is well-suited to that focused-repo context.

The **gap GitButler doesn't address**: cross-submodule sessions. If a feature spans `agent-doc` + `corky` + `agent-loop`, you're managing multiple virtual-branch contexts. That's where agent-doc's approach (document as session, git as persistence) has an advantage — the session document can span repos while each commit goes into the right submodule.

So: submodules + focused PRs solves the scale problem GitButler is pitching against, and does so without requiring a new tool. GitButler adds value *within* each focused repo but doesn't solve cross-repo coordination.

### Re: Entire

**Entire is a separate company from GitButler.** The $60M news article linked under GitButler in the Key Tools section is actually about Entire — worth correcting there.

- **Founder:** Thomas Dohmke (former CEO of GitHub)
- **Funding:** $60M seed
- **First product:** Checkpoints — captures AI agent sessions directly in git

**What Checkpoints does:**
When an agent (Claude Code, Gemini CLI) generates code and commits, Checkpoints automatically records the transcript, prompts, files touched, token usage, and tool calls alongside that commit — stored on a separate branch (`entire/checkpoints/v1`). MIT-licensed CLI.

**Overlap with agent-doc:** High. Both use git as the persistence layer for agent sessions. Key difference: Entire stores metadata on a side branch (non-intrusive), agent-doc commits inline to the working branch as the document surface. Entire is a passive recorder; agent-doc is the workflow UI itself.

**Strategic read:** Dohmke building this with $60M and GitHub credibility signals that "git as agent memory" is becoming a platform-level primitive. agent-doc is ahead on the interactive/document side; Entire is ahead on the passive capture / multi-agent side. These could be complementary.

❯ How about entire?
❯ - https://entire.io/
❯ - https://entire.io/blog/hello-entire-world/
❯ - https://dev.ua/en/news/kolyshnii-ceo-github-zaluchyv-60-mln-dlia-novoho-startapu-entire-shcho-intehruie-shi-ahentiv-u-git-1770805513
**On GitButler + submodules:**

GitButler's virtual branch model operates within a single repo's working tree — it doesn't have special submodule awareness. The premise you're identifying ("PRs don't scale for monorepos") is more about *diff noise and review fatigue* than a hard technical limit.

Your submodule approach threads the needle well:

- **Focused repos** → small, reviewable PRs with clear context
- **Aggregator monorepo** → unified view, cross-repo builds, single deploy surface
- **Submodule pointers** → explicit versioning of inter-repo dependencies

The real friction is the **update cycle**: when you change `src/corky/` you need to commit there, then update the pointer in `agent-loop/`, then in any other aggregators. This is disciplined but mechanical — exactly the kind of thing an agent workflow can automate.

Where GitButler *could* help within a submodule setup: managing concurrent work *within* a focused repo (e.g., working on two features in `corky` simultaneously without branches). The virtual branch model is well-suited to that focused-repo context.

The **gap GitButler doesn't address**: cross-submodule sessions. If a feature spans `agent-doc` + `corky` + `agent-loop`, you're managing multiple virtual-branch contexts. That's where agent-doc's approach (document as session, git as persistence) has an advantage — the session document can span repos while each commit goes into the right submodule.

So: submodules + focused PRs solves the scale problem GitButler is pitching against, and does so without requiring a new tool. GitButler adds value *within* each focused repo but doesn't solve cross-repo coordination.

❯ How about entire?
❯ - https://entire.io/
❯ - https://entire.io/blog/hello-entire-world/
❯ - https://dev.ua/en/news/kolyshnii-ceo-github-zaluchyv-60-mln-dlia-novoho-startapu-entire-shcho-intehruie-shi-ahentiv-u-git-1770805513

### Re: Entire

**Entire is a separate company from GitButler.** The $60M news article linked under GitButler in the Key Tools section is actually about Entire — worth correcting there.

- **Founder:** Thomas Dohmke (former CEO of GitHub)
- **Funding:** $60M seed
- **First product:** Checkpoints — captures AI agent sessions directly in git

**What Checkpoints does:**
When an agent (Claude Code, Gemini CLI) generates code and commits, Checkpoints automatically records the transcript, prompts, files touched, token usage, and tool calls alongside that commit — stored on a separate branch (`entire/checkpoints/v1`). MIT-licensed CLI.

**Overlap with agent-doc:** High. Both use git as the persistence layer for agent sessions. Key difference: Entire stores metadata on a side branch (non-intrusive), agent-doc commits inline to the working branch as the document surface. Entire is a passive recorder; agent-doc is the workflow UI itself.

❯ Should agent-doc also use the side-branch approach to be non-intrusive? Or support both modes?
❯ Perhaps a key distinction is does Entire manage any content or only metadata?

**Strategic read:** Dohmke building this with $60M and GitHub credibility signals that "git as agent memory" is becoming a platform-level primitive. agent-doc is ahead on the interactive/document side; Entire is ahead on the passive capture / multi-agent side. These could be complementary.

❯ I have been working on explicit harness context shaping is libraries:
❯ - skill-harness
❯ - module-harness
❯ - agent-runbooks
❯ - agent-memories
❯ - agent-rules

❯ Along with the agent docs themselves.
❯ Would multiple agent memory systems compat well or have reliability issues?
**On GitButler + submodules:**

GitButler's virtual branch model operates within a single repo's working tree — it doesn't have special submodule awareness. The premise you're identifying ("PRs don't scale for monorepos") is more about *diff noise and review fatigue* than a hard technical limit.

Your submodule approach threads the needle well:

- **Focused repos** → small, reviewable PRs with clear context
- **Aggregator monorepo** → unified view, cross-repo builds, single deploy surface
- **Submodule pointers** → explicit versioning of inter-repo dependencies

The real friction is the **update cycle**: when you change `src/corky/` you need to commit there, then update the pointer in `agent-loop/`, then in any other aggregators. This is disciplined but mechanical — exactly the kind of thing an agent workflow can automate.

Where GitButler *could* help within a submodule setup: managing concurrent work *within* a focused repo (e.g., working on two features in `corky` simultaneously without branches). The virtual branch model is well-suited to that focused-repo context.

The **gap GitButler doesn't address**: cross-submodule sessions. If a feature spans `agent-doc` + `corky` + `agent-loop`, you're managing multiple virtual-branch contexts. That's where agent-doc's approach (document as session, git as persistence) has an advantage — the session document can span repos while each commit goes into the right submodule.

So: submodules + focused PRs solves the scale problem GitButler is pitching against, and does so without requiring a new tool. GitButler adds value *within* each focused repo but doesn't solve cross-repo coordination.

How about entire?
- https://entire.io/
- https://entire.io/blog/hello-entire-world/
- https://dev.ua/en/news/kolyshnii-ceo-github-zaluchyv-60-mln-dlia-novoho-startapu-entire-shcho-intehruie-shi-ahentiv-u-git-1770805513

### Re: Entire

**Entire is a separate company from GitButler.** The $60M news article linked under GitButler in the Key Tools section is actually about Entire — worth correcting there.

- **Founder:** Thomas Dohmke (former CEO of GitHub)
- **Funding:** $60M seed
- **First product:** Checkpoints — captures AI agent sessions directly in git

**What Checkpoints does:**
When an agent (Claude Code, Gemini CLI) generates code and commits, Checkpoints automatically records the transcript, prompts, files touched, token usage, and tool calls alongside that commit — stored on a separate branch (`entire/checkpoints/v1`). MIT-licensed CLI.

**Overlap with agent-doc:** High. Both use git as the persistence layer for agent sessions. Key difference: Entire stores metadata on a side branch (non-intrusive), agent-doc commits inline to the working branch as the document surface. Entire is a passive recorder; agent-doc is the workflow UI itself.

Should agent-doc also use the side-branch approach to be non-intrusive? Or support both modes?
Perhaps a key distinction is does Entire manage any content or only metadata?

**On the side-branch question:** Entire stores only metadata (transcripts, prompts, file lists, token usage) — no content. It's passive capture. The document/code on the working branch is untouched.

agent-doc's inline approach is load-bearing by design. The git gutter (green/blue markers on line changes) is the workflow's visual layer — it's how you see where the agent response ends and your new input begins. The snapshot commits on `main` are what create that boundary. Moving to a side branch would lose the gutter entirely.

The distinction: **content** (agent-doc) vs **metadata-only** (Entire). agent-doc manages the document as content. Entire records what happened without changing the working tree. These aren't competing design choices — they're different artifacts. Complementary.

**Strategic read:** Dohmke building this with $60M and GitHub credibility signals that "git as agent memory" is becoming a platform-level primitive. agent-doc is ahead on the interactive/document side; Entire is ahead on the passive capture / multi-agent side. These could be complementary.

I have been working on explicit harness context shaping is libraries:
- skill-harness
- module-harness
- agent-runbooks
- agent-memories
- agent-rules

Along with the agent docs themselves.
Would multiple agent memory systems compat well or have reliability issues?

### Re: Multiple memory system compatibility

The systems you've built operate at different resolutions:
- **agent-memories** — cross-session user/project/feedback (coarsest, highest persistence)
- **agent-rules** — behavior constraints (invariants)
- **agent-runbooks** — procedural steps (task-scoped)
- **skill-harness / module-harness** — code-level specs (finest grain)
- **agent-doc documents** — session-level conversation (ephemeral, most detailed)

This is a scope stack, not competing systems. The risk isn't incompatibility — it's:

1. **Contradiction drift**: a rule in agent-rules contradicts feedback in agent-memories. No reconciliation mechanism.
2. **Priority ambiguity**: when two systems give different guidance, which wins? Not defined.
3. **Context bloat**: loading all layers at session start costs tokens regardless of relevance.
4. **Staleness skew**: coarser layers update less frequently, so they can lag finer layers.

The integration point is `CLAUDE.md` — it's the priority resolver. The CLAUDE.md ontology gives a framework (Scope → Context → Resolution), but the actual precedence order isn't explicit. Making that explicit (e.g., "runbook overrides rule if task-specific") would harden the system.

**Reliability verdict:** Compatible by scope, but fragile under contradiction. The more layers, the more important it is to have explicit priority rules at the top-level CLAUDE.md.
<!-- agent:boundary:785aebf1 -->
<!-- /agent:exchange -->
