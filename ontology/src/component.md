# Component

## [Ontology](../../../existence-lang/ontology/src/ontology.md)

A named region in a template-format [Document](./document.md) bounded by opening and closing HTML comment markers. A Component is an [Entity](../../../existence-lang/ontology/src/entity.md) with identity (its name), bounds (open/close markers), content (the text between markers), and optional inline attributes (patch mode, write strategy).

Opening marker form: `<!-- agent:name -->` or `<!-- agent:name patch=append -->`
Closing marker form: `<!-- /agent:name -->`

## [Axiology](../../../existence-lang/ontology/src/axiology.md)

Components matter because they give the agent precise write targets. Without named regions, an agent response must replace the entire document or use heuristic matching to find where to write. With Components, a [Patch](./patch.md) names its target explicitly — the binary applies it deterministically without LLM involvement.

Component attributes control write behavior locally:
- `patch=replace` — the agent overwrites the component's content
- `patch=append` — the agent appends below existing content
- `patch=prepend` — the agent prepends above existing content

Precedence for patch mode: inline attribute > `components.toml` > built-in default.

## [Epistemology](../../../existence-lang/ontology/src/epistemology.md)

### Pattern Expression

A Component is the [Scope](../../../existence-lang/ontology/src/scope.md) applied to a [Document](./document.md)'s write surface. It bounds what the agent is permitted to modify in a single interaction. Common components in practice:

- `exchange` — the [Exchange](./exchange.md) where human-agent conversation accumulates
- `output` — agent-generated artifact (code, prose, analysis)
- `log` — chain-of-thought or thinking content routed away from the main response
- `pending` — queued user requests awaiting agent action

The [Boundary](./boundary.md) marker is a special positional signal that lives inside or adjacent to components, not a Component itself.
