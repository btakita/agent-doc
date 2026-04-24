# Spec: --pending-add Custom ID Prefix Syntax

**Status:** Open
**Date:** 2026-04-24
**Relates to:** `--pending-add`, `--pending-add-gated`, `pending_cmd`

---

## Problem

LLM agents consistently embed custom IDs as literal text inside `--pending-add` values instead of using the `id=<custom> ` prefix syntax. This produces double-bracketed pending items:

```
--pending-add "[#p2dk] P1: Raise threshold to 7:1"
```

Result in document:

```
- [ ] [#wzw8] [#p2dk] P1: Raise threshold to 7:1
```

The binary auto-generates `#wzw8` and treats `[#p2dk]` as part of the item text. The agent intended `#p2dk` as the ID.

**Correct syntax:**

```
--pending-add "id=p2dk P1: Raise threshold to 7:1"
```

Result:

```
- [ ] [#p2dk] P1: Raise threshold to 7:1
```

### Observed instance

In `tasks/claudescore-3.md`, the Claude Code skill (opus-4-6) used `--pending-add "[#p2dk] P1: ..."` and `--pending-add "[#q7mf] P1: ..."`, producing `[#wzw8] [#p2dk]` and `[#bc0m] [#q7mf]`. Required manual `--pending-edit` to clean up.

---

## Root Cause

The `id=<custom> ` prefix convention is only documented in `--help` output. The skill instructions, runbooks, and CLAUDE.md surfaces reference `--pending-add` but never show the custom-ID prefix syntax. Agents default to the `[#id]` bracketed format they see in document content because that's the only ID syntax visible in their context.

---

## Options

### A. Binary-side: detect and strip embedded `[#id]` prefix (defensive)

When `--pending-add` text starts with `[#<alphanum>]`, treat it as the custom ID rather than literal text. This makes both syntaxes work:

- `--pending-add "id=p2dk P1: ..."` (explicit prefix, already works)
- `--pending-add "[#p2dk] P1: ..."` (bracketed, currently broken)

**Pros:** Zero skill/prompt changes needed. Agents get it right without knowing the syntax.
**Cons:** Ambiguous if someone genuinely wants `[#...]` as text content (unlikely but possible).

### B. Prompt-side: add example to skill instructions

Add the `id=<custom> ` prefix syntax to the `--pending-add` documentation in the skill file and runbooks so agents see it in context.

**Pros:** No binary change. Correct by instruction.
**Cons:** Agents still have to remember it. Doesn't prevent the failure — just reduces frequency.

### C. Both A + B

Defensive binary parsing plus prompt documentation. Belt and suspenders.

---

## Recommendation

Option **C** (both). The binary should accept the bracketed format since that's what agents naturally produce, and the prompt surfaces should document the prefix syntax so agents use the canonical form. The binary detection is a one-line change in `pending_cmd.rs` (or equivalent); the prompt update is a few lines in the skill/runbook.
