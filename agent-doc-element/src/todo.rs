//! Pure `agent:todo` component helpers (`#operatortodo`).
//!
//! `agent:todo` is the operator-owned action checklist: concrete actions the
//! operator, not the agent, must perform. Agents add brief checklist items to
//! it; the operator checks them off. Session documents often predate the
//! component, so every agent-side todo write must be able to create it.
//!
//! ## Spec
//! - [`ensure_todo_component_in_document`] returns the document unchanged when
//!   an `agent:todo` component exists. Otherwise it inserts an empty
//!   `## Todo` section at one deterministic location, matching the default
//!   template scaffold order (`agent:queue`, `agent:todo`, `agent:backlog`,
//!   `agent:review`): directly after the last `agent:queue`; else after the
//!   last `agent:backlog` / legacy `agent:pending`; else after the last
//!   `agent:exchange`; else at the end of the document. `agent:review` is never
//!   an anchor: inserting after it would put Todo below Review, and inserting
//!   before its marker would split the `## Review` heading from its component.
//! - [`add_todo_item_to_document`] appends one `- [ ] [#<id>] <action>` line
//!   to the end of the component body, creating the component first when
//!   absent. The leading `[#<id>]` is the item's own stable id (caller-chosen
//!   via `id=<id> ` / `[#<id>] `, which always wins, else derived from the
//!   action, collision-free against every id the todo body cites and the
//!   document's tracked-work ids). It never removes, reorders, or rewrites an existing line, so
//!   every existing checklist item and every operator edit survives
//!   byte-for-byte.
//! - Adding is idempotent: when any existing checklist item (open, done, or
//!   gated) already carries the same action text, the add is a no-op. A replayed
//!   closeout therefore never duplicates a todo, and never resurrects one the
//!   operator already checked off.
//! - `#nestedtodoadd`: add/edit text may carry nested sub-items. The first
//!   line is the item; every later line is one `  - <text>` sub-item, rendered
//!   under the item as `  - <text>` (two columns deeper than the item). The
//!   input is validated strictly ([`split_todo_payload`]): no blank lines, no
//!   flush-left or non-bullet lines, one indentation level, no checkbox
//!   children (a checkbox row is a separate todo item), no HTML comments or
//!   component markers. Sub-items are part of the item, never items: identity,
//!   id minting, source linking and dedupe read the item line alone, so a
//!   replayed add with sub-items is a no-op, and when the existing item's
//!   nested lines differ the add still changes nothing
//!   ([`TodoAddOutcome::sub_items_kept`]). `--todo-done`/`--todo-reopen` flip
//!   the item's checkbox and keep its sub-items; `--todo-remove` deletes them
//!   with it; a multi-line `--todo-edit` replaces the nested lines with the
//!   new sub-items, and a single-line edit keeps them.
//! - Only a LEADING id is an item's own id: bracketed `[#id]`, or bare `#id`
//!   followed by whitespace with at least one letter (`#sdktododone`; a
//!   leading `#157` stays a GitHub citation). Rewrites keep the row's
//!   spelling. An id cited later in the action (`Verify [#gate] live`,
//!   `Decide X (#gate)`) is a reference to tracked work. Operator-authored and
//!   legacy items may have no own id.
//! - [`assign_missing_todo_ids`] (`#todoidbackfill`) gives each id-less row
//!   a minted own id (never a tracked item's id, `#todoaddidhijack`), behind a
//!   typing gate, never re-id'ing a row and inserting only the id token.
//!   Preflight runs it every cycle.
//! - [`todo_owns_id`] reports whether a todo owns an id, so a closeout's
//!   `--done <id>` naming a todo's own id (and no tracked work) checks that
//!   todo off instead of failing (`#sdktododone`).
//! - Item selectors (`--todo-done`, `--todo-reopen`, `--todo-edit`,
//!   `--todo-remove`) resolve, first match wins: the item's own id; else the
//!   one item citing `#id` (narrowed to open items for done, checked items for
//!   reopen); else the one item whose action text equals the selector. Zero or
//!   several candidates is an error that names them.
//! - [`set_todo_item_state_in_document`] checks (`[x]`) or unchecks (`[ ]`) an
//!   item IN PLACE: position, own id, and text are kept, and a repeated call is
//!   a no-op. Done items stay in `agent:todo` (they are not archived to
//!   `agent:done`, which holds agent work); the operator prunes them.
//! - [`edit_todo_item_in_document`] replaces an item's action, keeping its
//!   position, state, and own id; [`remove_todo_item_from_document`] deletes an
//!   item that no longer applies, with its deeper-indented continuation lines.
//! - [`reconcile_todo_body`] (`#tododupdone`) folds duplicate copies of one
//!   todo into a single item at the earlier copy's position: same own id, same
//!   action text, or one copy is the other plus a completion note
//!   (`<action> — moot`) with at least one copy checked. Any checked copy makes
//!   the folded item checked. Distinct items, including two items that merely
//!   cite the same tracked id, are never touched.
//! - [`todo_unreferenced_ids`] reports which tracked-work ids the todo
//!   component does not reference as `#id`, for the closeout advisory;
//!   [`open_todo_items_referencing`] reports open items tied to ids a closeout
//!   resolved, for the "mark it done" advisory.
//!
//! ### Todo id discipline (`#todoids`, `#todoaddidhijack`)
//! - A todo's own id is its OWN identity: it never takes the id an open
//!   `agent:review` / `agent:backlog` / `agent:icebox` item already owns, and
//!   a `#<id>` citation in the action stays a reference (`#todoaddidhijack`
//!   supersedes the `#todoidlayer` id reuse). Every `--todo-add` item carries
//!   an own id: the caller's explicit `id=`, which always wins, else a minted
//!   one; an id-less add is never written. [`TodoLinkIndex`] carries the
//!   tracked-work evidence (open `agent:review` / `agent:backlog` /
//!   `agent:icebox` ids and resolved ids) the caller read from the document.
//! - A todo is still the visibility layer over the tracked work it surfaces
//!   an operator decision for: when the action cites exactly one OPEN
//!   `agent:review`, `agent:backlog`, or `agent:icebox` id (`Verify [#gate1]
//!   live`), the row links to it through the canonical source locator —
//!   `- [ ] [#t1a2] Verify [#gate1] live (source: agent:review [#gate1])`.
//!   When an OPEN todo is already linked to that item (locator, or a legacy
//!   row that shares its id) an add without `id=` is a no-op (duplicate todos
//!   for one item fold into the existing row).
//! - An explicit `id=<id>` / `[#<id>]` is used verbatim, and is REJECTED when
//!   `<id>` is owned by an open tracked item (cite it instead), names tracked
//!   work that is already resolved (a todo for finished work is moot), or is
//!   already a todo's own id.
//! - Legacy rows whose own id equals a tracked id (written by `#todoidlayer`)
//!   keep working: the link index reads that shared id as a backlink.
//! - Minted ids avoid every `#id` token in the document plus every id the
//!   link index knows (open or resolved), so a minted id can never be mistaken
//!   for a link to tracked work.
//! - [`preserve_checked_todo_items`] makes an operator's `[x]` sticky against
//!   a `patch:todo` replacement: an item the document already holds checked
//!   stays checked when the patch carries it open. Only `--todo-reopen`
//!   unchecks an item.
//! - [`todo_link_findings`] evaluates todos against the link index for the
//!   preflight signals, and [`auto_complete_linked_todos`] checks off an OPEN
//!   item whose OWN id is resolved tracked work or whose explicit tracked-work
//!   backlinks are all resolved. A plain citation remains advisory: only the
//!   canonical `(source: agent:<component> [#<id>])` locator or a tracked
//!   item's `(todos: [#<todo-id>] ...)` metadata establishes a completion link.
//!
//! ## Agentic Contracts
//! - All functions are pure: no file IO, no clock, no randomness.
//! - Placement depends only on the component layout, so the same document
//!   always yields the same insertion point.
//! - An action must be one non-empty line; later lines are accepted only as
//!   strictly validated nested sub-items of that one item, never silently split
//!   into several items (`#nestedtodoadd`).

use anyhow::{Context, Result};

use crate::element::{self, Component};

/// Canonical component name for the operator action checklist.
pub const TODO_COMPONENT: &str = "todo";

/// Section inserted when a document has no `agent:todo` component. It matches
/// the heading and markers of the default template scaffold.
pub const TODO_SECTION: &str =
    "\n## Todo\n\n<!-- agent:todo priority -->\n<!-- /agent:todo -->\n";

/// Check whether a component name refers to the operator todo checklist.
pub fn is_todo_component(name: &str) -> bool {
    name == TODO_COMPONENT
}

/// Return the action text of one Markdown checklist line (`- [ ] text`,
/// `* [x] text`, `1. [/] text`, ...), or `None` for any other line.
pub fn checklist_item_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let rest = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
        .or_else(|| {
            let digit_run = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
            if digit_run > 0 {
                trimmed[digit_run..].strip_prefix(". ")
            } else {
                None
            }
        })?;
    rest.strip_prefix("[ ] ")
        .or_else(|| rest.strip_prefix("[x] "))
        .or_else(|| rest.strip_prefix("[/] "))
}

/// Count the Markdown checklist items in a todo body.
pub fn count_checklist_items(body: &str) -> usize {
    body.lines()
        .filter(|line| checklist_item_text(line).is_some())
        .count()
}

/// Find the `agent:todo` component in already-read document content.
pub fn find_todo_component(content: &str) -> Result<Option<Component>> {
    let components = element::parse(content).context("failed to parse components")?;
    Ok(components
        .into_iter()
        .find(|component| is_todo_component(&component.name)))
}

/// Byte offset where a missing `agent:todo` section is inserted.
fn todo_insertion_offset(content: &str, components: &[Component]) -> usize {
    let anchors: [fn(&str) -> bool; 3] = [
        |name| name == "queue",
        element::is_backlog_component,
        |name| name == "exchange",
    ];
    anchors
        .iter()
        .find_map(|is_anchor| {
            components
                .iter()
                .rfind(|component| is_anchor(&component.name))
                .map(|component| component.close_end)
        })
        .unwrap_or(content.len())
}

/// Insert an empty `agent:todo` section when the document has none.
///
/// Returns the possibly-updated content, the todo component span in that
/// content, and whether the section was created.
pub fn ensure_todo_component_in_document(content: &str) -> Result<(String, Component, bool)> {
    let components = element::parse(content).context("failed to parse components")?;
    if let Some(todo) = components
        .iter()
        .find(|component| is_todo_component(&component.name))
    {
        return Ok((content.to_string(), todo.clone(), false));
    }
    let offset = todo_insertion_offset(content, &components);
    let mut out = String::with_capacity(content.len() + TODO_SECTION.len() + 1);
    out.push_str(&content[..offset]);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(TODO_SECTION);
    out.push_str(&content[offset..]);
    let todo =
        find_todo_component(&out)?.context("document has no todo component after insertion")?;
    Ok((out, todo, true))
}

/// Checkbox state of one `agent:todo` checklist item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoState {
    /// `[ ]` — the operator still has to do it.
    Open,
    /// `[/]` — waiting on something else before the operator can act.
    Gated,
    /// `[x]` — the operator did it.
    Done,
}

impl TodoState {
    fn from_marker(marker: char) -> Option<Self> {
        match marker {
            ' ' => Some(Self::Open),
            '/' => Some(Self::Gated),
            'x' => Some(Self::Done),
            _ => None,
        }
    }

    fn marker(self) -> char {
        match self {
            Self::Open => ' ',
            Self::Gated => '/',
            Self::Done => 'x',
        }
    }

    /// Whether the item still needs the operator (`[ ]` or `[/]`).
    pub fn is_open(self) -> bool {
        !matches!(self, Self::Done)
    }
}

/// One parsed `agent:todo` checklist line.
///
/// The layout is `<prefix>[<state>] [#<own id>] <action>`, where `prefix` is
/// the indentation plus list marker (`- `, `* `, `1. `, ...) and the
/// `[#<own id>] ` token is optional. Only a LEADING id is the item's own id:
/// bracketed `[#id]`, or bare `#id` (`#sdktododone`, see
/// `split_leading_bare_id`). An id cited later in the action
/// (`Verify [#gate] live`, `... (#gate)`) is a reference to tracked work, not
/// the item's identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TodoLine<'a> {
    pub prefix: &'a str,
    pub state: TodoState,
    pub own_id: Option<&'a str>,
    /// `#sdktododone`: the own id was written bare (`#<id> action`) rather than
    /// bracketed (`[#<id>] action`). Rewrites keep the spelling the row uses.
    pub own_id_bare: bool,
    pub action: &'a str,
}

/// Whether `id` is a well-formed tracked-work / todo id (`[A-Za-z0-9-]+`).
fn is_valid_todo_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Split a leading `[#id]` token off `text`, returning `(id, rest)`.
fn split_leading_bracket_id(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("[#")?;
    let close = rest.find(']')?;
    let id = &rest[..close];
    if !is_valid_todo_id(id) {
        return None;
    }
    let after = &rest[close + 1..];
    if !(after.is_empty() || after.starts_with(' ') || after.starts_with('\t')) {
        return None;
    }
    Some((id, after.trim_start()))
}

/// `#sdktododone`: split a leading BARE `#id` token off `text`, returning
/// `(id, rest)`.
///
/// A bare own id is the `- [ ] #<id> action` spelling some documents use. It
/// must be followed by whitespace (or end the line) and contain at least one
/// ASCII letter, so a GitHub reference that opens a row (`#157 needs review`)
/// stays a citation rather than becoming the item's identity.
fn split_leading_bare_id(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix('#')?;
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .unwrap_or(rest.len());
    let id = &rest[..end];
    if !is_valid_todo_id(id) || !id.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let after = &rest[end..];
    if !(after.is_empty() || after.starts_with(' ') || after.starts_with('\t')) {
        return None;
    }
    Some((id, after.trim_start()))
}

/// Split a row's leading own id off `text`: bracketed `[#id]` first, else a
/// bare `#id`. Returns `(id, rest, bare)`.
fn split_leading_own_id(text: &str) -> Option<(&str, &str, bool)> {
    if let Some((id, rest)) = split_leading_bracket_id(text) {
        return Some((id, rest, false));
    }
    split_leading_bare_id(text).map(|(id, rest)| (id, rest, true))
}

/// Parse one checklist line of an `agent:todo` body (no trailing newline).
pub fn parse_todo_line(line: &str) -> Option<TodoLine<'_>> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let text = checklist_item_text(line)?;
    // `checklist_item_text` returns the tail after the 4-byte `[s] ` box.
    let box_start = line.len() - text.len() - 4;
    let marker = line[box_start + 1..].chars().next()?;
    let state = TodoState::from_marker(marker)?;
    let (own_id, action, own_id_bare) = match split_leading_own_id(text) {
        Some((id, rest, bare)) => (Some(id), rest, bare),
        None => (None, text, false),
    };
    Some(TodoLine {
        prefix: &line[..box_start],
        state,
        own_id,
        own_id_bare,
        action: action.trim_end(),
    })
}

/// Render one todo line in the canonical layout. `bare` writes the own id as
/// `#id` (the spelling a bare-id row already uses) instead of `[#id]`.
fn render_todo_line(
    prefix: &str,
    state: TodoState,
    own_id: Option<&str>,
    bare: bool,
    action: &str,
) -> String {
    let mut out = String::with_capacity(prefix.len() + action.len() + 16);
    out.push_str(prefix);
    out.push('[');
    out.push(state.marker());
    out.push_str("] ");
    if let Some(id) = own_id {
        if bare {
            out.push('#');
            out.push_str(id);
            out.push(' ');
        } else {
            out.push_str("[#");
            out.push_str(id);
            out.push_str("] ");
        }
    }
    out.push_str(action);
    out
}

/// Result of [`add_todo_item_to_document`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoAddOutcome {
    /// The document after the add (unchanged when nothing was inserted).
    pub content: String,
    /// Whether a new checklist line was appended.
    pub inserted: bool,
    /// Whether `inserted` means an existing linked row gained its visible
    /// source locator rather than a new checklist line being appended.
    pub source_reference_added: bool,
    /// Whether the `agent:todo` component had to be created.
    pub created_component: bool,
    /// The item's own id: the new id when inserted, or the matching existing
    /// item's id (`None` for an id-less legacy item) when the add was a no-op.
    pub id: Option<String>,
    /// `#todoaddidhijack`: the one open tracked item, as `(id, component)`,
    /// that the row names in its canonical `(source: agent:<component>
    /// [#<id>])` locator; `None` when the action cites zero or several open
    /// tracked items. A new row's own id is never this id.
    pub source: Option<(String, String)>,
    /// `#nestedtodoadd`: the add was a no-op on an existing item whose nested
    /// lines differ from the sub-items the add carried. The existing item's
    /// lines are kept (an operator edit to them survives a replay); a caller
    /// that means to replace them uses `--todo-edit`.
    pub sub_items_kept: bool,
}

/// `#todoids`: tracked-work evidence a todo id is evaluated against.
///
/// Built by the caller from the document (and, where available, its done
/// archive): `open` maps each open tracked-work id to its component
/// (`review`, `backlog`, `icebox`), `resolved` holds ids of finished work. An
/// id present in both is OPEN: contradictory evidence never counts as
/// resolution. An id in neither is unknown (a minted todo id, a GitHub issue
/// number, prose) and never drives an automatic action.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TodoLinkIndex {
    open: std::collections::BTreeMap<String, String>,
    resolved: std::collections::BTreeSet<String>,
    /// Todo ids that actually exist in the current document. Backlink metadata
    /// naming an absent Todo is dangling and does not satisfy the invariant.
    todo_ids: std::collections::BTreeSet<String>,
    /// Durable reverse edges: tracked-work id -> todo own ids. The edge may be
    /// carried by the tracked item or resolved from a Todo source locator.
    todo_backlinks: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    /// `#todoaddonsamecycledone`: open ids the CURRENT closeout resolves
    /// (`--done` / `--review-resolve`). They stay open evidence (the item is
    /// still `[ ]` when the closeout's todo ops run), but a todo is never
    /// LAYERED on one: its operator prompt would be checked off by this same
    /// closeout, and a replay would no longer recognise the layered row.
    resolving_this_closeout: std::collections::BTreeSet<String>,
}

fn normalize_link_id(id: &str) -> String {
    id.trim().trim_start_matches('#').to_ascii_lowercase()
}

impl TodoLinkIndex {
    /// Record open tracked work `id` in `component` (first component wins).
    pub fn mark_open(&mut self, id: &str, component: &str) {
        let id = normalize_link_id(id);
        if !id.is_empty() {
            self.open.entry(id).or_insert_with(|| component.to_string());
        }
    }

    /// Record finished tracked work `id`.
    pub fn mark_resolved(&mut self, id: &str) {
        let id = normalize_link_id(id);
        if !id.is_empty() {
            self.resolved.insert(id);
        }
    }

    /// `#todoaddonsamecycledone`: record that the current closeout resolves
    /// open tracked work `id`. A `--todo-add` evaluated in that closeout never
    /// layers on it (no `(source: ...)` locator, no fold into its open todo):
    /// the add mints a standalone follow-up that outlives the item.
    pub fn mark_resolving_this_closeout(&mut self, id: &str) {
        let id = normalize_link_id(id);
        if !id.is_empty() {
            self.resolving_this_closeout.insert(id);
        }
    }

    /// The component holding open tracked work `id`, if any.
    pub fn open_component(&self, id: &str) -> Option<&str> {
        self.open.get(&normalize_link_id(id)).map(String::as_str)
    }

    /// Whether `id` is resolved and not also open.
    pub fn is_resolved(&self, id: &str) -> bool {
        let id = normalize_link_id(id);
        !self.open.contains_key(&id) && self.resolved.contains(&id)
    }

    /// Whether the index has any evidence about `id`.
    pub fn knows(&self, id: &str) -> bool {
        let id = normalize_link_id(id);
        self.open.contains_key(&id) || self.resolved.contains(&id)
    }

