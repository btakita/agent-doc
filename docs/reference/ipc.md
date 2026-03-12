# IPC

## Ontology

IPC (Inter-Process Communication) in agent-doc is a [System](../../ontology/src/system.md) for delivering document patches from the agent-doc binary to an IDE [Plugin](../guide/editor-integration.md) without triggering external file change detection. It derives from [Context](../../ontology/src/context.md) (the IDE's document editing environment) and [Resolution](../../ontology/src/resolution.md) (applying changes at the component level rather than the whole-file level).

IPC is the bridge between the CLI [Domain](../../ontology/src/domain.md) (agent-doc binary) and the IDE [Domain](../../ontology/src/domain.md) (JetBrains/VS Code plugin), enabling conflict-free document updates that preserve user [Focus](../../ontology/src/focus.md) — cursor position, selection, and editing flow.

## Axiology

External file writes (tempfile + rename) trigger IDE reload behaviors:
- **Cursor displacement** — IDE moves caret to the changed region during reload
- **"File changed externally" dialog** — blocks user flow, risks data loss on Esc
- **Undo history disruption** — external writes break the IDE's undo chain

IPC eliminates all three by routing patches through the IDE's native Document API, where changes are applied in-process with full cursor preservation and undo batching.

## Epistemology

### Architecture

```
                        .agent-doc/patches/
                        ┌──────────────────┐
agent-doc write --ipc ──┤  <hash>.json     │
                        │                  │
                        │  { file, patches,│
                        │    unmatched,    │
                        │    baseline }    │
                        └────────┬─────────┘
                                 │
                    NIO WatchService (inotify/FSEvents)
                                 │
                        ┌────────▼─────────┐
                        │  PatchWatcher.kt │
                        │                  │
                        │  1. Read JSON    │
                        │  2. Find Document│
                        │  3. Apply patches│
                        │  4. Save to disk │
                        │  5. Delete JSON  │
                        │     (ACK)        │
                        └────────┬─────────┘
                                 │
                    WriteCommandAction.runWriteCommandAction
                    (holds doc lock, batches undo, preserves cursor)
                                 │
                        ┌────────▼─────────┐
                        │  IntelliJ Doc    │
                        │  (in-memory)     │
                        │                  │
                        │  <!-- agent:X -->│
                        │  patched content │
                        │  <!-- /agent:X-->│
                        └──────────────────┘
```

### Sequence

```
Binary                    Filesystem              Plugin
  │                          │                      │
  │  write <hash>.json       │                      │
  ├─────────────────────────>│                      │
  │                          │  ENTRY_CREATE event  │
  │                          ├─────────────────────>│
  │                          │                      │ read JSON
  │                          │                      │ find Document
  │                          │                      │ apply patches
  │                          │                      │ save document
  │                          │  delete <hash>.json  │
  │                          │<─────────────────────┤
  │  poll: file gone (ACK)   │                      │
  │<─────────────────────────│                      │
  │                          │                      │
  │  read file, save         │                      │
  │  snapshot + CRDT state   │                      │
  │                          │                      │
```

### Patch JSON Format

```json
{
  "file": "/absolute/path/to/document.md",
  "patches": [
    {
      "component": "exchange",
      "content": "Response content for the exchange component."
    },
    {
      "component": "status",
      "content": "**Version:** v0.17.0 | **Tests:** 303 passing"
    }
  ],
  "unmatched": "Content not targeting a specific component.",
  "baseline": "Document content at response generation time."
}
```

Each patch targets a `<!-- agent:name -->...<!-- /agent:name -->` component. The plugin replaces the content between markers with the patch content.

### Fallback

If the patch file is not consumed within 2 seconds (plugin not installed or IDE not running), the binary:

1. Deletes the unconsumed patch file
2. Falls back to direct CRDT stream write (`run_stream()`)
3. Logs `[write] IPC timeout — falling back to direct write`

This makes `--ipc` safe to use unconditionally in the SKILL.md workflow.

### Component Mapping

| Binary | Plugin | Purpose |
|--------|--------|---------|
| `write.rs:run_ipc()` | `PatchWatcher.kt` | End-to-end IPC flow |
| `template::parse_patches()` | `applyComponentPatch()` | Patch extraction/application |
| `snapshot::save()` | `FileDocumentManager.saveDocument()` | Persistence |
| `atomic_write()` (patch JSON) | NIO `WatchService` | File-based IPC transport |

### Pattern Expression

#### IDE Scope

The IPC pattern maps to IntelliJ's threading model:
- **EDT (Event Dispatch Thread)**: `invokeLater` schedules patch application on the EDT
- **WriteCommandAction**: Acquires the document write lock, groups changes as a single undo unit
- **FileDocumentManager**: Flushes the in-memory Document to disk after patching

This is the same mechanism IntelliJ uses for its own refactoring operations — the cursor and selection are preserved because the change originates from within the IDE process, not from an external file modification.

#### CLI Scope

The binary side is deliberately minimal:
- Parse patches from stdin (reuses existing `template::parse_patches()`)
- Serialize to JSON (serde_json)
- Atomic write of patch file (same `atomic_write()` used everywhere)
- Poll for deletion with timeout
- Update snapshot from the file the plugin saved

No new dependencies, no new IPC protocol, no sockets — just a JSON file in a watched directory.
