# Agent Doc Completed Work

- 2026-05-13 [#clearsessionmismatch] #agent-doc-bug [done]: Clear Session Context harness mismatch false positive — protected_clear_input_reason inferred wrong harness from document frontmatter instead of pane's actual command. Plan: tasks/agent-doc/plan-clear-session-harness-mismatch-false-positive.md
- 2026-05-13 [#tmuxcontrolmode] Migrate tmux-router to control mode (tmux -CC) for event-driven pane output and lifecycle notifications, replacing capture-pane polling. Natural fit for existing Tmux struct. [recommended]
- 2026-05-13 [#tmuxhybridpty] Implement hybrid control-mode + pipe-pane + owned-PTY architecture: control mode for lifecycle events, pipe-pane for output streaming, owned PTY only for managed panes (OpenCode, Claude) where input reliability matters. Depends on #tmuxcontrolmode.
