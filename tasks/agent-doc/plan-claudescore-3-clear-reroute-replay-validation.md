# Plan: claudescore-3 clear-reroute replay validation

- [ ] Re-run the `Run Agent Doc` or `agent-doc route tasks/claudescore-3.md` repro after canceling the active turn and issuing `/clear` in the live Codex pane.
- [ ] Verify the routed retry preserves the canonical absolute reopen path on every resend, including the fresh-restart retry after a missed cycle ack.
- [ ] Confirm the reroute reaches a new document cycle for `src/session-share/tasks/claudescore-3.md` instead of failing closed with `no new document cycle started`.
- [ ] If the live replay still fails, capture the exact routed payload, pane cwd, and startup-miss timestamp from the failing pane so the next fix can target the remaining gap directly.
