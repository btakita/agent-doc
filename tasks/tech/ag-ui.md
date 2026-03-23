---
agent_doc_session: cae346cc-0060-4783-90dd-393d38fba12a
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Explain how the shared state works https://docs.copilotkit.ai/built-in-agent/shared-state
Does the backend ai model or harness have state

### Re: AG-UI Shared State (HEAD)

AG-UI's shared state creates a **bidirectional synchronization channel** between agent backends and frontend UIs. Here's how it works:

#### Architecture

State is a structured data object that:
1. **Persists** across interactions with an agent
2. **Is accessible** by both agent and frontend
3. **Updates in real-time** as the interaction progresses
4. **Provides context** for decision-making on both sides

#### Two Sync Methods

**1. State Snapshots (`STATE_SNAPSHOT`)** — delivers the complete state object:
```typescript
interface StateSnapshotEvent {
  type: EventType.STATE_SNAPSHOT
  snapshot: any // Complete state object
}
```
Used at interaction start, after connection interruptions, or major state changes. Frontend replaces its entire state model with the snapshot.

**2. State Deltas (`STATE_DELTA`)** — incremental updates via JSON Patch (RFC 6902):
```typescript
interface StateDeltaEvent {
  type: EventType.STATE_DELTA
  delta: JsonPatchOperation[] // Array of JSON Patch operations
}
```
Operations: `add`, `remove`, `replace`, `move`, `copy`, `test`. Bandwidth-efficient — only sends what changed.

#### CopilotKit Implementation

**Frontend** (React):
```jsx
const { state: agentState, setState: setAgentState } = useCoAgent({
  name: "agent",
  initialState: { someProperty: "initialValue" },
})
```

**Backend** (LangGraph/Python):
```python
async def tool_node(self, state: ResearchState, config: RunnableConfig):
    tool_state = {
        "title": new_state.get("title", ""),
        "outline": new_state.get("outline", {}),
    }
    await copilotkit_emit_state(config, tool_state)
    return tool_state
```

The `useCoAgent` hook creates real-time read/write access to agent state. `copilotkit_emit_state` pushes updates from the agent to the frontend via AG-UI's snapshot/delta events.

#### Key Design Points
- Patches are applied atomically (all or none) using `fast-json-patch`
- Enables human-in-the-loop: agent proposes actions via state, user approves/modifies
- Best practice: prefer deltas for small updates, snapshots for baselines

---

**Note:** Your message was cut off at "Does th" — please complete the question and I'll answer it.
<!-- agent:boundary:c3944007 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
