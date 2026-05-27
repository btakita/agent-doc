<!--
Autosync on editor file change + `Sync Tmux Layout` lands on a 1 tmux pane and 3 tmux pane layout with 2 editor panes.
spec-test-build-install-commit-push
---
The managed proof check tmux prompt should be terse and more "human readable" on a single line. Explain the status without the extra information.
spec-test-build-install-commit-push
---
`--pending-only` should be renamed to `--backlog-only`.
spec-test-build-install-commit-push
---
Please ensure the JB opencode fixes are propagated to the VSCode plugin. Ensure the specs are updated for all editor plugins. Should we implement in the ffi plugin?
#next-steps
---
JB `Run Agent Doc` on root.md with `agent: claude` sends an incorrect command
```
❯ agent-doc /home/brian/work/btakita/agent-loop/src/session-share/tasks/root.md
  ⎿  API Error: 400 Could not process image
```
spec-test-build-install-commit-push
---
Should tsift fail fast in this case?
```
• Ran tsift search "transcript not found livekit-101"
  └ Error: tsift search timed out after 30s (strategy: lexical). The search root looks fresh, so reindexing is unlikely
    to help. Re-run with `--timeout 0` to disable the timeout, narrow `--path` / `--scope`, or try a different
    strategy.
```
spec-test-build-install-commit-push
---
How can we move the agent-doc concept to html files?
Can the agent-doc supervisor run a web server, using contenteditable via tinymcp or another editor to write back to the source file?
Would major editors such as VSCode, JetBrains, Zig, and TUI editors support this workflow?
---
With the latest supply chain attacks on npm and pypi, should agent-doc should run in a docker container? Should all software written run in a docker container?
---
JB `Restart Supervisor Process` error:
```
agent-doc command failed (exit 1): Error: session_restart refused for /home/brian/work/btakita/agent-loop/src/session-share/tasks/root.md because pane %2 is alive-busy (source=authoritative_actor, current_command=agent-doc, tail="⏵⏵ bypass permissions on (shift+tab to cycle)"). Run `agent-doc session status /home/brian/work/btakita/agent-loop/src/session-share/tasks/root.md` and wait for an idle prompt, or inspect/stop the pane explicitly before clearing or restarting it.
```
spec-test-build-install-commit-push
---
Move JB `Copy Session Diagnostics` outside the numeric shortcuts.
---
I don't have a good track on the lifecycle logic that starts panes, kills panes, edge cases, etc.
Can we make a nested list and diagram of the lifecycle logic?
---
Why is agent-doc starting a child Codex run?
```
• agent-doc started a child Codex run. Its preflight noticed the previous patchback was already committed, then
  submitted the new follow-up prompt for execution.
```
---
```
3. Workaround: delegate SSH via the host — if OpenCode can't SSH directly, have agent-doc run the SSH deploy           step outside the managed pane (in the harness supervisor process), not inside the OpenCode child.
```
---
Confirm that z.ai subscriptions are message based and not token based. If message based, is agent-doc inefficient on message based plans? Please review code + logs.
#next-steps
---
minor (v0.34.0) release + publish. merge into main
---
/home/brian/work/btakita/agent-loop/tasks/agent-doc/plan-project-controller-session-actor-mvp.md
---
"Press Enter to restart, or 'q' to exit."
-->
