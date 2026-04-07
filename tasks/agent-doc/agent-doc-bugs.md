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
### Re: PyPI publish failure — trusted publisher not configured (HEAD)

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
<!-- agent:boundary:99b2a1d2 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Fix PyPI trusted publisher for `pypi.yml` + environment `pypi`, then rerun CI
<!-- /agent:pending -->
