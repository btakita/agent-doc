# Compose Skills Examples

## Content Workflow

Avoid one broad `content-creation` skill that researches ideas, writes scripts, drafts social posts, publishes content, and analyzes performance.

Better split:

- `research-video-ideas`: collect and rank ideas. Resources: references for audience, scripts for source scraping.
- `write-video-script`: produce script drafts from selected ideas. Resources: examples and voice references.
- `draft-social-post`: adapt finished content for LinkedIn or X. Resources: platform style references.
- `publish-content`: explicit or user-only side-effect skill for publishing.

Why split: research improves every downstream workflow, writing has different quality checks, and publishing carries higher risk.

## Agent-Doc Workflow

Avoid one helper skill that owns every agent-doc concern: session cycle, routing diagnostics, commit closeout, repair, and tmux operations.

Better split:

- `agent-doc-session-cycle`: execute preflight, plan, response, finalize, and session-check.
- `agent-doc-route-diagnostics`: inspect tmux/session routing evidence and classify readiness failures.
- `agent-doc-closeout`: handle manual repo commit boundaries, full-suite verification, and final document persistence.
- `agent-doc-repair`: recover missed responses or interrupted cycles through binary-owned write/commit paths.

Why split: the session cycle is frequent and procedural, routing diagnostics are evidence-heavy, closeout has strict staging rules, and repair must stay narrow and fail-closed.

## Browser Automation Workflow

Avoid baking one browser tool into every task-specific skill.

Better split:

- `browser-automation`: generic navigation, click, type, wait, inspect.
- `webapp-testing`: test user flows using browser automation plus test assertions.
- `site-ops`: site-specific operational runbooks that may call browser automation.

Why split: browser actions are reusable, webapp testing has its own validation shape, and site operations carry domain-specific context.
