# PRD: External Task Sources

## Problem

agent-doc currently treats the active markdown document as the work surface and
source of local task state. Teams may also track work in external project
management systems such as GitHub Issues, Linear, Jira, or bespoke PM tools. If
those systems are modeled as cross-document dependencies too early, the document
authority model becomes ambiguous: it is unclear which artifact owns status,
priority, dependency edges, and completion proof.

## Goals

- Represent external PM tools as source adapters that project tasks into the
  active document and work graph.
- Keep the active markdown document as the local interaction surface.
- Preserve source identity with stable metadata: provider, external id, URL,
  last-seen revision, sync status, and imported fields.
- Route imported task nodes through the same source-agnostic work graph used by
  document-local tasks and Auto-DAG scheduling.
- Keep external writes explicit and auditable; no silent remote mutation from a
  local closeout.

## Non-Goals

- Do not add cross-document dependencies as part of the first implementation.
- Do not make an external PM tool the runtime authority for the current
  document's live queue or closeout proof.
- Do not require model-assisted planning to import or reconcile external tasks.

## Model

External tasks are represented in the single active document as projected task
nodes. A node may include external-source metadata, but its local ordering,
queueing, and closeout proof remain document-local until an explicit sync step
pushes an update back to the source.

The external source adapter owns fetch, fingerprinting, mapping, and optional
push-back. The work graph owns dependency classification between projected
nodes. The document owns the user-visible task list, queue, and response history.

## Acceptance Criteria

- A provider-agnostic `ExternalTaskSource` model exists for imported tasks,
  source metadata, and sync state.
- Imported tasks can be rendered into backlog or icebox entries without
  requiring another agent-doc document.
- Auto-DAG/work-graph logic can consume imported task nodes through the same
  graph API as document-local tasks.
- External dependencies are stored as source-backed graph edges, not as implicit
  cross-document runtime dependencies.
- Push-back to external PM tools is explicit, logged, and separately testable
  from local document closeout.

## Open Questions

- Which provider should be the first adapter: GitHub Issues, Linear, Jira, or a
  generic JSON/CSV source?
- Should imported tasks default to backlog, icebox, or a separate
  `agent:external` projection section?
- What is the minimum conflict UI when the external task changed since the last
  import?
