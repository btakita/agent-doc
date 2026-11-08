//! `#blockerdisregard`: an operator disregard of a blocker an agent invented.
//!
//! Queue drain still reads [`crate::queue_blocked::QueueBlockers`]. This module
//! records the exact edges, request ids, and `actor_turn_busy` catch-all the
//! operator named, then checks off invented request todos and repaints `⛔`.
//! A real operator gate, a question that cites an upstream the operator still
//! has to answer, and a harness permission prompt are not recorded.

use std::collections::{BTreeMap, BTreeSet};

use agent_doc_document::queue_projection::{
    BLOCKED_ANNOTATION_OPEN, BLOCKED_MARKER, QueueLifecycleMarker, apply_lifecycle_marker,
    apply_lifecycle_marker_with_blockers, blocked_marker_ids, lifecycle_marker,
    operator_action_ids,
};
use agent_doc_element::element;
use agent_doc_element::todo::{self, OwnedTodoIndex, TodoTransition};
use agent_doc_element_backlog::backlog;
use agent_doc_element_queue::QueueItemIdentity;
use anyhow::{Result, bail};

use crate::document_queue::project_all_prompt_operator_actions;
use crate::queue_blocked::queue_blockers;
use crate::queue_consume::project_waiting_on_you_tree;

const MARKER_PREFIX: &str = "<!-- agent-doc:blocker-disregard";
const REQUEST_PREFIX: &str = "subagent-operator-request-";
const DISPATCH_TOKEN: &str = "dispatch=actor_turn_busy";

/// What an operator command or a document gesture asks to disregard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisregardTarget {
    /// Every invented blocker, plus the `actor_turn_busy` dispatch catch-all.
    AllInvented,
    /// One queue row or request. Does not record the dispatch disregard.
    Item(String),
    /// Only the `actor_turn_busy` dispatch catch-all.
    Dispatch,
    /// Gestures already written in the document (`disregard`, `[disregard]`,
    /// `disregard blockers`).
    Gestures,
}

/// Why a blocker stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeptReason {
    RealOperatorGate,
    ClosedUpstreamAnswer,
    HarnessPermissionPrompt,
}

impl KeptReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RealOperatorGate => "real operator gate",
            Self::ClosedUpstreamAnswer => "closed upstream the operator still has to answer",
            Self::HarnessPermissionPrompt => "harness permission prompt",
        }
    }
}

/// Successor of one disregard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisregardPlan {
    pub content: String,
    pub changed: bool,
    /// Human lines for what this apply recorded (`queue a: b`, `request id`,
    /// `dispatch actor_turn_busy`).
    pub disregarded: Vec<String>,
    /// `(id, reason)` for protected blockers this apply left in place.
    pub kept: Vec<(String, String)>,
}

/// Edges, request ids, and the dispatch catch-all recorded in the document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueDisregards {
    edges: BTreeMap<String, BTreeSet<String>>,
    requests: BTreeSet<String>,
    dispatch_actor_turn_busy: bool,
}

impl QueueDisregards {
    pub fn from_content(content: &str) -> Self {
        let mut record = Self::default();
        for line in content.lines() {
            if let Some(body) = marker_body(line) {
                record.merge_body(body);
            }
        }
        record
    }

    /// Whether `(item, dep)` is an edge the operator disregarded.
    pub fn ignores(&self, item: &str, dep: &str) -> bool {
        let Some(item) = norm_id(item) else {
            return false;
        };
        let Some(dep) = norm_id(dep) else {
            return false;
        };
        self.edges
            .get(&item)
            .is_some_and(|deps| deps.contains(&dep))
    }

    fn is_empty(&self) -> bool {
        self.edges.is_empty() && self.requests.is_empty() && !self.dispatch_actor_turn_busy
    }

    fn has_item(&self, id: &str) -> bool {
        self.edges.contains_key(id) || self.requests.contains(id)
    }

    fn merge_body(&mut self, body: &str) {
        for token in body.split_whitespace() {
            if token == DISPATCH_TOKEN {
                self.dispatch_actor_turn_busy = true;
                continue;
            }
            if let Some(id) = token.strip_prefix("request=") {
                if let Some(id) = norm_id(id) {
                    self.requests.insert(id);
                }
                continue;
            }
            let Some(spec) = token.strip_prefix("queue=") else {
                continue;
            };
            let Some((item, deps)) = spec.split_once(':') else {
                continue;
            };
            let Some(item) = norm_id(item) else {
                continue;
            };
            let slot = self.edges.entry(item.clone()).or_default();
            for dep in deps.split(',') {
                if let Some(dep) = norm_id(dep) {
                    slot.insert(dep);
                }
            }
            if slot.is_empty() {
                self.edges.remove(&item);
            }
        }
    }

    fn render_comment(&self) -> String {
        let mut tokens = Vec::new();
        for (item, deps) in &self.edges {
            if deps.is_empty() {
                continue;
            }
            let deps = deps.iter().cloned().collect::<Vec<_>>().join(",");
            tokens.push(format!("queue={item}:{deps}"));
        }
        for id in &self.requests {
            tokens.push(format!("request={id}"));
        }
        if self.dispatch_actor_turn_busy {
            tokens.push(DISPATCH_TOKEN.to_string());
        }
        format!("{MARKER_PREFIX} {} -->", tokens.join(" "))
    }
}

/// True when the document records the `actor_turn_busy` dispatch disregard.
pub fn disregards_actor_turn_busy(content: &str) -> bool {
    QueueDisregards::from_content(content).dispatch_actor_turn_busy
}

