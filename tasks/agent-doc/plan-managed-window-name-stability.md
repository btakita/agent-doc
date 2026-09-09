# Managed tmux window name stability

Invariant: repairing a managed layout keeps target and stash names under layout
ownership; child process titles must not turn a stash into a duplicate target.

Policy owner: `agent-doc-sync-io::sync::repair_layout`, shared by manual sync,
controller structural effects, and session doctor.

Transition table: owned window with automatic naming enabled → disable it before
resize/consolidation; owned window already pinned → no mutation; unrelated window
→ preserve its options; option-write failure → fail with context rather than
continue destructive classification. A repeated repair is converged.

Evidence inputs: one tmux window survey with ID, role name, height, and effective
automatic-rename / allow-rename flags. Live evidence showed a stash being resized,
then classified as a duplicate target and joined into an already reconciled
two-column layout; the visible managed window also inherited automatic naming.

Reactive topology: the existing ProcessScope desired-layout Source → projection
Computed → structural Effect → layout receipt Source remains the owner. The
one-shot tmux repair adapter pins naming while applying that effect; no new graph,
polling loop, or authority cache is needed.

Imperative extraction audit: no independent derived-state owner is introduced.
Window options are external facts surveyed at the existing repair boundary.

Allowed edit surfaces: sync repair and its isolated-tmux tests; sync specification;
release version projections and parity ledger; the tmux CI target, which must
include repair tests from the extracted sync crate rather than only the root.

Verification: isolated tmux with two visible panes and four stashed panes,
automatic naming and child renames enabled on managed windows, untouched unrelated
window, repeated repair idempotence; full `make check` and `make tmux-ci`.

Out of scope: changing operator-owned pane protections or editor adapters;
terminating live sessions; claiming stash windows from arbitrary process names.
