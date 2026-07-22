// agent-doc turn-in-progress pane monitor (#claude-busy-status-during-active-turn).
// Auto-installed by `agent-doc turn-status install`. Sets the agent's own tmux
// pane border title on turn start (chat.message) and clears it on turn end
// (session.idle). Best-effort: agent-doc turn-status no-ops outside tmux.
export const AgentDocTurnStatus = async ({ $ }) => ({
  "chat.message": async () => {
    try { await $`agent-doc turn-status active` } catch {}
  },
  event: async ({ event }) => {
    if (event.type === "session.idle") {
      try { await $`agent-doc turn-status idle` } catch {}
    }
  },
})
