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
Does the backend ai model or harness have state that can be synced with the UI?

### Re: AG-UI Shared State

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
Describe RFC 6902. Show some examples of messages.

### Re: RFC 6902 & AG-UI Message Examples

#### RFC 6902 — JSON Patch

RFC 6902 defines a format for expressing a sequence of operations to apply to a JSON document. It's a standard way to describe mutations without sending the entire document.

**Core concepts:**
- A patch is an **array of operation objects**, applied in order
- Each operation has an `op` (operation type), a `path` (JSON Pointer per RFC 6901), and optionally `value` or `from`
- Paths use `/` separators: `/foo/bar/0` means `obj.foo.bar[0]`
- Operations are atomic — if any fails, the whole patch should be rejected

**The 6 operations:**

| Op | Purpose | Required fields |
|---|---|---|
| `add` | Insert value at path (creates or appends to arrays) | `path`, `value` |
| `remove` | Delete value at path | `path` |
| `replace` | Overwrite value at path (must exist) | `path`, `value` |
| `move` | Remove from `from`, add at `path` | `path`, `from` |
| `copy` | Copy from `from` to `path` | `path`, `from` |
| `test` | Assert value at path equals `value` (fails patch if not) | `path`, `value` |

#### AG-UI SSE Message Examples

Here's what the actual Server-Sent Events look like on the wire between an AG-UI agent backend and the frontend:

**1. State Snapshot — initial state delivery:**
```
event: STATE_SNAPSHOT
data: {
  "type": "STATE_SNAPSHOT",
  "snapshot": {
    "messages": [],
    "research": {
      "title": "",
      "outline": {},
      "sections": [],
      "status": "idle"
    }
  }
}
```

**2. State Delta — agent starts researching (replace status):**
```
event: STATE_DELTA
data: {
  "type": "STATE_DELTA",
  "delta": [
    { "op": "replace", "path": "/research/status", "value": "researching" },
    { "op": "replace", "path": "/research/title", "value": "AG-UI Protocol Analysis" }
  ]
}
```

**3. State Delta — agent adds outline sections (add to array):**
```
event: STATE_DELTA
data: {
  "type": "STATE_DELTA",
  "delta": [
    { "op": "add", "path": "/research/sections/-", "value": { "title": "Architecture", "content": "" } },
    { "op": "add", "path": "/research/sections/-", "value": { "title": "Events", "content": "" } }
  ]
}
```
Note: `/-` is JSON Pointer syntax for "append to array."

**4. State Delta — agent fills in section content (replace):**
```
event: STATE_DELTA
data: {
  "type": "STATE_DELTA",
  "delta": [
    { "op": "replace", "path": "/research/sections/0/content", "value": "AG-UI uses an event-based SSE architecture..." }
  ]
}
```

**5. State Delta — move an item between lists:**
```
event: STATE_DELTA
data: {
  "type": "STATE_DELTA",
  "delta": [
    { "op": "move", "path": "/completed_items/0", "from": "/pending_items/0" }
  ]
}
```

**6. State Delta — remove temporary data:**
```
event: STATE_DELTA
data: {
  "type": "STATE_DELTA",
  "delta": [
    { "op": "remove", "path": "/research/scratch_notes" }
  ]
}
```

**7. Test operation — guard before mutation:**
```
event: STATE_DELTA
data: {
  "type": "STATE_DELTA",
  "delta": [
    { "op": "test", "path": "/research/status", "value": "researching" },
    { "op": "replace", "path": "/research/status", "value": "complete" }
  ]
}
```
The `test` ensures status is still "researching" before marking complete — if another update already changed it, the whole patch is rejected.

#### Answer: Backend State Sync

Yes — the backend agent maintains its own state object, and AG-UI syncs it to the UI via these SSE events. The agent emits `STATE_SNAPSHOT` or `STATE_DELTA` events as it works. The frontend applies them to its local copy. The frontend can also push state changes back to the agent (via `useCoAgent`'s `setState`), making it truly bidirectional.
Why does RFC 6902 have a redundant event and data.type?

### Re: Why `event:` and `data.type` appear redundant

They serve different layers — it's not actually RFC 6902 doing this, it's the **SSE transport + AG-UI protocol** combination:

**`event:` (SSE layer)** — This is a standard [Server-Sent Events](https://html.spec.whatwg.org/multipage/server-sent-events.html) field. The browser's `EventSource` API uses it for routing: `eventSource.addEventListener("STATE_DELTA", handler)`. It's transport-level — it tells the SSE client *which listener* to invoke. If you're not using SSE (e.g., WebSocket transport), this field doesn't exist.

**`data.type` (AG-UI protocol layer)** — This is inside the JSON payload and is part of the AG-UI event schema. It's transport-agnostic — it identifies the event type regardless of how it was delivered (SSE, WebSocket, webhook, etc.).

**Why both?**
1. **Transport independence** — AG-UI is transport-agnostic. The `data.type` field works whether the event arrives via SSE, WebSocket, or HTTP polling. The `event:` field is SSE-specific syntax.
2. **Self-describing payloads** — If you log, queue, or replay events, the JSON payload carries its own type. You don't need the SSE envelope to understand it.
3. **Middleware routing** — AG-UI's middleware layer processes events by `data.type`. It doesn't depend on transport-specific routing.

In practice, when using SSE they'll always match. But they exist at different abstraction levels — one is "how the transport delivers it" and the other is "what the protocol says it is."

My examples showed both because that's what the wire format looks like with SSE transport. If AG-UI were running over WebSocket, you'd only see the JSON `{ "type": "STATE_DELTA", ... }` without the `event:` line.
<!-- agent:boundary:45ae454b -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