/// True when `id` is a request the operator disregarded.
pub fn request_was_disregarded(content: &str, id: &str) -> bool {
    let Some(id) = norm_id(id) else {
        return false;
    };
    QueueDisregards::from_content(content)
        .requests
        .contains(&id)
}

/// True when a derived packet todo (`id=<packet> …`) names a disregarded request.
pub fn response_todo_is_disregarded(content: &str, todo_text: &str) -> bool {
    let Some(rest) = todo_text.trim().strip_prefix("id=") else {
        return false;
    };
    let id = rest.split_whitespace().next().unwrap_or("");
    request_was_disregarded(content, id)
}

/// True when `text` cites a GitHub issue or pull request the operator still
/// has to answer. A bare repo URL or `issue N` does not count.
pub fn cites_upstream(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    cites_github_issue_or_pr(&lower) || cites_owner_repo_hash(&lower)
}

/// Apply `target` to `content`. A refused `--item` is `Err` and describes the
/// protected blocker. Gestures never fail the document: a protected row becomes
/// an inert note.
pub fn plan(content: &str, target: DisregardTarget) -> Result<DisregardPlan> {
    let (working, selection, mut kept) = match &target {
        DisregardTarget::Gestures => consume_gestures(content),
        DisregardTarget::AllInvented => (content.to_string(), Selection::all(), Vec::new()),
        DisregardTarget::Dispatch => (content.to_string(), Selection::dispatch_only(), Vec::new()),
        DisregardTarget::Item(raw) => {
            let selection = selection_for_item(content, raw)?;
            (content.to_string(), selection, Vec::new())
        }
    };
    if selection.is_empty() {
        return Ok(DisregardPlan {
            changed: working != content,
            content: if working != content {
                working
            } else {
                content.to_string()
            },
            disregarded: Vec::new(),
            kept,
        });
    }

    let blockers = queue_blockers(&working);
    let components = element::parse(&working).unwrap_or_default();
    let todos = OwnedTodoIndex::from_components(&working, &components);
    let mut record = QueueDisregards::from_content(&working);
    let mut add_edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut add_requests: BTreeSet<String> = BTreeSet::new();
    let mut fresh_refusals: Vec<(String, String)> = Vec::new();
    let mut applied = false;

    let item_ids: Vec<String> = if selection.all_items {
        blockers.iter().map(|(id, _)| id.to_string()).collect()
    } else {
        selection.items.iter().cloned().collect()
    };
    for id in &item_ids {
        match item_effect(id, &blockers, &todos, &working) {
            ItemEffect::Apply {
                deps,
                requests,
                kept: item_kept,
            } => {
                applied = true;
                if !deps.is_empty() {
                    add_edges.entry(id.clone()).or_default().extend(deps);
                }
                add_requests.extend(requests);
                kept.extend(item_kept);
            }
            ItemEffect::Refuse(reasons) if !record.has_item(id) => {
                fresh_refusals.extend(reasons);
            }
            ItemEffect::Refuse(_) | ItemEffect::None => {}
        }
    }
    for id in &selection.requests {
        match consider_request(id, &todos, true) {
            RequestClass::Invented => {
                applied = true;
                add_requests.insert(id.clone());
            }
            RequestClass::Protected(reason) if !record.requests.contains(id) => {
                fresh_refusals.push((id.clone(), reason.as_str().to_string()));
            }
            RequestClass::Protected(_) | RequestClass::Absent => {}
        }
    }
    if selection.all_requests {
        let mut open = todos.open_ids().iter().cloned().collect::<Vec<_>>();
        open.sort();
        for id in open {
            if !id.starts_with(REQUEST_PREFIX) {
                continue;
            }
            match consider_request(&id, &todos, true) {
                RequestClass::Invented => {
                    applied = true;
                    add_requests.insert(id);
                }
                RequestClass::Protected(reason) => {
                    push_kept(&mut kept, id, reason);
                }
                RequestClass::Absent => {}
            }
        }
    }
    if selection.refuse_if_protected && !applied && !fresh_refusals.is_empty() {
        bail!("{}", refusal_message(&fresh_refusals));
    }
    kept.extend(fresh_refusals);
    kept.sort();
    kept.dedup();

    let mut disregarded = Vec::new();
    for (item, deps) in &add_edges {
        let mut names = deps.iter().cloned().collect::<Vec<_>>();
        names.sort();
        for dep in &names {
            disregarded.push(format!("queue {item}: {dep}"));
        }
        record.edges.entry(item.clone()).or_default().extend(names);
    }
    let mut request_names = add_requests.iter().cloned().collect::<Vec<_>>();
    request_names.sort();
    for id in &request_names {
        disregarded.push(format!("request {id}"));
        record.requests.insert(id.clone());
    }
    if selection.dispatch && !record.dispatch_actor_turn_busy {
        record.dispatch_actor_turn_busy = true;
        disregarded.push("dispatch actor_turn_busy".to_string());
    }
    if disregarded.is_empty() && request_names.is_empty() {
        return Ok(DisregardPlan {
            changed: working != content,
            content: if working != content {
                working
            } else {
                content.to_string()
            },
            disregarded,
            kept,
        });
    }

    let mut next = place_marker(&working, &record);
    let mut closed_any = false;
    for id in &request_names {
        if !todo::open_todo_ids(&next).contains(id) {
            continue;
        }
        let Ok(outcome) = todo::set_todo_item_state_in_document(&next, id, TodoTransition::Done)
        else {
            continue;
        };
        if outcome.changed {
            closed_any = true;
            next = outcome.content;
        }
    }
    if closed_any || !request_names.is_empty() {
        if let Some(projected) = project_waiting_on_you_tree(&next, "")? {
            next = projected;
        }
        let open = todo::open_todo_ids(&next);
        let actions: &[(String, String)] = &[];
        if let Some(projected) = project_all_prompt_operator_actions(&next, actions, &open)? {
            next = projected;
        }
    }
    next = repaint_blocked(&next);
    Ok(DisregardPlan {
        changed: next != content,
        content: next,
        disregarded,
        kept,
    })
}

