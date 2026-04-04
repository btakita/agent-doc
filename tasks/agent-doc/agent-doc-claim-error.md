---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
v0.31.5 released. Commit-on-claim, auto-setup, VCS refresh, --diff-only, skill runbooks, .ai/runbooks convention.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Features: commit-on-claim, auto-setup untracked files, --diff-only flag, skill runbook bundling. Conventions: `.ai/runbooks/` (project root, vendor-neutral), `runbooks/` (skill-scoped). v0.31.5 released (cargo + GitHub). Open: PyPI manylinux build, `.ai/runbooks` blog post.*

Should the `compact exchange` runbook also include summarizing the document in the current uncommitted state? 
### Re: Compact exchange and uncommitted state

The runbook's step 2 already covers summarizing exchange content (decisions, facts, open items). The question is whether it should also capture **uncommitted user input** — text written after the last boundary marker that hasn't been responded to yet.

**Current behavior:** `agent-doc compact` archives everything inside `&lt;!-- agent:exchange --&gt;`, including uncommitted user input. The summary (passed via `--message`) is written by the skill, which sees the full exchange before compacting.

**Should the runbook explicitly mention this?** Yes — it's a subtle point. If the user has pending questions/instructions in the exchange when they request compaction, those should be:
1. Acknowledged in the summary (so they're not silently dropped)
2. Or preserved as active items after the archive pointer

This is a documentation-only change to the runbook — adding a note to step 2: "If the exchange contains unanswered user input, note it as an open item in the summary or address it before compacting."

The binary behavior is already correct (archives everything). The skill just needs to be mindful of pending user content.
do
- [ ] Update compact-exchange runbook: note about preserving uncommitted user input
### Re: Updated compact-exchange runbook

Updated the source runbook at `src/agent-doc/runbooks/compact-exchange.md` — added a bullet under step 2:

> **Unanswered user input** — if the exchange contains uncommitted questions or instructions that haven't been responded to, note them as open items in the summary (don't silently drop them)

Binary rebuilt (v0.31.8) and skill reinstalled. The bundled runbook and installed copy are both updated.

I transferred a task from tasks/briantakita.me.md to tasks/software/corky.md
The `agent-doc transfer` should have written a prompt of the transfer into tasks/software/corky.md
The response patch in tasks/software/corky.md claims:
> agent-doc has no cross-file write capability — each session only manages its own document. This is a current limitation.

Is this true? If it is, we should have a transfer and extract directly write to the target document with the annotated prompt.
### Re: `agent-doc transfer` does write to the target document (HEAD)

The claim is **incorrect**. Looking at `src/agent-doc/src/extract.rs:148-203`, `agent-doc transfer` absolutely writes to the target document:

1. Reads source component content (line 169)
2. Clears source component (lines 175-177)
3. Formats a source annotation: `*Transfer from <source>*` (lines 180-181)
4. Finds the matching component in the target, appends the annotated content (lines 183-192)
5. Atomic writes + snapshot saves for both files (lines 176-177, 194-195)

So when you ran `agent-doc transfer`, it should have written the transferred content into corky.md's exchange component with an annotation.

If the response in corky.md claimed "no cross-file write capability," that was the skill/LLM hallucinating — the binary handled the transfer correctly. The LLM in the corky session may have responded to the transferred content without realizing the binary already wrote it.

**No code change needed.** The transfer and extract commands already write directly to the target document with annotated prompts.
<!-- agent:boundary:0a5415ef -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Release v0.31.5
- [x] Update compact-exchange runbook: note about preserving uncommitted user input
- [ ] PyPI publish (needs manylinux Docker build)
<!-- /agent:pending -->
