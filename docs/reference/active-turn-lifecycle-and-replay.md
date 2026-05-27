# Active Turn Lifecycle And Replay Paths

This reference stores the generated diagrams for active agent-doc turns, routed
dispatch readiness, post-baseline prompt ownership, JetBrains stale
cache/conflict replay, and late fallback patch replay.

The implementation source of truth remains the Rust code and split specs. Keep
this page current when the implementation, workflow, or architecture changes in
ways that alter these lifecycle boundaries.

## Active Turn Lifecycle

`agent-doc` owns the document lifecycle: preflight, baseline selection, diff
classification, response merge, snapshot/capture state, commit, and
`session-check`. Claude, Codex, or OpenCode owns the live reasoning/tool turn
between `plan` and `finalize`.

```mermaid
sequenceDiagram
    autonumber
    participant User
    participant Doc as session markdown document
    participant AD as agent-doc CLI
    participant Snap as snapshots and captures
    participant Harness as Claude or Codex session
    participant Tools as repo tools and shell
    participant Git as git commit/session-check

    User->>Doc: edit prompt or queue directive
    User->>AD: invoke agent-doc <file>
    AD->>Snap: preflight reads last snapshot/baseline
    AD->>Doc: diff user edits against snapshot
    AD->>AD: classify prompt, claims, queue, model tier
    AD->>Harness: expose prompt targets and execution contract
    Harness->>Doc: read current document context
    Harness->>Tools: perform requested repo work when needed
    Tools-->>Harness: command output and verification evidence
    Harness->>AD: finalize response patch with baseline
    AD->>Doc: merge response into exchange
    AD->>Snap: update snapshot/capture state
    AD->>Git: commit session document writeback
    Git->>AD: post-commit session-check result
    AD-->>Harness: closeout success or recovery interruption
    Harness-->>User: console summary after committed closeout
```

A response that only appears in the console is not complete. It becomes part of
the managed turn only after `finalize` writes it into the document and the
binary-owned commit/session-check boundary passes.

## Dispatch Readiness Block

Dispatch-only routing can reuse an authoritative actor only when the live pane
proves it can consume input for the same generation. An idle-looking console is
not enough proof.

```mermaid
flowchart LR
    A["User invokes Run Agent Doc"] --> B["route resolves document target"]
    B --> C["authoritative actor store owns a pane for the document"]
    C --> D["dispatch-only route checks for a prompt-ready live pane"]
    D --> E["pane does not prove harness-specific readiness"]
    E --> F["wait for the same generation to return ready"]
    F --> G{"ready prompt observed?"}
    G -->|"yes"| H["inject trigger into the owned pane"]
    G -->|"no"| I["fail closed: do not inject into a busy or unproven actor"]
```

This guard prevents a second trigger from being mixed into an active or
unproven turn. Future queue-first dispatch should route repeated dispatches
through a single document lease and a durable queue instead of directly trying
another pane injection.

## Prompt Ownership After Baseline

Whole-buffer editor ACK content is an observation, not authority. If the ACK
contains user-owned prompt text created after preflight, agent-doc must preserve
that prompt for the next cycle and must not absorb it into the committed
snapshot for the current response.

```mermaid
flowchart TD
    A["preflight baseline"] --> B["user prompt entity P1"]
    B --> C["agent builds response entity R1"]
    C --> D["editor ACK returns whole buffer"]
    B --> E["user keeps typing prompt entity P2 after baseline"]
    E --> D
    D --> F{"ACK contains post-baseline user entity?"}
    F -->|"no"| G["adopt response snapshot"]
    F -->|"yes"| H["block ACK adoption for user-owned region"]
    H --> I["commit R1 from agent-owned response image"]
    H --> J["leave P2 visible for next preflight"]
    F -->|"duplicate P1/P2 detected"| K["repair duplicate artifact or fail closed"]
```

The durable model should distinguish user prompt entities from agent response
entities. Component ranges, content hashes, owner fields, and cycle ids give the
write path enough evidence to preserve post-baseline prompt text without
duplicating older prompts.

## JetBrains Stale Cache Conflict Replay

The intended replay exists because JetBrains may hold an older cached buffer
after disk changed underneath it. If the user accepts that cached editor buffer
hours later, agent-doc should treat the next managed cycle as delayed replay
classification, not as permission to trust the whole buffer.

```mermaid
flowchart TD
    A["agent-doc commits document at HEAD"] --> B["editor still has older cached buffer"]
    B --> C["JetBrains File Cache Conflict appears"]
    C --> D{"operator choice"}
    D -->|"reload from disk"| E["safe: editor matches HEAD"]
    D -->|"accept cached editor buffer"| F["stale replay into working tree"]
    F --> G{"dedupe/replay classifier matches HEAD?"}
    G -->|"yes"| H["repair working tree/snapshot without new response"]
    G -->|"no"| I["fail closed: preserve drift for investigation"]
    G -->|"classifier misses duplicate"| J["bad path: duplicate exchange or old prompt can persist"]
```

agent-doc may warn, disable stale patch application, and remove its own stale
sidecars after timeout. It must not dismiss the IDE dialog by choosing disk or
cached content for the user unless the editor exposes an explicit
cancel-agent-patch operation that preserves the user's content choice.

## Late Fallback Patch Replay

Fallback patch replay exists so an IPC timeout can still deliver a response if
the socket path failed while the cycle is still open. After the same cycle is
committed, every fallback for that cycle is stale.

```mermaid
flowchart TD
    A["finalize sends socket IPC patch for cycle C"] --> B{"socket ACK before timeout?"}
    B -->|"yes"| C["response visible and committed"]
    B -->|"no"| D["write fallback patch file"]
    D --> E{"cycle C still open?"}
    E -->|"yes"| F["file watcher may apply fallback"]
    F --> G["closeout commits cycle C"]
    E -->|"no, already committed"| H["reject stale fallback and clean up"]
    C --> I["delayed watcher sees old fallback"]
    I --> J{"cycle-state guard present?"}
    J -->|"yes"| H
    J -->|"no"| K["bad path: stale response patch re-dirties document"]
```

Cycle id, patch id, capture hashes, and terminal closeout state should reject
stale fallback files before they can mutate the editor buffer, snapshot, or
working tree.

## Elimination Boundary

Full-document IPC corruption is eliminated by construction only for first-party
paths that obey these invariants:

- template and component documents use component patches or guarded disk repair;
- first-party senders do not emit `fullContent` payloads for managed documents;
- first-party editor plugins reject or delete legacy `fullContent` payloads;
- ACK/file-read content cannot become snapshot authority when it contains
  post-baseline prompt drift absent from the agent-owned response image;
- committed-cycle fallback patches are rejected by cycle state before visible
  mutation;
- duplicate prompt repair remains defense in depth, not the primary guarantee.

External editors, stale caches, or foreign tools can still write arbitrary
whole-file content. agent-doc can detect, refuse, repair bounded duplicate
shapes, or fail closed in those cases, but it cannot make external writes
impossible.