    /// Every id the index knows, open or resolved.
    pub fn known_ids(&self) -> impl Iterator<Item = &String> {
        self.open.keys().chain(self.resolved.iter())
    }

    /// Record that tracked work `tracked_id` completes or unblocks `todo_id`.
    pub fn mark_todo_backlink(&mut self, tracked_id: &str, todo_id: &str) {
        let tracked_id = normalize_link_id(tracked_id);
        let todo_id = normalize_link_id(todo_id);
        if tracked_id.is_empty() || todo_id.is_empty() {
            return;
        }
        self.todo_backlinks
            .entry(tracked_id)
            .or_default()
            .insert(todo_id);
    }

    /// Read canonical `(todos: [#id] ...)` metadata from tracked-work text.
    pub fn mark_todo_backlinks_from_text(&mut self, tracked_id: &str, text: &str) {
        for todo_id in tracked_work_todo_ids(text) {
            self.mark_todo_backlink(tracked_id, &todo_id);
        }
    }

    /// Resolve backlinks from canonical Todo source locators and the legacy
    /// same-id layering convention without rewriting source rows.
    pub fn resolve_todo_backlinks_from_document(&mut self, content: &str) -> Result<()> {
        let Some(todo) = find_todo_component(content)? else {
            return Ok(());
        };
        let body = TodoBody::parse(todo.content(content));
        for item in body.items {
            let Some(todo_id) = item.own_id.as_deref() else {
                continue;
            };
            self.todo_ids.insert(normalize_link_id(todo_id));
            if self.knows(todo_id) {
                self.mark_todo_backlink(todo_id, todo_id);
            }
            for tracked_id in canonical_source_ids(&item.action) {
                if self.knows(&tracked_id) {
                    self.mark_todo_backlink(&tracked_id, todo_id);
                }
            }
        }
        Ok(())
    }

