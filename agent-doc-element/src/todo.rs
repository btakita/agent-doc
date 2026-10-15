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
//!   template scaffold order (`backlog`, `todo`, `review`): directly after the
//!   last `agent:backlog` / legacy `agent:pending` component (so it lands
//!   before any `## Review` section that follows the backlog); else after the
//!   last `agent:queue`; else after the last `agent:exchange`; else at the end
//!   of the document. `agent:review` is never an anchor: inserting after it
//!   would put Todo below Review, and inserting before its marker would split
//!   the `## Review` heading from its component.
//! - [`add_todo_item_to_document`] appends one `- [ ] [#<id>] <action>` line
//!   to the end of the component body, creating the component first when
//!   absent. The leading `[#<id>]` is the item's own stable id (derived from
//!   the action, collision-free against every id the todo body cites and the
//!   document's tracked-work ids, or caller-chosen via `id=<id> ` /
//!   `[#<id>] `). It never removes, reorders, or rewrites an existing line, so
//!   every existing checklist item and every operator edit survives
//!   byte-for-byte.
//! - Adding is idempotent: when any existing checklist item (open, done, or
//!   gated) already carries the same action text, the add is a no-op. A replayed
//!   closeout therefore never duplicates a todo, and never resurrects one the
//!   operator already checked off.
//! - Only a LEADING `[#id]` is an item's own id. An id cited later in the
//!   action (`Verify [#gate] live`, `Decide X (#gate)`) is a reference to
//!   tracked work. Operator-authored and legacy items may have no own id.
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
//! ## Agentic Contracts
//! - All functions are pure: no file IO, no clock, no randomness.
//! - Placement depends only on the component layout, so the same document
//!   always yields the same insertion point.
//! - An action must be one non-empty line; multi-line text is rejected rather
//!   than silently split into several items.

use anyhow::{Context, Result};

use crate::element::{self, Component};

/// Canonical component name for the operator action checklist.
pub const TODO_COMPONENT: &str = "todo";

/// Section inserted when a document has no `agent:todo` component. It matches
/// the heading and markers of the default template scaffold.
pub const TODO_SECTION: &str = "\n## Todo\n\n<!-- agent:todo -->\n<!-- /agent:todo -->\n";

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
        element::is_backlog_component,
        |name| name == "queue",
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
/// `[#<own id>] ` token is optional. Only a LEADING bracketed id is the item's
/// own id; an id cited later in the action (`Verify [#gate] live`,
/// `... (#gate)`) is a reference to tracked work, not the item's identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TodoLine<'a> {
    pub prefix: &'a str,
    pub state: TodoState,
    pub own_id: Option<&'a str>,
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

/// Parse one checklist line of an `agent:todo` body (no trailing newline).
pub fn parse_todo_line(line: &str) -> Option<TodoLine<'_>> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let text = checklist_item_text(line)?;
    // `checklist_item_text` returns the tail after the 4-byte `[s] ` box.
    let box_start = line.len() - text.len() - 4;
    let marker = line[box_start + 1..].chars().next()?;
    let state = TodoState::from_marker(marker)?;
    let (own_id, action) = match split_leading_bracket_id(text) {
        Some((id, rest)) => (Some(id), rest),
        None => (None, text),
    };
    Some(TodoLine {
        prefix: &line[..box_start],
        state,
        own_id,
        action: action.trim_end(),
    })
}