enum DepClass {
    Invented,
    Protected(KeptReason),
}

enum RequestClass {
    Invented,
    Protected(KeptReason),
    Absent,
}

enum ItemEffect {
    Apply {
        deps: BTreeSet<String>,
        requests: BTreeSet<String>,
        kept: Vec<(String, String)>,
    },
    Refuse(Vec<(String, String)>),
    None,
}

fn classify_todo(id: &str, text: &str) -> DepClass {
    if is_operator_verify(text) {
        return DepClass::Protected(KeptReason::RealOperatorGate);
    }
    if is_permission_request_text(text) {
        return DepClass::Protected(KeptReason::HarnessPermissionPrompt);
    }
    if cites_upstream(text) {
        return DepClass::Protected(KeptReason::ClosedUpstreamAnswer);
    }
    if !id.starts_with(REQUEST_PREFIX) {
        return DepClass::Protected(KeptReason::RealOperatorGate);
    }
    DepClass::Invented
}

fn consider_request(id: &str, todos: &OwnedTodoIndex, require_open: bool) -> RequestClass {
    let Some(item) = todos.owning(id) else {
        return RequestClass::Absent;
    };
    if require_open && !item.state.is_open() {
        return RequestClass::Absent;
    }
    match classify_todo(&item.id, &item.text) {
        DepClass::Invented => RequestClass::Invented,
        DepClass::Protected(reason) => RequestClass::Protected(reason),
    }
}

fn item_effect(
    id: &str,
    blockers: &crate::queue_blocked::QueueBlockers,
    todos: &OwnedTodoIndex,
    content: &str,
) -> ItemEffect {
    let mut deps = BTreeSet::new();
    let mut requests = BTreeSet::new();
    let mut refused = Vec::new();
    if let Some(blocking) = blockers.blockers_for_id(id) {
        for dep in blocking {
            match dep_class(dep, todos) {
                DepClass::Invented => {
                    deps.insert(dep.clone());
                    if matches!(consider_request(dep, todos, true), RequestClass::Invented) {
                        requests.insert(dep.clone());
                    }
                }
                DepClass::Protected(reason) => {
                    refused.push((dep.clone(), reason.as_str().to_string()));
                }
            }
        }
    }
    for row in list_bodies(content) {
        if row_item_id(&row).as_deref() != Some(id) {
            continue;
        }
        for request in operator_action_ids(&row) {
            match consider_request(&request, todos, true) {
                RequestClass::Invented => {
                    requests.insert(request);
                }
                RequestClass::Protected(reason) => {
                    refused.push((request, reason.as_str().to_string()));
                }
                RequestClass::Absent => {}
            }
        }
    }
    if matches!(consider_request(id, todos, true), RequestClass::Invented) {
        requests.insert(id.to_string());
    } else if let RequestClass::Protected(reason) = consider_request(id, todos, true)
        && id.starts_with(REQUEST_PREFIX)
    {
        refused.push((id.to_string(), reason.as_str().to_string()));
    }
    if !deps.is_empty() || !requests.is_empty() {
        ItemEffect::Apply {
            deps,
            requests,
            kept: refused,
        }
    } else if !refused.is_empty() {
        ItemEffect::Refuse(refused)
    } else {
        ItemEffect::None
    }
}

fn dep_class(dep: &str, todos: &OwnedTodoIndex) -> DepClass {
    match consider_request(dep, todos, true) {
        RequestClass::Invented => DepClass::Invented,
        RequestClass::Protected(reason) => DepClass::Protected(reason),
        // An open backlog or review id is tracked work, not an operator gate.
        RequestClass::Absent => DepClass::Invented,
    }
}

fn is_operator_verify(text: &str) -> bool {
    text.to_ascii_lowercase().contains("[operator-verify]")
}

fn is_permission_request_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("blocking subagent permission request")
        || lower.contains("harness permission")
        || lower.contains("active permission prompt")
}

fn cites_github_issue_or_pr(lower: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = lower[from..].find("github.com/") {
        let mut at = from + rel + "github.com/".len();
        let Some(owner_len) = slug_len(&lower[at..]) else {
            from = at;
            continue;
        };
        at += owner_len;
        if !lower[at..].starts_with('/') {
            from = at;
            continue;
        }
        at += 1;
        let Some(repo_len) = slug_len(&lower[at..]) else {
            from = at;
            continue;
        };
        at += repo_len;
        let kind = if lower[at..].starts_with("/issues/") {
            Some("/issues/".len())
        } else if lower[at..].starts_with("/pull/") {
            Some("/pull/".len())
        } else {
            None
        };
        if let Some(kind_len) = kind {
            at += kind_len;
            if lower[at..]
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_digit())
            {
                return true;
            }
        }
        from = at;
    }
    false
}

