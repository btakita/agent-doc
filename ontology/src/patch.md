# Patch

## [Ontology](../../../existence-lang/ontology/src/ontology.md)

A content delta targeting a specific [Component](./component.md) in a [Document](./document.md). A Patch is a [Signal](../../../existence-lang/ontology/src/signal.md) — it carries the agent's intended write: a component name, a patch mode (replace, append, or prepend), and the new content to apply.

In template-format documents, patches are expressed as fenced blocks in the agent's response:

```
<!-- patch:component-name -->
content here
<!-- /patch:component-name -->
```

In IPC mode, patches are written as JSON files to `.agent-doc/patches/<hash>.json` for the IDE plugin to apply via the Document API.

## [Axiology](../../../existence-lang/ontology/src/axiology.md)

The Patch matters because it decouples the agent's intent from the document's current state. Rather than the agent rewriting the full document, it emits targeted deltas. The binary applies them atomically, preserving user edits in regions the agent did not target.

Patch application is fully deterministic and owned by the binary. The skill (LLM) decides which component to target and what content to write; the binary handles finding the component markers, applying the mode, and performing the atomic write. This separation ensures that a malformed patch fails with a clear error rather than corrupting the document.

## [Epistemology](../../../existence-lang/ontology/src/epistemology.md)

### Pattern Expression

A Patch is the agent-doc instance of a [Communication](../../../existence-lang/ontology/src/communication.md) pattern found across software: the diff/patch protocol in git, the JSON Patch RFC 6902, surgical DOM mutations in browser rendering. The common thread — express change as a delta over a named target, not as a full replacement of the whole — minimizes information loss and enables conflict detection.

Patch mode resolution follows a precedence chain: inline component attribute > `components.toml` > built-in default. This layered [Context](../../../existence-lang/ontology/src/context.md) resolution keeps individual patches simple while allowing per-document and per-component policy.