/// Render one todo line in the canonical layout.
fn render_todo_line(prefix: &str, state: TodoState, own_id: Option<&str>, action: &str) -> String {
    let mut out = String::with_capacity(prefix.len() + action.len() + 16);
    out.push_str(prefix);
    out.push('[');
    out.push(state.marker());
    out.push_str("] ");
    if let Some(id) = own_id {
        out.push_str("[#");
        out.push_str(id);
        out.push_str("] ");
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
    /// Whether the `agent:todo` component had to be created.
    pub created_component: bool,
    /// The item's own id: the new id when inserted, or the matching existing
    /// item's id (`None` for an id-less legacy item) when the add was a no-op.
    pub id: Option<String>,
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

/// Split an add/edit input into an optional caller-chosen id and the action.
///
/// The id spellings mirror `--backlog-add`: canonical `id=<id> <action>`, or
/// the compatibility form `[#<id>] <action>`.
fn parse_todo_input(text: &str, flag: &str) -> Result<(Option<String>, String)> {
    let action = normalize_todo_action_for(text, flag)?;
    if let Some(rest) = action.strip_prefix("id=") {
        let (id, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        anyhow::ensure!(
            is_valid_todo_id(id),
            "{flag}: invalid todo id {id:?} (use letters, digits, and '-')"
        );
        let tail = tail.trim();
        anyhow::ensure!(!tail.is_empty(), "{flag} expects an action after id={id}");
        return Ok((Some(id.to_ascii_lowercase()), tail.to_string()));
    }
    if let Some((id, tail)) = split_leading_bracket_id(&action) {
        anyhow::ensure!(!tail.is_empty(), "{flag} expects an action after [#{id}]");
        return Ok((Some(id.to_ascii_lowercase()), tail.to_string()));
    }
    Ok((None, action))
}

/// Collapse whitespace so two spellings of one action compare equal.
fn normalize_action_text(action: &str) -> String {
    action.split_whitespace().collect::<Vec<_>>().join(" ")
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
/// by identity rather than by its text. `id_for(action, counter)` derives a
/// candidate id; the first candidate not already cited in the todo body and
/// not in `reserved` (the document's tracked-work ids) wins. A caller-supplied
/// `id=<id>` / `[#<id>]` prefix is used verbatim.
pub fn add_todo_item_to_document(
    content: &str,
    text: &str,
    id_for: &dyn Fn(&str, u64) -> String,
    reserved: &std::collections::HashSet<String>,
) -> Result<TodoAddOutcome> {
    let (explicit_id, action) = parse_todo_input(text, "--todo-add")?;
    let (ensured, todo, created_component) = ensure_todo_component_in_document(content)?;
    let body = todo.content(&ensured);
    let wanted = normalize_action_text(&action);
    let items: Vec<TodoLine<'_>> = body.lines().filter_map(parse_todo_line).collect();
    if let Some(existing) = items
        .iter()
        .find(|item| normalize_action_text(item.action) == wanted)
    {
        return Ok(TodoAddOutcome {
            content: ensured.clone(),
            inserted: false,
            created_component,
            id: existing.own_id.map(str::to_ascii_lowercase),
        });
    }
    let id = match explicit_id {
        Some(id) => {
            anyhow::ensure!(
                !items
                    .iter()
                    .any(|item| item.own_id.is_some_and(|own| own.eq_ignore_ascii_case(&id))),
                "--todo-add: agent:todo already has an item with id [#{id}]; use --todo-edit \"{id}=<text>\" to change it"
            );
            id
        }
        None => {
            let taken = body_id_tokens(body);
            let mut chosen = None;
            for counter in 0..256u64 {
                let candidate = id_for(&action, counter).to_ascii_lowercase();
                if is_valid_todo_id(&candidate)
                    && !taken.contains(&candidate)
                    && !reserved.contains(&candidate)
                {
                    chosen = Some(candidate);
                    break;
                }
            }
            chosen.context("--todo-add could not derive a collision-free todo id")?
        }
    };
    let mut new_body = String::with_capacity(body.len() + action.len() + 16);
    new_body.push_str(body);
    if !new_body.is_empty() && !new_body.ends_with('\n') {
        new_body.push('\n');
    }
    new_body.push_str(&render_todo_line("- ", TodoState::Open, Some(&id), &action));
    new_body.push('\n');
    Ok(TodoAddOutcome {
        content: todo.replace_content(&ensured, &new_body),
        inserted: true,
        created_component,
        id: Some(id),
    })
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
        let prefix = parse_todo_line(bare)
            .map(|line| line.prefix)
            .unwrap_or("- ");
        let mut rendered = render_todo_line(prefix, state, own_id, action);
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
    let (new_id, action) = parse_todo_input(text, flag)?;
    if let Some(new_id) = new_id {
        anyhow::ensure!(
            item.own_id.as_deref() == Some(new_id.as_str()),
            "{flag} cannot change a todo item's id (item {}, text names [#{new_id}])",
            item_label(item.own_id.as_deref(), &item.action)
        );
    }
    let label = item_label(item.own_id.as_deref(), &item.action);
    body.rewrite_line(item.line, item.state, item.own_id.as_deref(), &action);
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
        add_todo_item_to_document(content, text, &test_id, &std::collections::HashSet::new())
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
        let todo_start = updated.find("<!-- agent:todo -->").unwrap();
        let review_heading = updated.find("## Review").unwrap();
        assert!(
            backlog_end < todo_start && todo_start < review_heading,
            "{updated}"
        );
        assert!(
            updated.contains("<!-- /agent:backlog -->\n\n## Todo\n\n<!-- agent:todo -->\n<!-- /agent:todo -->\n\n## Review\n\n<!-- agent:review -->"),
            "{updated}"
        );
        // Everything outside the inserted section is byte-identical.
        assert_eq!(updated.replacen(TODO_SECTION, "", 1), content);
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
            updated.contains("<!-- /agent:backlog -->\n\n## Todo\n\n<!-- agent:todo -->\n<!-- /agent:todo -->\n## Review"),
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
            updated.contains("<!-- /agent:queue -->\n\n## Todo\n\n<!-- agent:todo -->\n<!-- /agent:todo -->\n## Review"),
            "{updated}"
        );

        let queue = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n<!-- agent:queue -->\n- do x\n<!-- /agent:queue -->\ntrailing prose\n";
        let (updated, _, _) = ensure_todo_component_in_document(queue).unwrap();
        assert!(
            updated.contains("<!-- /agent:queue -->\n\n## Todo\n\n<!-- agent:todo -->\n<!-- /agent:todo -->\ntrailing prose\n"),
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
        let outcome = add_todo_item_to_document(&content, action, &test_id, &reserved).unwrap();
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
}
