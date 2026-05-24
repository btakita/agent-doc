# Full-Document IPC Corruption Chain

This reference records the repeated live failure chain where whole-document
editor IPC could race user typing, leave duplicated prompt text, and then repeat
on the next visible edit. The mitigation is intentionally wide: full-document
IPC is disabled at both the binary sender and editor-plugin receiver.

## Logic Chain

```mermaid
flowchart TD
  typing["User types a prompt in agent:exchange"]
  staleState["Snapshot, sidecar, or patch state was computed before the latest keystrokes"]
  fullPayload{"A fullContent payload exists?\nold sender, stale file patch, or foreign tool"}
  oldReceiver["Old editor plugin accepts fullContent after source proof"]
  wholeReplace["Editor replaces the whole document buffer"]
  clobber["Live prompt bytes are overwritten, duplicated, or reinserted from stale content"]
  promptVariant["Prompt-prefix normalization sees adjacent raw/prefixed prompt variants"]
  duplicateRepair{"Repair catches every duplicate before closeout?"}
  committedClean["Snapshot and committed blob stay clean"]
  residue["Duplicate prompt residue remains visible or becomes new drift"]
  nextTyping["User keeps typing the same prompt"]
  repeat["The stale full-document path can fire again and repeat the sequence"]

  disabledBinary["New binary: try_ipc_full_content logs disabled and returns false before socket/file payload construction"]
  guardedDisk["Caller uses guarded disk/snapshot repair or narrow component patching"]
  disabledPlugin["New editor plugins: delete file-watch fullContent patches and reject socket fullContent payloads"]
  noReplace["No whole-buffer editor replacement occurs"]
  narrowPatch["Only component patches, prefix normalization patches, reposition patches, or guarded disk repair remain"]

  typing --> staleState --> fullPayload
  fullPayload -- "yes, before this fix" --> oldReceiver --> wholeReplace --> clobber --> promptVariant --> duplicateRepair
  duplicateRepair -- "yes" --> committedClean
  duplicateRepair -- "no" --> residue --> nextTyping --> repeat --> fullPayload
  fullPayload -- "yes, after this fix" --> disabledPlugin --> noReplace --> narrowPatch
  staleState --> disabledBinary --> guardedDisk --> narrowPatch
  fullPayload -- "no" --> narrowPatch
```

## Disabled Path

- The Rust binary keeps the committed-cycle cleanup guard, then returns `false`
  from full-document IPC paths before constructing socket or file payloads.
- JetBrains and VS Code plugins no longer treat source-buffer proof as
  permission to apply `fullContent`.
- File-watch payloads containing `fullContent` are logged and deleted as
  disabled stale/foreign patches so they cannot retry indefinitely.
- Socket payloads containing `fullContent` fail closed.
- Duplicate prompt repair remains as defense in depth, but correctness no
  longer depends on it catching corruption caused by a whole-buffer editor
  replacement.