    /// Todo ids linked from one tracked-work item.
    pub fn todo_ids_for_tracked(&self, tracked_id: &str) -> Vec<String> {
        self.todo_backlinks
            .get(&normalize_link_id(tracked_id))
            .map(|ids| ids.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Whether a Todo with `id` actually exists in the current document.
    pub fn has_todo(&self, id: &str) -> bool {
        self.todo_ids.contains(&normalize_link_id(id))
    }

    /// Open backlog/icebox sources whose reverse edges resolve to no actual
    /// Todo. Review gates are excluded: they are operator-decision sources,
    /// while queue/backlog/icebox own the mandatory tracked-work invariant.
    fn missing_todo_backlinks(&self) -> Vec<(String, String)> {
        self.open
            .iter()
            .filter(|(_, component)| {
                matches!(component.as_str(), "backlog" | "pending" | "icebox")
            })
            .filter(|(tracked_id, _)| {
                !self
                    .todo_ids_for_tracked(tracked_id)
                    .iter()
                    .any(|todo_id| self.has_todo(todo_id))
            })
            .map(|(id, component)| (id.clone(), component.clone()))
            .collect()
    }

    /// Source projections used by completion and prerequisite findings.
    fn sources_for_todo(&self, todo_id: &str) -> Vec<(String, Option<String>, bool)> {
        let todo_id = normalize_link_id(todo_id);
        self.todo_backlinks
            .iter()
            .filter(|(_, todos)| todos.contains(&todo_id))
            .map(|(tracked_id, _)| {
                (
                    tracked_id.clone(),
                    self.open.get(tracked_id).cloned(),
                    self.is_resolved(tracked_id),
                )
            })
            .collect()
    }

    /// A non-empty backlink set completes only when every source is terminal.
    fn linked_sources_resolved(&self, todo_id: &str) -> bool {
        let sources = self.sources_for_todo(todo_id);
        !sources.is_empty() && sources.iter().all(|(_, _, resolved)| *resolved)
    }

    /// The component an OPEN `id` may be linked from by a todo: `review`,
    /// `backlog` (legacy `pending` counts as backlog), or `icebox`
    /// (`#todoidlayer`: a parked item can still need an operator decision —
    /// thaw it, drop it — and the todo is the visibility layer over it).
    fn linkable_component(&self, id: &str) -> Option<&str> {
        if self
            .resolving_this_closeout
            .contains(&normalize_link_id(id))
        {
            return None;
        }
        self.open_component(id)
            .filter(|component| matches!(*component, "review" | "backlog" | "pending" | "icebox"))
    }
}

/// Parse canonical source-side backlink metadata
/// `(todos: [#todo-a] [#todo-b])`. Ordinary citations are ignored.
pub fn tracked_work_todo_ids(text: &str) -> Vec<String> {
    let Some(start) = text.rfind("(todos:") else {
        return Vec::new();
    };
    let tail = &text[start + "(todos:".len()..];
    let Some(end) = tail.find(')') else {
        return Vec::new();
    };
    if !tail[end + 1..].trim().is_empty() {
        return Vec::new();
    }
    let mut ids = Vec::new();
    let mut rest = &tail[..end];
    while let Some(open) = rest.find("[#") {
        rest = &rest[open + 2..];
        let Some(close) = rest.find(']') else {
            return Vec::new();
        };
        let id = normalize_link_id(&rest[..close]);
        if !is_valid_todo_id(&id) {
            return Vec::new();
        }
        if !ids.contains(&id) {
            ids.push(id);
        }
        rest = &rest[close + 1..];
    }
    ids
}

/// Tracked ids in canonical Todo-side source locators. Plain citations do not
/// establish automatic-completion edges.
fn canonical_source_ids(action: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = action;
    while let Some(start) = rest.find("(source: agent:") {
        rest = &rest[start + "(source: agent:".len()..];
        let Some(close) = rest.find(')') else {
            break;
        };
        let locator = &rest[..close];
        if let Some(id_start) = locator.find("[#") {
            let after = &locator[id_start + 2..];
            if let Some(id_end) = after.find(']') {
                let id = normalize_link_id(&after[..id_end]);
                if is_valid_todo_id(&id) && !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        rest = &rest[close + 1..];
    }
    ids
}

/// Every valid todo-id token cited in `action`, lowercase, in first-seen order.
fn cited_ids(action: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (start, _) in action.match_indices('#') {
        let id: String = action[start + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect::<String>()
            .to_ascii_lowercase();
        if is_valid_todo_id(&id) && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// Normalize one `--todo-add` action into its checklist text.
///
/// A caller may pass a bare action or an already-formatted `- [ ] action`
/// line; both yield the same action text.
pub fn normalize_todo_action(text: &str) -> Result<String> {
    normalize_todo_action_for(text, "--todo-add")
}

fn normalize_todo_action_for(text: &str, flag: &str) -> Result<String> {
    let trimmed = text.trim();
    anyhow::ensure!(
        !trimmed.contains('\n'),
        "{flag} expects one single-line operator action, got multi-line text"
    );
    let action = checklist_item_text(trimmed)
        .or_else(|| trimmed.strip_prefix("- "))
        .unwrap_or(trimmed)
        .trim();
    // A bare checkbox (`- [ ]`, `[x]`) carries no action.
    let action = match action {
        "[ ]" | "[x]" | "[/]" => "",
        other => other,
    };
    anyhow::ensure!(
        !action.is_empty(),
        "{flag} expects a non-empty operator action"
    );
    Ok(action.to_string())
}

/// One parsed add/edit input: optional caller-chosen id, the item's action,
/// and its nested sub-items (`#nestedtodoadd`).
struct TodoInput {
    id: Option<String>,
    action: String,
    /// Sub-item texts, without indentation or the `- ` bullet, in order.
    children: Vec<String>,
}

/// `#nestedtodoadd`: split add/edit text into the item line and its nested
/// sub-items.
///
/// The first line is the item. Every later line must be one nested list item,
/// `<indent>- <text>`, with the same non-empty indentation as its siblings
/// (one level only) and single-line, non-empty text. Rejected, so nothing is
/// silently split into several items or written outside the item: blank
/// lines, flush-left or non-bullet lines, mixed indentation, checkbox children
/// (`- [ ] ...`, which would parse as separate todo items), and HTML comments
/// or component markers.
fn split_todo_payload<'a>(text: &'a str, flag: &str) -> Result<(&'a str, Vec<String>)> {
    let trimmed = text.trim_end().trim_start_matches(['\n', '\r']);
    let mut lines = trimmed.lines();
    let first = lines.next().unwrap_or("");
    let mut children = Vec::new();
    let mut sibling_indent: Option<&str> = None;
    for (offset, line) in lines.enumerate() {
        let number = offset + 2;
        anyhow::ensure!(
            !line.trim().is_empty(),
            "{flag}: line {number} is blank; nested sub-items follow the item line directly, one `  - <text>` line each"
        );
        let body = line.trim_start();
        let indent = &line[..line.len() - body.len()];
        // A bare `-` is an empty sub-item (trailing spaces were trimmed).
        let child = body
            .strip_prefix("- ")
            .map(str::trim)
            .or((body == "-").then_some(""));
        anyhow::ensure!(
            !indent.is_empty() && child.is_some(),
            "{flag}: line {number} ({line:?}) is not a nested sub-item; every line after the first must be an indented list item, `  - <text>`"
        );
        let child = child.unwrap_or_default();
        match sibling_indent {
            None => sibling_indent = Some(indent),
            Some(expected) => anyhow::ensure!(
                expected == indent,
                "{flag}: line {number} ({line:?}) changes indentation; nested sub-items are one level deep, all at the same indent"
            ),
        }
        anyhow::ensure!(
            !child.is_empty(),
            "{flag}: line {number} is an empty sub-item"
        );
        anyhow::ensure!(
            !["[ ]", "[x]", "[X]", "[/]"]
                .iter()
                .any(|checkbox| child == *checkbox || child.starts_with(&format!("{checkbox} "))),
            "{flag}: line {number} ({line:?}) is a checkbox row; a sub-item is plain text (`  - <text>`), since a checkbox row is a separate todo item"
        );
        anyhow::ensure!(
            !child.contains("<!--") && !child.contains("-->"),
            "{flag}: line {number} ({line:?}) carries an HTML comment or component marker; sub-items are plain text"
        );
        children.push(child.to_string());
    }
    Ok((first, children))
}

/// Split an add/edit input into an optional caller-chosen id, the action, and
/// its nested sub-items ([`split_todo_payload`]).
///
/// The id spellings mirror `--backlog-add`: canonical `id=<id> <action>`, or
/// the compatibility form `[#<id>] <action>`. Both belong to the first line.
fn parse_todo_input(text: &str, flag: &str) -> Result<TodoInput> {
    let (first, children) = split_todo_payload(text, flag)?;
    let action = normalize_todo_action_for(first, flag)?;
    if let Some(rest) = action.strip_prefix("id=") {
        let (id, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        anyhow::ensure!(
            is_valid_todo_id(id),
            "{flag}: invalid todo id {id:?} (use letters, digits, and '-')"
        );
        let tail = tail.trim();
        anyhow::ensure!(!tail.is_empty(), "{flag} expects an action after id={id}");
        return Ok(TodoInput {
            id: Some(id.to_ascii_lowercase()),
            action: tail.to_string(),
            children,
        });
    }
    if let Some((id, tail)) = split_leading_bracket_id(&action) {
        anyhow::ensure!(!tail.is_empty(), "{flag} expects an action after [#{id}]");
        return Ok(TodoInput {
            id: Some(id.to_ascii_lowercase()),
            action: tail.to_string(),
            children,
        });
    }
    Ok(TodoInput {
        id: None,
        action,
        children,
    })
}

/// Render nested sub-items under an item whose own line is indented `indent`
/// columns: one `<indent+2>- <text>\n` line each.
fn render_todo_children(indent: usize, children: &[String]) -> String {
    let pad = " ".repeat(indent + 2);
    children
        .iter()
        .map(|child| format!("{pad}- {child}\n"))
        .collect()
}

/// The trimmed continuation lines of the item at `items[index]` (its nested
/// sub-items and any operator notes under it), for comparing against an add's
/// requested sub-items.
fn existing_children(body: &TodoBody, index: usize) -> Vec<String> {
    let item = &body.items[index];
    let span = body.item_span(item);
    body.lines[span.start + 1..span.end]
        .iter()
        .map(|line| {
            let line = line.trim();
            line.strip_prefix("- ").unwrap_or(line).trim().to_string()
        })
        .collect()
}

/// Collapse whitespace so two spellings of one action compare equal.
fn normalize_action_text(action: &str) -> String {
    action.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Make the tracked-work source of a singly linked todo visible in the checklist.
///
/// A leading todo id and an unqualified `#id` citation are identity/reference
/// tokens, not enough explanatory context. Every todo with exactly one open
/// tracked-work source carries one canonical component/id locator. A legacy
/// locator with the wrong component is replaced in place.
fn action_with_source_reference(action: &str, id: &str, linked: Option<&str>) -> String {
    let Some(component) = linked else {
        return action.to_string();
    };
    let suffix = format!("(source: agent:{component} [#{id}])");
    if action.ends_with(&suffix) {
        return action.to_string();
    }
    let base = strip_trailing_source_reference(action).unwrap_or(action).trim_end();
    format!("{base} {suffix}")
}

/// Strip the canonical source-locator shape from the end of an action.
///
/// This intentionally recognizes only the binary-owned spelling. Free-form
/// prose containing `source:` is operator text and must not be rewritten.
fn strip_trailing_source_reference(action: &str) -> Option<&str> {
    let marker = " (source: agent:";
    let start = action.rfind(marker)?;
    let tail = &action[start + marker.len()..];
    let (component, id_tail) = tail.split_once(" [#")?;
    let id = id_tail.strip_suffix("])")?;
    if component.is_empty()
        || !component
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        || !is_valid_todo_id(id)
    {
        return None;
    }
    Some(&action[..start])
}

/// Every `#id`-shaped token in `body`, lowercase.
fn body_id_tokens(body: &str) -> std::collections::HashSet<String> {
    let mut ids = std::collections::HashSet::new();
    for (start, _) in body.match_indices('#') {
        let id: String = body[start + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if !id.is_empty() {
            ids.insert(id.to_ascii_lowercase());
        }
    }
    ids
}

/// Append one operator action to `agent:todo`, creating the component when
/// absent and preserving every existing line.
///
/// The new item gets a stable own id, `- [ ] [#<id>] <action>`, so later
/// `--todo-done`/`--todo-edit` calls (and the keyed document merge) address it
/// by identity rather than by its text (`#todoids`):
/// - `#todoaddidhijack`: an explicit `id=<id>` / `[#<id>]` prefix ALWAYS
///   wins — `#<id>` citations in the action never replace it. It is rejected when
///   a todo already owns it, when it names tracked work `links` reports
///   resolved, and when it names an OPEN review/backlog/icebox item: a todo
///   never takes the id an open tracked item owns;
/// - otherwise, when the action cites exactly one OPEN `agent:review` /
///   `agent:backlog` / `agent:icebox` id and an OPEN todo is already linked to
///   it (canonical source locator, or a legacy row sharing the id), the add is
///   a no-op;
/// - otherwise `id_for(action, counter)` derives candidates and the first one
///   not cited anywhere in the document, not in `reserved` (the document's
///   tracked-work ids), and unknown to `links` wins.
///
/// Whatever the own id, an action citing exactly one open tracked item gets
/// the canonical `(source: agent:<component> [#<id>])` locator, the same one
/// preflight's `#todocontextmigration` would add.
pub fn add_todo_item_to_document(
    content: &str,
    text: &str,
    id_for: &dyn Fn(&str, u64) -> String,
    reserved: &std::collections::HashSet<String>,
    links: &TodoLinkIndex,
) -> Result<TodoAddOutcome> {
    let TodoInput {
        id: explicit_id,
        action,
        children,
    } = parse_todo_input(text, "--todo-add")?;
    // `#queueoperatoraction`: request packets deliberately own an identity
    // distinct from the tracked work whose queue row they pause. Preserve that
    // reserved explicit id and leave the action as written (no source
    // locator), even when it cites exactly one tracked id.
    let explicit_request_id = explicit_id.as_deref().filter(|id| {
        id.to_ascii_lowercase()
            .starts_with("subagent-operator-request-")
    });
    let (ensured, todo, created_component) = ensure_todo_component_in_document(content)?;
    let body = todo.content(&ensured);
    let explicit_linked = explicit_id
        .as_deref()
        .and_then(|id| links.linkable_component(id));
    let contextual_source = explicit_id
        .as_deref()
        .zip(explicit_linked)
        .map(|(id, component)| (normalize_link_id(id), component.to_string()))
        .or_else(|| single_todo_source(None, &action, links));
    let contextual_action = contextual_source
        .as_ref()
        .map(|(id, component)| {
            action_with_source_reference(&action, id, Some(component.as_str()))
        })
        .unwrap_or_else(|| action.clone());
    let wanted = normalize_action_text(&contextual_action);
    let wanted_legacy = normalize_action_text(&action);
    let items: Vec<TodoLine<'_>> = body.lines().filter_map(parse_todo_line).collect();
    // `#nestedtodoadd`: the located view of the same rows (same order), for
    // the nested lines an existing item already carries.
    let located = TodoBody::parse(body);
    // `#todoaddonsamecycledone`: the binary-owned source locator is not part
    // of the action the caller wrote. A replay evaluated after the source was
    // resolved computes no locator, so it must still recognise the row the
    // first pass wrote WITH one; otherwise it mints a second, surrogate copy.
    //
    // `#nestedtodoadd`: identity is the item line alone, so a replayed add
    // with sub-items never duplicates the item, and an operator edit to the
    // nested lines survives the replay (`sub_items_kept` reports a mismatch).
    if let Some(existing_index) = items.iter().position(|item| {
        let existing = normalize_action_text(item.action);
        let existing_bare = strip_trailing_source_reference(item.action)
            .map(normalize_action_text)
            .unwrap_or_else(|| existing.clone());
        existing == wanted || existing == wanted_legacy || existing_bare == wanted_legacy
    }) {
        let existing = &items[existing_index];
        let id = existing.own_id.map(str::to_ascii_lowercase);
        let source = contextual_source.clone();
        let sub_items_kept = !children.is_empty()
            && existing_index < located.items.len()
            && existing_children(&located, existing_index) != children;
        if wanted != wanted_legacy
            && normalize_action_text(existing.action) == wanted_legacy
            && id.as_deref() == contextual_source.as_ref().map(|(id, _)| id.as_str())
        {
            let old_line = render_todo_line(
                existing.prefix,
                existing.state,
                existing.own_id,
                existing.own_id_bare,
                existing.action,
            );
            let new_line = render_todo_line(
                existing.prefix,
                existing.state,
                existing.own_id,
                existing.own_id_bare,
                &contextual_action,
            );
            let new_body = body.replacen(&old_line, &new_line, 1);
            return Ok(TodoAddOutcome {
                content: todo.replace_content(&ensured, &new_body),
                inserted: true,
                source_reference_added: true,
                created_component,
                id,
                source,
                sub_items_kept,
            });
        }
        return Ok(TodoAddOutcome {
            content: ensured.clone(),
            inserted: false,
            source_reference_added: false,
            created_component,
            id,
            source,
            sub_items_kept,
        });
    }
    let owner = |id: &str| {
        items
            .iter()
            .find(|item| item.own_id.is_some_and(|own| own.eq_ignore_ascii_case(id)))
    };
    if let Some(id) = explicit_id.as_deref() {
        anyhow::ensure!(
            owner(id).is_none(),
            "--todo-add: agent:todo already has an item with id [#{id}]; use --todo-edit \"{id}=<text>\" to change it"
        );
        anyhow::ensure!(
            !links.is_resolved(id),
            "--todo-add: [#{id}] names tracked work that is already resolved, so an operator todo for it is moot (#todoids). Cite it in a new action without id= to get a fresh id, or reopen the tracked item first"
        );
        // `#todoaddidhijack`: one id names one row. A todo never takes the id
        // an open tracked item already owns; it cites the item instead.
        if explicit_request_id.is_none()
            && let Some(component) = links.open_component(id)
        {
            anyhow::bail!(
                "--todo-add: id={id} is already owned by the open agent:{component} item [#{id}]; a todo never takes a tracked item's id (#todoaddidhijack). Pass a new id (`id=<todo-id> ... [#{id}] ...`), or drop `id=` and cite [#{id}] in the action to get a fresh id plus `(source: agent:{component} [#{id}])`"
            );
        }
    }
    // `#todoaddidhijack`: a todo's own id is never taken from tracked work.
    // An explicit `id=` always wins; a `#<id>` citation in the action stays a
    // reference. When the action cites exactly one open review/backlog/icebox
    // item, the row links to it through the canonical
    // `(source: agent:<component> [#<id>])` locator instead, which is the edge
    // auto-completion and the operator-done signals read.
    let source = if explicit_request_id.is_some() {
        None
    } else {
        contextual_source.clone()
    };
    let id = match explicit_id.clone() {
        Some(id) => id,
        None => {
            // `#todoids`: one open todo per tracked source — a second add for
            // the same item folds into the open todo already linked to it
            // (its canonical locator, or a legacy row that shares the id).
            if let Some((source_id, _)) = source.as_ref()
                && let Some(existing_index) = items.iter().position(|item| {
                    item.state.is_open() && todo_line_links_source(item, source_id)
                })
            {
                let sub_items_kept = !children.is_empty()
                    && existing_index < located.items.len()
                    && existing_children(&located, existing_index) != children;
                return Ok(TodoAddOutcome {
                    content: ensured.clone(),
                    inserted: false,
                    source_reference_added: false,
                    created_component,
                    id: items[existing_index].own_id.map(str::to_ascii_lowercase),
                    source,
                    sub_items_kept,
                });
            }
            let taken = body_id_tokens(&ensured);
            let mut chosen = None;
            for counter in 0..256u64 {
                let candidate = id_for(&action, counter).to_ascii_lowercase();
                if is_valid_todo_id(&candidate)
                    && !taken.contains(&candidate)
                    && !reserved.contains(&candidate)
                    && !links.knows(&candidate)
                {
                    chosen = Some(candidate);
                    break;
                }
            }
            chosen.context("--todo-add could not derive a collision-free todo id")?
        }
    };
    let action = match source.as_ref() {
        Some((source_id, component)) => {
            action_with_source_reference(&action, source_id, Some(component.as_str()))
        }
        None => action,
    };
    let mut new_body = String::with_capacity(body.len() + action.len() + 16);
    new_body.push_str(body);
    if !new_body.is_empty() && !new_body.ends_with('\n') {
        new_body.push('\n');
    }
    new_body.push_str(&render_todo_line(
        "- ",
        TodoState::Open,
        Some(&id),
        false,
        &action,
    ));
    new_body.push('\n');
    new_body.push_str(&render_todo_children(0, &children));
    Ok(TodoAddOutcome {
        content: todo.replace_content(&ensured, &new_body),
        inserted: true,
        source_reference_added: false,
        created_component,
        id: Some(id),
        source,
        sub_items_kept: false,
    })
}

/// One own id [`assign_missing_todo_ids`] gave an id-less todo row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoIdAssignment {
    /// The new own id, always minted (`#todoaddidhijack`).
    pub id: String,
    /// The row's action text, unchanged.
    pub action: String,
}

/// Result of [`assign_missing_todo_ids`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoIdAssignOutcome {
    /// The document after the assignment (unchanged when nothing was assigned).
    pub content: String,
    /// The rows that received an own id, in item order.
    pub assigned: Vec<TodoIdAssignment>,
}

/// `#todoidbackfill`: give every id-less `agent:todo` row an own id, by the
/// same rules `--todo-add` uses, so a row the operator typed by hand (or an
/// older binary or a `patch:todo` wrote) can be addressed and completed by id.
///
/// - A row that already has an own id (`[#id]` or bare `#id`) is never re-id'd.
/// - `admit(line)` is the typing gate: a row it rejects (still being typed) is
///   left for a later pass. The line is passed trimmed, list marker included.
/// - Every assigned id is minted with `id_for`, avoiding every `#id` token in
///   the document, `reserved`, every id `links` knows, and ids assigned
///   earlier in this pass (`#todoaddidhijack`: a row never takes a tracked
///   item's id). A row citing exactly one open review/backlog/icebox item is
///   linked to it by [`qualify_todo_source_references`], so the `#todoids`
///   signals still apply (an operator check reports the prerequisite done;
///   resolving the work checks the row off).
/// - Rows whose action text duplicates another row's are skipped: duplicate
///   folding (`#tododupdone`) handles them, and two distinct ids would make the
///   copies permanent.
/// - Only the id token is inserted (`- [ ] [#id] <action>`); the checkbox, list
///   marker, indentation, and action text are kept byte-for-byte.
///
/// Pure and deterministic: the same inputs always yield the same ids.
pub fn assign_missing_todo_ids(
    content: &str,
    id_for: &dyn Fn(&str, u64) -> String,
    reserved: &std::collections::HashSet<String>,
    links: &TodoLinkIndex,
    admit: &dyn Fn(&str) -> bool,
) -> Result<TodoIdAssignOutcome> {
    let unchanged = |content: &str| TodoIdAssignOutcome {
        content: content.to_string(),
        assigned: Vec::new(),
    };
    let Some(todo) = find_todo_component(content)? else {
        return Ok(unchanged(content));
    };
    let mut body = TodoBody::parse(todo.content(content));
    let mut owned: std::collections::HashSet<String> = body
        .items
        .iter()
        .filter_map(|item| item.own_id.clone())
        .collect();
    let mut taken = body_id_tokens(content);
    let mut text_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for item in &body.items {
        *text_counts
            .entry(normalize_action_text(&item.action))
            .or_default() += 1;
    }
    let mut assigned = Vec::new();
    for item in body.items.clone() {
        if item.own_id.is_some() {
            continue;
        }
        let normalized = normalize_action_text(&item.action);
        if normalized.is_empty() || text_counts.get(&normalized).copied().unwrap_or(0) > 1 {
            continue;
        }
        let line = body.lines[item.line].trim_end_matches(['\n', '\r']).trim();
        if !admit(line) {
            continue;
        }
        // `#todoaddidhijack`: a backfilled row never takes a tracked item's
        // id; a single cited source is linked by the canonical locator that
        // `qualify_todo_source_references` appends.
        let mut chosen = None;
        for counter in 0..256u64 {
            let candidate = id_for(&item.action, counter).to_ascii_lowercase();
            if is_valid_todo_id(&candidate)
                && !taken.contains(&candidate)
                && !reserved.contains(&candidate)
                && !links.knows(&candidate)
                && !owned.contains(&candidate)
            {
                chosen = Some(candidate);
                break;
            }
        }
        let id = chosen.context("todo id backfill could not derive a collision-free todo id")?;
        body.rewrite_line(item.line, item.state, Some(&id), &item.action);
        owned.insert(id.clone());
        taken.insert(id.clone());
        assigned.push(TodoIdAssignment {
            id,
            action: item.action.clone(),
        });
    }
    if assigned.is_empty() {
        return Ok(unchanged(content));
    }
    Ok(TodoIdAssignOutcome {
        content: todo.replace_content(content, &body.render()),
        assigned,
    })
}

/// One legacy todo row whose single tracked-work source is not visibly
/// component-qualified in its action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoSourceQualification {
    /// Selector suitable for `--todo-*`: own id when present, else action text.
    pub selector: String,
    /// Canonical tracked-work id.
    pub id: String,
    /// Canonical source component (`review`, `backlog`, `pending`, or `icebox`).
    pub component: String,
    /// Action before qualification.
    pub action: String,
    /// Canonical action after qualification.
    pub qualified_action: String,
}

/// Result of [`qualify_todo_source_references`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoSourceQualificationOutcome {
    /// Document after admitted legacy rows were qualified.
    pub content: String,
    /// Every legacy singly linked row found, including rows held by `admit`.
    pub findings: Vec<TodoSourceQualification>,
    /// Selectors of findings actually rewritten in `content`.
    pub qualified: Vec<String>,
}

/// Whether todo row `item` is linked to tracked work `source_id`: its
/// canonical `(source: agent:<component> [#<id>])` locator names it, or it is a
/// legacy row whose own id IS that id (written before `#todoaddidhijack`).
fn todo_line_links_source(item: &TodoLine<'_>, source_id: &str) -> bool {
    item.own_id
        .is_some_and(|own| normalize_link_id(own) == source_id)
        || canonical_source_ids(item.action)
            .iter()
            .any(|id| id == source_id)
}

/// Return the one open tracked-work source represented by a todo row.
///
/// The row may link explicitly by owning the tracked id or implicitly by
/// citing it in its action. More than one distinct tracked id is ambiguous and
/// intentionally has no canonical single source.
fn single_todo_source(
    own_id: Option<&str>,
    action: &str,
    links: &TodoLinkIndex,
) -> Option<(String, String)> {
    let mut candidates: Vec<(String, String)> = Vec::new();
    if let Some(own) = own_id
        && let Some(component) = links.linkable_component(own)
    {
        candidates.push((normalize_link_id(own), component.to_string()));
    }
    for id in cited_ids(action) {
        let Some(component) = links.linkable_component(&id) else {
            continue;
        };
        if !candidates.iter().any(|(seen, _)| seen == &id) {
            candidates.push((id, component.to_string()));
        }
    }
    match candidates.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    }
}

/// Audit and canonically qualify every legacy singly linked `agent:todo` row.
///
/// Both explicit links (the todo owns an open tracked-work id) and implicit
/// links (the action cites exactly one open tracked-work id) receive the same
/// visible `(source: agent:<component> [#<id>])` suffix. Existing canonical
/// rows are no-ops. `admit` is the typing gate: rejected rows remain unchanged
/// but still appear in `findings`, so callers can surface an audit result and
/// retry safely on a later pass.
pub fn qualify_todo_source_references(
    content: &str,
    links: &TodoLinkIndex,
    admit: &dyn Fn(&str) -> bool,
) -> Result<TodoSourceQualificationOutcome> {
    let unchanged = |content: &str| TodoSourceQualificationOutcome {
        content: content.to_string(),
        findings: Vec::new(),
        qualified: Vec::new(),
    };
    let Some(todo) = find_todo_component(content)? else {
        return Ok(unchanged(content));
    };
    let mut body = TodoBody::parse(todo.content(content));
    let mut findings = Vec::new();
    let mut qualified = Vec::new();
    for item in body.items.clone() {
        let Some((id, component)) =
            single_todo_source(item.own_id.as_deref(), &item.action, links)
        else {
            continue;
        };
        let qualified_action =
            action_with_source_reference(&item.action, &id, Some(&component));
        if normalize_action_text(&qualified_action) == normalize_action_text(&item.action) {
            continue;
        }
        let selector = item.own_id.clone().unwrap_or_else(|| item.action.clone());
        findings.push(TodoSourceQualification {
            selector: selector.clone(),
            id,
            component,
            action: item.action.clone(),
            qualified_action: qualified_action.clone(),
        });
        let line = body.lines[item.line].trim_end_matches(['\n', '\r']).trim();
        if admit(line) {
            body.rewrite_line(
                item.line,
                item.state,
                item.own_id.as_deref(),
                &qualified_action,
            );
            qualified.push(selector);
        }
    }
    let updated = if qualified.is_empty() {
        content.to_string()
    } else {
        todo.replace_content(content, &body.render())
    };
    Ok(TodoSourceQualificationOutcome {
        content: updated,
        findings,
        qualified,
    })
}

/// `#todoidbackfill`: carry the ids [`assign_missing_todo_ids`] gave `before`'s
/// todo rows into `snapshot`, when the snapshot's todo body still equals
/// `before`'s (no unsnapshotted operator todo edit). `None` when there is
/// nothing to carry or the bodies differ, so a pending operator edit stays
/// visible as a diff.
pub fn carry_todo_body_into_snapshot(snapshot: &str, before: &str, after: &str) -> Option<String> {
    let snap_todo = find_todo_component(snapshot).ok()??;
    let before_todo = find_todo_component(before).ok()??;
    let after_todo = find_todo_component(after).ok()??;
    let after_body = after_todo.content(after);
    if snap_todo.content(snapshot) != before_todo.content(before)
        || snap_todo.content(snapshot) == after_body
    {
        return None;
    }
    Some(snap_todo.replace_content(snapshot, after_body))
}

/// One checklist item located in a todo body, by line index.
#[derive(Debug, Clone)]
struct LocatedItem {
    line: usize,
    state: TodoState,
    own_id: Option<String>,
    action: String,
    indent: usize,
}

/// Body split into lines that keep their terminators, plus the parsed items.
struct TodoBody {
    lines: Vec<String>,
    items: Vec<LocatedItem>,
}

fn line_indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

impl TodoBody {
    fn parse(body: &str) -> Self {
        let lines: Vec<String> = body.split_inclusive('\n').map(str::to_string).collect();
        let items = lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| {
                let bare = line.strip_suffix('\n').unwrap_or(line);
                let parsed = parse_todo_line(bare)?;
                Some(LocatedItem {
                    line: index,
                    state: parsed.state,
                    own_id: parsed.own_id.map(str::to_ascii_lowercase),
                    action: parsed.action.to_string(),
                    indent: line_indent(bare),
                })
            })
            .collect();
        Self { lines, items }
    }

    fn render(&self) -> String {
        self.lines.concat()
    }

    /// Line indices of `item` and its continuation lines: the following
    /// non-blank lines indented deeper than the item itself.
    fn item_span(&self, item: &LocatedItem) -> std::ops::Range<usize> {
        let mut end = item.line + 1;
        while end < self.lines.len() {
            let line = self.lines[end].trim_end_matches(['\n', '\r']);
            if line.trim().is_empty() || line_indent(line) <= item.indent {
                break;
            }
            end += 1;
        }
        item.line..end
    }

    fn rewrite_line(&mut self, index: usize, state: TodoState, own_id: Option<&str>, action: &str) {
        let original = &self.lines[index];
        let bare = original.trim_end_matches(['\n', '\r']);
        let ending = &original[bare.len()..];
        let parsed = parse_todo_line(bare);
        let prefix = parsed.map(|line| line.prefix).unwrap_or("- ");
        // `#sdktododone`: keep the row's own id spelling. A bare `#id` row
        // stays bare when the rewrite keeps that same id, so `--todo-done`
        // flips only the checkbox and `--todo-edit` keeps the id it replaces
        // the action after.
        let keep_bare = parsed.is_some_and(|line| {
            line.own_id_bare
                && line
                    .own_id
                    .zip(own_id)
                    .is_some_and(|(was, now)| was.eq_ignore_ascii_case(now))
        });
        let own_id = if keep_bare {
            parsed.and_then(|line| line.own_id)
        } else {
            own_id
        };
        let mut rendered = render_todo_line(prefix, state, own_id, keep_bare, action);
        rendered.push_str(ending);
        self.lines[index] = rendered;
    }
}

/// Human-readable handle for one item in receipts: `#id`, else its action.
fn item_label(own_id: Option<&str>, action: &str) -> String {
    match own_id {
        Some(id) => format!("#{id}"),
        None => {
            let short: String = action.chars().take(60).collect();
            if short.len() < action.len() {
                format!("\"{short}...\"")
            } else {
                format!("\"{short}\"")
            }
        }
    }
}

/// The id a selector names, when it is id-shaped (`id`, `#id`, `[#id]`).
fn selector_id(selector: &str) -> Option<String> {
    let trimmed = selector.trim();
    let unbracketed = trimmed
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(trimmed);
    let id = unbracketed.strip_prefix('#').unwrap_or(unbracketed);
    is_valid_todo_id(id).then(|| id.to_ascii_lowercase())
}

/// Whether `action` cites `#id` as a whole id token.
fn action_references_id(action: &str, id: &str) -> bool {
    body_references_id(&action.to_ascii_lowercase(), id)
}

/// Resolve a todo selector to the index (into `body.items`) of one item.
///
/// Resolution order, first match wins:
/// 1. the item whose own `[#id]` equals the selector;
/// 2. the item whose action cites `#id` (the `... (#gate)` convention),
///    narrowed to items in `prefer` state when several cite it;
/// 3. the item whose action text equals the selector (id-less items).
///
/// Anything but exactly one candidate at the deciding step is an error that
/// names the candidates, so a selector never silently picks one of several.
fn resolve_todo_selector(
    body: &TodoBody,
    selector: &str,
    prefer: Option<fn(TodoState) -> bool>,
    flag: &str,
) -> Result<usize> {
    let trimmed = selector.trim();
    anyhow::ensure!(
        !trimmed.is_empty(),
        "{flag} expects a todo id or action text"
    );
    let pick = |candidates: Vec<usize>, how: &str| -> Result<Option<usize>> {
        match candidates.len() {
            0 => Ok(None),
            1 => Ok(Some(candidates[0])),
            _ => {
                let labels = candidates
                    .iter()
                    .map(|&i| {
                        let item = &body.items[i];
                        item_label(item.own_id.as_deref(), &item.action)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                anyhow::bail!(
                    "{flag} {trimmed:?} is ambiguous: {} agent:todo items {how} ({labels}); select one by its own [#id] or its exact text",
                    candidates.len()
                )
            }
        }
    };
    if let Some(id) = selector_id(trimmed) {
        let by_own: Vec<usize> = (0..body.items.len())
            .filter(|&i| body.items[i].own_id.as_deref() == Some(id.as_str()))
            .collect();
        if let Some(found) = pick(by_own, "carry that id")? {
            return Ok(found);
        }
        let mut by_ref: Vec<usize> = (0..body.items.len())
            .filter(|&i| action_references_id(&body.items[i].action, &id))
            .collect();
        if by_ref.len() > 1
            && let Some(prefer) = prefer
        {
            let preferred: Vec<usize> = by_ref
                .iter()
                .copied()
                .filter(|&i| prefer(body.items[i].state))
                .collect();
            if !preferred.is_empty() {
                by_ref = preferred;
            }
        }
        if let Some(found) = pick(by_ref, "cite that id")? {
            return Ok(found);
        }
    }
    let wanted = normalize_action_text(
        &normalize_todo_action_for(trimmed, flag).unwrap_or_else(|_| trimmed.to_string()),
    );
    let by_text: Vec<usize> = (0..body.items.len())
        .filter(|&i| {
            let item = &body.items[i];
            normalize_action_text(&item.action) == wanted
                || item.own_id.as_deref().is_some_and(|id| {
                    normalize_action_text(&format!("[#{id}] {}", item.action)) == wanted
                })
        })
        .collect();
    if let Some(found) = pick(by_text, "have that text")? {
        return Ok(found);
    }
    anyhow::bail!("{flag}: no agent:todo item matches {trimmed:?}")
}

/// Result of one `agent:todo` item mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoMutationOutcome {
    /// The document after the mutation (unchanged when `changed` is false).
    pub content: String,
    /// Whether the document changed.
    pub changed: bool,
    /// `#id` or quoted action of the item the selector resolved to.
    pub label: String,
    /// Duplicate copies [`reconcile_todo_body`] folded into the item.
    pub reconciled: usize,
}

fn todo_body_for_mutation(content: &str, flag: &str) -> Result<(Component, TodoBody)> {
    let todo = find_todo_component(content)?
        .with_context(|| format!("{flag}: document has no agent:todo component"))?;
    let body = TodoBody::parse(todo.content(content));
    Ok((todo, body))
}

/// Which way [`set_todo_item_state_in_document`] moves an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoTransition {
    /// `[ ]`/`[/]` → `[x]` (`--todo-done`). A no-op on an `[x]` item.
    Done,
    /// `[x]`/`[/]` → `[ ]` (`--todo-reopen`). A no-op on an `[ ]` item.
    Reopen,
}

/// Check (`--todo-done`) or uncheck (`--todo-reopen`) one todo item in place.
///
/// The item keeps its position, id, and text; only its checkbox changes, so the
/// operator's ordering and wording survive. A repeated call is a no-op. After
/// `--todo-done`, any duplicate copy of the same item elsewhere in the
/// checklist is folded into it ([`reconcile_todo_body`]), so completing an item
/// that an earlier merge duplicated leaves exactly one checked line.
pub fn set_todo_item_state_in_document(
    content: &str,
    selector: &str,
    transition: TodoTransition,
) -> Result<TodoMutationOutcome> {
    let (flag, prefer, target): (&str, fn(TodoState) -> bool, TodoState) = match transition {
        TodoTransition::Done => ("--todo-done", TodoState::is_open, TodoState::Done),
        TodoTransition::Reopen => ("--todo-reopen", |s| !s.is_open(), TodoState::Open),
    };
    let (todo, mut body) = todo_body_for_mutation(content, flag)?;
    let index = resolve_todo_selector(&body, selector, Some(prefer), flag)?;
    let item = body.items[index].clone();
    let label = item_label(item.own_id.as_deref(), &item.action);
    if item.state != target {
        body.rewrite_line(item.line, target, item.own_id.as_deref(), &item.action);
    }
    let mut new_body = body.render();
    let mut reconciled = 0;
    if transition == TodoTransition::Done {
        let (folded, removed) = reconcile_todo_body(&new_body);
        reconciled = removed;
        new_body = folded;
    }
    let changed = new_body != todo.content(content);
    Ok(TodoMutationOutcome {
        content: if changed {
            todo.replace_content(content, &new_body)
        } else {
            content.to_string()
        },
        changed,
        label,
        reconciled,
    })
}

/// Replace one todo item's action text (`--todo-edit "id=text"`).
///
/// The item keeps its position, checkbox state, and own id. Re-applying the
/// same text is a no-op. The new text may repeat the item's own id
/// (`[#id] text`) but cannot change it.
pub fn edit_todo_item_in_document(
    content: &str,
    selector: &str,
    text: &str,
) -> Result<TodoMutationOutcome> {
    let flag = "--todo-edit";
    let (todo, mut body) = todo_body_for_mutation(content, flag)?;
    let index = resolve_todo_selector(&body, selector, None, flag)?;
    let item = body.items[index].clone();
    let TodoInput {
        id: new_id,
        mut action,
        children,
    } = parse_todo_input(text, flag)?;
    // `#sdktododone`: text that repeats a bare own id (`#id new text`) names
    // the same item; drop the repeat so the row does not read `#id #id ...`.
    if new_id.is_none()
        && let Some(own) = item.own_id.as_deref()
        && let Some((repeated, rest)) = split_leading_bare_id(&action)
        && repeated.eq_ignore_ascii_case(own)
        && !rest.is_empty()
    {
        action = rest.to_string();
    }
    if let Some(new_id) = new_id {
        anyhow::ensure!(
            item.own_id.as_deref() == Some(new_id.as_str()),
            "{flag} cannot change a todo item's id (item {}, text names [#{new_id}])",
            item_label(item.own_id.as_deref(), &item.action)
        );
    }
    let label = item_label(item.own_id.as_deref(), &item.action);
    // `#nestedtodoadd`: multi-line edit text replaces the item's nested lines
    // with its sub-items; single-line text keeps them as they are.
    let span = body.item_span(&item);
    body.rewrite_line(item.line, item.state, item.own_id.as_deref(), &action);
    if !children.is_empty() {
        let last = &body.lines[span.end - 1];
        let ends_with_newline = last.ends_with('\n');
        let mut rendered = render_todo_children(item.indent, &children);
        if !ends_with_newline {
            rendered.pop();
        }
        if !body.lines[item.line].ends_with('\n') {
            body.lines[item.line].push('\n');
        }
        body.lines
            .splice(span.start + 1..span.end, std::iter::once(rendered));
    }
    let new_body = body.render();
    let changed = new_body != todo.content(content);
    Ok(TodoMutationOutcome {
        content: if changed {
            todo.replace_content(content, &new_body)
        } else {
            content.to_string()
        },
        changed,
        label,
        reconciled: 0,
    })
}

/// Delete one todo item and its continuation lines (`--todo-remove`).
///
/// Use it for an item that no longer applies; a finished item is checked off
/// with `--todo-done` instead, so the operator can see it was done.
pub fn remove_todo_item_from_document(
    content: &str,
    selector: &str,
) -> Result<TodoMutationOutcome> {
    let flag = "--todo-remove";
    let (todo, mut body) = todo_body_for_mutation(content, flag)?;
    let index = resolve_todo_selector(&body, selector, None, flag)?;
    let item = body.items[index].clone();
    let label = item_label(item.own_id.as_deref(), &item.action);
    let span = body.item_span(&item);
    body.lines.drain(span);
    let new_body = body.render();
    Ok(TodoMutationOutcome {
        content: todo.replace_content(content, &new_body),
        changed: true,
        label,
        reconciled: 0,
    })
}

/// `#sdktododone`: whether an `agent:todo` item in `content` carries `id` as
/// its OWN id (bracketed `[#id]` or bare `#id`), in any checkbox state.
///
/// A closeout's `--done <id>` that names no tracked work but names a todo's
/// own id completes that todo instead of failing with "id not found". An id
/// the todo only cites does not count.
pub fn todo_owns_id(content: &str, id: &str) -> bool {
    let id = normalize_link_id(id);
    if id.is_empty() {
        return false;
    }
    let Ok(Some(todo)) = find_todo_component(content) else {
        return false;
    };
    TodoBody::parse(todo.content(content))
        .items
        .iter()
        .any(|item| item.own_id.as_deref() == Some(id.as_str()))
}

/// Whether `selector` still resolves to an `agent:todo` item in `content`.
/// An ambiguous selector counts as resolving (the write path reports it).
pub fn todo_selector_resolves(content: &str, selector: &str) -> bool {
    let Ok(Some(todo)) = find_todo_component(content) else {
        return false;
    };
    let body = TodoBody::parse(todo.content(content));
    match resolve_todo_selector(&body, selector, None, "todo") {
        Ok(_) => true,
        Err(err) => err.to_string().contains("ambiguous"),
    }
}

/// Separators that introduce a completion note after an action
/// (`<action> — moot: ...`, `<action>: done in #12`).
const COMPLETION_NOTE_SEPARATORS: [&str; 7] = [" — ", " – ", " -- ", " - ", ": ", "; ", " ("];

/// Whether `annotated` is `base` followed by a completion note.
fn is_annotated_copy(base: &str, annotated: &str) -> bool {
    annotated.len() > base.len()
        && annotated.starts_with(base)
        && COMPLETION_NOTE_SEPARATORS
            .iter()
            .any(|sep| annotated[base.len()..].starts_with(sep))
}

/// Whether two items are copies of one todo, and which text the folded item
/// keeps (`true` = keep `b`'s text).
fn same_todo(a: &LocatedItem, b: &LocatedItem) -> Option<bool> {
    let (a_text, b_text) = (
        normalize_action_text(&a.action),
        normalize_action_text(&b.action),
    );
    match (&a.own_id, &b.own_id) {
        // Two different own ids are two different items, whatever the text says.
        (Some(x), Some(y)) if x != y => return None,
        (Some(_), Some(_)) => return Some(b_text.len() > a_text.len()),
        _ => {}
    }
    if a_text == b_text {
        return Some(false);
    }
    // A completion-annotated copy only folds into its original when at least
    // one side is checked: two OPEN items that differ by a trailing note are
    // two operator items, not a duplicate.
    let one_done = !a.state.is_open() || !b.state.is_open();
    if one_done && is_annotated_copy(&a_text, &b_text) {
        return Some(true);
    }
    if one_done && is_annotated_copy(&b_text, &a_text) {
        return Some(false);
    }
    None
}

/// Fold duplicate copies of one todo into a single item (`#tododupdone`).
///
/// Two items are copies when they carry the same own `[#id]`, have the same
/// action text, or one is the other plus a completion note (`<action> — moot`)
/// and at least one of them is checked. The folded item:
/// - sits at the EARLIER copy's position (the operator's ordering);
/// - is `[x]` when any copy is `[x]` (completion is never lost), else `[/]`
///   when any copy is gated, else `[ ]`;
/// - keeps the longer (annotated) text and the first own id found.
///
/// Items that are not copies are never touched, so every distinct operator item
/// survives byte-for-byte. Returns the folded body and how many copies were
/// removed.
pub fn reconcile_todo_body(body: &str) -> (String, usize) {
    let mut parsed = TodoBody::parse(body);
    let mut removed_lines: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    let mut removed = 0usize;
    let mut items = parsed.items.clone();
    for keep in 0..items.len() {
        if removed_lines.contains(&items[keep].line) {
            continue;
        }
        for other in keep + 1..items.len() {
            if removed_lines.contains(&items[other].line) {
                continue;
            }
            let Some(take_other_text) = same_todo(&items[keep], &items[other]) else {
                continue;
            };
            let state = if !items[keep].state.is_open() || !items[other].state.is_open() {
                TodoState::Done
            } else if items[keep].state == TodoState::Gated
                || items[other].state == TodoState::Gated
            {
                TodoState::Gated
            } else {
                TodoState::Open
            };
            let action = if take_other_text {
                items[other].action.clone()
            } else {
                items[keep].action.clone()
            };
            let own_id = items[keep].own_id.clone().or(items[other].own_id.clone());
            parsed.rewrite_line(items[keep].line, state, own_id.as_deref(), &action);
            items[keep].state = state;
            items[keep].action = action;
            items[keep].own_id = own_id;
            for line in parsed.item_span(&items[other]) {
                removed_lines.insert(line);
            }
            removed += 1;
        }
    }
    if removed == 0 {
        return (body.to_string(), 0);
    }
    let lines: Vec<String> = parsed
        .lines
        .into_iter()
        .enumerate()
        .filter(|(index, _)| !removed_lines.contains(index))
        .map(|(_, line)| line)
        .collect();
    (lines.concat(), removed)
}

/// Apply [`reconcile_todo_body`] to a document's `agent:todo` component.
/// Returns the possibly-updated document and the number of copies removed.
pub fn reconcile_todo_in_document(content: &str) -> Result<(String, usize)> {
    let Some(todo) = find_todo_component(content)? else {
        return Ok((content.to_string(), 0));
    };
    let (folded, removed) = reconcile_todo_body(todo.content(content));
    if removed == 0 {
        return Ok((content.to_string(), 0));
    }
    Ok((todo.replace_content(content, &folded), removed))
}

/// One OPEN todo item that cites tracked work this closeout resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTodoReference {
    /// The resolved tracked-work id the item cites (normalized, no `#`).
    pub resolved_id: String,
    /// The `--todo-done` selector that closes the item: its own id when it has
    /// one, else its action text.
    pub selector: String,
}

/// The action text of every open (`[ ]`/`[/]`) `agent:todo` item, in document
/// order. Feeds the GH #318 operator-todo response advisory, which treats an
/// ask an open item already records as recorded.
pub fn open_todo_actions(content: &str) -> Vec<String> {
    let Ok(Some(todo)) = find_todo_component(content) else {
        return Vec::new();
    };
    TodoBody::parse(todo.content(content))
        .items
        .into_iter()
        .filter(|item| item.state.is_open())
        .map(|item| item.action)
        .collect()
}

/// Stable own ids of every open (`[ ]`/`[/]`) operator todo.
///
/// Queue operator-action annotations use this projection to remain visible
/// exactly while their same-id todo is actionable.
pub fn open_todo_ids(content: &str) -> std::collections::HashSet<String> {
    let Ok(Some(todo)) = find_todo_component(content) else {
        return std::collections::HashSet::new();
    };
    TodoBody::parse(todo.content(content))
        .items
        .into_iter()
        .filter(|item| item.state.is_open())
        .filter_map(|item| item.own_id)
        .map(|id| id.to_ascii_lowercase())
        .collect()
}

/// Open (`[ ]`/`[/]`) todo items whose own id is, or whose action cites, one of
/// `ids` — operator actions tied to work this closeout just resolved, which are
/// usually finished too. Feeds the non-blocking closeout advisory.
pub fn open_todo_items_referencing(content: &str, ids: &[String]) -> Vec<OpenTodoReference> {
    let Ok(Some(todo)) = find_todo_component(content) else {
        return Vec::new();
    };
    let body = TodoBody::parse(todo.content(content));
    let mut out = Vec::new();
    for id in ids {
        let id = id.trim().trim_start_matches('#').to_ascii_lowercase();
        if id.is_empty() {
            continue;
        }
        for item in body.items.iter().filter(|item| item.state.is_open()) {
            if item.own_id.as_deref() == Some(id.as_str())
                || action_references_id(&item.action, &id)
            {
                let selector = item.own_id.clone().unwrap_or_else(|| item.action.clone());
                if !out
                    .iter()
                    .any(|seen: &OpenTodoReference| seen.selector == selector)
                {
                    out.push(OpenTodoReference {
                        resolved_id: id.clone(),
                        selector,
                    });
                }
            }
        }
    }
    out
}

/// Whether `body` mentions `#id` as a whole id token.
fn body_references_id(body: &str, id: &str) -> bool {
    let needle = format!("#{id}");
    body.match_indices(&needle).any(|(start, _)| {
        body[start + needle.len()..]
            .chars()
            .next()
            .is_none_or(|next| !(next.is_ascii_alphanumeric() || next == '-' || next == '_'))
    })
}

/// Return the ids (normalized, without `#`) that the document's `agent:todo`
/// component does not reference as `#id`. Every id is unreferenced when the
/// component is absent or the document does not parse.
pub fn todo_unreferenced_ids(content: &str, ids: &[String]) -> Vec<String> {
    let body = find_todo_component(content)
        .ok()
        .flatten()
        .map(|todo| todo.content(content).to_string())
        .unwrap_or_default();
    let mut missing: Vec<String> = Vec::new();
    for id in ids {
        let id = id.trim().trim_start_matches('#');
        if id.is_empty() || missing.iter().any(|seen| seen == id) {
            continue;
        }
        if !body_references_id(&body, id) {
            missing.push(id.to_string());
        }
    }
    missing
}

/// Whether a `before` item is the same todo as an `after` item for the
/// sticky-check rule: the same own id, or — when the `after` item has no own
/// id — an id-less `before` item with the same action text.
fn same_item_identity(before: &LocatedItem, after: &LocatedItem) -> bool {
    match (&before.own_id, &after.own_id) {
        (Some(x), Some(y)) => x == y,
        (None, None) => normalize_action_text(&before.action) == normalize_action_text(&after.action),
        // An id-less `after` copy of an id-bearing item (the agent dropped the
        // id while reproducing it) is still that item when the text matches.
        (Some(_), None) => normalize_action_text(&before.action) == normalize_action_text(&after.action),
        (None, Some(_)) => false,
    }
}

/// `#todoids`: keep every operator `[x]` that a `patch:todo` replacement
/// carries open.
///
/// `before` is the todo body the patch replaces (the document as the write
/// read it) and `after` the patched body. An `after` item that is `[ ]`/`[/]`
/// while the same item is `[x]` in `before` is rewritten to `[x]` in place, so
/// an agent patch built from a stale read can never revert the operator's
/// check. Text edits the patch made are kept. Only `--todo-reopen` unchecks an
/// item. Returns the body and how many items were re-checked.
pub fn preserve_checked_todo_items(before: &str, after: &str) -> (String, usize) {
    let checked: Vec<LocatedItem> = TodoBody::parse(before)
        .items
        .into_iter()
        .filter(|item| !item.state.is_open())
        .collect();
    if checked.is_empty() {
        return (after.to_string(), 0);
    }
    let mut parsed = TodoBody::parse(after);
    let mut restored = 0usize;
    for item in parsed.items.clone() {
        if !item.state.is_open() {
            continue;
        }
        if checked.iter().any(|done| same_item_identity(done, &item)) {
            parsed.rewrite_line(item.line, TodoState::Done, item.own_id.as_deref(), &item.action);
            restored += 1;
        }
    }
    if restored == 0 {
        return (after.to_string(), 0);
    }
    (parsed.render(), restored)
}

/// `#todoids`: actions of items a `patch:todo` ADDED without an own id — items
/// in `after` with no `[#id]` whose text no `before` item carries. The write
/// path reports them (the id-bearing path is `--todo-add`); it does not mint
/// ids for them, because the keyed merge keys an id-less item by its text and
/// re-keying a line the operator may be editing would duplicate it.
pub fn new_idless_todo_items(before: &str, after: &str) -> Vec<String> {
    let known: std::collections::HashSet<String> = TodoBody::parse(before)
        .items
        .into_iter()
        .map(|item| normalize_action_text(&item.action))
        .collect();
    TodoBody::parse(after)
        .items
        .into_iter()
        .filter(|item| item.own_id.is_none())
        .map(|item| normalize_action_text(&item.action))
        .filter(|text| !known.contains(text))
        .collect()
}

/// What a todo's link to tracked work says should happen next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoLinkFindingKind {
    /// An open backlog/icebox source has no reverse edge to an actual Todo.
    /// Preflight flags both absent metadata and metadata naming missing Todos.
    TrackedWorkMissingTodoBacklink,
    /// A legacy singly linked todo does not visibly qualify its tracked-work
    /// source with both component and id. Preflight maintenance migrates it.
    LegacySourceUnqualified,
    /// The operator checked a todo whose own id is an OPEN `agent:review`
    /// gate: the agent resolves the gate (or records the follow-up) — the
    /// binary does not, because which outcome the operator chose is judgement.
    OperatorDoneReviewOpen,
    /// The operator checked a todo whose own id is OPEN `agent:backlog` work:
    /// the operator prerequisite is done, so the item is unblocked.
    OperatorDoneBacklogOpen,
    /// `#todoidlayer`: the operator checked a todo whose own id is an OPEN
    /// `agent:icebox` item: the operator made the decision the todo surfaced
    /// (thaw, drop, keep parked); the agent reads it and acts — the binary
    /// never moves or closes parked work from a checkbox.
    OperatorDoneIceboxOpen,
    /// An OPEN todo whose OWN id is resolved tracked work: the operator prompt
    /// is finished or moot. A closeout auto-completes it.
    LinkedWorkResolved,
    /// An OPEN todo that only CITES tracked work, all of it resolved: surfaced
    /// only, because the action may cover more than the cited work.
    CitedWorkResolved,
}

impl TodoLinkFindingKind {
    /// Stable snake_case name for preflight JSON and ops.log.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TrackedWorkMissingTodoBacklink => "tracked_work_missing_todo_backlink",
            Self::LegacySourceUnqualified => "legacy_source_unqualified",
            Self::OperatorDoneReviewOpen => "operator_done_review_open",
            Self::OperatorDoneBacklogOpen => "operator_done_backlog_open",
            Self::OperatorDoneIceboxOpen => "operator_done_icebox_open",
            Self::LinkedWorkResolved => "linked_work_resolved",
            Self::CitedWorkResolved => "cited_work_resolved",
        }
    }
}

