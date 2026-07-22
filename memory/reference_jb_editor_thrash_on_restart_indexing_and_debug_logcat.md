---
name: reference_jb_editor_thrash_on_restart_indexing_and_debug_logcat
description: "IntelliJ high-CPU thrash after JB plugin install/restart — two root causes (400GB reindex + leftover DEBUG log category), both IDE-config fixes, no plugin rebuild"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 100dee66-cec0-44f6-a14d-ace26d21017c
---

Symptom: "installed latest JB plugin, restarted, editor thrashing" — IDEA JVM hit ~1295% CPU (13 cores) on restart, settling to idle (~3%) after indexing finished. The thrash is a **one-time restart cost**, not steady plugin churn.

Diagnosis method (ground truth beats guessing):
- `jstack <ideaPID>` → rank threads by `cpu=Nms`. Startup spike = C2 CompilerThread + IntelliJ index writers (Stubs/IdIndex/Trigram) + many `DefaultDispatcher-worker`. The huge `emitAllInternal`/`combineInternal` counts in a jstack are the **coroutine-scope dump section** (IntelliJ platform's own per-editor toolbar/notebook customizers), NOT running threads — don't mistake them for a Flow storm.
- Real per-thread CPU = sample `/proc/<pid>/task/*/stat` (utime+stime) deltas over 3s. This showed idle (~3%) once indexing was done.

Two contributing factors, both fixed via IDE config (NO plugin rebuild — respects "STOP cutting agent-doc releases; confirm first"):

1. **400GB monorepo reindex.** `du -sh src/*/target src/*/node_modules .venv` → `src/agent-doc/target`=273GB, `src/lazily-rs/target`=54GB (both already excluded), but `src/patchboard/target` (8GB) + ~11 `node_modules` were **INDEXED**. Fix: add `<excludeFolder>` to the owning `.iml`. ROOT-owned dirs (no sibling `.iml`, e.g. patchboard, equityfundingsource-dev) → `agent-loop.iml`; module-owned → each `src/X/X.iml` (expand self-closing `<content url=".../>` to a `<content>…</content>` pair first). Audit script in scratchpad compares heavy dirs vs union of all `.iml` excludeFolder entries.
2. **Leftover DEBUG log category.** `~/.config/JetBrains/IntelliJIdea<ver>/options/log-categories.xml` had `com.github.btakita.agentdoc` (and `com.intellij.platform.lsp`) at DEBUG → plugin computed/formatted/wrote `crdt-perf` FINE lines every 5s drain cycle across every open session-doc replica (per-cycle overhead + unbounded log growth, worst during the indexing window). Fix: reset categories to `[]`. Applies on next IDE start. Risk: Settings Sync (`syncEnabled=true`) could restore it.

External `.iml`/options edits survive a clean IDE shutdown and are read on next open (IntelliJ reloads `.iml` from disk on external change). Verify XML with `xml.dom.minidom.parse`.

Not thrash causes (idle / deferred, would need a plugin rebuild → confirm first): CRDT drain loop polls every reopened session-doc tab every 5s indefinitely (backoff caps at 5s, never detaches no-peer replicas); ~20+ `agent-doc-patch-watcher-<project>` WatchService threads, one per sibling repo (0.09ms cpu each). IntelliJ's own `SettingsSyncBridge "Pending events is empty"` WARN fires every 1s — IDE noise, not the plugin. The 273GB+54GB target dirs are also disk bloat (excluded from index) — `cargo clean` reclaims but forces full rebuilds.
