---
name: feedback_lintfence_fenced_component_markers_break_lint_and_parser
description: "Fenced <!-- agent:queue/exchange --> markers inside ``` code blocks in queue/exchange items break BOTH the finalize lint-gate AND the granular-write convergence parser"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 8984241a-86de-4772-b2dd-1cd5a6c280f6
---

When a queue/exchange item contains a fenced ``` code block that itself contains agent-doc component markers (`<!-- agent:queue -->`, `<!-- /agent:queue -->`, `<!-- agent:exchange -->`), two failures fire (live-repro 2026-06-13 in `tasks/agent-doc/agent-doc-bugs2.md`, the tsift.md queue example pasted as a fenced bug report):

1. **finalize/write lint-gate** raises `agent-doc/duplicate-component` ("`agent:queue` opened again before its previous close") **at the fenced lines** — fenced code is not excluded from component-structure linting. Blocks `finalize`/`write --commit`. Workaround: `--lint off`.
2. **granular-write convergence parser** fails `mismatched component: opened 'exchange' but closed 'backlog'` ("failed to parse adopted snapshot for editor convergence"). This means `agent-doc write --pending-add/--done/...` (any managed-component mutation) **cannot run at all** while the doc has fenced component markers in its items — even with `--lint off`. There is no granular-mutation workaround; the only path is to commit the prose response (which DOES go through via `finalize --force-disk --lint off` then `write --commit --lint off`).

**Why:** the tagpath agent-doc dialect parser/linter treats `<!-- agent:* -->` inside ``` fences as real component boundaries. **How to apply:** capture follow-up bugs in the committed prose response itself when the doc carries fenced markers (you can't `--pending-add`); the durable fix is to exclude fenced code spans from component-structure parsing+linting. Related: [[feedback_supervisor_crdt_replay_reset_from_current]].
