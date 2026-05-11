# Plan: #agent-doc-bug declaration chain backlog order

## Problem

When a user provides a chain of bug declarations that each invoke
`#agent-doc-bug`, the resulting backlog entries can appear in reverse order from
the declaration chain.

This is adjacent to the completed `#pendrev` bug for multiple `--pending-add`
flags in one command, but it is a separate workflow surface: independent
`#agent-doc-bug` declarations across a prompt chain should preserve declaration
order when captured into the target backlog.

## Fix

1. Treat a prompt chain of `#agent-doc-bug` declarations as an ordered batch
   before mutating the target backlog.
2. Insert the ordered batch so the first declaration remains above later
   declarations.
3. Keep existing backlog priority explicit: if the agent intentionally
   reprioritizes, the closeout must say so instead of silently reversing the
   declaration chain.
4. Add diagnostics that show the declaration order and final inserted order
   when more than one backlog item is captured.

## Tests

1. Two `#agent-doc-bug` declarations in one prompt chain are inserted in the
   same order as declared.
2. Three or more declarations preserve order.
3. A single declaration is unchanged.
4. Intentional priority override is represented explicitly in the response or
   mutation metadata, not by accidental LIFO insertion.
