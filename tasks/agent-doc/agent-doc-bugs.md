---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
agent-doc v0.31.19 — active
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Sessions: v0.31.18 release (cross-session swap-pane bug fix, slash command dispatch from diff, partial compact --keep N, JB plugin v0.2.48 snapshot-timestamp dedup guard, CRDT boundary-synthesis fix); v0.31.19 release (AGENT_PROCESSES guard on wrong-session recovery + lazy claim Strategy 2, resync --fix --session <target> via join-pane, swap_pane_guarded() helper). Root cause analysis: rescue_from_stash() (pre-v0.31.18) swapped corky %544 from session $14 into session $10 without session check; lazy claim then registered corky as justin-caird.md pane. All fixes shipped. Plugin v0.2.48 confirmed installed in IntelliJIdea2026.1. Pending: none.

Fix https://github.com/btakita/agent-doc/actions/runs/24093596675
### Re: PyPI publish failure — trusted publisher not configured

The `publish` job fails with:

```
invalid-publisher: valid token, but no corresponding publisher
```

OIDC token was issued correctly, but PyPI has no matching trusted publisher record for `btakita/agent-doc/.github/workflows/pypi.yml` + environment `pypi`.

**Fix:** Add the trusted publisher on PyPI.

Go to: https://pypi.org/manage/project/agent-doc/settings/publishing/

Add a new **GitHub Actions** trusted publisher:
- **Owner:** `btakita`
- **Repository name:** `agent-doc`
- **Workflow filename:** `pypi.yml`
- **Environment:** `pypi`

Then re-run the failed `publish` job:
```bash
gh run rerun 24093596675 --failed --repo btakita/agent-doc
```

**Note:** The trusted publisher might have been set up for a different workflow filename (e.g., `publish.yml`) in a prior setup. Check the existing publishers list on PyPI before adding a new one.

Why was part of my prompt included in the response in tasks/research/filevine.md
> Help me prepare for an interview with the Filevine development team.
> - https://www.filevine.com/
> - https://www.linkedin.com/company/filevine
> - Chris Smith
> - Cain Elliott
> - Stuart Hiller
> - Emily Lippincott

Feature request: After an agent doc run...each line of the person's prompts should have a `❯ ` prefix
### Re: prompt echoed in filevine.md (HEAD)

The agent included the user's prompt verbatim in its `<!-- patch:exchange -->` response content. Since the exchange component uses `patch=append`, the write added the entire patch content (prompt + response) after the existing exchange text, which already contained the user's typed prompt.

Result: prompt appears twice — once as the user's direct edit, once repeated as the first block of the agent's appended response.

**Root cause:** The agent treated the prompt as "context to include in the response" rather than "already-present content to respond to." It shouldn't echo input it received via the diff — that content is already in the exchange.

**Fix:** Agents responding to `patch=append` exchange components must never re-include user input lines in their patch block. The patch should contain only new agent content.

This is a skill/prompt discipline issue, not a binary bug. Worth adding a note to SKILL.md:

> For `patch=append` exchange components, do NOT include the user's input in the patch block — it is already in the document. Patch content = new agent response only.

### Re: feature — `❯ ` prefix on user prompts (HEAD)

**Concept:** After each agent-doc cycle, user-added lines in the exchange get a `❯ ` prefix, making the conversation visually scannable.

**Implementation options:**

1. **Normalization in `agent-doc write`** — when `patch=append` merges, transform user-added diff lines by prepending `❯ ` to each non-blank non-metadata line before inserting into the document. The CRDT diff already identifies which lines are user-added.

2. **Agent-side normalization** — the agent includes a `<!-- patch:exchange -->` block with the user's input reformatted as `❯ text`, then its response. Works for replace-mode exchanges; awkward for append (would echo the prompt again, see above).

3. **Plugin-side rendering** — the JB plugin adds a visual gutter or color for user-authored lines without modifying the file. Non-destructive, but doesn't affect raw file.

**Recommendation:** Option 1 — binary-level normalization in `write` when processing user-added content in an `exchange` component. The CRDT already knows which hunks came from the user; apply the prefix during the merge step before writing the final document.

**Open questions for design:**
- Apply prefix to ALL non-blank user lines, or only paragraph-starting lines?
- What about user input that already has a prefix (bullets, `>`)?
- Is the prefix stored in the file, or only rendered?
<!-- agent:boundary:1c13bef4 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Fix PyPI trusted publisher for `pypi.yml` + environment `pypi`, then rerun CI
- [ ] Add SKILL.md note: patch:exchange must not echo user input (append mode)
- [ ] Design `❯ ` prefix normalization for user-added exchange lines (option 1: binary-level in write)
<!-- /agent:pending -->