fn cites_owner_repo_hash(lower: &str) -> bool {
    for (index, ch) in lower.char_indices() {
        if ch != '#' {
            continue;
        }
        let after = &lower[index + 1..];
        if !after.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        let before = &lower[..index];
        let repo_start = slug_start(before);
        if repo_start == index {
            continue;
        }
        if repo_start == 0
            || !before
                .as_bytes()
                .get(repo_start - 1)
                .is_some_and(|b| *b == b'/')
        {
            continue;
        }
        let owner = &before[..repo_start - 1];
        let owner_start = slug_start(owner);
        if owner_start == owner.len() {
            continue;
        }
        return true;
    }
    false
}

fn slug_len(text: &str) -> Option<usize> {
    let len = text
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-'))
        .count();
    (len > 0).then_some(len)
}

fn slug_start(text: &str) -> usize {
    text.rfind(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-')))
        .map_or(0, |index| index + 1)
}

fn norm_id(raw: &str) -> Option<String> {
    let id = backlog::normalize_pending_id(raw);
    backlog::is_valid_pending_id(&id).then_some(id)
}

fn push_kept(kept: &mut Vec<(String, String)>, id: String, reason: KeptReason) {
    let row = (id, reason.as_str().to_string());
    if !kept.contains(&row) {
        kept.push(row);
    }
}

fn refusal_message(kept: &[(String, String)]) -> String {
    let body = kept
        .iter()
        .map(|(id, reason)| format!("{id}: {reason}"))
        .collect::<Vec<_>>()
        .join("; ");
    format!("queue disregard: kept — {body}")
}

#[derive(Clone)]
struct Selection {
    all_items: bool,
    items: BTreeSet<String>,
    all_requests: bool,
    requests: BTreeSet<String>,
    dispatch: bool,
    refuse_if_protected: bool,
}

impl Selection {
    fn all() -> Self {
        Self {
            all_items: true,
            items: BTreeSet::new(),
            all_requests: true,
            requests: BTreeSet::new(),
            dispatch: true,
            refuse_if_protected: false,
        }
    }

    fn dispatch_only() -> Self {
        Self {
            all_items: false,
            items: BTreeSet::new(),
            all_requests: false,
            requests: BTreeSet::new(),
            dispatch: true,
            refuse_if_protected: false,
        }
    }

    fn items(ids: BTreeSet<String>, requests: BTreeSet<String>) -> Self {
        Self {
            all_items: false,
            items: ids,
            all_requests: false,
            requests,
            dispatch: false,
            refuse_if_protected: true,
        }
    }

    fn is_empty(&self) -> bool {
        !self.all_items
            && self.items.is_empty()
            && !self.all_requests
            && self.requests.is_empty()
            && !self.dispatch
    }
}

fn selection_for_item(content: &str, raw: &str) -> Result<Selection> {
    let raw = raw.trim();
    if raw.is_empty() {
        bail!("queue disregard: `--item` must be nonempty");
    }
    let mut items = BTreeSet::new();
    let mut requests = BTreeSet::new();
    if let Some(id) = explicit_id(raw)
        && document_mentions_id(content, &id)
    {
        items.insert(id);
        return Ok(Selection::items(items, requests));
    }
    let mut matched = false;
    for row in list_bodies(content) {
        if !row_text_matches(&row, raw) {
            continue;
        }
        matched = true;
        if let Some(id) = row_item_id(&row) {
            items.insert(id);
        }
        requests.extend(operator_action_ids(&row));
    }
    if !matched {
        bail!("queue disregard: no queue item or request matches `{raw}`");
    }
    Ok(Selection::items(items, requests))
}

fn explicit_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let without_do = trimmed
        .strip_prefix("do ")
        .or_else(|| trimmed.strip_prefix("DO "))
        .unwrap_or(trimmed)
        .trim();
    match QueueItemIdentity::from_prompt(without_do) {
        QueueItemIdentity::Id(id) => norm_id(&id),
        QueueItemIdentity::FreeText(_) => norm_id(without_do),
    }
}

fn document_mentions_id(content: &str, id: &str) -> bool {
    if let Ok(components) = element::parse(content) {
        let todos = OwnedTodoIndex::from_components(content, &components);
        if todos.owning(id).is_some() {
            return true;
        }
        for component in &components {
            if !(element::is_backlog_component(&component.name)
                || element::is_review_component(&component.name))
            {
                continue;
            }
            let (_, items, _) = backlog::parse_items(component.content(content));
            if items
                .iter()
                .any(|item| backlog::normalize_pending_id(&item.id) == id)
            {
                return true;
            }
        }
    }
    list_bodies(content)
        .iter()
        .any(|row| row_item_id(row).as_deref() == Some(id))
}

fn row_text_matches(row: &str, raw: &str) -> bool {
    let row = row.trim();
    row == raw || row.eq_ignore_ascii_case(raw)
}

