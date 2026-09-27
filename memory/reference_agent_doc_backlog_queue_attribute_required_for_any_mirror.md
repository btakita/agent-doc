---
name: reference-agent-doc-backlog-queue-attribute-required-for-any-mirror
description: No agent-doc command can add a component attribute, so backlog items never reach agent:queue unless the agent:backlog marker already carries `queue` — and hand-editing the marker is unsafe on a live document.
metadata:
  type: reference
---

Every backlog-to-queue mirror path is gated on the `queue` attribute of the
`<!-- agent:backlog ... -->` marker:

- `--backlog-queue-placement` acts only "when the backlog carries a `queue` attribute".
- `agent-doc queue sync` is "sync from backlog items with `queue` attribute".

Verified 2026-09-26 by probe, not by reading help text: on a scratch copy of a document
whose marker was bare `<!-- agent:backlog -->`, two `--backlog-add` items landed in
`agent:backlog` and mirrored **nothing** into `agent:queue`.

**No command can add the attribute.** `agent-doc backlog` has no `set-attr`; `write`
exposes no marker flag; `patch` replaces component *content* only; `migrate` only
*strips* deprecated attributes. The marker span itself is modelled
(`Component.open_start`/`open_end`, `parse_attrs` in `agent-doc-element/src/element.rs`),
so the mutation class simply does not exist yet.

**Do not hand-edit the marker to work around this** when the document is live under an
editor — `agent-doc reliable-sync-status` shows the registration. A raw disk edit under a
live editor becomes a disk candidate the agent must never resolve, which is a wedge risk.
Ask the operator for the one-token marker edit in the editor they already have open, or
land the missing `backlog set-attr` command.

Documents that already carry it: `tasks/software/lazily.md`, `tasks/software/corky.md`,
`tasks/brookebrodack-dev.md` (`<!-- agent:backlog priority queue -->`).

Related: [[reference-agent-doc-cross-document-backlog-add-trips-coined-id-guard]]
