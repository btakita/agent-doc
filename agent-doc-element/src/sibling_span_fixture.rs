//! `#componentownclose` regression fixture: the 2026-10-26 `agent-doc-bugs.md`
//! shape where an `agent:loop` is followed directly by an `agent:todo`.
//!
//! The operator deleted loop rows in the editor. One editor transaction removed
//! everything from just after the loop's Watch row up to, but not including,
//! `<!-- /agent:todo -->`: the leftover loop rows, `<!-- /agent:loop -->`, the
//! `## Todo` heading, `<!-- agent:todo -->`, and every todo item. The result
//! opened `agent:loop` and closed it with `/agent:todo`. A fresh controller then
//! adopted that buffer as canonical. These helpers reproduce each step so every
//! crate that rewrites or adopts component spans can prove that a sibling
//! component never moves, shrinks, or gets absorbed.
//!
//! Exposed (not `cfg(test)`) so other crates' tests can share one copy of the
//! incident shape; nothing in production calls it.

/// The rows the operator kept and deleted.
pub const WATCH_ROW: &str = "- 📥 Watch https://github.com/btakita/agent-doc/issues for new issues. Add new issues into the backlog + queue except issue #40 + #45.\n";
const LOOP_TAIL_ROWS: &str = "\
- 📥 do [#layoutunprovenliveverify]
- 📥 do [#grokqueuesubagents]
- 🔄 release + publish
- 📥 What does `🔄` mean? Is it documented?
";

/// The todo body that was lost (byte-for-byte shape of the six items).
pub const TODO_ITEMS: &str = "\
- [ ] [#ahfe] Live-check GH #267 on the 263 IDE: after `agent-doc upgrade` WITHOUT restart, Run Agent Doc should complete.
- [ ] [#ctg7] Under heavy load, switch the editor 2 panes -> 1 -> 2 and confirm tmux follows and stays at 2 (stale-controller fix).
- [ ] [#coder-remote-install-02805] Repair Coder Remote Dev sync after the two-sided install.
- [ ] [#jb263-feed-verify] In IntelliJ IDEA 2026.3, configure only the 263 feed, refresh Plugins, update Agent Doc, restart, and verify.
- [ ] [#layoutunprovenliveverify] Open sdk-demo.md and agent-doc-bugs.md together, run Sync Tmux Layout, and confirm two panes.
- [ ] [#autosync-restart-verify] Restart IntelliJ IDEA 2026.3 and Rider 2026.2, then switch editor tabs: tmux should follow within about a second.
";

/// How the component openers are spelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenerStyle {
    /// `## Run: Issues` on its own line, then the bare opener (the committed
    /// 13:02 shape).
    HeadingLine,
    /// `## Loop: Issues <!-- agent:loop id="issues" ... -->` (the shape the
    /// live buffer had by 17:58Z, after `568aa7102e`).
    HeadingPrefixed,
}

/// One step of the operator's edit sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopEdit {
    /// Nothing deleted.
    None,
    /// Delete every loop row except Watch; both components stay valid.
    DeleteRowsKeepClose,
    /// Delete every loop row except Watch AND the `<!-- /agent:loop -->` line;
    /// the todo component is untouched but the loop is unclosed.
    DeleteRowsAndLoopClose,
    /// The incident: delete from after Watch up to `<!-- /agent:todo -->`, so
    /// `agent:loop` is closed by `/agent:todo`.
    DeleteThroughTodoBody,
}

pub const ALL_EDITS: [LoopEdit; 4] = [
    LoopEdit::None,
    LoopEdit::DeleteRowsKeepClose,
    LoopEdit::DeleteRowsAndLoopClose,
    LoopEdit::DeleteThroughTodoBody,
];
pub const ALL_STYLES: [OpenerStyle; 2] = [OpenerStyle::HeadingLine, OpenerStyle::HeadingPrefixed];

fn loop_open(style: OpenerStyle) -> &'static str {
    match style {
        OpenerStyle::HeadingLine => {
            "## Run: Issues\n<!-- agent:loop id=issues interval=1h priority -->\n"
        }
        OpenerStyle::HeadingPrefixed => {
            "## Loop: Issues <!-- agent:loop id=\"issues\" interval=\"1h\" priority -->\n"
        }
    }
}

fn todo_open(style: OpenerStyle) -> &'static str {
    match style {
        OpenerStyle::HeadingLine => "## Todo\n<!-- agent:todo -->\n",
        OpenerStyle::HeadingPrefixed => "## Todo <!-- agent:todo -->\n",
    }
}

const HEAD: &str = "\
# agent-doc bugs

## Queue: Main
<!-- agent:run id=\"main\" subagents priority go -->
- 📥 end a Reported queue claim on every strike path
<!-- /agent:run -->

";

const TAIL: &str = "
## Backlog
<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review
<!-- agent:review -->
- [/] [#layoutunprovenliveverify] Integrate subagent commit cb2f9068e3 and live-verify Sync Tmux Layout.
<!-- /agent:review -->
";

/// The `agent:todo` component region (heading + opener + items + close).
pub fn todo_region(style: OpenerStyle) -> String {
    format!("{}{TODO_ITEMS}<!-- /agent:todo -->\n", todo_open(style))
}

/// The document after `edit`, in `style`.
pub fn document(style: OpenerStyle, edit: LoopEdit) -> String {
    let loop_open = loop_open(style);
    let todo = todo_region(style);
    let todo_after_items = "<!-- /agent:todo -->\n";
    let middle = match edit {
        LoopEdit::None => {
            format!("{loop_open}{WATCH_ROW}{LOOP_TAIL_ROWS}<!-- /agent:loop -->\n\n{todo}")
        }
        LoopEdit::DeleteRowsKeepClose => {
            format!("{loop_open}{WATCH_ROW}<!-- /agent:loop -->\n\n{todo}")
        }
        LoopEdit::DeleteRowsAndLoopClose => format!("{loop_open}{WATCH_ROW}\n{todo}"),
        LoopEdit::DeleteThroughTodoBody => format!("{loop_open}{WATCH_ROW}{todo_after_items}"),
    };
    format!("{HEAD}{middle}{TAIL}")
}

/// Whether `edit` leaves every component closed by its own close tag.
pub fn edit_is_structurally_valid(edit: LoopEdit) -> bool {
    matches!(edit, LoopEdit::None | LoopEdit::DeleteRowsKeepClose)
}

/// Whether `edit` keeps the todo region byte-for-byte.
pub fn edit_keeps_todo(edit: LoopEdit) -> bool {
    !matches!(edit, LoopEdit::DeleteThroughTodoBody)
}

/// The tail after the loop and todo (Backlog + Review), which every edit keeps.
pub fn tail_region() -> &'static str {
    TAIL
}
