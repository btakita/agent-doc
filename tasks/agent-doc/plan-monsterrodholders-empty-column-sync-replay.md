---
title: Validate JetBrains mixed-split sync preserves the remembered monsterrodholders pane
created: 2026-05-02
status: open
---

# Goal

Prove that JetBrains `Sync Layout` and claim-follow-up sync keep a two-pane tmux layout when the editor has a mixed split: a non-markdown file in one column and `src/boost-client/tasks/monsterrodholders.md` focused in the other.

# Steps

1. Open a mixed JetBrains split where `monsterrodholders.md` is on the right and the sibling split contains a non-markdown file.
2. Trigger the JetBrains sync path that previously showed `Sync: --col /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md [focus: ...]`.
3. Confirm the plugin now preserves the empty left-column placeholder in the `agent-doc sync` command and the Rust sync layer restores the remembered left tmux pane instead of collapsing to one pane.
4. Verify the tmux window ends with two panes, with `monsterrodholders.md` on the right and in focus.
5. Split any remaining mismatch into a follow-up bug if live replay still diverges from the new command/column-memory contract.