/// One todo whose tracked-work link calls for an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoLinkFinding {
    pub kind: TodoLinkFindingKind,
    /// The tracked-work ids the finding is about (the own id, or the cited ids).
    pub ids: Vec<String>,
    /// The `--todo-*` selector for the item: its own id, else its action text.
    pub selector: String,
    /// `#id` or quoted action of the item, for receipts.
    pub label: String,
    /// Source component for a source-qualification finding.
    pub component: Option<String>,
}

/// `#todoids`: evaluate every `agent:todo` item against `links`.
///
/// Pure and deterministic: the same document and index always yield the same
/// findings in item order followed by missing-source findings in source-id
/// order. A document without `agent:todo` can still report missing backlinks; a
/// document whose components do not parse is an error (never "no findings"),
/// so a caller can fail closed.
pub fn todo_link_findings(content: &str, links: &TodoLinkIndex) -> Result<Vec<TodoLinkFinding>> {
    let mut links = links.clone();
    links.resolve_todo_backlinks_from_document(content)?;
    let body = find_todo_component(content)?
        .map(|todo| TodoBody::parse(todo.content(content)))
        .unwrap_or_else(|| TodoBody::parse(""));
    let mut out = Vec::new();
    for item in &body.items {
        let selector = item.own_id.clone().unwrap_or_else(|| item.action.clone());
        let label = item_label(item.own_id.as_deref(), &item.action);
        let own = item.own_id.as_deref();
        let finding = |kind, ids: Vec<String>| TodoLinkFinding {
            kind,
            ids,
            selector: selector.clone(),
            label: label.clone(),
            component: None,
        };
        if let Some((id, component)) = single_todo_source(own, &item.action, &links) {
            let qualified = action_with_source_reference(&item.action, &id, Some(&component));
            if normalize_action_text(&qualified) != normalize_action_text(&item.action) {
                out.push(TodoLinkFinding {
                    kind: TodoLinkFindingKind::LegacySourceUnqualified,
                    ids: vec![id],
                    selector: selector.clone(),
                    label: label.clone(),
                    component: Some(component),
                });
            }
        }
        if !item.state.is_open() {
            let mut open_sources: Vec<(String, String)> = own
                .and_then(|id| links.open_component(id).map(|component| {
                    (id.to_string(), component.to_string())
                }))
                .into_iter()
                .collect();
            if let Some(own) = own {
                for (id, component, _) in links.sources_for_todo(own) {
                    let Some(component) = component else { continue };
                    if !open_sources.iter().any(|(seen, _)| seen == &id) {
                        open_sources.push((id, component));
                    }
                }
            }
            for (id, component) in open_sources {
                match component.as_str() {
                    "review" => out.push(finding(
                        TodoLinkFindingKind::OperatorDoneReviewOpen,
                        vec![id],
                    )),
                    "backlog" | "pending" => out.push(finding(
                        TodoLinkFindingKind::OperatorDoneBacklogOpen,
                        vec![id],
                    )),
                    "icebox" => out.push(finding(
                        TodoLinkFindingKind::OperatorDoneIceboxOpen,
                        vec![id],
                    )),
                    _ => {}
                }
            }
            continue;
        }
        if let Some(own) = own {
            if links.is_resolved(own) {
                out.push(finding(TodoLinkFindingKind::LinkedWorkResolved, vec![own.to_string()]));
                continue;
            }
            if links.linked_sources_resolved(own) {
                let ids = links.sources_for_todo(own).into_iter()
                    .map(|(id, _, _)| id).collect();
                out.push(finding(TodoLinkFindingKind::LinkedWorkResolved, ids));
                continue;
            }
        }
        if own.is_some_and(|own| links.knows(own)) {
            // Linked to OPEN work: nothing to do until the operator acts.
            continue;
        }
        let cited: Vec<String> = cited_ids(&item.action)
            .into_iter()
            .filter(|id| Some(id.as_str()) != own && links.knows(id))
            .collect();
        if !cited.is_empty() && cited.iter().all(|id| links.is_resolved(id)) {
            out.push(finding(TodoLinkFindingKind::CitedWorkResolved, cited));
        }
    }
    for (id, component) in links.missing_todo_backlinks() {
        out.push(TodoLinkFinding {
            kind: TodoLinkFindingKind::TrackedWorkMissingTodoBacklink,
            ids: vec![id.clone()],
            selector: id.clone(),
            label: format!("#{id}"),
            component: Some(component),
        });
    }
    Ok(out)
}