fn row_item_id(text: &str) -> Option<String> {
    match QueueItemIdentity::from_prompt(text) {
        QueueItemIdentity::Id(id) => norm_id(&id),
        QueueItemIdentity::FreeText(_) => {
            let rest = text.trim();
            let rest = rest
                .strip_prefix("[ ]")
                .or_else(|| rest.strip_prefix("[/]"))
                .or_else(|| rest.strip_prefix("[x]"))
                .map(str::trim)
                .unwrap_or(rest);
            match QueueItemIdentity::from_prompt(rest) {
                QueueItemIdentity::Id(id) => norm_id(&id),
                QueueItemIdentity::FreeText(_) => None,
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GestureKind {
    Child,
    All,
    Token,
}

struct ListHit {
    indent: usize,
    marker: String,
    text: String,
    kind: Option<GestureKind>,
}

fn consume_gestures(content: &str) -> (String, Selection, Vec<(String, String)>) {
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let mut in_fence = false;
    let mut hits: Vec<Option<ListHit>> = Vec::with_capacity(lines.len());
    for line in &lines {
        let body = line.trim_end_matches(['\n', '\r']);
        if is_fence(body) {
            in_fence = !in_fence;
            hits.push(None);
            continue;
        }
        if in_fence || marker_body(body).is_some() {
            hits.push(None);
            continue;
        }
        hits.push(split_list(body).map(|(indent, marker, text)| ListHit {
            indent,
            marker,
            kind: gesture_kind(&text),
            text,
        }));
    }

    let blockers = queue_blockers(content);
    let components = element::parse(content).unwrap_or_default();
    let todos = OwnedTodoIndex::from_components(content, &components);
    let mut selection = Selection {
        all_items: false,
        items: BTreeSet::new(),
        all_requests: false,
        requests: BTreeSet::new(),
        dispatch: false,
        refuse_if_protected: false,
    };
    let mut kept = Vec::new();
    let mut edits = vec![LineEdit::Keep; lines.len()];
    for index in 0..lines.len() {
        let Some((indent, text, kind)) = hits[index]
            .as_ref()
            .map(|hit| (hit.indent, hit.text.clone(), hit.kind))
        else {
            continue;
        };
        let Some(kind) = kind else {
            continue;
        };
        match kind {
            GestureKind::All => {
                edits[index] = LineEdit::Drop;
                selection.all_items = true;
                selection.all_requests = true;
                selection.dispatch = true;
            }
            GestureKind::Child | GestureKind::Token => {
                let target = if kind == GestureKind::Child {
                    parent_index(&hits, index)
                } else {
                    Some(index)
                };
                let Some(target) = target else {
                    edits[index] = LineEdit::Replace(note_line(indent, "no queue row"));
                    continue;
                };
                let (row, row_indent, row_marker) = hits[target]
                    .as_ref()
                    .map(|hit| (hit.text.clone(), hit.indent, hit.marker.clone()))
                    .unwrap_or_default();
                let decision = row_decision(&row, &blockers, &todos);
                if decision.apply {
                    if let Some(id) = decision.item {
                        selection.items.insert(id);
                    }
                    selection.requests.extend(decision.requests);
                }
                let reason = decision
                    .kept
                    .iter()
                    .map(|(_, reason)| reason.as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                kept.extend(
                    decision
                        .kept
                        .iter()
                        .map(|(id, reason)| (id.clone(), reason.as_str().to_string())),
                );
                match kind {
                    GestureKind::Child if decision.apply || decision.kept.is_empty() => {
                        edits[index] = LineEdit::Drop;
                    }
                    GestureKind::Child => {
                        edits[index] = LineEdit::Replace(note_line(indent, &reason));
                    }
                    GestureKind::Token => {
                        let stripped = strip_disregard_token(&text).unwrap_or_default();
                        if stripped.is_empty() {
                            edits[index] = if decision.kept.is_empty() {
                                LineEdit::Drop
                            } else {
                                LineEdit::Replace(note_line(row_indent, &reason))
                            };
                        } else if decision.kept.is_empty() || decision.apply {
                            edits[index] =
                                LineEdit::Replace(render_row(row_indent, &row_marker, &stripped));
                        } else {
                            edits[index] = LineEdit::ReplaceAndNote {
                                line: render_row(row_indent, &row_marker, &stripped),
                                note: note_line(row_indent + 2, &reason),
                            };
                        }
                    }
                    GestureKind::All => {}
                }
            }
        }
    }

    let mut out = String::new();
    for (index, line) in lines.iter().enumerate() {
        let newline = if line.ends_with('\n') { "\n" } else { "" };
        match &edits[index] {
            LineEdit::Keep => out.push_str(line),
            LineEdit::Drop => {}
            LineEdit::Replace(body) => {
                out.push_str(body);
                out.push_str(newline);
            }
            LineEdit::ReplaceAndNote { line, note } => {
                out.push_str(line);
                out.push_str(newline);
                out.push_str(note);
                out.push('\n');
            }
        }
    }
    kept.sort();
    kept.dedup();
    (out, selection, kept)
}

#[derive(Clone)]
enum LineEdit {
    Keep,
    Drop,
    Replace(String),
    ReplaceAndNote { line: String, note: String },
}

struct RowDecision {
    item: Option<String>,
    requests: BTreeSet<String>,
    kept: Vec<(String, KeptReason)>,
    apply: bool,
}

fn row_decision(
    row: &str,
    blockers: &crate::queue_blocked::QueueBlockers,
    todos: &OwnedTodoIndex,
) -> RowDecision {
    let item = row_item_id(row);
    let mut requests = BTreeSet::new();
    let mut kept = Vec::new();
    let mut invented = false;
    if let Some(id) = item.as_deref() {
        if let Some(deps) = blockers.blockers_for_id(id) {
            for dep in deps {
                match dep_class(dep, todos) {
                    DepClass::Invented => invented = true,
                    DepClass::Protected(reason) => kept.push((dep.clone(), reason)),
                }
            }
        }
        match consider_request(id, todos, true) {
            RequestClass::Invented => {
                invented = true;
                requests.insert(id.to_string());
            }
            RequestClass::Protected(reason) if id.starts_with(REQUEST_PREFIX) => {
                kept.push((id.to_string(), reason));
            }
            RequestClass::Protected(_) | RequestClass::Absent => {}
        }
    }
    for request in operator_action_ids(row) {
        match consider_request(&request, todos, true) {
            RequestClass::Invented => {
                invented = true;
                requests.insert(request);
            }
            RequestClass::Protected(reason) => kept.push((request, reason)),
            RequestClass::Absent => {}
        }
    }
    // A mixed row still disregards its invented edges. The note is only for a
    // row whose every blocker is protected.
    let apply = invented;
    if apply {
        kept.retain(|(id, _)| !requests.contains(id));
    }
    RowDecision {
        item,
        requests: if apply { requests } else { BTreeSet::new() },
        kept: if apply { Vec::new() } else { kept },
        apply,
    }
}

fn parent_index(hits: &[Option<ListHit>], child: usize) -> Option<usize> {
    let child_indent = hits[child].as_ref()?.indent;
    for index in (0..child).rev() {
        let Some(hit) = hits[index].as_ref() else {
            continue;
        };
        if hit.kind.is_some() {
            continue;
        }
        if hit.indent < child_indent {
            return Some(index);
        }
    }
    None
}

fn gesture_kind(text: &str) -> Option<GestureKind> {
    let trimmed = text.trim();
    if trimmed.eq_ignore_ascii_case("disregard blockers") {
        return Some(GestureKind::All);
    }
    if trimmed.eq_ignore_ascii_case("disregard") {
        return Some(GestureKind::Child);
    }
    strip_disregard_token(trimmed).map(|_| GestureKind::Token)
}

fn strip_disregard_token(body: &str) -> Option<String> {
    let lower = body.to_ascii_lowercase();
    let needle = "[disregard]";
    let mut cuts = Vec::new();
    let mut from = 0;
    while let Some(rel) = lower[from..].find(needle) {
        let at = from + rel;
        let end = at + needle.len();
        let before_ok = at == 0 || body.as_bytes()[at - 1].is_ascii_whitespace();
        let after_ok = end >= body.len() || body.as_bytes()[end].is_ascii_whitespace();
        if before_ok && after_ok {
            cuts.push((at, end));
            from = end;
        } else {
            from = at + 1;
        }
    }
    if cuts.is_empty() {
        return None;
    }
    let mut out = String::new();
    let mut cursor = 0;
    for (at, end) in cuts {
        out.push_str(&body[cursor..at]);
        cursor = end;
    }
    out.push_str(&body[cursor..]);
    Some(collapse_ws(&out))
}

fn collapse_ws(text: &str) -> String {
    let mut out = String::new();
    let mut pending_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !out.is_empty() {
                pending_space = true;
            }
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(ch);
    }
    out
}

fn note_line(indent: usize, reason: &str) -> String {
    format!("{:indent$}- **Disregard:** kept — {reason} ▫️", "")
}

fn render_row(indent: usize, marker: &str, text: &str) -> String {
    format!("{:indent$}{marker}{text}", "")
}

fn split_list(line: &str) -> Option<(usize, String, String)> {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    if let Some(text) = trimmed.strip_prefix("- ") {
        return Some((indent, "- ".to_string(), text.to_string()));
    }
    if let Some(text) = trimmed.strip_prefix("* ") {
        return Some((indent, "* ".to_string(), text.to_string()));
    }
    let bytes = trimmed.as_bytes();
    let mut digits = 0;
    while digits < bytes.len() && bytes[digits].is_ascii_digit() {
        digits += 1;
    }
    if digits > 0 && trimmed[digits..].starts_with(". ") {
        let marker = trimmed[..digits + 2].to_string();
        let text = trimmed[digits + 2..].to_string();
        return Some((indent, marker, text));
    }
    None
}

fn list_bodies(content: &str) -> Vec<String> {
    let mut bodies = Vec::new();
    let mut in_fence = false;
    for line in content.lines() {
        if is_fence(line) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || marker_body(line).is_some() {
            continue;
        }
        if let Some((_, _, text)) = split_list(line) {
            bodies.push(text);
        }
    }
    bodies
}

fn is_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

fn marker_body(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix(MARKER_PREFIX)?;
    let rest = rest.strip_suffix("-->")?.trim();
    Some(rest)
}

fn place_marker(content: &str, record: &QueueDisregards) -> String {
    let stripped = strip_marker_lines(content);
    if record.is_empty() {
        return stripped;
    }
    let comment = format!("{}\n", record.render_comment());
    if let Some(at) = frontmatter_end(&stripped) {
        let mut out = String::with_capacity(stripped.len() + comment.len());
        out.push_str(&stripped[..at]);
        out.push_str(&comment);
        out.push_str(&stripped[at..]);
        return out;
    }
    format!("{comment}{stripped}")
}

fn strip_marker_lines(content: &str) -> String {
    let mut out = String::new();
    for line in content.split_inclusive('\n') {
        if marker_body(line.trim_end_matches(['\n', '\r'])).is_some() {
            continue;
        }
        out.push_str(line);
    }
    out
}

fn frontmatter_end(content: &str) -> Option<usize> {
    let rest = content.strip_prefix("---\n")?;
    let close = rest.find("\n---\n")?;
    Some("---\n".len() + close + "\n---\n".len())
}

fn repaint_blocked(content: &str) -> String {
    let blockers = queue_blockers(content);
    let mut out = String::new();
    let mut in_fence = false;
    for line in content.split_inclusive('\n') {
        let newline = if line.ends_with('\n') { "\n" } else { "" };
        let body = line.trim_end_matches(['\n', '\r']);
        if is_fence(body) {
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        if in_fence {
            out.push_str(line);
            continue;
        }
        out.push_str(&repaint_line(body, &blockers));
        out.push_str(newline);
    }
    out
}

fn repaint_line(line: &str, blockers: &crate::queue_blocked::QueueBlockers) -> String {
    let Some(at) = line
        .find(BLOCKED_MARKER)
        .or_else(|| line.find(BLOCKED_ANNOTATION_OPEN))
    else {
        return line.to_string();
    };
    let (prefix, rest) = line.split_at(at);
    if lifecycle_marker(rest) != Some(QueueLifecycleMarker::Blocked)
        && !rest.starts_with(BLOCKED_ANNOTATION_OPEN)
    {
        return line.to_string();
    }
    if let Some(live) = blockers.blockers_for_row(rest) {
        if blocked_marker_ids(rest) == live {
            return line.to_string();
        }
        return format!(
            "{prefix}{}",
            apply_lifecycle_marker_with_blockers(rest, QueueLifecycleMarker::Blocked, live)
        );
    }
    format!(
        "{prefix}{}",
        apply_lifecycle_marker(rest, QueueLifecycleMarker::Pending)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn review_block() -> &'static str {
        concat!(
            "<!-- agent:run -->\n",
            "- ⛔ ⟦blocked by pre⟧ do [#dep]\n",
            "<!-- /agent:run -->\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#dep] after=#pre dependent work\n",
            "<!-- /agent:backlog -->\n",
            "<!-- agent:review -->\n",
            "- [/] [#pre] waiting for verification\n",
            "<!-- /agent:review -->\n",
        )
    }

    #[test]
    fn upstream_citation_requires_an_issue_or_pull_number() {
        assert!(cites_upstream(
            "see https://github.com/btakita/agent-doc/issues/12"
        ));
        assert!(cites_upstream("github.com/btakita/agent-doc/pull/3"));
        assert!(cites_upstream("answered in btakita/agent-doc#12"));
        assert!(!cites_upstream("https://github.com/btakita/agent-doc"));
        assert!(!cites_upstream("issue 12 is still open"));
        assert!(!cites_upstream("blocked behind PR #749"));
    }

    #[test]
    fn an_invented_review_edge_unblocks_and_stays_open() {
        let first = plan(review_block(), DisregardTarget::AllInvented).unwrap();
        assert!(first.changed);
        assert!(first.content.contains("queue=dep:pre"));
        assert!(first.content.contains(DISPATCH_TOKEN));
        assert!(first.content.contains("- [/] [#pre]"));
        assert!(first.content.contains("- [ ] [#dep]"));
        assert!(first.content.contains("📥 do [#dep]"));
        assert!(!first.content.contains("⛔"));
        assert_eq!(queue_blockers(&first.content).blockers_for_id("dep"), None);
        let again = plan(&first.content, DisregardTarget::AllInvented).unwrap();
        assert!(!again.changed);
        assert_eq!(again.content, first.content);
    }

    #[test]
    fn protected_deps_stay_and_a_protected_item_is_refused() {
        let doc = concat!(
            "<!-- agent:run -->\n",
            "- ⛔ ⟦blocked by real-gate, pre⟧ do [#dep]\n",
            "- ⛔ ⟦blocked by real-gate⟧ do [#only]\n",
            "<!-- /agent:run -->\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#dep] (stays open: blocked behind #real-gate and #pre) work\n",
            "- [ ] [#only] (stays open: blocked behind #real-gate) work\n",
            "<!-- /agent:backlog -->\n",
            "<!-- agent:review -->\n",
            "- [/] [#pre] review work\n",
            "<!-- /agent:review -->\n",
            "<!-- agent:todo -->\n",
            "- [ ] [#real-gate] choose the port\n",
            "- [ ] [#subagent-operator-request-perm] Answer the blocking subagent permission request for the harness\n",
            "- [ ] [#subagent-operator-request-up] Answer the blocking subagent question about btakita/agent-doc#12\n",
            "- [ ] [#subagent-operator-request-verify] [operator-verify] confirm the install\n",
            "- [ ] [#subagent-operator-request-inv] Answer the blocking subagent question about the port\n",
            "<!-- /agent:todo -->\n",
        );
        let applied = plan(doc, DisregardTarget::AllInvented).unwrap();
        assert!(applied.content.contains("queue=dep:pre"));
        assert!(!applied.content.contains("queue=dep:real-gate"));
        assert!(!applied.content.contains("queue=only:"));
        assert!(applied.content.contains("- [ ] [#real-gate]"));
        assert!(
            applied
                .content
                .contains("- [ ] [#subagent-operator-request-perm]")
        );
        assert!(
            applied
                .content
                .contains("- [ ] [#subagent-operator-request-up]")
        );
        assert!(
            applied
                .content
                .contains("- [ ] [#subagent-operator-request-verify]")
        );
        assert!(
            applied
                .content
                .contains("- [x] [#subagent-operator-request-inv]")
        );
        assert!(
            applied
                .content
                .contains("request=subagent-operator-request-inv")
        );
        assert!(applied.content.contains("⟦blocked by real-gate⟧"));
        assert_eq!(
            queue_blockers(&applied.content).blockers_for_id("dep"),
            Some(&["real-gate".to_string()][..])
        );
        let err = plan(doc, DisregardTarget::Item("only".into())).unwrap_err();
        assert!(err.to_string().contains("real operator gate"), "{err}");
        let item = plan(doc, DisregardTarget::Item("dep".into())).unwrap();
        assert!(item.content.contains("queue=dep:pre"));
        assert!(!item.content.contains(DISPATCH_TOKEN));
        assert!(item.changed);
    }

    #[test]
    fn gestures_consume_invented_rows_and_note_protected_ones() {
        let doc = concat!(
            "<!-- agent:run -->\n",
            "- ⛔ ⟦blocked by pre⟧ do [#dep]\n",
            "  - disregard\n",
            "- ⛔ ⟦blocked by real-gate⟧ do [#only] [disregard]\n",
            "- disregard blockers\n",
            "```\n",
            "- disregard blockers\n",
            "```\n",
            "<!-- /agent:run -->\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#dep] after=#pre work\n",
            "- [ ] [#only] (stays open: blocked behind #real-gate) work\n",
            "<!-- /agent:backlog -->\n",
            "<!-- agent:review -->\n",
            "- [/] [#pre] review work\n",
            "<!-- /agent:review -->\n",
            "<!-- agent:todo -->\n",
            "- [ ] [#real-gate] choose the port\n",
            "<!-- /agent:todo -->\n",
        );
        let applied = plan(doc, DisregardTarget::Gestures).unwrap();
        assert!(applied.changed);
        assert!(!applied.content.contains("\n  - disregard\n"));
        assert!(
            applied
                .content
                .contains("**Disregard:** kept — real operator gate ▫️")
        );
        assert!(!applied.content.contains("[disregard]"));
        assert!(applied.content.contains("```\n- disregard blockers\n```"));
        assert!(applied.content.contains("queue=dep:pre"));
        assert!(applied.content.contains(DISPATCH_TOKEN));
        assert!(applied.content.contains("- [/] [#pre]"));
        assert!(
            agent_doc_markdown_ast::overlay::is_generated_response_structure_row(
                "**Disregard:** kept — real operator gate ▫️"
            )
        );
        let again = plan(&applied.content, DisregardTarget::Gestures).unwrap();
        assert!(!again.changed, "{}", again.content);
    }

    #[test]
    fn dispatch_only_records_the_catch_all() {
        let first = plan(review_block(), DisregardTarget::Dispatch).unwrap();
        assert!(first.content.contains(DISPATCH_TOKEN));
        assert!(!first.content.contains("queue="));
        assert_eq!(
            queue_blockers(&first.content).blockers_for_id("dep"),
            Some(&["pre".to_string()][..])
        );
        assert!(disregards_actor_turn_busy(&first.content));
        let again = plan(&first.content, DisregardTarget::Dispatch).unwrap();
        assert!(!again.changed);
    }

    #[test]
    fn a_disregarded_request_leaves_the_waiting_row_and_the_operator_glyph() {
        let doc = concat!(
            "<!-- agent:queue id=\"main\" -->\n",
            "- 🙋 [#subagent-operator-request-port] ⛔ ⟦blocked by subagent-operator-request-port⟧ do [#work]\n",
            "- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n",
            "  - 🙋 [#subagent-operator-request-port] Name the port <!-- agent-doc:waiting-item -->\n",
            "    - **Next:** you — answer (evidence: Question) ▫️\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:todo -->\n",
            "- [ ] [#subagent-operator-request-port] Answer the blocking subagent question about the port\n",
            "<!-- /agent:todo -->\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#work] (stays open: blocked behind #subagent-operator-request-port) ship it\n",
            "<!-- /agent:backlog -->\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n",
        );
        let plan = plan(doc, DisregardTarget::AllInvented).unwrap();
        assert!(plan.changed, "{plan:?}");
        assert!(
            plan.content
                .contains("- [x] [#subagent-operator-request-port]")
        );
        assert!(
            plan.content
                .contains("request=subagent-operator-request-port")
        );
        let queue = plan
            .content
            .split("<!-- agent:queue id=\"main\" -->")
            .nth(1)
            .and_then(|body| body.split("<!-- /agent:queue -->").next())
            .unwrap_or("");
        assert!(!queue.contains('🙋'), "{queue}");
        assert!(!queue.contains("agent-doc:waiting-on-you"), "{queue}");
        assert!(!queue.contains("agent-doc:waiting-item"), "{queue}");
        assert!(queue.contains("📥 do [#work]"), "{queue}");
        assert!(
            plan.content
                .contains("archived waiting item `#subagent-operator-request-port`"),
            "{}",
            plan.content
        );
        assert!(!plan.content.contains("⛔"), "{}", plan.content);
        assert!(response_todo_is_disregarded(
            &plan.content,
            "id=subagent-operator-request-port Answer the blocking subagent question"
        ));
        assert!(!response_todo_is_disregarded(
            &plan.content,
            "id=subagent-operator-request-perm Answer the blocking subagent permission request"
        ));
    }

    #[test]
    fn the_marker_lands_after_frontmatter() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#dep] after=#pre work\n",
            "<!-- /agent:backlog -->\n",
            "<!-- agent:review -->\n",
            "- [/] [#pre] review\n",
            "<!-- /agent:review -->\n",
        );
        let plan = plan(doc, DisregardTarget::AllInvented).unwrap();
        let comment_at = plan.content.find(MARKER_PREFIX).unwrap();
        let fence_at = plan.content.find("\n---\n").unwrap();
        assert!(comment_at > fence_at, "{}", plan.content);
        assert!(
            plan.content
                .starts_with("---\nagent_doc_format: template\n---\n")
        );
    }
}
