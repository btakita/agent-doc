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

<!-- /agent:exchange -->