/// Result of [`auto_complete_linked_todos`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoAutoCompleteOutcome {
    /// The document after the auto-completion (unchanged when none applied).
    pub content: String,
    /// Own ids of the items checked off, in item order.
    pub completed: Vec<String>,
    /// Duplicate copies folded afterwards (`#tododupdone`).
    pub reconciled: usize,
}

/// `#todoids`: check off every OPEN todo whose OWN id is resolved tracked work
/// in `links` ([`TodoLinkFindingKind::LinkedWorkResolved`]), in place, then
/// fold duplicate copies.
///
/// This is the only automatic todo completion. It never touches an item that
/// merely cites resolved work, an id-less item, or an item whose id `links`
/// does not know (fail closed: missing evidence is never resolution).
pub fn auto_complete_linked_todos(
    content: &str,
    links: &TodoLinkIndex,
) -> Result<TodoAutoCompleteOutcome> {
    auto_complete_linked_todos_except(content, links, &[])
}

/// [`auto_complete_linked_todos`], leaving every todo whose own id is in
/// `exempt` open (`#todoaddonsamecycledone`: the todos the same closeout's
/// `--todo-add` created).
pub fn auto_complete_linked_todos_except(
    content: &str,
    links: &TodoLinkIndex,
    exempt: &[String],
) -> Result<TodoAutoCompleteOutcome> {
    let unchanged = |content: &str| TodoAutoCompleteOutcome {
        content: content.to_string(),
        completed: Vec::new(),
        reconciled: 0,
    };
    let Some(todo) = find_todo_component(content)? else {
        return Ok(unchanged(content));
    };
    let mut body = TodoBody::parse(todo.content(content));
    let mut completed = Vec::new();
    for item in body.items.clone() {
        let Some(own) = item.own_id.as_deref() else {
            continue;
        };
        if exempt
            .iter()
            .any(|id| normalize_link_id(id) == normalize_link_id(own))
        {
            continue;
        }
        if item.state.is_open() && (links.is_resolved(own) || links.linked_sources_resolved(own)) {
            body.rewrite_line(item.line, TodoState::Done, Some(own), &item.action);
            if !completed.iter().any(|seen: &String| seen == own) {
                completed.push(own.to_string());
            }
        }
    }
    if completed.is_empty() {
        return Ok(unchanged(content));
    }
    let (folded, reconciled) = reconcile_todo_body(&body.render());
    Ok(TodoAutoCompleteOutcome {
        content: todo.replace_content(content, &folded),
        completed,
        reconciled,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn todo_body(content: &str) -> String {
        find_todo_component(content)
            .unwrap()
            .expect("todo component")
            .content(content)
            .to_string()
    }

    /// Deterministic test id source: `t<counter>` plus a text-length suffix.
    fn test_id(text: &str, counter: u64) -> String {
        format!("t{counter}{}", text.len() % 10)
    }

    fn add(content: &str, text: &str) -> Result<TodoAddOutcome> {
        add_todo_item_to_document(
            content,
            text,
            &test_id,
            &std::collections::HashSet::new(),
            &TodoLinkIndex::default(),
        )
    }

    fn add_linked(content: &str, text: &str, links: &TodoLinkIndex) -> Result<TodoAddOutcome> {
        add_todo_item_to_document(content, text, &test_id, &std::collections::HashSet::new(), links)
    }

    fn wrap(body: &str) -> String {
        format!(
            "<!-- agent:exchange -->\nhi\n<!-- /agent:exchange -->\n<!-- agent:todo -->\n{body}<!-- /agent:todo -->\n"
        )
    }

    #[test]
    fn ensure_inserts_todo_below_backlog_and_above_review_with_scaffold_heading() {
        let content = concat!(
            "<!-- agent:exchange -->\n### Re: prior\n<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n<!-- agent:backlog -->\n- [ ] [#work] do work\n<!-- /agent:backlog -->\n\n",
            "## Review\n\n<!-- agent:review -->\n- [/] [#gate] gated\n<!-- /agent:review -->\n\n",
            "## Done\n\n<!-- agent:done -->\n<!-- /agent:done -->\n",
        );
        let (updated, todo, created) = ensure_todo_component_in_document(content).unwrap();
        assert!(created);
        assert_eq!(todo.content(&updated), "");
        let backlog_end = updated.find("<!-- /agent:backlog -->").unwrap();
        let todo_start = updated.find("<!-- agent:todo priority -->").unwrap();
        let review_heading = updated.find("## Review").unwrap();
        assert!(
            backlog_end < todo_start && todo_start < review_heading,
            "{updated}"
        );
        assert!(
            updated.contains("<!-- /agent:backlog -->\n\n## Todo\n\n<!-- agent:todo priority -->\n<!-- /agent:todo -->\n\n## Review\n\n<!-- agent:review -->"),
            "{updated}"
        );
        // Everything outside the inserted section is byte-identical.
        assert_eq!(updated.replacen(TODO_SECTION, "", 1), content);
    }

    #[test]
    fn ensure_places_agent_todo_below_agent_queue_and_above_agent_backlog() {
        let content = concat!(
            "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n",
            "## Queue\n\n<!-- agent:queue -->\n- do x\n<!-- /agent:queue -->\n",
            "## Backlog\n\n<!-- agent:backlog -->\n- [ ] [#work] do work\n<!-- /agent:backlog -->\n",
            "## Review\n\n<!-- agent:review -->\n<!-- /agent:review -->\n",
        );
        let (updated, _, created) = ensure_todo_component_in_document(content).unwrap();
        assert!(created);
        assert!(
            updated.contains("<!-- /agent:queue -->\n\n## Todo\n\n<!-- agent:todo priority -->\n<!-- /agent:todo -->\n## Backlog"),
            "{updated}"
        );
        let queue_end = updated.find("<!-- /agent:queue -->").unwrap();
        let todo_start = updated.find("<!-- agent:todo priority -->").unwrap();
        let backlog_heading = updated.find("## Backlog").unwrap();
        assert!(
            queue_end < todo_start && todo_start < backlog_heading,
            "agent:todo must sit below agent:queue and above agent:backlog:\n{updated}"
        );
    }

    #[test]
    fn ensure_anchors_on_the_last_backlog_and_never_on_review() {
        // Legacy `agent:pending` counts as backlog; review after it does not
        // pull Todo below Review.
        let content = concat!(
            "<!-- agent:review -->\n<!-- /agent:review -->\n",
            "<!-- agent:pending -->\n- [ ] a\n<!-- /agent:pending -->\n",
            "<!-- agent:backlog -->\n- [ ] b\n<!-- /agent:backlog -->\n",
            "## Review\n\n<!-- agent:review -->\n<!-- /agent:review -->\n",
        );
        let (updated, _, _) = ensure_todo_component_in_document(content).unwrap();
        assert!(
            updated.contains("<!-- /agent:backlog -->\n\n## Todo\n\n<!-- agent:todo priority -->\n<!-- /agent:todo -->\n## Review"),
            "{updated}"
        );
    }

    #[test]
    fn ensure_falls_back_to_queue_then_exchange_then_end_without_backlog() {
        // No backlog: after the queue, which keeps Todo above a later Review.
        let queue_and_review = concat!(
            "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n- do x\n<!-- /agent:queue -->\n",
            "## Review\n\n<!-- agent:review -->\n- [/] [#gate] gated\n<!-- /agent:review -->\n",
        );
        let (updated, _, _) = ensure_todo_component_in_document(queue_and_review).unwrap();
        assert!(
            updated.contains("<!-- /agent:queue -->\n\n## Todo\n\n<!-- agent:todo priority -->\n<!-- /agent:todo -->\n## Review"),
            "{updated}"
        );

        let queue = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n<!-- agent:queue -->\n- do x\n<!-- /agent:queue -->\ntrailing prose\n";
        let (updated, _, _) = ensure_todo_component_in_document(queue).unwrap();
        assert!(
            updated.contains("<!-- /agent:queue -->\n\n## Todo\n\n<!-- agent:todo priority -->\n<!-- /agent:todo -->\ntrailing prose\n"),
            "{updated}"
        );

        // Exchange + review only: after the exchange, still above Review.
        let exchange_and_review = "<!-- agent:exchange -->\nhi\n<!-- /agent:exchange -->\n<!-- agent:review -->\n<!-- /agent:review -->\n";
        let (updated, _, _) = ensure_todo_component_in_document(exchange_and_review).unwrap();
        assert_eq!(
            updated,
            format!(
                "<!-- agent:exchange -->\nhi\n<!-- /agent:exchange -->\n{TODO_SECTION}<!-- agent:review -->\n<!-- /agent:review -->\n"
            )
        );

        let exchange = "# Title\n\n<!-- agent:exchange -->\nhi\n<!-- /agent:exchange -->";
        let (updated, _, _) = ensure_todo_component_in_document(exchange).unwrap();
        assert_eq!(
            updated,
            format!(
                "# Title\n\n<!-- agent:exchange -->\nhi\n<!-- /agent:exchange -->\n{TODO_SECTION}"
            )
        );

        // Review alone is not an anchor: end of document.
        let review_only = "# Notes\n\n<!-- agent:review -->\n<!-- /agent:review -->\nprose";
        let (updated, _, _) = ensure_todo_component_in_document(review_only).unwrap();
        assert_eq!(updated, format!("{review_only}\n{TODO_SECTION}"));

        let plain = "# Notes only\n\nprose";
        let (updated, _, _) = ensure_todo_component_in_document(plain).unwrap();
        assert_eq!(updated, format!("# Notes only\n\nprose\n{TODO_SECTION}"));
    }

    #[test]
    fn ensure_is_a_noop_when_todo_exists() {
        let content = "<!-- agent:review -->\n<!-- /agent:review -->\n<!-- agent:todo -->\n- [ ] keep\n<!-- /agent:todo -->\n";
        let (updated, todo, created) = ensure_todo_component_in_document(content).unwrap();
        assert!(!created);
        assert_eq!(updated, content);
        assert_eq!(todo.content(&updated), "- [ ] keep\n");
    }

    #[test]
    fn add_creates_component_and_appends_item_with_its_own_id() {
        let content = "<!-- agent:review -->\n- [/] [#gate] gated\n<!-- /agent:review -->\n";
        let outcome = add(content, "Publish draft docs/launch-note.md").unwrap();
        assert!(outcome.inserted && outcome.created_component);
        assert_eq!(outcome.id.as_deref(), Some("t03"));
        assert_eq!(
            todo_body(&outcome.content),
            "- [ ] [#t03] Publish draft docs/launch-note.md\n"
        );
    }

    #[test]
    fn add_preserves_every_existing_item_and_operator_text() {
        let content = concat!(
            "<!-- agent:todo -->\n",
            "Operator note: do these after lunch\n",
            "- [x] Merged the release PR\n",
            "- [ ] Rotate the deploy key\n",
            "<!-- /agent:todo -->\n",
        );
        let outcome = add(content, "Verify [#gate] in a live pane").unwrap();
        assert!(outcome.inserted);
        assert!(!outcome.created_component);
        assert_eq!(
            todo_body(&outcome.content),
            concat!(
                "Operator note: do these after lunch\n",
                "- [x] Merged the release PR\n",
                "- [ ] Rotate the deploy key\n",
                "- [ ] [#t09] Verify [#gate] in a live pane\n",
            )
        );
        assert_eq!(count_checklist_items(&todo_body(&outcome.content)), 3);
    }

    #[test]
    fn add_is_idempotent_against_open_and_checked_items_with_or_without_ids() {
        let content = "<!-- agent:todo -->\n- [x] Rotate the deploy key\n- [ ] [#k3vd] Publish the note\n<!-- /agent:todo -->\n";
        for text in [
            "Rotate the deploy key",
            "- [ ] Rotate the deploy key",
            "  Rotate the deploy key  ",
        ] {
            let outcome = add(content, text).unwrap();
            assert!(!outcome.inserted, "{text:?}");
            assert_eq!(outcome.content, content);
            assert_eq!(outcome.id, None);
        }
        let outcome = add(content, "Publish  the note").unwrap();
        assert!(!outcome.inserted);
        assert_eq!(outcome.id.as_deref(), Some("k3vd"));
    }

    #[test]
    fn add_ids_avoid_ids_already_cited_or_reserved() {
        // `t0x` is cited by an existing item and `t1x` is a reserved
        // tracked-work id, so the third candidate wins.
        let action = "Rotate key";
        let len_digit = action.len() % 10;
        let content = wrap(&format!("- [ ] Check (#t0{len_digit})\n"));
        let reserved: std::collections::HashSet<String> =
            [format!("t1{len_digit}")].into_iter().collect();
        let outcome = add_todo_item_to_document(
            &content,
            action,
            &test_id,
            &reserved,
            &TodoLinkIndex::default(),
        )
        .unwrap();
        assert_eq!(outcome.id, Some(format!("t2{len_digit}")));
    }

    #[test]
    fn add_accepts_an_explicit_id_and_rejects_reusing_one() {
        let content = wrap("");
        let outcome = add(&content, "id=rotate Rotate the deploy key").unwrap();
        assert_eq!(
            todo_body(&outcome.content),
            "- [ ] [#rotate] Rotate the deploy key\n"
        );
        let bracket = add(&content, "[#pub] Publish").unwrap();
        assert_eq!(todo_body(&bracket.content), "- [ ] [#pub] Publish\n");
        let err = add(&outcome.content, "id=rotate Something else").unwrap_err();
        assert!(
            err.to_string()
                .contains("already has an item with id [#rotate]"),
            "{err}"
        );
    }

    #[test]
    fn add_handles_body_without_trailing_newline() {
        let no_newline = "<!-- agent:todo -->\n- [ ] first<!-- /agent:todo -->\n";
        let outcome = add(no_newline, "second").unwrap();
        assert_eq!(
            todo_body(&outcome.content),
            "- [ ] first\n- [ ] [#t06] second\n"
        );
    }

    #[test]
    fn add_rejects_empty_and_multi_line_actions() {
        let content = "<!-- agent:todo -->\n<!-- /agent:todo -->\n";
        assert!(add(content, "   ").is_err());
        assert!(add(content, "- [ ] ").is_err());
        assert!(add(content, "one\ntwo").is_err());
        assert!(add(content, "id=x").is_err());
    }

    /// `#nestedtodoadd`: later lines of an add are the item's nested
    /// sub-items, rendered under it; the item's identity is its own line.
    #[test]
    fn add_renders_nested_sub_items_under_the_item_and_replays_as_a_noop() {
        let content = wrap("- [ ] [#keep] Operator item\n");
        let text = "id=q1 Decide the sync model\n  - Pull or push?\n  - Who owns conflicts?\n  - Which providers first?";
        let outcome = add(&content, text).unwrap();
        assert!(outcome.inserted);
        assert_eq!(outcome.id.as_deref(), Some("q1"));
        assert_eq!(
            todo_body(&outcome.content),
            concat!(
                "- [ ] [#keep] Operator item\n",
                "- [ ] [#q1] Decide the sync model\n",
                "  - Pull or push?\n",
                "  - Who owns conflicts?\n",
                "  - Which providers first?\n",
            )
        );
        // The sub-items are not checklist items.
        assert_eq!(TodoBody::parse(&todo_body(&outcome.content)).items.len(), 2);

        // A replay (same text, or the same item without its id) is a no-op.
        let replay = add(&outcome.content, text).unwrap();
        assert!(!replay.inserted);
        assert!(!replay.sub_items_kept);
        assert_eq!(replay.content, outcome.content);
        let minted = add(&content, "Decide the sync model\n\t- Pull or push?").unwrap();
        let replay_minted =
            add(&minted.content, "Decide the sync model\n\t- Pull or push?").unwrap();
        assert!(!replay_minted.inserted);
        assert_eq!(replay_minted.content, minted.content);
        assert!(
            minted
                .content
                .contains("] Decide the sync model\n  - Pull or push?\n")
        );

        // An operator edit to a sub-item survives a replay, which reports it.
        let operator = outcome.content.replace(
            "  - Pull or push?\n",
            "  - Pull or push? (push, per Brian)\n",
        );
        let replay = add(&operator, text).unwrap();
        assert!(!replay.inserted);
        assert!(replay.sub_items_kept);
        assert_eq!(replay.content, operator);
    }

    #[test]
    fn add_rejects_malformed_nested_sub_items_and_writes_nothing() {
        let content = wrap("");
        for (text, why) in [
            ("Item\n\n  - child", "blank"),
            ("Item\n  - child\n", "trailing newline is fine"),
            ("Item\n- flush child", "not a nested sub-item"),
            ("Item\n  plain prose", "not a nested sub-item"),
            ("Item\n  * star child", "not a nested sub-item"),
            ("Item\n  - one\n    - deeper", "changes indentation"),
            ("Item\n  - [ ] checkbox child", "checkbox row"),
            ("Item\n  - [x]", "checkbox row"),
            ("Item\n  - <!-- agent:queue -->", "HTML comment"),
            ("Item\n  - ends -->", "HTML comment"),
            ("Item\n  - ", "empty sub-item"),
            ("   \n  - child", "non-empty operator action"),
        ] {
            let result = add(&content, text);
            if why == "trailing newline is fine" {
                assert!(result.is_ok(), "{text:?}: {result:?}");
                continue;
            }
            let err = format!("{:#}", result.expect_err(text));
            assert!(err.contains(why), "{text:?}: {err}");
        }
    }

    /// `#nestedtodoadd`: done/reopen keep the sub-items with the item, remove
    /// deletes them with it, a multi-line edit replaces them, and a
    /// single-line edit keeps them.
    #[test]
    fn mutations_keep_nested_sub_items_with_their_item() {
        let content = wrap(concat!(
            "- [ ] [#q1] Decide the sync model\n",
            "  - Pull or push?\n",
            "  - Who owns conflicts?\n",
            "- [ ] [#q2] Next item\n",
        ));
        let done = set_todo_item_state_in_document(&content, "q1", TodoTransition::Done).unwrap();
        assert_eq!(
            todo_body(&done.content),
            concat!(
                "- [x] [#q1] Decide the sync model\n",
                "  - Pull or push?\n",
                "  - Who owns conflicts?\n",
                "- [ ] [#q2] Next item\n",
            )
        );
        let edited =
            edit_todo_item_in_document(&done.content, "q1", "Decide the sync model (decided)")
                .unwrap();
        assert_eq!(
            todo_body(&edited.content),
            concat!(
                "- [x] [#q1] Decide the sync model (decided)\n",
                "  - Pull or push?\n",
                "  - Who owns conflicts?\n",
                "- [ ] [#q2] Next item\n",
            )
        );
        let replaced = edit_todo_item_in_document(
            &edited.content,
            "q1",
            "Decide the sync model (decided)\n  - Push.",
        )
        .unwrap();
        assert!(replaced.changed);
        assert_eq!(
            todo_body(&replaced.content),
            concat!(
                "- [x] [#q1] Decide the sync model (decided)\n",
                "  - Push.\n",
                "- [ ] [#q2] Next item\n",
            )
        );
        let same = edit_todo_item_in_document(
            &replaced.content,
            "q1",
            "Decide the sync model (decided)\n  - Push.",
        )
        .unwrap();
        assert!(!same.changed);
        // Children added to the LAST item, whose line has no trailing newline.
        let tail = "<!-- agent:todo -->\n- [ ] [#q3] Last<!-- /agent:todo -->\n";
        let grown = edit_todo_item_in_document(tail, "q3", "Last\n  - a\n  - b").unwrap();
        assert_eq!(todo_body(&grown.content), "- [ ] [#q3] Last\n  - a\n  - b");
        let removed = remove_todo_item_from_document(&replaced.content, "q1").unwrap();
        assert_eq!(todo_body(&removed.content), "- [ ] [#q2] Next item\n");
        // Fresh ids are never minted for sub-item lines.
        let assigned = assign_missing_todo_ids(
            &content,
            &test_id,
            &Default::default(),
            &TodoLinkIndex::default(),
            &|_| true,
        )
        .unwrap();
        assert!(
            assigned.assigned.is_empty(),
            "sub-items are not id-less todo rows"
        );
        assert_eq!(assigned.content, content);
    }

    #[test]
    fn parse_todo_line_separates_prefix_state_own_id_and_action() {
        let line = parse_todo_line("  - [x] [#k3vd] Verify [#gate] live").unwrap();
        assert_eq!(line.prefix, "  - ");
        assert_eq!(line.state, TodoState::Done);
        assert_eq!(line.own_id, Some("k3vd"));
        assert_eq!(line.action, "Verify [#gate] live");
        // A cited id later in the line is a reference, not the item's own id.
        let line = parse_todo_line("- [/] Decide merge order (#apimigrate)").unwrap();
        assert_eq!(line.state, TodoState::Gated);
        assert_eq!(line.own_id, None);
        assert_eq!(line.action, "Decide merge order (#apimigrate)");
        assert!(parse_todo_line("- plain bullet").is_none());
    }

    #[test]
    fn done_then_edit_then_reopen_then_remove_round_trip_by_own_id() {
        let content = wrap(concat!(
            "Operator note\n",
            "- [ ] [#k3vd] Rotate the deploy key\n",
            "- [ ] Keep me\n",
        ));
        let done = set_todo_item_state_in_document(&content, "k3vd", TodoTransition::Done).unwrap();
        assert!(done.changed);
        assert_eq!(done.label, "#k3vd");
        assert_eq!(
            todo_body(&done.content),
            "Operator note\n- [x] [#k3vd] Rotate the deploy key\n- [ ] Keep me\n"
        );
        // Replaying `--todo-done` is a no-op.
        let again =
            set_todo_item_state_in_document(&done.content, "#k3vd", TodoTransition::Done).unwrap();
        assert!(!again.changed);
        assert_eq!(again.content, done.content);

        let edited = edit_todo_item_in_document(
            &done.content,
            "[#k3vd]",
            "Rotate the deploy key (done Tue)",
        )
        .unwrap();
        assert_eq!(
            todo_body(&edited.content),
            "Operator note\n- [x] [#k3vd] Rotate the deploy key (done Tue)\n- [ ] Keep me\n"
        );
        let same = edit_todo_item_in_document(
            &edited.content,
            "k3vd",
            "[#k3vd] Rotate the deploy key (done Tue)",
        )
        .unwrap();
        assert!(!same.changed);
        assert!(edit_todo_item_in_document(&edited.content, "k3vd", "id=other Text").is_err());

        let reopened =
            set_todo_item_state_in_document(&edited.content, "k3vd", TodoTransition::Reopen)
                .unwrap();
        assert_eq!(
            todo_body(&reopened.content),
            "Operator note\n- [ ] [#k3vd] Rotate the deploy key (done Tue)\n- [ ] Keep me\n"
        );
        let removed = remove_todo_item_from_document(&reopened.content, "k3vd").unwrap();
        assert_eq!(
            todo_body(&removed.content),
            "Operator note\n- [ ] Keep me\n"
        );
        assert!(!todo_selector_resolves(&removed.content, "k3vd"));
        assert!(todo_selector_resolves(&removed.content, "Keep me"));
        assert!(remove_todo_item_from_document(&removed.content, "k3vd").is_err());
    }

    #[test]
    fn selectors_resolve_cited_ids_and_exact_text_for_id_less_items() {
        let content = wrap(concat!(
            "- [ ] Decide the merge order of #11 vs #22 (#mergeorder)\n",
            "- [x] Approve PR #40 (#bundle) - merged\n",
            "- [ ] Approve PR #41 (#bundle)\n",
            "- [ ] Seed the runner folder\n",
        ));
        // A cited `(#id)` resolves when exactly one item cites it.
        let done =
            set_todo_item_state_in_document(&content, "mergeorder", TodoTransition::Done).unwrap();
        assert!(todo_body(&done.content).starts_with("- [x] Decide the merge order"));
        // Two items cite #bundle: done prefers the open one, reopen the checked one.
        let done =
            set_todo_item_state_in_document(&content, "bundle", TodoTransition::Done).unwrap();
        assert!(todo_body(&done.content).contains("- [x] Approve PR #41 (#bundle)\n"));
        let reopened =
            set_todo_item_state_in_document(&content, "bundle", TodoTransition::Reopen).unwrap();
        assert!(todo_body(&reopened.content).contains("- [ ] Approve PR #40 (#bundle) - merged\n"));
        // Edit has no state preference, so the shared citation is ambiguous.
        let err = edit_todo_item_in_document(&content, "bundle", "x").unwrap_err();
        assert!(err.to_string().contains("ambiguous"), "{err}");
        assert!(todo_selector_resolves(&content, "bundle"));
        // Exact text selects an id-less item.
        let done = set_todo_item_state_in_document(
            &content,
            "Seed the runner folder",
            TodoTransition::Done,
        )
        .unwrap();
        assert!(todo_body(&done.content).ends_with("- [x] Seed the runner folder\n"));
        let err =
            set_todo_item_state_in_document(&content, "nothing like this", TodoTransition::Done)
                .unwrap_err();
        assert!(
            err.to_string().contains("no agent:todo item matches"),
            "{err}"
        );
        assert!(
            set_todo_item_state_in_document(
                "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n",
                "x",
                TodoTransition::Done
            )
            .is_err()
        );
    }

    #[test]
    fn remove_takes_continuation_lines_with_the_item() {
        let content = wrap("- [ ] [#a1] Parent\n  - note under parent\n- [ ] [#b2] Sibling\n");
        let removed = remove_todo_item_from_document(&content, "a1").unwrap();
        assert_eq!(todo_body(&removed.content), "- [ ] [#b2] Sibling\n");
    }

    /// `#tododupdone`, anonymized from the live 2026-10-15 session: a merge
    /// left each open original in place and appended its checked, annotated
    /// copy (plus a second copy of an operator-reworded item). Folding keeps
    /// one checked line at the original's position and every distinct item.
    #[test]
    fn reconcile_folds_checked_annotated_copies_into_their_open_originals() {
        let body = concat!(
            "- [ ] Raise the CI spending limit\n",
            "- [ ] Decide the api merge order of #645 vs #715 (#apimigrate)\n",
            "- [ ] Approve/merge contracts #160 and pick the version bump (#bundleadopt)\n",
            "- [ ] Decide provisional sessions, option 3. Talk to the leads. (#provauth)\n",
            "- [ ] Get the retention PR merged (#wheelretention)\n",
            "- [x] Decide the api merge order of #645 vs #715 (#apimigrate) — moot: #715 renumbered, either order works\n",
            "- [x] Approve/merge contracts #160 and pick the version bump (#bundleadopt) — #160 merged; continues as #162 below\n",
            "- [ ] Decide provisional sessions, option 3. Talk to the leads. (#provauth)\n",
            "- [ ] Approve/merge contracts #162 once CI can run, and pick the version (#bundleadopt)\n",
        );
        let (folded, removed) = reconcile_todo_body(body);
        assert_eq!(removed, 3);
        assert_eq!(
            folded,
            concat!(
                "- [ ] Raise the CI spending limit\n",
                "- [x] Decide the api merge order of #645 vs #715 (#apimigrate) — moot: #715 renumbered, either order works\n",
                "- [x] Approve/merge contracts #160 and pick the version bump (#bundleadopt) — #160 merged; continues as #162 below\n",
                "- [ ] Decide provisional sessions, option 3. Talk to the leads. (#provauth)\n",
                "- [ ] Get the retention PR merged (#wheelretention)\n",
                // A different item citing the same tracked id is NOT a copy.
                "- [ ] Approve/merge contracts #162 once CI can run, and pick the version (#bundleadopt)\n",
            )
        );
        // Folding is idempotent.
        assert_eq!(reconcile_todo_body(&folded), (folded.clone(), 0));
    }

    #[test]
    fn reconcile_never_folds_distinct_open_items_or_different_own_ids() {
        let body = concat!(
            "- [ ] Approve #136\n",
            "- [ ] Approve #136 - after the rebase\n",
            "- [ ] [#a1] Same text\n",
            "- [x] [#b2] Same text\n",
            "operator prose line\n",
        );
        assert_eq!(reconcile_todo_body(body), (body.to_string(), 0));
        // Same own id folds even when the texts differ.
        let (folded, removed) =
            reconcile_todo_body("- [ ] [#a1] Old text\n- [x] [#a1] Old text, done\n");
        assert_eq!(
            (folded.as_str(), removed),
            ("- [x] [#a1] Old text, done\n", 1)
        );
    }

    #[test]
    fn todo_done_folds_a_duplicated_item_into_one_checked_line() {
        let content = wrap(concat!(
            "- [ ] Decide the merge order (#apimigrate)\n",
            "- [ ] Other\n",
            "- [ ] Decide the merge order (#apimigrate)\n",
        ));
        let done = set_todo_item_state_in_document(
            &content,
            "Decide the merge order (#apimigrate)",
            TodoTransition::Done,
        );
        // Two identical open copies: the text selector is ambiguous...
        assert!(done.is_err());
        // ...but the cited id resolves to the first open copy, and the fold
        // removes the second.
        let err = set_todo_item_state_in_document(&content, "apimigrate", TodoTransition::Done)
            .unwrap_err();
        assert!(err.to_string().contains("ambiguous"), "{err}");
        let (folded, removed) = reconcile_todo_in_document(&content).unwrap();
        assert_eq!(removed, 1);
        let done =
            set_todo_item_state_in_document(&folded, "apimigrate", TodoTransition::Done).unwrap();
        assert_eq!(
            todo_body(&done.content),
            "- [x] Decide the merge order (#apimigrate)\n- [ ] Other\n"
        );

        // An open original plus a checked annotated copy: done folds them.
        let content = wrap(concat!(
            "- [ ] [#q1] Decide the merge order\n",
            "- [ ] Other\n",
            "- [x] Decide the merge order — moot\n",
        ));
        let done = set_todo_item_state_in_document(&content, "q1", TodoTransition::Done).unwrap();
        assert_eq!(done.reconciled, 1);
        assert_eq!(
            todo_body(&done.content),
            "- [x] [#q1] Decide the merge order — moot\n- [ ] Other\n"
        );
    }

    #[test]
    fn open_items_referencing_resolved_ids_name_their_selectors() {
        let content = wrap(concat!(
            "- [ ] [#k3vd] Verify [#gate1] in a live pane\n",
            "- [x] [#done1] Verify [#gate1] once more\n",
            "- [ ] Decide the merge order (#gate2)\n",
            "- [ ] [#gate3] Approve the gate itself\n",
            "- [ ] Unrelated (#gate10)\n",
        ));
        let refs = open_todo_items_referencing(
            &content,
            &[
                "gate1".to_string(),
                "#gate2".to_string(),
                "gate3".to_string(),
                "nothing".to_string(),
            ],
        );
        assert_eq!(
            refs,
            vec![
                OpenTodoReference {
                    resolved_id: "gate1".into(),
                    selector: "k3vd".into()
                },
                OpenTodoReference {
                    resolved_id: "gate2".into(),
                    selector: "Decide the merge order (#gate2)".into()
                },
                OpenTodoReference {
                    resolved_id: "gate3".into(),
                    selector: "gate3".into()
                },
            ]
        );
        assert!(open_todo_items_referencing("no components", &["gate1".to_string()]).is_empty());
    }

    #[test]
    fn open_todo_actions_list_open_items_only() {
        let content = wrap(concat!(
            "- [ ] [#k3vd] Verify [#gate1] in a live pane\n",
            "- [x] [#done1] Verify [#gate1] once more\n",
            "- [/] Decide the merge order (#gate2)\n",
        ));
        assert_eq!(
            open_todo_actions(&content),
            vec![
                "Verify [#gate1] in a live pane".to_string(),
                "Decide the merge order (#gate2)".to_string(),
            ]
        );
        assert!(open_todo_actions("no components").is_empty());
    }

    #[test]
    fn unreferenced_ids_match_whole_id_tokens_only() {
        let content = "<!-- agent:todo -->\n- [ ] Verify [#gate] live\n- [ ] Check #gate2x\n<!-- /agent:todo -->\n";
        let ids = vec![
            "gate".to_string(),
            "#gate2".to_string(),
            "gate2x".to_string(),
            "gate".to_string(),
        ];
        assert_eq!(todo_unreferenced_ids(content, &ids), vec!["gate2"]);
        assert_eq!(
            todo_unreferenced_ids("<!-- agent:review -->\n<!-- /agent:review -->\n", &ids),
            vec!["gate", "gate2", "gate2x"]
        );
    }

    #[test]
    fn checklist_item_text_accepts_list_markers_and_states() {
        assert_eq!(checklist_item_text("- [ ] a"), Some("a"));
        assert_eq!(checklist_item_text("  * [x] b"), Some("b"));
        assert_eq!(checklist_item_text("+ [/] c"), Some("c"));
        assert_eq!(checklist_item_text("12. [ ] d"), Some("d"));
        assert_eq!(checklist_item_text("- plain bullet"), None);
        assert_eq!(checklist_item_text("[ ] no marker"), None);
    }
    fn links(open: &[(&str, &str)], resolved: &[&str]) -> TodoLinkIndex {
        let mut index = TodoLinkIndex::default();
        for (id, component) in open {
            index.mark_open(id, component);
        }
        for id in resolved {
            index.mark_resolved(id);
        }
        index
    }

    #[test]
    fn link_index_treats_contradictory_evidence_as_open() {
        let index = links(&[("gate1", "review")], &["gate1", "#Done2"]);
        assert_eq!(index.open_component("#GATE1"), Some("review"));
        assert!(!index.is_resolved("gate1"), "open evidence wins");
        assert!(index.is_resolved("done2"));
        assert!(index.knows("gate1") && index.knows("done2"));
        assert!(!index.knows("k3vd"));
    }

    /// `#todoaddidhijack`: a todo citing exactly one open tracked item gets a
    /// minted own id and LINKS to the item through the canonical source
    /// locator; it never takes the item's id.
    #[test]
    fn add_links_the_one_open_review_or_backlog_item_the_action_cites_by_locator() {
        let index = links(&[("gate1", "review"), ("work", "backlog")], &[]);
        let review = add_linked(&wrap(""), "Verify [#gate1] in a live pane", &index).unwrap();
        assert!(review.inserted);
        let id = review.id.clone().unwrap();
        assert_ne!(id, "gate1", "a todo never takes a tracked item's id");
        assert!(id.starts_with('t'), "{id}");
        assert_eq!(review.source, Some(("gate1".to_string(), "review".to_string())));
        assert_eq!(
            todo_body(&review.content),
            format!("- [ ] [#{id}] Verify [#gate1] in a live pane (source: agent:review [#gate1])\n")
        );
        let backlog = add_linked(&wrap(""), "Grant CI access for (#work)", &index).unwrap();
        assert_ne!(backlog.id.as_deref(), Some("work"));
        assert_eq!(backlog.source, Some(("work".to_string(), "backlog".to_string())));
    }

    /// `#todoaddonsamecycledone`: an add evaluated in a closeout that also
    /// resolves the one item it cites never layers on it. Before the fix it
    /// took `(source: agent:backlog [#cfs])`, so the same closeout's
    /// auto-completion checked it off at once.
    #[test]
    fn add_never_layers_on_an_item_the_same_closeout_resolves() {
        let mut index = links(&[("cfs", "backlog"), ("other", "backlog")], &[]);
        index.mark_resolving_this_closeout("#CFS");
        let action = "Decide the strike policy (raised by the #cfs fix).";
        let outcome = add_linked(&wrap(""), action, &index).unwrap();
        assert!(outcome.inserted);
        assert_eq!(outcome.source, None, "no locator for finished work");
        let id = outcome.id.clone().unwrap();
        assert_eq!(
            todo_body(&outcome.content),
            format!("- [ ] [#{id}] {action}\n")
        );
        // The settling item's open todo is not a fold target either: the
        // follow-up is a separate operator decision.
        let existing = wrap("- [ ] [#k3vd] Verify #cfs live (source: agent:backlog [#cfs])\n");
        let follow = add_linked(&existing, action, &index).unwrap();
        assert!(follow.inserted && follow.source.is_none());
        assert_eq!(todo_body(&follow.content).lines().count(), 2);
        // An item the closeout does NOT resolve still layers.
        let layered = add_linked(&wrap(""), "Grant CI access for (#other)", &index).unwrap();
        assert_eq!(
            layered.source,
            Some(("other".to_string(), "backlog".to_string()))
        );
        // The exemption leaves the item's open evidence alone.
        assert_eq!(index.open_component("cfs"), Some("backlog"));
        assert!(!index.is_resolved("cfs"));
    }

    /// `#todoaddonsamecycledone`: a replay evaluated after the cited item was
    /// resolved computes no locator; it must still recognise the row an
    /// earlier pass wrote WITH one (in any checkbox state) instead of minting a
    /// second, surrogate copy (observed: `[x] [#12t8] ... (source: agent:backlog
    /// [#claimfollowstruck])` beside a fresh `[ ] [#krwr] ...`).
    #[test]
    fn replayed_add_recognises_its_row_after_the_cited_item_resolved() {
        let action = "Decide the strike policy (raised by the #cfs fix).";
        let content = wrap(&format!(
            "- [x] [#k3vd] {action} (source: agent:backlog [#cfs])\n"
        ));
        let index = links(&[], &["cfs"]);
        let replay = add_linked(&content, action, &index).unwrap();
        assert!(!replay.inserted, "a replay is a no-op");
        assert_eq!(replay.id.as_deref(), Some("k3vd"));
        assert_eq!(replay.content, content);
        // Operator prose that merely contains `source:` is not a locator.
        let prose = wrap("- [ ] [#k3vd] Ship it (source: the vendor)\n");
        let other = add_linked(&prose, "Ship it", &index).unwrap();
        assert!(
            other.inserted,
            "free-form `(source: ...)` is the operator's text"
        );
    }

    /// `#todoaddonsamecycledone`: auto-completion skips the todos the same
    /// closeout added, even when their linked work resolved.
    #[test]
    fn auto_complete_leaves_exempt_same_closeout_todos_open() {
        let mut index = links(&[], &["cfs"]);
        let content = wrap(concat!(
            "- [ ] [#k3vd] Old prompt (source: agent:backlog [#cfs])\n",
            "- [ ] [#t9z1] New follow-up (source: agent:backlog [#cfs])\n",
        ));
        index
            .resolve_todo_backlinks_from_document(&content)
            .unwrap();
        let outcome =
            auto_complete_linked_todos_except(&content, &index, &["#T9Z1".to_string()]).unwrap();
        assert_eq!(outcome.completed, vec!["k3vd".to_string()]);
        assert!(todo_body(&outcome.content).contains("- [ ] [#t9z1] New follow-up"));
    }

    #[test]
    fn add_mints_when_the_action_links_nothing_or_several_things() {
        let index = links(
            &[("gate1", "review"), ("gate2", "review"), ("cold", "icebox")],
            &["old"],
        );
        // Two open gates: the action spans both, so no single source.
        let two = add_linked(&wrap(""), "Decide #gate1 vs #gate2", &index).unwrap();
        assert_eq!(two.source, None);
        assert!(two.id.as_deref().is_some_and(|id| id.starts_with('t')));
        // An open review id plus an icebox id is two tracked items.
        let mixed = add_linked(&wrap(""), "Decide #gate1 before thawing #cold", &index).unwrap();
        assert_eq!(mixed.source, None);
        assert!(mixed.id.as_deref().is_some_and(|id| id.starts_with('t')));
        // Resolved work and unknown ids (GitHub numbers) never link.
        for action in ["Re-check (#old)", "Watch GH #267"] {
            let outcome = add_linked(&wrap(""), action, &index).unwrap();
            assert_eq!(outcome.source, None, "{action}");
            assert!(outcome.id.as_deref().is_some_and(|id| id.starts_with('t')), "{action}");
        }
    }

    #[test]
    fn add_for_an_already_linked_open_todo_folds_into_it() {
        let index = links(&[("gate1", "review")], &[]);
        // A legacy row that shares the tracked id, and a canonical row linked
        // by its locator, both already surface the gate.
        for content in [
            wrap("- [ ] [#gate1] Verify [#gate1] in a live pane\n"),
            wrap("- [ ] [#t0a] Verify in a live pane (source: agent:review [#gate1])\n"),
        ] {
            let again = add_linked(&content, "Verify [#gate1] in the 263 IDE", &index).unwrap();
            assert!(!again.inserted, "one open todo per tracked item: {content}");
            assert_eq!(again.content, content);
        }
        // Once the operator checked it, a new action for the gate is a follow-up
        // with its own minted id, still linked to the gate.
        let checked = wrap("- [x] [#gate1] Verify [#gate1] in a live pane\n");
        let follow = add_linked(&checked, "Re-verify [#gate1] after the fix", &index).unwrap();
        assert!(follow.inserted);
        assert_ne!(follow.id.as_deref(), Some("gate1"));
        assert_eq!(follow.source, Some(("gate1".to_string(), "review".to_string())));
    }

    /// `#todoaddidhijack`: an explicit id that an OPEN tracked item owns is
    /// refused with an actionable error; a custom id is kept; an id naming
    /// resolved work is moot. A row the document already holds stays a no-op,
    /// so replaying an older closeout never fails.
    #[test]
    fn add_explicit_id_refuses_open_work_keeps_custom_and_rejects_resolved_work() {
        let index = links(&[("gate1", "review"), ("work", "backlog")], &["shipped"]);
        for (id, component) in [("gate1", "review"), ("work", "backlog")] {
            let err = add_linked(&wrap(""), &format!("id={id} Verify live"), &index).unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains(&format!("already owned by the open agent:{component} item [#{id}]"))
                    && msg.contains("#todoaddidhijack")
                    && msg.contains("id=<todo-id>"),
                "{msg}"
            );
        }
        // Replaying an add a legacy row already satisfies stays a no-op /
        // locator upgrade rather than an error.
        let canonical = wrap("- [ ] [#gate1] Verify live (source: agent:review [#gate1])\n");
        let repeated = add_linked(&canonical, "id=gate1 Verify live", &index).unwrap();
        assert!(!repeated.inserted);
        assert_eq!(repeated.content, canonical);
        let legacy = wrap("- [ ] [#gate1] Verify live\n");
        let upgraded = add_linked(&legacy, "id=gate1 Verify live", &index).unwrap();
        assert!(upgraded.inserted && upgraded.source_reference_added);
        assert_eq!(upgraded.content, canonical);
        let custom = add_linked(&wrap(""), "id=rotate Rotate the key", &index).unwrap();
        assert_eq!(custom.source, None);
        assert_eq!(custom.id.as_deref(), Some("rotate"));
        let err = add_linked(&wrap(""), "id=shipped Verify the release", &index).unwrap_err();
        assert!(err.to_string().contains("already resolved"), "{err}");
    }

    /// `#todoaddidhijack`, the exact live 2026-10-26 shape: an explicit
    /// `id=jb339r1decision` whose action mentions the open backlog item
    /// `#jbshrinkrestarts` must keep its explicit id. The cited item stays a
    /// reference (linked by the canonical locator), never the todo's identity.
    #[test]
    fn todoaddidhijack_explicit_id_wins_over_a_cited_open_backlog_id() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [/] [#jbshrinkrestarts] Shrink JetBrains restarts; report the benchmark\n",
            "<!-- /agent:backlog -->\n",
            "<!-- agent:todo -->\n",
            "<!-- /agent:todo -->\n",
        );
        let index = links(&[("jbshrinkrestarts", "backlog")], &[]);
        let outcome = add_linked(
            content,
            "id=jb339r1decision Decide whether to queue GH #339 shell stage R1 (restart-free shell) once #jbshrinkrestarts reports its benchmark and moves. Same decision: restate the \"Exact-263 decision\" reopen rule",
            &index,
        )
        .unwrap();
        assert!(outcome.inserted);
        assert_eq!(outcome.id.as_deref(), Some("jb339r1decision"));
        assert_eq!(
            outcome.source,
            Some(("jbshrinkrestarts".to_string(), "backlog".to_string()))
        );
        let body = todo_body(&outcome.content);
        assert!(
            body.starts_with("- [ ] [#jb339r1decision] Decide whether to queue GH #339"),
            "{body}"
        );
        assert!(!body.contains("[ ] [#jbshrinkrestarts]"), "{body}");
        assert!(
            body.trim_end()
                .ends_with("(source: agent:backlog [#jbshrinkrestarts])"),
            "{body}"
        );
        // A second, different decision about the same item with its own
        // explicit id is a second row: the explicit id wins over the fold.
        let second = add_linked(
            &outcome.content,
            "id=jb339r2decision Decide R2 once #jbshrinkrestarts reports",
            &index,
        )
        .unwrap();
        assert!(second.inserted);
        assert_eq!(second.id.as_deref(), Some("jb339r2decision"));
        // And without an explicit id the same citation mints, never the
        // tracked id.
        let minted = add_linked(content, "Decide once #jbshrinkrestarts reports", &index).unwrap();
        assert!(minted.inserted);
        assert_ne!(minted.id.as_deref(), Some("jbshrinkrestarts"));
    }

    #[test]
    fn legacy_source_qualification_audits_and_migrates_explicit_and_implicit_links() {
        let index = links(
            &[("gate1", "review"), ("work", "backlog"), ("cold", "icebox")],
            &[],
        );
        let content = wrap(concat!(
            "- [ ] [#gate1] Decide the launcher policy\n",
            "- [ ] [#todo2] Grant access for [#work]\n",
            "- [ ] [#cold] Decide thawing (source: agent:review [#cold])\n",
            "- [ ] [#many] Decide [#gate1] before [#work]\n",
            "- [ ] [#done] Already canonical (source: agent:review [#gate1])\n",
        ));
        let outcome = qualify_todo_source_references(&content, &index, &|_| true).unwrap();
        assert_eq!(outcome.findings.len(), 3, "{:?}", outcome.findings);
        assert_eq!(outcome.qualified, vec!["gate1", "todo2", "cold"]);
        assert_eq!(
            todo_body(&outcome.content),
            concat!(
                "- [ ] [#gate1] Decide the launcher policy (source: agent:review [#gate1])\n",
                "- [ ] [#todo2] Grant access for [#work] (source: agent:backlog [#work])\n",
                "- [ ] [#cold] Decide thawing (source: agent:icebox [#cold])\n",
                "- [ ] [#many] Decide [#gate1] before [#work]\n",
                "- [ ] [#done] Already canonical (source: agent:review [#gate1])\n",
            )
        );
        let again = qualify_todo_source_references(&outcome.content, &index, &|_| true).unwrap();
        assert!(again.findings.is_empty());
        assert_eq!(again.content, outcome.content);

        let held = qualify_todo_source_references(&content, &index, &|_| false).unwrap();
        assert_eq!(held.findings.len(), 3);
        assert!(held.qualified.is_empty());
        assert_eq!(held.content, content);
    }

    /// `#todoaddidhijack`: a todo surfacing an operator decision on one parked
    /// icebox item links to it like review and backlog items, with a minted
    /// own id; an explicit id naming the icebox item is refused.
    #[test]
    fn add_links_the_one_icebox_item_the_action_cites() {
        let index = links(&[("cold", "icebox"), ("gate1", "review")], &[]);
        let outcome = add_linked(&wrap(""), "Thaw or drop (#cold)?", &index).unwrap();
        assert!(outcome.inserted);
        let id = outcome.id.clone().unwrap();
        assert_ne!(id, "cold");
        assert_eq!(outcome.source, Some(("cold".to_string(), "icebox".to_string())));
        assert_eq!(
            todo_body(&outcome.content),
            format!("- [ ] [#{id}] Thaw or drop (#cold)? (source: agent:icebox [#cold])\n")
        );
        // An unknown id (a GitHub number) beside the one tracked id does not
        // count as a second tracked item.
        let gh = add_linked(&wrap(""), "Verify GH #267 against [#gate1]", &index).unwrap();
        assert_eq!(gh.source, Some(("gate1".to_string(), "review".to_string())));
        assert_ne!(gh.id.as_deref(), Some("gate1"));
        let err = add_linked(&wrap(""), "id=cold Decide whether to thaw", &index).unwrap_err();
        assert!(err.to_string().contains("agent:icebox item [#cold]"), "{err}");
    }

    /// `#todoaddidhijack`: a caller-chosen `id=` naming no tracked work is
    /// ALWAYS used verbatim, whatever the action cites (this reverses the
    /// `#todoidlayer` override that let a cited id replace it).
    #[test]
    fn explicit_custom_id_always_wins_over_cited_tracked_ids() {
        let index = links(
            &[("jb262dynlauncherrecommen", "review"), ("gate2", "review")],
            &[],
        );
        let one = add_linked(
            &wrap(""),
            "id=tdlauncher Decide the launcher policy (#jb262dynlauncherrecommen)",
            &index,
        )
        .unwrap();
        assert!(one.inserted);
        assert_eq!(one.id.as_deref(), Some("tdlauncher"));
        assert_eq!(
            todo_body(&one.content),
            "- [ ] [#tdlauncher] Decide the launcher policy (#jb262dynlauncherrecommen) (source: agent:review [#jb262dynlauncherrecommen])\n"
        );
        // An open todo already linked to the same item does not swallow a new
        // explicit id either.
        let again = add_linked(
            &one.content,
            "id=tdother Re-decide (#jb262dynlauncherrecommen)",
            &index,
        )
        .unwrap();
        assert!(again.inserted);
        assert_eq!(again.id.as_deref(), Some("tdother"));
        // Several tracked items, or none: verbatim, no locator.
        let several = add_linked(
            &wrap(""),
            "id=tdpair Decide #jb262dynlauncherrecommen vs #gate2",
            &index,
        )
        .unwrap();
        assert_eq!(several.id.as_deref(), Some("tdpair"));
        assert_eq!(several.source, None);
        let none = add_linked(&wrap(""), "id=tdkey Rotate the deploy key", &index).unwrap();
        assert_eq!(none.id.as_deref(), Some("tdkey"));
        // A bracketed `[#id]` prefix is the same explicit request.
        let bracket = add_linked(&wrap(""), "[#tdbracket] Verify [#gate2] live", &index).unwrap();
        assert_eq!(bracket.id.as_deref(), Some("tdbracket"));
    }

    /// `#todoaddidhijack`: the operator checking a todo linked to an open
    /// icebox item is a signal for the agent, never an automatic move or close;
    /// when the icebox item is resolved the open linked todo auto-completes.
    /// Both the canonical locator row (what `--todo-add` writes now) and a
    /// legacy row sharing the id (written by `#todoidlayer`) take these paths.
    #[test]
    fn icebox_link_joins_the_operator_done_and_auto_complete_paths() {
        for (own, row) in [
            ("t0x", "[#t0x] Thaw or drop (#cold)? (source: agent:icebox [#cold])"),
            ("cold", "[#cold] Thaw or drop (#cold)? (source: agent:icebox [#cold])"),
        ] {
            let open = links(&[("cold", "icebox")], &[]);
            let checked = wrap(&format!("- [x] {row}\n"));
            let findings = todo_link_findings(&checked, &open).unwrap();
            assert_eq!(findings.len(), 1, "{findings:?}");
            assert_eq!(findings[0].kind, TodoLinkFindingKind::OperatorDoneIceboxOpen);
            assert_eq!(findings[0].kind.as_str(), "operator_done_icebox_open");
            assert_eq!(findings[0].ids, vec!["cold".to_string()]);
            let pending = wrap(&format!("- [ ] {row}\n"));
            assert!(todo_link_findings(&pending, &open).unwrap().is_empty());
            let mut resolved = links(&[], &["cold"]);
            resolved.resolve_todo_backlinks_from_document(&pending).unwrap();
            let done = auto_complete_linked_todos(&pending, &resolved).unwrap();
            assert_eq!(done.completed, vec![own.to_string()]);
            assert_eq!(todo_body(&done.content), format!("- [x] {row}\n"));
        }
    }

    /// `#todoaddidhijack`: id backfill mints for an open id-less row citing
    /// one open icebox item (the locator pass links it), and for a row citing
    /// two tracked items.
    #[test]
    fn backfill_never_takes_a_cited_tracked_id() {
        let content = wrap(concat!(
            "- [ ] Thaw or drop (#cold)?\n",
            "- [ ] Decide #cold before #gate1\n",
        ));
        let index = links(&[("cold", "icebox"), ("gate1", "review")], &[]);
        let outcome = assign_missing_todo_ids(
            &content,
            &test_id,
            &std::collections::HashSet::new(),
            &index,
            &|_| true,
        )
        .unwrap();
        assert_eq!(outcome.assigned.len(), 2);
        for assignment in &outcome.assigned {
            assert!(assignment.id.starts_with('t'), "{assignment:?}");
        }
        let qualified =
            qualify_todo_source_references(&outcome.content, &index, &|_| true).unwrap();
        assert_eq!(
            todo_body(&qualified.content),
            format!(
                "- [ ] [#{}] Thaw or drop (#cold)? (source: agent:icebox [#cold])\n- [ ] [#{}] Decide #cold before #gate1\n",
                outcome.assigned[0].id, outcome.assigned[1].id
            )
        );
    }

    #[test]
    fn minted_ids_avoid_every_document_token_and_known_link_id() {
        let action = "Rotate key";
        let d = action.len() % 10;
        // `t0d` is cited outside agent:todo, `t1d` is a resolved (archived) id.
        let content = format!(
            "<!-- agent:exchange -->\nsee #t0{d}\n<!-- /agent:exchange -->\n<!-- agent:todo -->\n<!-- /agent:todo -->\n"
        );
        let index = links(&[], &[&format!("t1{d}")]);
        let outcome = add_linked(&content, action, &index).unwrap();
        assert_eq!(outcome.id, Some(format!("t2{d}")));
    }

    #[test]
    fn preserve_checked_keeps_an_operator_check_a_stale_patch_carries_open() {
        let before = "- [x] [#a] Rotate key\n- [x] Call the vendor\n- [ ] [#b] Publish\n";
        let after = "- [ ] [#a] Rotate key — note\n- [ ] Call the vendor\n- [ ] [#b] Publish\n- [ ] [#c] New\n";
        let (body, restored) = preserve_checked_todo_items(before, after);
        assert_eq!(restored, 2);
        assert_eq!(
            body,
            "- [x] [#a] Rotate key — note\n- [x] Call the vendor\n- [ ] [#b] Publish\n- [ ] [#c] New\n"
        );
        // Nothing checked before: the patch is untouched.
        assert_eq!(preserve_checked_todo_items("- [ ] x\n", after), (after.to_string(), 0));
        // A patch that drops the id while reproducing the checked item still keeps the check.
        let (body, restored) = preserve_checked_todo_items("- [x] [#a] Rotate key\n", "- [ ] Rotate key\n");
        assert_eq!((body.as_str(), restored), ("- [x] Rotate key\n", 1));
    }

    #[test]
    fn new_idless_items_lists_only_patch_added_items_without_an_id() {
        let before = "- [ ] Call the vendor\n- [ ] [#a] Rotate\n";
        let after = "- [ ] Call the vendor\n- [ ] [#a] Rotate\n- [ ] [#n] Has id\n- [ ] Publish  draft\n";
        assert_eq!(new_idless_todo_items(before, after), vec!["Publish draft".to_string()]);
    }

    #[test]
    fn link_findings_map_each_state_to_one_documented_kind() {
        let index = links(
            &[("gate1", "review"), ("work", "backlog"), ("open2", "review")],
            &["shipped", "gone"],
        );
        let content = wrap(concat!(
            "- [x] [#gate1] Verify the gate\n",
            "- [x] [#work] Grant access\n",
            "- [ ] [#shipped] Verify the release\n",
            "- [ ] [#k3vd] Re-check (#gone)\n",
            "- [ ] [#k4vd] Decide (#gone) and (#open2)\n",
            "- [ ] [#open2] Pending operator\n",
            "- [ ] Watch GH #267\n",
            "- [x] [#k5vd] Done, unlinked\n",
        ));
        let findings = todo_link_findings(&content, &index).unwrap();
        let summary: Vec<(&str, Vec<String>, String)> = findings
            .iter()
            .map(|f| (f.kind.as_str(), f.ids.clone(), f.selector.clone()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("legacy_source_unqualified", vec!["gate1".to_string()], "gate1".to_string()),
                ("operator_done_review_open", vec!["gate1".to_string()], "gate1".to_string()),
                ("legacy_source_unqualified", vec!["work".to_string()], "work".to_string()),
                ("operator_done_backlog_open", vec!["work".to_string()], "work".to_string()),
                ("linked_work_resolved", vec!["shipped".to_string()], "shipped".to_string()),
                ("cited_work_resolved", vec!["gone".to_string()], "k3vd".to_string()),
                ("legacy_source_unqualified", vec!["open2".to_string()], "k4vd".to_string()),
                ("legacy_source_unqualified", vec!["open2".to_string()], "open2".to_string()),
            ]
        );
        let missing = todo_link_findings("no components", &index).unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].kind, TodoLinkFindingKind::TrackedWorkMissingTodoBacklink);
        assert_eq!(missing[0].ids, vec!["work".to_string()]);
    }

    #[test]
    fn auto_complete_checks_only_open_items_whose_own_id_is_resolved() {
        let index = links(&[("open2", "review")], &["shipped", "gone"]);
        let content = wrap(concat!(
            "- [ ] [#shipped] Verify the release\n",
            "- [ ] [#k3vd] Re-check (#gone)\n",
            "- [ ] [#open2] Pending operator\n",
            "- [ ] Re-check gone\n",
            "- [x] [#shipped] Verify the release — done\n",
        ));
        let outcome = auto_complete_linked_todos(&content, &index).unwrap();
        assert_eq!(outcome.completed, vec!["shipped".to_string()]);
        assert_eq!(outcome.reconciled, 1, "the checked copy folds into the item");
        assert_eq!(
            todo_body(&outcome.content),
            concat!(
                "- [x] [#shipped] Verify the release — done\n",
                "- [ ] [#k3vd] Re-check (#gone)\n",
                "- [ ] [#open2] Pending operator\n",
                "- [ ] Re-check gone\n",
            )
        );
        // Idempotent, and a no-op without evidence.
        let again = auto_complete_linked_todos(&outcome.content, &index).unwrap();
        assert!(again.completed.is_empty());
        assert_eq!(again.content, outcome.content);
        let none = auto_complete_linked_todos(&content, &TodoLinkIndex::default()).unwrap();
        assert_eq!(none.content, content);
    }

    #[test]
    fn tracked_work_todo_backlinks_are_explicit_trailing_metadata() {
        assert_eq!(
            tracked_work_todo_ids("Ship it (todos: [#Operator-A] [#todo-b] [#operator-a])"),
            vec!["operator-a".to_string(), "todo-b".to_string()]
        );
        assert!(tracked_work_todo_ids("Ship it; see [#operator-a]").is_empty());
        assert!(tracked_work_todo_ids("Ship it (todos: [#operator-a]) trailing").is_empty());
        assert!(tracked_work_todo_ids("Ship it (todos: [#bad_id])").is_empty());
    }

    #[test]
    fn open_backlog_and_icebox_require_a_backlink_to_an_existing_todo() {
        let mut index = links(
            &[("unlinked", "backlog"), ("dangling", "icebox"), ("gate", "review")],
            &[],
        );
        index.mark_todo_backlink("dangling", "missing-todo");
        let findings = todo_link_findings("no todo component", &index).unwrap();
        let missing = findings
            .iter()
            .filter(|finding| {
                finding.kind == TodoLinkFindingKind::TrackedWorkMissingTodoBacklink
            })
            .map(|finding| finding.ids[0].clone())
            .collect::<Vec<_>>();
        assert_eq!(missing, vec!["dangling".to_string(), "unlinked".to_string()]);

        let content = wrap(concat!(
            "- [ ] [#approval] Approve (source: agent:backlog [#unlinked])\n",
            "- [ ] [#missing-todo] Decide (source: agent:icebox [#dangling])\n",
        ));
        assert!(todo_link_findings(&content, &index)
            .unwrap()
            .iter()
            .all(|finding| finding.kind != TodoLinkFindingKind::TrackedWorkMissingTodoBacklink));
    }

    #[test]
    fn backlink_completion_waits_for_all_sources_and_closes_every_linked_todo() {
        let content = wrap(concat!(
            "- [ ] [#operator-a] Approve rollout\n",
            "- [ ] [#operator-b] Confirm telemetry\n",
            "- [ ] [#unlinked] Read the release note\n",
        ));
        let mut index = links(&[("work-b", "backlog")], &["work-a", "work-c"]);
        index.mark_todo_backlink("work-a", "operator-a");
        index.mark_todo_backlink("work-b", "operator-a");
        index.mark_todo_backlink("work-c", "operator-b");
        index.mark_todo_backlink("work-c", "operator-a");

        let first = auto_complete_linked_todos(&content, &index).unwrap();
        assert_eq!(first.completed, vec!["operator-b".to_string()]);
        assert!(todo_body(&first.content).contains("- [ ] [#operator-a] Approve rollout"));
        assert!(todo_body(&first.content).contains("- [x] [#operator-b] Confirm telemetry"));

        let mut resolved = links(&[], &["work-a", "work-b", "work-c"]);
        resolved.mark_todo_backlink("work-a", "operator-a");
        resolved.mark_todo_backlink("work-b", "operator-a");
        resolved.mark_todo_backlink("work-c", "operator-b");
        resolved.mark_todo_backlink("work-c", "operator-a");
        let final_outcome = auto_complete_linked_todos(&first.content, &resolved).unwrap();
        assert_eq!(final_outcome.completed, vec!["operator-a".to_string()]);
        assert!(todo_body(&final_outcome.content).contains("- [x] [#operator-a] Approve rollout"));
        assert!(todo_body(&final_outcome.content).contains("- [ ] [#unlinked] Read the release note"));
    }

    #[test]
    fn todo_source_locator_resolves_a_backlink_and_checked_todo_unblocks_source() {
        let checked = wrap(
            "- [x] [#approval] Approve rollout (source: agent:backlog [#ship])\n",
        );
        let mut index = links(&[("ship", "backlog")], &[]);
        index.resolve_todo_backlinks_from_document(&checked).unwrap();
        assert_eq!(index.todo_ids_for_tracked("ship"), vec!["approval".to_string()]);

        let findings = todo_link_findings(&checked, &index).unwrap();
        assert!(findings.iter().any(|finding| {
            finding.kind == TodoLinkFindingKind::OperatorDoneBacklogOpen
                && finding.ids == vec!["ship".to_string()]
        }));
    }

    /// `#sdktododone`: the anonymized `tasks/sdk.md` todo shapes from
    /// 2026-10-15. Most rows wrote their own id bare (`- [ ] #tdfoo action`), a
    /// workaround for the pre-0.35.510 shadow-backlog guard; a few use the
    /// bracketed `[#tdfoo]`. A row's own id is its LEADING id in either
    /// spelling; a leading GitHub number stays a citation.
    fn sdk_todo_body() -> String {
        wrap(concat!(
            "- [x] #tdactionsbudget Raise the Actions spending limit (#sdkmainciverify)\n",
            "- [ ] #tdclireview Review haiven-cli #2 and make the login decision (#cligoshell)\n",
            "- [ ] #tdsdkdevrename Rename haiven-sdk and haiven-cli `dev` to `sdk-dev`\n",
            "- [ ] #tdcontracts041 Decide the contracts 0.4.1 bump (split from #tdsdkdevrename; #publicbundleadopt)\n",
            "- [ ] [#tdsdkwebui] Decide the SDK web UI shape (#sdkjiraweb)\n",
            "- [ ] #157 needs review before Friday\n",
        ))
    }

    #[test]
    fn sdktododone_bare_and_bracketed_leading_ids_are_own_ids() {
        let bare =
            parse_todo_line("- [ ] #tdclireview Review haiven-cli #2 (#cligoshell)").unwrap();
        assert_eq!(bare.own_id, Some("tdclireview"));
        assert!(bare.own_id_bare);
        assert_eq!(bare.action, "Review haiven-cli #2 (#cligoshell)");
        let bracketed = parse_todo_line("- [ ] [#tdsdkwebui] Decide (#sdkjiraweb)").unwrap();
        assert_eq!(bracketed.own_id, Some("tdsdkwebui"));
        assert!(!bracketed.own_id_bare);
        // A leading GitHub number, a glued token, or an id-only row without
        // a letter is not an own id.
        for line in [
            "- [ ] #157 needs review",
            "- [ ] #tdfoo: see the doc",
            "- [ ] #2026-10-15 cut",
        ] {
            let parsed = parse_todo_line(line).unwrap();
            assert_eq!(parsed.own_id, None, "{line}");
            assert!(!parsed.own_id_bare, "{line}");
        }
        let content = sdk_todo_body();
        assert!(todo_owns_id(&content, "tdsdkdevrename"));
        assert!(todo_owns_id(&content, "#tdsdkwebui"));
        assert!(todo_owns_id(&content, "tdactionsbudget"));
        assert!(
            !todo_owns_id(&content, "publicbundleadopt"),
            "cited, not owned"
        );
        assert!(!todo_owns_id(&content, "157"));
    }

    /// `#sdktododone`: `--todo-done <bare own id>` selects the row that OWNS the
    /// id even when another open row cites it (`split from #tdsdkdevrename`),
    /// and flips only the checkbox: the bare spelling survives byte-for-byte.
    /// Before the fix the selector fell through to "cite that id" and failed
    /// as ambiguous between the two rows.
    #[test]
    fn sdktododone_todo_done_by_bare_own_id_flips_only_the_checkbox() {
        let content = sdk_todo_body();
        let outcome =
            set_todo_item_state_in_document(&content, "tdsdkdevrename", TodoTransition::Done)
                .unwrap();
        assert!(outcome.changed);
        assert_eq!(outcome.label, "#tdsdkdevrename");
        assert_eq!(
            outcome.content,
            content.replace(
                "- [ ] #tdsdkdevrename Rename",
                "- [x] #tdsdkdevrename Rename"
            )
        );
        let again = set_todo_item_state_in_document(
            &outcome.content,
            "#tdsdkdevrename",
            TodoTransition::Done,
        )
        .unwrap();
        assert!(!again.changed);
        // The bracketed row keeps its brackets.
        let web =
            set_todo_item_state_in_document(&content, "tdsdkwebui", TodoTransition::Done).unwrap();
        assert!(
            todo_body(&web.content).contains("- [x] [#tdsdkwebui] Decide the SDK web UI shape")
        );
    }

    /// `#sdktododone`: the 23:30 sync ran `--todo-edit "tdclireview=Re-run ..."`
    /// on bare-id rows, and each rewritten row LOST its id (the bare `#id` was
    /// part of the replaced action). An edit keeps the bare own id, and text
    /// that repeats it does not double it.
    #[test]
    fn sdktododone_todo_edit_keeps_a_bare_own_id() {
        let content = sdk_todo_body();
        let edited = edit_todo_item_in_document(
            &content,
            "tdclireview",
            "Re-run haiven-cli #2 CI, then review it (#cligoshell)",
        )
        .unwrap();
        assert!(
            todo_body(&edited.content).contains(
                "- [ ] #tdclireview Re-run haiven-cli #2 CI, then review it (#cligoshell)\n"
            ),
            "{}",
            edited.content
        );
        let repeated = edit_todo_item_in_document(
            &content,
            "tdclireview",
            "#tdclireview Re-run haiven-cli #2 CI (#cligoshell)",
        )
        .unwrap();
        assert!(
            todo_body(&repeated.content)
                .contains("- [ ] #tdclireview Re-run haiven-cli #2 CI (#cligoshell)\n"),
            "{}",
            repeated.content
        );
        // Every other row is untouched.
        assert_eq!(
            edited.content.replace(
                "#tdclireview Re-run haiven-cli #2 CI, then review it (#cligoshell)",
                "#tdclireview Review haiven-cli #2 and make the login decision (#cligoshell)"
            ),
            content
        );
    }

    /// `#sdktododone`: the closeout auto-check ("own id resolved") reaches a bare
    /// own id, and a row that only CITES the resolved id stays open.
    #[test]
    fn sdktododone_auto_complete_reaches_bare_own_ids_only() {
        let index = links(&[], &["tdsdkdevrename", "publicbundleadopt"]);
        let content = sdk_todo_body();
        let outcome = auto_complete_linked_todos(&content, &index).unwrap();
        assert_eq!(outcome.completed, vec!["tdsdkdevrename".to_string()]);
        let body = todo_body(&outcome.content);
        assert!(body.contains("- [x] #tdsdkdevrename Rename"), "{body}");
        assert!(
            body.contains("- [ ] #tdcontracts041 Decide the contracts 0.4.1 bump"),
            "a cite-only row is surfaced, never completed: {body}"
        );
        let findings = todo_link_findings(&outcome.content, &index).unwrap();
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].kind, TodoLinkFindingKind::CitedWorkResolved);
        assert_eq!(findings[0].selector, "tdcontracts041");
    }

    /// `#todoidbackfill`: the anonymized `tasks/docs.md` row from 2026-10-15.
    /// The operator re-authenticated the Drive connector, but the id-less row
    /// stayed open: nothing could address it by id, and the closeout's
    /// "own id resolved" check never saw it.
    fn docs_todo_doc() -> String {
        concat!(
            "<!-- agent:exchange -->\nhi\n<!-- /agent:exchange -->\n",
            "<!-- agent:backlog -->\n- [ ] [#httpsdocsgoogle] Does the Google Doc change PR #51?\n<!-- /agent:backlog -->\n",
            "<!-- agent:review -->\n- [/] [#cligoshell] CLI shell review\n<!-- /agent:review -->\n",
            "<!-- agent:todo -->\n",
            "- [ ] Re-authenticate the Google Drive connector (/mcp, sign in) or paste the Google Doc text into tasks/docs.md so #httpsdocsgoogle can proceed; or say skip\n",
            "- [ ] #tdsdk157 Review/approve haiven-sdk #157 (#cligoshell)\n",
            "- [ ] [#tdwebui] Decide the SDK web UI shape (#cligoshell)\n",
            "- [ ] Re-run haiven-cli #2 CI, then review it (#cligoshell)\n",
            "- [ ] Optional: delete the clean inspection worktrees\n",
            "- [x] Already done by me (#httpsdocsgoogle)\n",
            "  - [ ] Nested follow-up with no id\n",
            "- [ ] Call the vendor\n",
            "- [ ] Call the vendor\n",
            "<!-- /agent:todo -->\n",
        )
        .to_string()
    }

    fn backfill_links(content: &str) -> TodoLinkIndex {
        let _ = content;
        links(
            &[("httpsdocsgoogle", "backlog"), ("cligoshell", "review")],
            &[],
        )
    }

    #[test]
    fn todoidbackfill_assigns_ids_to_id_less_rows_only_and_keeps_their_text() {
        let content = docs_todo_doc();
        let index = backfill_links(&content);
        let outcome = assign_missing_todo_ids(
            &content,
            &test_id,
            &std::collections::HashSet::new(),
            &index,
            &|_| true,
        )
        .unwrap();
        let ids: Vec<&str> = outcome.assigned.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids.len(), 5, "{ids:?}");
        // `#todoaddidhijack`: every backfilled id is minted — the docs.md row
        // cites the open backlog item `#httpsdocsgoogle` and the re-run row
        // cites the open gate `#cligoshell`, but neither takes that id.
        for id in &ids {
            assert!(
                id.starts_with('t') && !["httpsdocsgoogle", "cligoshell"].contains(id),
                "{ids:?}"
            );
        }
        let body = todo_body(&outcome.content);
        assert_eq!(
            body,
            format!(
                concat!(
                    "- [ ] [#{}] Re-authenticate the Google Drive connector (/mcp, sign in) or paste the Google Doc text into tasks/docs.md so #httpsdocsgoogle can proceed; or say skip\n",
                    "- [ ] #tdsdk157 Review/approve haiven-sdk #157 (#cligoshell)\n",
                    "- [ ] [#tdwebui] Decide the SDK web UI shape (#cligoshell)\n",
                    "- [ ] [#{}] Re-run haiven-cli #2 CI, then review it (#cligoshell)\n",
                    "- [ ] [#{}] Optional: delete the clean inspection worktrees\n",
                    "- [x] [#{}] Already done by me (#httpsdocsgoogle)\n",
                    "  - [ ] [#{}] Nested follow-up with no id\n",
                    "- [ ] Call the vendor\n",
                    "- [ ] Call the vendor\n",
                ),
                ids[0], ids[1], ids[2], ids[3], ids[4]
            ),
            "only the id token is inserted; owned rows and duplicate copies are untouched"
        );
        // Idempotent: a second pass assigns nothing.
        let again = assign_missing_todo_ids(
            &outcome.content,
            &test_id,
            &std::collections::HashSet::new(),
            &index,
            &|_| true,
        )
        .unwrap();
        assert!(again.assigned.is_empty());
        assert_eq!(again.content, outcome.content);
    }

    #[test]
    fn todoidbackfill_respects_the_typing_gate_and_minted_ids_avoid_known_ids() {
        let content = docs_todo_doc();
        let index = backfill_links(&content);
        let gated = assign_missing_todo_ids(
            &content,
            &test_id,
            &std::collections::HashSet::new(),
            &index,
            &|line| !line.contains("Optional"),
        )
        .unwrap();
        assert!(
            todo_body(&gated.content)
                .contains("- [ ] Optional: delete the clean inspection worktrees\n"),
            "a row the gate holds stays id-less this pass"
        );
        // A minted candidate colliding with a reserved or cited id is skipped.
        let first = test_id("Optional: delete the clean inspection worktrees", 0);
        let reserved: std::collections::HashSet<String> = [first.clone()].into_iter().collect();
        let outcome =
            assign_missing_todo_ids(&content, &test_id, &reserved, &index, &|_| true).unwrap();
        let optional = outcome
            .assigned
            .iter()
            .find(|a| a.action.starts_with("Optional"))
            .unwrap();
        assert_ne!(optional.id, first);
    }

    /// `#todoidbackfill`: once the row owns a (minted) id and preflight's
    /// locator pass links it, the existing completion paths reach it — the
    /// operator's check reports the prerequisite done, and resolving the
    /// backlog item checks the row off.
    #[test]
    fn todoidbackfill_rows_join_the_todoids_completion_paths() {
        let content = docs_todo_doc();
        let assigned = assign_missing_todo_ids(
            &content,
            &test_id,
            &std::collections::HashSet::new(),
            &backfill_links(&content),
            &|_| true,
        )
        .unwrap();
        let docs_id = assigned.assigned[0].id.clone();
        let qualified =
            qualify_todo_source_references(&assigned.content, &backfill_links(&content), &|_| true)
                .unwrap();
        let mut resolved = links(&[("cligoshell", "review")], &["httpsdocsgoogle"]);
        resolved
            .resolve_todo_backlinks_from_document(&qualified.content)
            .unwrap();
        let done = auto_complete_linked_todos(&qualified.content, &resolved).unwrap();
        assert!(done.completed.contains(&docs_id), "{:?}", done.completed);
        assert!(
            todo_body(&done.content).starts_with(&format!("- [x] [#{docs_id}] Re-authenticate")),
            "{}",
            todo_body(&done.content)
        );
        let checked = qualified.content.replace(
            &format!("- [ ] [#{docs_id}] Re-auth"),
            &format!("- [x] [#{docs_id}] Re-auth"),
        );
        let findings = todo_link_findings(&checked, &backfill_links(&checked)).unwrap();
        assert!(
            findings
                .iter()
                .any(|f| f.kind == TodoLinkFindingKind::OperatorDoneBacklogOpen
                    && f.selector == docs_id),
            "{findings:?}"
        );
        let outcome = assigned;
        // A snapshot holding the same todo body gets the ids carried over; one
        // with an unsnapshotted operator todo edit does not.
        let carried = carry_todo_body_into_snapshot(&content, &content, &outcome.content).unwrap();
        assert_eq!(todo_body(&carried), todo_body(&outcome.content));
        let edited = content.replace("Call the vendor\n<!--", "Call the vendor today\n<!--");
        assert!(carry_todo_body_into_snapshot(&edited, &content, &outcome.content).is_none());
    }
}
