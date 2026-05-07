# Plan — narrow prompt-contract escalation from copied session frontmatter

## Problem

The `tasks/software/tmux-router.md` turn added copied session frontmatter
containing `prompt_presets` plus an exchange line `spec-test-build-install-commit-push`.
`agent-doc plan` treated the combined diff as an `#agent-doc-bug`
`plan_backlog_only` contract and suppressed the user’s requested implementation
work, even though the user’s concrete prompt was a tmux-router compile report.

## Goal

Clarify or tighten prompt-contract detection so copied preset definitions do not
silently escalate unrelated software task turns into agent-doc bug-planning
cycles unless the user actually invokes that preset.

## Completed implementation

- [x] Reproduced the detection path with a minimal template document that adds
  `prompt_presets` plus a non-bug imperative line in `agent:exchange`.
- [x] Tightened prompt preset expansion so preset definition lines are ignored
  when added diff text is scanned for invoked presets.
- [x] Added regression coverage for the intended distinction between "preset
  defined" and "preset requested".

## Exit criteria

- Non-agent-doc software task turns do not fall into `plan_backlog_only` solely
  because copied frontmatter introduced preset definitions.
