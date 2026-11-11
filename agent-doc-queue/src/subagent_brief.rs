//! Operator authorization carried to a dispatched queue subagent
//! (`#waypostauthorization`).
//!
//! A background subagent dispatched for a `queue_subagent_dispatch` item sees
//! only the prompt its coordinator writes. Without help it gets none of the
//! operator's authorization: the queue's `preset=` symbol lookup, the item's own
//! directive text, or the backlog (or Todo) text an `do [#id]` head points at. It then
//! either refuses outward actions (commit, push) the operator authorized, or the
//! coordinator paraphrases the authorization by hand.
//!
//! This module resolves that authorization from the document, verbatim, and
//! renders a ready-to-paste `subagent_prompt_preamble`. It fails closed: a
//! declared preset that frontmatter does not define is reported as
//! unresolved, its body is never invented, and the preamble tells the
//! subagent no authorization beyond the verbatim item text was resolved.
//! Resolved item and preset directives are current task authorization: a
//! worker must not ask the operator to repeat `go`, `push`, or `#auth` merely
//! to reconfirm an action already named there. Harness-native tool permission
//! remains a separate grant and still uses the operator-request handback.

use agent_doc_element::element;
use serde::{Deserialize, Serialize};

use crate::document_queue::QueueEntry;

/// How much operator authorization was resolved for a dispatch item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationStatus {
    /// Every declared preset resolved to a frontmatter body.
    Resolved,
    /// No preset applies: the verbatim item (and its backlog text) is the
    /// whole authorization.
    ItemTextOnly,
    /// A preset the queue declares has no frontmatter definition. Fail
    /// closed: nothing beyond the verbatim item text is authorized.
    UnresolvedPreset,
    /// The selected queue's `preset=` and `presets=` carry different nonempty
    /// values. Fail closed: neither spelling is authorization.
    ConflictingPresetAttrs,
}

/// Where a preset applied to the item came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetSource {
    /// `<!-- agent:queue preset="#name" -->` on the selected queue.
    QueueAttr,
    /// `<!-- agent:run preset="#name: body" -->` on the selected queue: an
    /// inline definition whose body is the operator's literal text after the
    /// colon (`#runpresetunresolved`).
    QueueAttrInline,
    /// A body-level `preset #name` line in the selected queue.
    QueueLine,
    /// A registered `#name` token in the item text itself.
    Item,
}

/// One preset whose body is part of the item's authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetAuthorization {
    /// The preset name as declared (canonical frontmatter key when resolved).
    pub name: String,
    pub source: PresetSource,
    /// Whether frontmatter `prompt_presets` / `presets` defines it.
    pub resolved: bool,
    /// The effective preset body after project/session interpolation. `None`
    /// when unresolved (never invented).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// The preset's declared runbook path, as written in frontmatter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runbook: Option<String>,
    /// Optional model policy carried by this structured preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Optional reasoning-effort policy carried by this structured preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

/// One tracked item an id-backed head names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackedItemAuthorization {
    /// The tracked id, without `#`.
    pub id: String,
    /// The component that holds it (`backlog`, `review`, `pending`, `todo`).
    pub component: String,
    /// The item's text, verbatim, with any continuation lines.
    pub text: String,
}

/// The operator authorization for one dispatched queue item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentAuthorization {
    pub status: AuthorizationStatus,
    /// The queue item, verbatim.
    pub item: String,
    /// Queue-local provenance captured when this item was dispatched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_id: Option<String>,
    /// Tracked items the head names, resolved to their text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracked_items: Vec<TrackedItemAuthorization>,
    /// Ids the head names that no backlog/review/pending/todo item carries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved_ids: Vec<String>,
    /// Presets that apply to the item (queue-scoped first, then item tokens).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<PresetAuthorization>,
    /// Set when the selected queue's `preset=` and `presets=` disagree. The
    /// marker values are not in [`Self::presets`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_attr_conflict: Option<String>,
    /// Resolved harness model override for this dispatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Resolved harness reasoning-effort override for this dispatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Machine-readable copy of the one packet serialized into the submitted prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_context: Option<agent_doc_prompt_context::lineage::PromptContextPacket>,
    /// Paste this verbatim at the top of the subagent's prompt.
    pub subagent_prompt_preamble: String,
    /// `#goalpresetterminal`: the interrupted goal-loop attempt this row was
    /// requeued after. Its hand-back is also appended to the preamble as data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_goal_attempt: Option<crate::goal_preset_terminal::GoalLoopHandoff>,
}

impl SubagentAuthorization {
    /// Carry `handoff` (an interrupted attempt on this same row) to the next
    /// worker: record it and append its data-framed section to the preamble.
    pub fn attach_prior_goal_attempt(
        &mut self,
        handoff: crate::goal_preset_terminal::GoalLoopHandoff,
    ) {
        if self.prior_goal_attempt.as_ref() == Some(&handoff) {
            return;
        }
        if !self.subagent_prompt_preamble.is_empty()
            && !self.subagent_prompt_preamble.ends_with('\n')
        {
            self.subagent_prompt_preamble.push('\n');
        }
        self.subagent_prompt_preamble.push_str(
            &crate::goal_preset_terminal::prior_attempt_brief_section(&handoff),
        );
        self.prior_goal_attempt = Some(handoff);
    }
}

const TRACKED_COMPONENTS: &[&str] = &["backlog", "review", "pending"];

/// Resolve the item an id-backed head names: tracked work first, then the
/// `agent:todo` item that OWNS the id (`#todosync`). A `do [#id]` head may name
/// an operator Todo; its brief quotes the Todo with its nested sub-items and
/// replies verbatim, exactly as it quotes a backlog item.
fn tracked_item(
    content: &str,
    components: &[element::Component],
    id: &str,
) -> Option<TrackedItemAuthorization> {
    tracked_work_item(content, components, id).or_else(|| {
        agent_doc_element::todo::todo_item_owning_id(content, id).map(|todo| {
            TrackedItemAuthorization {
                id: todo.id,
                component: "todo".to_string(),
                text: todo.text,
            }
        })
    })
}

fn tracked_work_item(
    content: &str,
    components: &[element::Component],
    id: &str,
) -> Option<TrackedItemAuthorization> {
    components
        .iter()
        .filter(|c| TRACKED_COMPONENTS.contains(&c.name.as_str()))
        .find_map(|comp| {
            let (_, items, _) =
                agent_doc_element_backlog::backlog::parse_items(comp.content(content));
            items
                .into_iter()
                .find(|item| !item.id.is_empty() && item.id.eq_ignore_ascii_case(id))
                .map(|item| {
                    let mut text = item.text.trim_end().to_string();
                    let continuation = item.continuation.trim_end();
                    if !continuation.trim().is_empty() {
                        text.push('\n');
                        text.push_str(continuation);
                    }
                    TrackedItemAuthorization {
                        id: item.id,
                        component: comp.name.clone(),
                        text,
                    }
                })
        })
}

/// The plan's origin queue for `item`.
///
/// A plan ranges across every `agent:run` and `agent:loop`. The origin is the
/// section that contains the head, so a later section's subprompts and
/// follow-ups stay in that section. When the selected queue also contains the
/// head, the origin stays the selected queue: a mirrored head does not move.
/// An unmatched item keeps the selected queue.
/// [`origin_queue_id`] for `content`: the queue a worker's follow-up rows
/// land in. `queue report-complete` uses it to place a hand-back's open items
/// in the same queue the dispatch brief named (`#workerresponsestructure`).
pub fn origin_queue_id_for_item(content: &str, item: &str) -> Option<String> {
    let components = element::parse(content).ok()?;
    origin_queue_id(content, &components, item)
}

fn origin_queue_id(content: &str, components: &[element::Component], item: &str) -> Option<String> {
    let queues = crate::queue_graph::QueueSet::parse(content, components).ok()?;
    let contains = |block: &crate::queue_graph::QueueBlock<'_>| block_contains_head(block, item);
    if let Some(selected) = queues.selected()
        && contains(selected)
    {
        return Some(selected.id.clone());
    }
    if let Some(block) = queues.blocks().iter().find(|block| contains(block)) {
        return Some(block.id.clone());
    }
    queues.selected().map(|block| block.id.clone())
}

fn block_contains_head(block: &crate::queue_graph::QueueBlock<'_>, item: &str) -> bool {
    let entries = crate::nested_claim_steer::expand_slice_if_installed(&block.entries);
    entries.iter().enumerate().any(|(index, entry)| {
        matches!(entry, QueueEntry::Prompt(_))
            && crate::document_queue::row_matches_submitted_head_text(entries.as_ref(), index, item)
    })
}

/// The selected queue's `preset=` attribute policy.
fn selected_queue_prompt_attrs(
    content: &str,
    components: &[element::Component],
) -> crate::prompt_component_attrs::PromptComponentAttrs {
    match crate::queue_set::selected_component(content, components) {
        Ok(Some(queue)) => {
            crate::prompt_component_attrs::prompt_component_attrs_for(queue, "queue")
        }
        _ => Default::default(),
    }
}

/// Preset names the selected queue declares: every reference in its
/// `preset=` attr (lookup names and inline `#name: body` definitions alike),
/// then any body-level `preset <name>` lines.
fn queue_preset_names(
    content: &str,
    components: &[element::Component],
) -> Vec<(String, PresetSource)> {
    let mut names = Vec::new();
    let Ok(Some(queue)) = crate::queue_set::selected_component(content, components) else {
        return names;
    };
    let attrs = crate::prompt_component_attrs::prompt_component_attrs_for(queue, "queue");
    for reference in attrs.preset_refs() {
        let source = match reference {
            crate::prompt_component_attrs::PresetRef::Lookup(_) => PresetSource::QueueAttr,
            crate::prompt_component_attrs::PresetRef::Inline { .. } => {
                PresetSource::QueueAttrInline
            }
        };
        names.push((reference.name().to_string(), source));
    }
    if let Ok(entries) = crate::document_queue::parse(queue.content(content)) {
        for entry in entries {
            if let QueueEntry::Preset(name) = entry {
                names.push((name, PresetSource::QueueLine));
            }
        }
    }
    names
}

/// Resolve the operator authorization for dispatch item `item` of `content`.
/// `document` is the path the preamble names.
pub fn subagent_authorization(document: &str, content: &str, item: &str) -> SubagentAuthorization {
    let components = element::parse(content).unwrap_or_default();
    let frontmatter = agent_doc_frontmatter::frontmatter::parse(content)
        .ok()
        .map(|(fm, _)| fm);
    let presets = frontmatter
        .as_ref()
        .map(|fm| fm.prompt_presets.clone())
        .unwrap_or_default();
    let submitted = crate::queue_response::submitted_queue_prompt_with_context(content, item);
    subagent_authorization_with_presets(
        document,
        content,
        item,
        &components,
        &presets,
        Some(submitted.as_str()),
    )
}

/// Resolve subagent authorization with caller-provided effective presets.
///
/// `submitted_prompt` is the `#nestsubmit` text for this item. `None` resolves
/// the first live row whose work text matches `item`.
pub fn subagent_authorization_with_presets(
    document: &str,
    content: &str,
    item: &str,
    components: &[element::Component],
    presets_map: &agent_doc_frontmatter::frontmatter::PromptPresets,
    submitted_prompt: Option<&str>,
) -> SubagentAuthorization {
    // `#runpresetunresolved`: an inline `preset="#name: body"` definition on
    // the selected queue resolves exactly like a frontmatter entry, for the
    // queue attribute and for item/lineage tokens naming it alike.
    let queue_prompt_attrs = selected_queue_prompt_attrs(content, components);
    let preset_attr_conflict = queue_prompt_attrs
        .preset_conflict
        .as_ref()
        .map(ToString::to_string);
    let overlaid = queue_prompt_attrs.overlay_inline_presets(presets_map);
    let presets_map = &overlaid;
    let mut presets: Vec<PresetAuthorization> = Vec::new();
    let mut push_preset = |requested: &str, source: PresetSource| {
        let requested = requested.trim().trim_matches(|c| c == '"' || c == '\'');
        if requested.is_empty() {
            return;
        }
        let key =
            agent_doc_frontmatter::frontmatter::resolve_prompt_preset_key(presets_map, requested);
        let name = key.clone().unwrap_or_else(|| requested.to_string());
        if presets.iter().any(|p| p.name.eq_ignore_ascii_case(&name)) {
            return;
        }
        let (body, runbook, model, effort) = match &key {
            Some(key) => (
                presets_map.get(key.as_str()).cloned(),
                presets_map.runbook(key).map(str::to_string),
                presets_map.model(key).map(str::to_string),
                presets_map.effort(key).map(str::to_string),
            ),
            None => (None, None, None, None),
        };
        presets.push(PresetAuthorization {
            name,
            source,
            resolved: body.is_some(),
            body,
            runbook,
            model,
            effort,
        });
    };
    for (name, source) in queue_preset_names(content, components) {
        push_preset(&name, source);
    }
    // A nested leaf inherits its parent lineage as prompt context. Resolve
    // preset tokens from that lineage as well as from the leaf itself; after
    // parents became context-only, looking at `item` alone silently dropped a
    // parent `#review` / `#rebase` authorization from the dispatched leaf.
    let item_preset_scope = crate::queue_response::nest_vector_in_document(content, item)
        .unwrap_or_else(|| item.into());
    for (name, _) in crate::queue_response::queue_prompt_preset_expansions_with_presets(
        &item_preset_scope,
        presets_map,
    ) {
        push_preset(&name, PresetSource::Item);
    }

    let mut ids: Vec<String> = Vec::new();
    if let agent_doc_element_queue::QueueItemIdentity::Id(id) =
        crate::queue_claim::claim_identity(item)
    {
        ids.push(id.to_ascii_lowercase());
    }
    for id in crate::queue_claim::referenced_queue_ids(item) {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let mut tracked_items = Vec::new();
    let mut unresolved_ids = Vec::new();
    for id in ids {
        match tracked_item(content, components, &id) {
            Some(found) => tracked_items.push(found),
            None => {
                // A preset-only id (`#gh-fix`) is a command, not missing work.
                // A reserved operator-request id is the worker's own question
                // annotation, not a backlog item to invent (`#nestedoutwardauth`).
                if id.starts_with(SUBAGENT_OPERATOR_REQUEST_ID_PREFIX)
                    || presets
                        .iter()
                        .any(|p| p.name.trim_start_matches('#').eq_ignore_ascii_case(&id))
                {
                    continue;
                }
                unresolved_ids.push(id);
            }
        }
    }

    let status = if preset_attr_conflict.is_some() {
        AuthorizationStatus::ConflictingPresetAttrs
    } else if presets.iter().any(|p| !p.resolved) {
        AuthorizationStatus::UnresolvedPreset
    } else if presets.is_empty() {
        AuthorizationStatus::ItemTextOnly
    } else {
        AuthorizationStatus::Resolved
    };
    let queue_id = origin_queue_id(content, components, item);
    let queue_policy = crate::queue_set::selected_component(content, components)
        .ok()
        .flatten()
        .map(|queue| {
            (
                queue.attrs.get("model").map(String::as_str),
                queue.attrs.get("effort").map(String::as_str),
            )
        });
    let frontmatter = agent_doc_frontmatter::frontmatter::parse(content)
        .ok()
        .map(|(fm, _)| fm)
        .unwrap_or_default();
    let preset_policy = |field: fn(&PresetAuthorization) -> Option<&String>| {
        presets
            .iter()
            .filter(|preset| preset.source == PresetSource::Item)
            .chain(
                presets
                    .iter()
                    .filter(|preset| preset.source != PresetSource::Item),
            )
            .find_map(field)
            .map(String::as_str)
    };
    let clean = |value: Option<&str>| {
        value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let model = clean(queue_policy.and_then(|policy| policy.0))
        .or_else(|| clean(preset_policy(|preset| preset.model.as_ref())))
        .or_else(|| clean(frontmatter.subagent_model.as_deref()));
    let effort = clean(queue_policy.and_then(|policy| policy.1))
        .or_else(|| clean(preset_policy(|preset| preset.effort.as_ref())))
        .or_else(|| clean(frontmatter.subagent_effort.as_deref()));
    let mut authorization = SubagentAuthorization {
        status,
        item: item.trim().to_string(),
        queue_id,
        tracked_items,
        unresolved_ids,
        presets,
        preset_attr_conflict,
        model,
        effort,
        prompt_context: None,
        subagent_prompt_preamble: String::new(),
        prior_goal_attempt: None,
    };
    let computed;
    let attached;
    let submitted = match submitted_prompt {
        Some(prompt) if agent_doc_prompt_context::lineage::split(prompt).0.is_some() => prompt,
        Some(prompt) => {
            computed = crate::queue_response::submitted_queue_prompt_with_context(content, item);
            if let Some(packet) = agent_doc_prompt_context::lineage::split(&computed).0 {
                attached = agent_doc_prompt_context::lineage::attach(&packet, prompt);
                attached.as_str()
            } else {
                // Queue slash commands such as `/goal ...` intentionally have no
                // lineage envelope. Preserve the submitted command verbatim
                // instead of assuming every canonical queue prompt has context.
                prompt
            }
        }
        None => {
            computed = crate::queue_response::submitted_queue_prompt_with_context(content, item);
            computed.as_str()
        }
    };
    authorization.prompt_context = agent_doc_prompt_context::lineage::split(submitted).0;
    authorization.subagent_prompt_preamble =
        render_preamble(document, content, &authorization, submitted);
    authorization
}

/// Live nested sub-prompts that name `push` or `publish` (`#nestedoutwardauth`).
///
/// A trailing `⟦effect:N⟧` marker is not part of the directive. A struck child
/// and a negated line (`do not push`, `never publish`) grant nothing.
fn nested_outward_action_lines(item: &str) -> Vec<String> {
    agent_doc_element_queue::subprompts::subprompt_rows(item)
        .into_iter()
        .filter(|row| !agent_doc_element_queue::subprompts::subprompt_row_is_struck(row))
        .filter_map(|row| {
            let row = agent_doc_element_queue::strip_attempt_projection_prefix(&row);
            let row = crate::document_queue::strip_queue_effect_annotation(row.trim());
            let row = row.trim().to_string();
            (!row.is_empty() && line_names_outward_action(&row)).then_some(row)
        })
        .collect()
}

fn line_names_outward_action(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    ["push", "publish"].into_iter().any(|token| {
        if !lower
            .split(|ch: char| !ch.is_ascii_alphanumeric())
            .any(|word| word == token)
        {
            return false;
        }
        let negated = [
            format!("not {token}"),
            format!("never {token}"),
            format!("without {token}"),
            format!("no {token}"),
            format!("don't {token}"),
            format!("dont {token}"),
        ];
        !negated.iter().any(|phrase| lower.contains(phrase.as_str()))
    })
}

fn quote(text: &str) -> String {
    text.trim_end()
        .lines()
        .map(|line| {
            if line.is_empty() {
                ">".to_string()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The scratch-isolation coordinator rule, as a string literal so other
/// crates can `concat!` it into their own `&'static str` guidance
/// (exchange-subagent dispatch has no preamble). Concurrent subagents
/// dispatched by one coordinator share its scratchpad, and a fixed
/// message-file path there let one subagent's commit carry a sibling's
/// message.
#[macro_export]
macro_rules! subagent_scratch_isolation_rule {
    () => {
        "Scratch isolation: concurrent subagents share one scratchpad, so each subagent passes its \
         commit message through `git commit -F -` (a stdin heredoc) or a file inside its own \
         worktree, never a file in a shared scratchpad or /tmp, and puts any other temp file in its \
         own worktree or a unique `mktemp` path."
    };
}

/// The scratch-isolation rule (`subagent_scratch_isolation_rule!`), restated
/// in every preamble and appended to the exchange-subagent dispatch texts.
pub const SUBAGENT_SCRATCH_ISOLATION_RULE: &str = subagent_scratch_isolation_rule!();

/// The live-tmux isolation coordinator rule (`#tmuxkittyterminal`), as a
/// string literal so other crates can `concat!` it like the scratch rule.
/// Subagents run inside the operator's tmux, so `$TMUX` points every bare
/// `tmux` command at the operator's server, and tmux reads `$TMUX` before
/// `TMUX_TMPDIR`. On 2026-10-26 a subagent tore down its test server with
/// `TMUX_TMPDIR=<dir> tmux kill-server` and killed the operator's server,
/// every agent pane, and the terminal. The `PreToolUse` hook enforces the
/// kill half of this rule; the brief states all of it.
#[macro_export]
macro_rules! subagent_tmux_isolation_rule {
    () => {
        "Live tmux isolation: you run inside the operator's live tmux server, and `$TMUX` points \
         every bare `tmux` command at it. tmux reads `$TMUX` before `TMUX_TMPDIR`, so \
         `TMUX_TMPDIR=<dir>` alone isolates nothing. Run every tmux command against a test \
         server, setup and teardown included, with BOTH `env -u TMUX` and a private socket \
         (`env -u TMUX tmux -L <unique-name> ...`, or `-S <path>`), and tear it down by that \
         name or by its exact pid only. Never kill, \
         respawn, send keys to, or resize the operator's server, sessions, windows, or panes, and \
         never `pkill`/`killall` tmux or the terminal."
    };
}

/// The live-tmux isolation rule (`subagent_tmux_isolation_rule!`), restated
/// in every preamble and appended to the exchange-subagent dispatch texts.
pub const SUBAGENT_TMUX_ISOLATION_RULE: &str = subagent_tmux_isolation_rule!();

/// `#workerresponsestructure`: the hand-back shape every brief asks for.
/// `queue report-complete` renders a hand-back with these labels into the
/// response structure and queues each **Action items** entry as a follow-up
/// row after the originating head, so work the worker reports is never buried.
/// **Notes** are never queued (`#handbackopenitemstriage`).
#[macro_export]
macro_rules! subagent_handback_shape_rule {
    () => {
        "Report back in this shape, one bold label per section, so `agent-doc queue report-complete` renders it into the response structure: **Summary** (one or two sentences); **What changed** (branch, every commit SHA, files changed); **Verification** (the exact commands run and their results); **Action items** (one `- ` bullet per piece of work someone still has to do, or `none`; report-complete queues each bullet as a follow-up row after the originating head, where it can dispatch with no review, so put nothing else here and do not also list those as follow-ups); **Notes** (one `- ` bullet per limit, state, or FYI that asks for no work, or `none`; never queued); **Next actions** (remaining steps, and any follow-up queue heads in order). Complete the prompt before handing back. A blocker is only the review structure (`<!-- agent-doc:prompt-blocker -->` with the incomplete work, **Complete by:**, and **Prompt:**); prose in `agent:exchange` does not complete the prompt, and a non-completing prose blocker is requeued (`#promptpressure`)."
    };
}

/// [`subagent_handback_shape_rule!`] as a constant.
pub const SUBAGENT_HANDBACK_SHAPE_RULE: &str = subagent_handback_shape_rule!();

/// Marker line that opens every preamble.
pub const SUBAGENT_PREAMBLE_MARKER: &str =
    "[agent-doc] operator authorization for a dispatched queue item (#waypostauthorization)";

/// Reserved tracked-id prefix for a blocking request a worker routes through
/// its coordinator. Every request packet appends a request-specific suffix so
/// two pending questions never claim the same document identity.
pub const SUBAGENT_OPERATOR_REQUEST_ID_PREFIX: &str = "subagent-operator-request-";

/// Marker template that opens a blocking request. Workers replace
/// `<unique-id>` with a short request-specific suffix.
pub const SUBAGENT_OPERATOR_REQUEST_MARKER: &str =
    "[#subagent-operator-request-<unique-id>] subagent operator request";

/// Read compatibility for packets emitted before request-specific ids.
pub const LEGACY_SUBAGENT_OPERATOR_REQUEST_MARKER: &str =
    "[#subagent-operator-request] subagent operator request";

/// Agent-owned lifecycle/disposition choices are not operator questions.
///
/// The worker and coordinator already have the queue item, prompt tree,
/// resolved presets, and current evidence needed to choose how authorized work
/// continues. Routing that bookkeeping back to the operator stalls the queue
/// without obtaining any product or authorization input.
pub const SUBAGENT_PROCEDURAL_CHOICE_RULE: &str = concat!(
    "Before sending an operator request, classify the missing input. Do not ask the operator to ",
    "choose an agent-doc lifecycle or work-disposition step for already-authorized work. Choices ",
    "such as `leave-tracked`, `review`, or `address`, queue/review/todo placement, which worker ",
    "continues, or whether to keep incomplete work open are agent-owned procedural choices. ",
    "Resolve them from the queue item, its prompt tree, resolved presets, current evidence, and ",
    "completion state: continue the concrete authorized objective while actionable work remains; ",
    "use a genuine review/external gate only when execution is complete; otherwise keep the item ",
    "tracked with its concrete blocker. Ask the operator only when the answer changes product ",
    "behavior or scope, supplies an external fact the agents cannot know, authorizes an outward ",
    "action not already authorized, or must be granted by the harness permission UI.\n"
);

/// The tracked id carried by a subagent operator-request marker line.
///
/// New markers require a non-empty alphanumeric/hyphen suffix. The fixed
/// legacy marker remains readable so existing session history and retained
/// closeouts still parse, but the worker protocol never emits it.
pub fn subagent_operator_request_id(line: &str) -> Option<&str> {
    const MARKER_SUFFIX: &str = "] subagent operator request";
    let line = line.trim();
    if line == LEGACY_SUBAGENT_OPERATOR_REQUEST_MARKER {
        return Some("subagent-operator-request");
    }
    let id = line.strip_prefix("[#")?.strip_suffix(MARKER_SUFFIX)?;
    let suffix = id.strip_prefix(SUBAGENT_OPERATOR_REQUEST_ID_PREFIX)?;
    let mut chars = suffix.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphanumeric() || !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '-') {
        return None;
    }
    Some(id)
}

/// Cross-harness fallback for questions and permission requests that a background
/// worker cannot present to the operator itself. The field names are deliberately
/// stable and emphasized in a Markdown list so people can scan the packet while
/// the coordinator can preserve the complete request in `agent:exchange`. The
/// free-text values of `queue_item`, `request`, and `context` are nested beneath
/// their key bullet so a long value never runs on beside the key.
pub const SUBAGENT_OPERATOR_REQUEST_PROTOCOL: &str = concat!(
    "If required scope or a choice is still missing, or a protected tool action still requires ",
    "harness approval, do not guess or silently deny it. Send the coordinator this exact packet ",
    "before ",
    "stopping; use the harness's native parent-message mechanism when available, otherwise ",
    "return the packet as your result:\n\n",
    "[#subagent-operator-request-<unique-id>] subagent operator request\n",
    "- *queue_item:*\n",
    "  - <origin-queue-item>\n",
    "- *kind:* question | permission\n",
    "- *blocking:* true\n",
    "- *request:*\n",
    "  - <exact question or exact tool/action requiring permission>\n",
    "- *context:*\n",
    "  - <why it is needed>\n",
    "  - <current state>\n",
    "  - <target/scope>\n",
    "  - <risk or effect>\n",
    "- *recommendation:*\n",
    "  - **<label>**: <reason, from the current evidence>\n",
    "- *choices:*\n",
    "  - **<label>**: <effect>\n\n",
    "Replace `<unique-id>` with a short request-specific lowercase suffix using letters, digits, ",
    "and hyphens. Use a different marker id for every request packet in the session document; ",
    "never reuse the legacy fixed `[#subagent-operator-request]` id. ",
    "Put the `queue_item`, `request`, and `context` text in a nested list beneath its key line, ",
    "never on the key line itself; split the context into short bullets, one fact per bullet. ",
    "The context renders beside the choices in the operator's queue row, so state there what each ",
    "term in a choice refers to: a bare `#4` needs its repository and what it does. ",
    "Keep the rendered `queue_item` value unchanged; it links this packet to the exact queue row. ",
    "Include every offered choice; write `- *choices:* []` only when the harness supplied none. ",
    "When you recommend one choice, write `recommendation` above `choices`, as one row naming its ",
    "exact label; otherwise omit the field. Prose outside the packet does not reach the operator's ",
    "structured row. ",
    "Claude Code may surface a background worker's tool-permission prompt natively, and Codex ",
    "provides inter-agent messaging; Grok Build and OpenCode may require returning the packet ",
    "through the child result. In every harness, the coordinator records the packet verbatim in ",
    "`agent:exchange`, presents the request from the main session, and sends the operator's answer ",
    "back to the same worker when continuation is available. A chat reply is context, not a tool ",
    "permission grant: retry a protected action only after the harness permission system records ",
    "the grant for that exact action.\n"
);

/// Task authorization guidance emitted only when all declared presets resolved
/// (or when the item text is the whole authorization). An unresolved preset
/// must not inherit example outward actions from this text.
/// `#effectbeforecloseout`: resolved authorization makes named effects worker
/// work, not coordinator-closeout leftovers.
pub const SUBAGENT_RESOLVED_AUTHORIZATION_RULE: &str = concat!(
    "The queue item and resolved preset lookup values above are current operator authorization for the ",
    "work and actions they name. If they say `go`, `push`, `publish`, `commit + push`, or carry an ",
    "authorizing preset such as `#auth`, do the work without asking the operator to repeat those ",
    "words, reconfirm, or invoke the preset again. Complete each authorized outward effect before ",
    "handback and include its result or receipt; returning a prepared artifact does not defer the ",
    "effect to coordinator closeout (`#effectbeforecloseout`). If the effect cannot run, return the ",
    "exact blocker and leave the task incomplete. This task authorization does not itself grant a ",
    "harness-native tool permission.\n"
);

/// `#nestedoutwardauth`: a live nested sub-prompt that names push or publish
/// is itself the operator's authorization, even when the preset body is only
/// "rebase" or "review" and a skill says to wait.
pub const SUBAGENT_NESTED_OUTWARD_AUTHORIZATION: &str = concat!(
    "Nested lines under the queue item are operator authorization for the actions they name. ",
    "They are not a chat reply and they are not a question. A nested line that says `push` or ",
    "`publish` authorizes that outward action now, including when a resolved preset or a skill ",
    "it names says to wait until the operator asks, and including when another declared preset ",
    "is unresolved. The missing preset still grants nothing by its name. `publish` on a review ",
    "authorizes posting the review as a plain comment unless the line says approve. A trailing ",
    "`⟦effect:N⟧` marker does not remove that authorization. This grant overrides a later line ",
    "that withholds push or publish only because a preset or skill says to wait.\n"
);

/// Marker line present exactly when the authorization failed closed.
pub const UNRESOLVED_AUTHORIZATION_MARKER: &str = "AUTHORIZATION NOT RESOLVED";

fn render_preamble(
    document: &str,
    content: &str,
    auth: &SubagentAuthorization,
    submitted: &str,
) -> String {
    let mut out = String::new();
    out.push_str(SUBAGENT_PREAMBLE_MARKER);
    out.push_str(&format!(
        "\n\nYou are a background subagent the agent-doc coordinator dispatched for ONE queue item \
         of `{document}`. The operator's authorization is quoted verbatim below; it is the operator's \
         own words, not the coordinator's paraphrase.\n\nQueue item (verbatim):\n{}\n",
        quote(&auth.item)
    ));
    let submitted = submitted.trim();
    if !submitted.is_empty() && submitted != auth.item.trim() {
        out.push_str(&format!(
            "\nSubmitted prompt (`#nestsubmit`), including parent lineage and the prompt tree:\n{}\n",
            quote(submitted)
        ));
    }
    if let Some(queue_id) = &auth.queue_id {
        out.push_str(&format!(
            "\nOrigin queue id (dispatch-time provenance):\n> {queue_id}\n\nAny follow-up queue heads produced by this work belong immediately after the originating head in this queue. Report them to the coordinator in order; the coordinator applies them with `agent-doc queue follow-up {document} --id {queue_id} --after <origin> --item <text>...`. Do not place them in a selected or mirrored sibling queue.\n"
        ));
    }
    // A lineage claim key (`url: #rebase`) has no folded `push` child. The
    // located row still does (`#nestedoutwardauth`, `#nestlistlineage`).
    let mut outward = nested_outward_action_lines(&auth.item);
    if outward.is_empty()
        && let Some(row) = crate::queue_response::queue_row_text_for_head(content, &auth.item)
    {
        outward = nested_outward_action_lines(&row);
        if outward.is_empty()
            && !agent_doc_element_queue::subprompts::subprompt_row_is_struck(&auth.item)
            && crate::queue_response::nest_vector_in_document(content, &auth.item).is_some()
        {
            let row = agent_doc_element_queue::strip_attempt_projection_prefix(&row);
            let row = crate::document_queue::strip_queue_effect_annotation(row.trim());
            let row = row.trim().to_string();
            if !row.is_empty() && line_names_outward_action(&row) {
                outward.push(row);
            }
        }
    }
    if !outward.is_empty() {
        out.push_str(
            "\nOperator authorization, nested under the queue item (effective directives):\n",
        );
        for line in &outward {
            out.push_str(&quote(line));
            out.push('\n');
        }
        out.push('\n');
        out.push_str(SUBAGENT_NESTED_OUTWARD_AUTHORIZATION);
    }
    for tracked in &auth.tracked_items {
        out.push_str(&format!(
            "\n{} item #{} (verbatim):\n{}\n",
            tracked.component,
            tracked.id,
            quote(&tracked.text)
        ));
    }
    let resolved_presets = auth
        .presets
        .iter()
        .filter(|preset| preset.resolved)
        .collect::<Vec<_>>();
    if !resolved_presets.is_empty() {
        out.push_str(
            "\nOperator-authorized prompt preset lookup table (symbol keys map to effective values; symbols remain unexpanded):\n",
        );
    }
    for preset in &resolved_presets {
        let origin = match preset.source {
            PresetSource::QueueAttr => "queue preset",
            PresetSource::QueueAttrInline => "queue preset, defined inline on the queue marker",
            PresetSource::QueueLine => "queue preset line",
            PresetSource::Item => "item preset",
        };
        out.push_str(&format!(
            "  # source: {origin}\n  \"{}\": |-\n",
            preset.name
        ));
        let body = preset.body.as_deref().unwrap_or_default().trim_end();
        if body.is_empty() {
            out.push_str("    \n");
        } else {
            for line in body.lines() {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    for preset in resolved_presets {
        if let Some(runbook) = &preset.runbook {
            out.push_str(&format!(
                "Load its runbook before acting: `agent-doc runbook show '{document}' {}` (`{runbook}`).\n",
                preset.name
            ));
        }
    }
    if auth.model.is_some() || auth.effort.is_some() {
        out.push_str("\nResolved subagent launch policy:\n");
        if let Some(model) = &auth.model {
            out.push_str(&format!("> model: {model}\n"));
        }
        if let Some(effort) = &auth.effort {
            out.push_str(&format!("> effort: {effort}\n"));
        }
        out.push_str(
            "Pass these values to the harness subagent launch: Claude Code Agent `model` / `effort`, or Codex `spawn_agent` `model` / `reasoning_effort`. Omit any value not listed here.\n",
        );
    }
    if !matches!(
        auth.status,
        AuthorizationStatus::UnresolvedPreset | AuthorizationStatus::ConflictingPresetAttrs
    ) {
        out.push('\n');
        out.push_str(SUBAGENT_RESOLVED_AUTHORIZATION_RULE);
    }
    match auth.status {
        AuthorizationStatus::ConflictingPresetAttrs => {
            let detail = auth
                .preset_attr_conflict
                .as_deref()
                .unwrap_or("`preset` and `presets` disagree");
            out.push_str(&format!(
                "\n{UNRESOLVED_AUTHORIZATION_MARKER}: {detail}. \
                 They alias one attribute, not two preset lists. Neither value is operator \
                 authorization. Do not commit, push, install, release, publish, or take any other \
                 outward action on the strength of either spelling. Do the in-worktree work the \
                 item text itself names. Operator fix: keep one spelling, or give both the same value.\n"
            ));
        }
        AuthorizationStatus::UnresolvedPreset => {
            let missing: Vec<String> = auth
                .presets
                .iter()
                .filter(|p| !p.resolved)
                .map(|p| format!("`{}`", p.name))
                .collect();
            out.push_str(&format!(
                "\n{UNRESOLVED_AUTHORIZATION_MARKER}: the queue declares preset {}, but the document's \
                 frontmatter `prompt_presets` does not define it. No operator authorization beyond the \
                 verbatim item text above was resolved. Do not commit, push, install, release, publish, \
                 or take any other outward action on the strength of that preset name. Do the \
                 in-worktree work the item text itself names, then report back what authorization is \
                 missing. Operator fix (the coordinator relays it; you never edit the document): {}\n",
                missing.join(", "),
                crate::prompt_component_attrs::UNRESOLVED_PRESET_FIX
            ));
        }
        AuthorizationStatus::ItemTextOnly => {
            out.push_str(
                "\nNo queue preset applies: the verbatim item text above is the whole authorization. \
                 It grants no outward action (commit, push, install, release, publish) it does not \
                 itself name.\n",
            );
        }
        AuthorizationStatus::Resolved => {}
    }
    if !auth.unresolved_ids.is_empty() {
        let ids: Vec<String> = auth
            .unresolved_ids
            .iter()
            .map(|id| format!("#{id}"))
            .collect();
        out.push_str(&format!(
            "\nNo backlog/review/todo item in the document carries {}; ask the coordinator for its text \
             rather than guessing.\n",
            ids.join(", ")
        ));
    }
    out.push_str(
        "\nScope: this authorization covers only the queue item above. It does not extend to other \
         queue items, backlog items, or documents.\n\nCoordinator rules (they override the \
         authorization above where the two conflict):\n\
         - Work only in your own git worktree outside the IDE-watched project, never in the \
         coordinator's checkout or a checkout another subagent uses.\n",
    );
    out.push_str(concat!("- ", crate::subagent_worktree_layout_rule!(), "\n"));
    out.push_str(
        "- Write command output, logs, and scratch files inside your own worktree, never to a fixed \
         name (such as `check.log` or `msg.txt`) in a shared scratchpad or temp directory: sibling \
         subagents share it, and two runs truncating one file interleave each other's output, so \
         you would read another checkout's results as your own.\n\
         - Never run `make install`, a release build, a version bump, a tag, or a release. Where the \
         authorization says build + install, build and test in your worktree; the coordinator runs \
         the single `make install` after integrating.\n\
         - Never edit, write, or commit the agent-doc session document or its queue, and never run \
         `agent-doc` write/respond/queue commands against it; the coordinator owns the claim and \
         closes the item.\n\
         - Do not hand back while a nested child task is still running. A wrapper response such as \
         `Task <id> is still running. Use /codex:status before continuing it.` is not a result or a \
         blocker: poll or resume that same task until it reaches a terminal state, then include its \
         completed result in your handback. Never return the still-running wrapper text as your \
         final response.\n",
    );
    out.push_str(concat!("- ", subagent_scratch_isolation_rule!(), "\n"));
    out.push_str(concat!("- ", subagent_tmux_isolation_rule!(), "\n"));
    out.push_str(concat!("- ", crate::subagent_build_slot_rule!(), "\n"));
    out.push_str(
        "- Commit and push only when the authorization above includes it. Use your own branch and \
         fast-forward by default. An explicitly named target branch or explicit force-push directive \
         in the verbatim authorization overrides only that default: execute the exact authorized \
         scope, never broaden it, and route any harness-native permission through the request protocol.\n",
    );
    out.push_str(
        "- When a pull request closes a todo or backlog item, annotate that item with the pull \
         request URL in the same work, without waiting to be asked. Do it when you open the pull \
         request for an item that already exists, and when you add the item for a pull request that \
         already exists. A `closing_pull_request` preflight signal names the item and the URL.\n",
    );
    out.push_str(concat!("- ", subagent_handback_shape_rule!(), "\n"));
    if auth.presets.iter().any(|preset| {
        preset
            .body
            .as_deref()
            .is_some_and(crate::goal_preset_terminal::preset_requires_operator_order_rule)
    }) {
        out.push_str("- ");
        out.push_str(crate::goal_preset_terminal::OPERATOR_ORDER_CLOSE_RULE);
        out.push('\n');
    }
    if auth
        .presets
        .iter()
        .any(|preset| preset.name.eq_ignore_ascii_case("#address"))
        || crate::address_fix_on_head::item_mentions_address_preset(&auth.item)
    {
        out.push_str("- ");
        out.push_str(crate::address_fix_on_head::ADDRESS_FIX_ON_HEAD_RULE);
        out.push('\n');
    }
    if crate::inline_discussion::item_mentions_inline_discussion_goal(&auth.item)
        || auth.presets.iter().any(|preset| {
            crate::inline_discussion::preset_name_loads_inline_discussions(&preset.name)
        })
    {
        out.push_str("- ");
        out.push_str(crate::inline_discussion::INLINE_DISCUSSION_RULE);
        out.push('\n');
    }
    out.push('\n');
    out.push_str(SUBAGENT_PROCEDURAL_CHOICE_RULE);
    out.push('\n');
    let queue_item = agent_doc_document::queue_projection::operator_request_origin_item(&auth.item);
    out.push_str(&SUBAGENT_OPERATOR_REQUEST_PROTOCOL.replace("<origin-queue-item>", &queue_item));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree_layout::SUBAGENT_WORKTREE_LAYOUT_RULE;

    fn doc(frontmatter: &str, queue_attr: &str, queue: &[&str], backlog: &[&str]) -> String {
        let queue: String = queue.iter().map(|p| format!("- {p}\n")).collect();
        let backlog: String = backlog.iter().map(|b| format!("- {b}\n")).collect();
        format!(
            "---\nagent_doc_session: s\nagent_doc_format: template\n{frontmatter}---\n\n\
             ## Backlog\n\n<!-- agent:backlog -->\n{backlog}<!-- /agent:backlog -->\n\n\
             ## Queue\n\n<!-- agent:queue {queue_attr} go -->\n{queue}<!-- /agent:queue -->\n"
        )
    }

    const PRESETS: &str = "prompt_presets:\n  \"#spec-test-build-install-commit-push\": \"update spec + tests. build + install for local testing. commit + push\"\n";

    #[test]
    fn preset_queue_carries_the_preset_body_verbatim() {
        let content = doc(
            PRESETS,
            "subagents preset=\"#spec-test-build-install-commit-push\"",
            &["do [#abc]"],
            &["[ ] [#abc] Add a way to post authorization"],
        );
        let auth = subagent_authorization("doc.md", &content, "do [#abc]");
        assert_eq!(auth.status, AuthorizationStatus::Resolved);
        assert_eq!(auth.presets.len(), 1);
        assert_eq!(auth.presets[0].name, "#spec-test-build-install-commit-push");
        assert_eq!(auth.presets[0].source, PresetSource::QueueAttr);
        assert_eq!(
            auth.presets[0].body.as_deref(),
            Some("update spec + tests. build + install for local testing. commit + push")
        );
        let p = &auth.subagent_prompt_preamble;
        assert!(p.starts_with(SUBAGENT_PREAMBLE_MARKER), "{p}");
        assert!(
            p.contains(
                "  \"#spec-test-build-install-commit-push\": |-\n    update spec + tests. build + install for local testing. commit + push"
            )
        );
        assert!(p.contains("> do [#abc]"));
        assert!(p.contains("> Add a way to post authorization"));
        assert!(p.contains("Never run `make install`"));
        assert!(p.contains("A `closing_pull_request` preflight signal names the item and the URL"));
        // Sibling subagents share the coordinator's scratchpad: one `make check`
        // read another worktree's nextest output from a shared `check.log`.
        assert!(
            p.contains("Write command output, logs, and scratch files inside your own worktree")
        );
        assert!(p.contains("covers only the queue item above"));
        assert!(p.contains(SUBAGENT_OPERATOR_REQUEST_MARKER));
        assert!(p.contains("\n- *kind:* question | permission\n"));
        assert!(p.contains(
            "\n- *context:*\n  - <why it is needed>\n  - <current state>\n  - <target/scope>\n  - <risk or effect>\n"
        ));
        assert!(p.contains("A chat reply is context, not a tool permission grant"));
        assert!(p.contains("current operator authorization for the work and actions they name"));
        assert!(p.contains("without asking the operator to repeat those words"));
        assert!(p.contains("does not itself grant a harness-native tool permission"));
        assert!(!p.contains(UNRESOLVED_AUTHORIZATION_MARKER));
        assert!(
            !p.contains("(#operatororderclosed)"),
            "a non-goal preset must not carry the operator-order close rule"
        );
    }

    #[test]
    fn adversarial_order_brief_forbids_closing_blocked_on_a_human_review() {
        let frontmatter = concat!(
            "prompt_presets:\n",
            "  '#adversarial-pr': >-\n",
            "    /goal Bring the pull request to an independently review-clean remote head.\n",
            "    Do not approve or dismiss a human review. A required human approval\n",
            "    this preset cannot submit stays BLOCKED.\n",
            "    Final states. Your final message must begin with exactly one of these lines:\n",
            "    - `REVIEW-CLEAN head=<sha>`.\n",
            "    - `BLOCKED <action> head=<sha>`.\n",
        );
        let item = "https://github.com/sample-org/sample-repo/pull/12: #adversarial-pr";
        let content = doc(frontmatter, "subagents", &[item], &[]);
        let auth = subagent_authorization("prs.md", &content, item);
        let p = &auth.subagent_prompt_preamble;
        assert!(p.contains("(#operatororderclosed)"), "{p}");
        assert!(p.contains("Do not approve or dismiss a human review."));
        assert!(p.contains("does not post another human review"));
        assert!(p.contains("still write the response on the pull request"));
        assert!(p.contains("says why the work is not progressing"));
        assert!(p.contains("(#addressinline)"), "{p}");
        assert!(p.contains("gh api repos/<owner>/<repo>/pulls/<number>/comments"));
        assert!(p.contains("THREAD-FIX-ON-HEAD"));
    }

    #[test]
    fn address_and_adversarial_briefs_require_inline_discussion_comments() {
        let frontmatter = concat!(
            "prompt_presets:\n",
            "  '#address': 'Address every actionable review item on the exact current head.'\n",
            "  '#adversarial-pr': >-\n",
            "    /goal Bring the pull request to a review-clean head.\n",
            "    Final states. Your final message must begin with exactly one of these lines:\n",
            "    - `REVIEW-CLEAN head=<sha>`.\n",
            "    - `BLOCKED <action> head=<sha>`.\n",
        );
        for item in [
            "https://github.com/haiven-dev/haiven-api/pull/869: #address",
            "https://github.com/haiven-dev/haiven-api/pull/869: #adversarial-pr",
        ] {
            let content = doc(frontmatter, "subagents", &[item], &[]);
            let auth = subagent_authorization("prs.md", &content, item);
            let preamble = &auth.subagent_prompt_preamble;
            assert!(preamble.contains("(#addressinline)"), "{item}\n{preamble}");
            assert!(
                preamble.contains("gh api repos/<owner>/<repo>/pulls/<number>/comments"),
                "{preamble}"
            );
            assert!(preamble.contains("THREAD-FIX-ON-HEAD"), "{preamble}");
            assert!(preamble.contains("INLINE-THREADS-LOADED 0"), "{preamble}");
        }
        let other = doc("", "subagents", &["do [#abc]"], &[]);
        let auth = subagent_authorization("prs.md", &other, "do [#abc]");
        assert!(
            !auth.subagent_prompt_preamble.contains("(#addressinline)"),
            "{}",
            auth.subagent_prompt_preamble
        );
    }

    #[test]
    fn address_run_brief_requires_the_written_fix_on_the_head() {
        let frontmatter = concat!(
            "prompt_presets:\n",
            "  '#address': 'Address every actionable review item on the exact current head.'\n",
        );
        let item = "https://github.com/sample-org/sample-repo/pull/12: #address";
        let content = doc(frontmatter, "subagents", &[item], &[]);
        let preamble = subagent_authorization("prs.md", &content, item).subagent_prompt_preamble;
        assert!(preamble.contains("(#reviewfixonhead)"), "{preamble}");
        assert!(preamble.contains("FIX-ON-HEAD <review-url> head=<sha>"));
        assert!(preamble.contains("written fix is the code on the exact current head"));

        let other = doc(
            frontmatter,
            "subagents",
            &["do [#abc]"],
            &["[ ] [#abc] something else"],
        );
        assert!(
            !subagent_authorization("prs.md", &other, "do [#abc]")
                .subagent_prompt_preamble
                .contains("(#reviewfixonhead)")
        );
    }

    #[test]
    fn slash_command_queue_prompt_without_context_builds_a_subagent_brief() {
        let item = "/goal Extract target haiven-sdk-* repos from the haiven-sdk repo.";
        let content = doc("", "subagents", &[item], &[]);

        let auth = subagent_authorization("sdk-release.md", &content, item);

        assert_eq!(auth.item, item);
        assert!(auth.prompt_context.is_none());
        assert!(
            auth.subagent_prompt_preamble
                .contains(&format!("Queue item (verbatim):\n> {item}\n")),
            "{}",
            auth.subagent_prompt_preamble
        );
        assert!(
            auth.subagent_prompt_preamble.contains(item),
            "{}",
            auth.subagent_prompt_preamble
        );
    }

    #[test]
    fn pr_worker_keeps_procedural_disposition_choices_off_the_operator_queue() {
        let item =
            "https://github.com/haiven-dev/haiven-sdk-unity-bossroom/pull/5 [#q-e935b5c271d6]";
        let content = doc("", "subagents", &[item], &[]);

        let auth = subagent_authorization("sdk-dev.md", &content, item);
        let preamble = &auth.subagent_prompt_preamble;

        assert!(preamble.contains(SUBAGENT_PROCEDURAL_CHOICE_RULE));
        assert!(preamble.contains("Choices such as `leave-tracked`, `review`, or `address`"));
        assert!(preamble.contains("are agent-owned procedural choices"));
        assert!(
            preamble.contains(
                "continue the concrete authorized objective while actionable work remains"
            )
        );
        assert!(
            preamble.contains(
                "Ask the operator only when the answer changes product behavior or scope"
            )
        );

        let source_runbook = include_str!("../../runbooks/respond.md");
        let bundled_runbook = include_str!("../../.claude/skills/agent-doc/runbooks/respond.md");
        assert_eq!(source_runbook, bundled_runbook);
        assert!(source_runbook.contains("#agentownedproceduralchoice"));
        assert!(
            source_runbook
                .contains("`leave-tracked` / `review` / `address`, queue/review/todo placement")
        );
        assert!(source_runbook.contains(
            "resume the worker with that derived\n     disposition instead of adding a question"
        ));

        let source_planning = include_str!("../../runbooks/planning-dispatch.md");
        let bundled_planning =
            include_str!("../../.claude/skills/agent-doc/runbooks/planning-dispatch.md");
        assert_eq!(source_planning, bundled_planning);
        assert!(
            source_planning
                .contains("Before presenting a worker packet, apply `#agentownedproceduralchoice`")
        );
        assert!(source_planning.contains(
            "Only genuinely operator-owned input\n   becomes an exchange question and TODO"
        ));
    }

    #[test]
    fn waiting_item_brief_carries_operator_prompt_nested_under_response_metadata() {
        let item = concat!(
            "Confirm the game integrator can git ls-remote private haiven-dev/haiven-sdk ",
            "from their account <!-- agent-doc:waiting-item -->"
        );
        let question = "What is the \"game integrator\"? Why do you need this?";
        let content = format!(
            concat!(
                "---\nagent_doc_session: sdk-release\nagent_doc_format: template\n---\n\n",
                "<!-- agent:run id=main subagents go -->\n",
                "- ### Waiting on you <!-- agent-doc:waiting-on-you -->\n",
                "  - {item}\n",
                "    - **Response:** Confirm repository access. <!-- agent-doc:waiting-metadata -->\n",
                "      - **Question:** What decision should be recorded? <!-- agent-doc:waiting-metadata -->\n",
                "      - **Next actions:** <!-- agent-doc:waiting-metadata -->\n",
                "        - **Action:** Confirm repository access. <!-- agent-doc:waiting-metadata -->\n",
                "        - {question}\n",
                "<!-- /agent:run -->\n"
            ),
            item = item,
            question = question,
        );

        let auth = subagent_authorization("sdk-release.md", &content, item);
        let packet = auth
            .prompt_context
            .as_ref()
            .expect("the executable waiting selector carries lineage");

        assert_eq!(packet.decision_evidence, [question]);
        assert!(
            auth.subagent_prompt_preamble.contains(question),
            "{}",
            auth.subagent_prompt_preamble
        );
        assert!(
            auth.subagent_prompt_preamble
                .contains("Prompt tree (`#nestsubmit`"),
            "{}",
            auth.subagent_prompt_preamble
        );
    }

    #[test]
    fn preset_queue_carries_guidance_for_the_active_agent() {
        let presets = concat!(
            "agent: codex\n",
            "prompt_presets:\n",
            "  '#authorize':\n",
            "    default: shared authorization\n",
            "    codex: codex authorization\n",
            "    claude: claude authorization\n",
        );
        let content = doc(presets, "subagents preset=#authorize", &["fix it"], &[]);
        let auth = subagent_authorization("doc.md", &content, "fix it");
        assert_eq!(auth.status, AuthorizationStatus::Resolved);
        assert_eq!(auth.presets[0].body.as_deref(), Some("codex authorization"));
        assert!(
            auth.subagent_prompt_preamble
                .contains("  \"#authorize\": |-\n    codex authorization"),
            "{}",
            auth.subagent_prompt_preamble
        );
        assert!(
            !auth
                .subagent_prompt_preamble
                .contains("    shared authorization"),
            "{}",
            auth.subagent_prompt_preamble
        );
        assert!(
            auth.subagent_prompt_preamble.contains(
                "Operator-authorized prompt preset lookup table (symbol keys map to effective values; symbols remain unexpanded)"
            ),
            "{}",
            auth.subagent_prompt_preamble
        );
    }

    /// `#nestsubmit`: the preamble quotes the parent lineage as the work, and
    /// the operator-request `queue_item` stays the leaf that claims the row.
    #[test]
    fn nested_rebase_preamble_quotes_parent_lineage_and_keeps_the_leaf() {
        let content = "\
---
agent_doc_session: s
agent_doc_format: template
---

## Queue

<!-- agent:queue subagents go -->
- 📥 https://github.com/haiven-dev/haiven-api/pull/736
  - #rebase
- 📥 https://github.com/haiven-dev/haiven-api/pull/758
  - #rebase
<!-- /agent:queue -->
";
        let heads = vec!["#rebase".to_string(), "#rebase".to_string()];
        let submitted = crate::queue_response::submitted_prompts_for_heads(content, &heads);
        assert_eq!(submitted.len(), 2);
        assert!(
            submitted[0]
                .starts_with("https://github.com/haiven-dev/haiven-api/pull/736: #rebase\n"),
            "{}",
            submitted[0]
        );
        assert!(
            submitted[1]
                .starts_with("https://github.com/haiven-dev/haiven-api/pull/758: #rebase\n"),
            "{}",
            submitted[1]
        );
        assert!(submitted[0].contains("pull/758"), "{}", submitted[0]);
        assert!(submitted[1].contains("pull/736"), "{}", submitted[1]);
        let first = subagent_authorization("api.md", content, "#rebase");
        assert_eq!(first.item, "#rebase");
        let preamble = &first.subagent_prompt_preamble;
        assert!(
            preamble.contains("Queue item (verbatim):\n> #rebase\n"),
            "{preamble}"
        );
        assert!(
            preamble.contains("Submitted prompt (`#nestsubmit`), including parent lineage and the prompt tree:\n> <agent-doc-prompt-context>\n"),
            "{preamble}"
        );
        assert!(
            preamble.contains("> https://github.com/haiven-dev/haiven-api/pull/736: #rebase\n"),
            "{preamble}"
        );
        assert!(
            preamble.contains("https://github.com/haiven-dev/haiven-api/pull/758"),
            "{preamble}"
        );
        assert!(preamble.contains("\n  - #rebase\n"), "{preamble}");
        assert!(!preamble.contains("pull/758: #rebase"), "{preamble}");

        let components = agent_doc_element::element::parse(content).unwrap_or_default();
        let presets = agent_doc_frontmatter::frontmatter::PromptPresets::default();
        let second = subagent_authorization_with_presets(
            "api.md",
            content,
            "#rebase",
            &components,
            &presets,
            Some(submitted[1].as_str()),
        );
        assert_eq!(second.item, "#rebase");
        assert!(
            second
                .subagent_prompt_preamble
                .contains("pull/758: #rebase"),
            "{}",
            second.subagent_prompt_preamble
        );
        assert!(second.subagent_prompt_preamble.contains("\n  - #rebase\n"));
    }

    #[test]
    fn dispatch_policy_resolves_queue_then_preset_then_frontmatter() {
        let frontmatter = concat!(
            "subagent_model: default-model\n",
            "subagent_effort: medium\n",
            "prompt_presets:\n",
            "  '#review':\n",
            "    prompt: review it\n",
            "    model: preset-model\n",
            "    effort: high\n",
        );
        let queue = doc(
            frontmatter,
            "subagents preset=#review model=queue-model effort=xhigh",
            &["fix it"],
            &[],
        );
        let auth = subagent_authorization("doc.md", &queue, "fix it");
        assert_eq!(auth.model.as_deref(), Some("queue-model"));
        assert_eq!(auth.effort.as_deref(), Some("xhigh"));
        assert!(
            auth.subagent_prompt_preamble
                .contains("> model: queue-model")
        );
        assert!(auth.subagent_prompt_preamble.contains("> effort: xhigh"));
        assert!(
            auth.subagent_prompt_preamble
                .contains("Codex `spawn_agent`")
        );

        let preset = queue
            .replace(" model=queue-model effort=xhigh", "")
            .replace("subagent_model: default-model\n", "")
            .replace("subagent_effort: medium\n", "");
        let auth = subagent_authorization("doc.md", &preset, "fix it");
        assert_eq!(auth.model.as_deref(), Some("preset-model"));
        assert_eq!(auth.effort.as_deref(), Some("high"));

        let defaults = doc(
            "subagent_model: default-model\nsubagent_effort: medium\n",
            "subagents",
            &["fix it"],
            &[],
        );
        let auth = subagent_authorization("doc.md", &defaults, "fix it");
        assert_eq!(auth.model.as_deref(), Some("default-model"));
        assert_eq!(auth.effort.as_deref(), Some("medium"));
    }

    #[test]
    fn resolved_auth_preset_is_not_reconfirmed_before_commit_and_push() {
        let content = doc(
            concat!(
                "prompt_presets:\n",
                "  \"#spec-test-build-install-commit-push\": \"update spec + tests. build + install for local testing. commit + push\"\n",
                "  \"#auth\": \"I authorize you to do this.\"\n"
            ),
            "subagents preset=\"#spec-test-build-install-commit-push\"",
            &["do [#abc] #auth"],
            &["[ ] [#abc] Fix directive authorization rejection"],
        );
        let auth = subagent_authorization("doc.md", &content, "do [#abc] #auth");
        assert_eq!(auth.status, AuthorizationStatus::Resolved);
        assert_eq!(auth.presets.len(), 2, "{auth:?}");
        assert_eq!(auth.presets[1].name, "#auth");
        assert_eq!(
            auth.presets[1].body.as_deref(),
            Some("I authorize you to do this.")
        );
        let p = &auth.subagent_prompt_preamble;
        assert!(
            p.contains("Operator-authorized prompt preset lookup table"),
            "{p}"
        );
        assert!(
            p.contains("  # source: item preset\n  \"#auth\": |-"),
            "{p}"
        );
        assert!(p.contains("    I authorize you to do this."), "{p}");
        assert!(
            p.contains("without asking the operator to repeat those words"),
            "{p}"
        );
        assert!(
            p.contains("protected tool action still requires harness approval"),
            "{p}"
        );
        assert!(
            !p.contains("treat prior authorization as the answer"),
            "{p}"
        );
    }

    #[test]
    fn queue_without_preset_is_item_text_only() {
        let content = doc("", "subagents", &["fix the flaky test in foo.rs"], &[]);
        let auth = subagent_authorization("doc.md", &content, "fix the flaky test in foo.rs");
        assert_eq!(auth.status, AuthorizationStatus::ItemTextOnly);
        assert!(auth.presets.is_empty());
        assert!(auth.tracked_items.is_empty());
        assert!(auth.unresolved_ids.is_empty());
        let p = &auth.subagent_prompt_preamble;
        assert!(p.contains("> fix the flaky test in foo.rs"));
        assert!(p.contains("the whole authorization"));
        assert!(!p.contains("Operator authorization, "));
        assert!(!p.contains(UNRESOLVED_AUTHORIZATION_MARKER));
    }

    #[test]
    fn id_head_resolves_backlog_text_with_continuation() {
        let content = doc(
            "",
            "subagents",
            &["#subagents do [#waypost]"],
            &["[ ] [#waypost] Post authorization to subagents\n  - include preset bodies"],
        );
        let auth = subagent_authorization("doc.md", &content, "#subagents do [#waypost]");
        assert_eq!(auth.tracked_items.len(), 1, "{auth:?}");
        assert_eq!(auth.tracked_items[0].id, "waypost");
        assert_eq!(auth.tracked_items[0].component, "backlog");
        assert!(
            auth.tracked_items[0]
                .text
                .contains("Post authorization to subagents")
        );
        assert!(auth.tracked_items[0].text.contains("include preset bodies"));
        assert!(auth.unresolved_ids.is_empty());
        assert!(
            auth.subagent_prompt_preamble
                .contains("backlog item #waypost (verbatim)")
        );
    }

    /// `#todosync`: a `do [#id]` head that names an `agent:todo` item resolves
    /// to the Todo text and its nested replies, not an unresolved id.
    #[test]
    fn id_head_resolves_todo_text_with_nested_replies() {
        let content = format!(
            "{}\n## Todo\n\n<!-- agent:todo priority -->\n- [ ] [#pmq] Answer the questions\n  - One per provider?\n    - One per provider.\n<!-- /agent:todo -->\n",
            doc("", "subagents", &["do [#pmq]"], &[])
        );
        let auth = subagent_authorization("doc.md", &content, "do [#pmq]");
        assert!(auth.unresolved_ids.is_empty(), "{auth:?}");
        assert_eq!(auth.tracked_items.len(), 1, "{auth:?}");
        assert_eq!(auth.tracked_items[0].component, "todo");
        assert_eq!(
            auth.tracked_items[0].text,
            "Answer the questions\n  - One per provider?\n    - One per provider."
        );
        let p = &auth.subagent_prompt_preamble;
        assert!(p.contains("todo item #pmq (verbatim)"), "{p}");
        assert!(p.contains(">     - One per provider."), "{p}");
    }

    /// Tracked work wins over a legacy Todo that shares its id.
    #[test]
    fn id_head_prefers_backlog_over_a_todo_sharing_its_id() {
        let content = format!(
            "{}\n<!-- agent:todo -->\n- [ ] [#abc] approve it\n<!-- /agent:todo -->\n",
            doc(
                "",
                "subagents",
                &["do [#abc]"],
                &["[ ] [#abc] the backlog work"]
            )
        );
        let auth = subagent_authorization("doc.md", &content, "do [#abc]");
        assert_eq!(auth.tracked_items.len(), 1, "{auth:?}");
        assert_eq!(auth.tracked_items[0].component, "backlog");
    }

    #[test]
    fn free_text_head_has_no_tracked_items() {
        let content = doc(
            PRESETS,
            "subagents preset=\"#spec-test-build-install-commit-push\"",
            &["rename the foo module to bar"],
            &[],
        );
        let auth = subagent_authorization("doc.md", &content, "rename the foo module to bar");
        assert_eq!(auth.status, AuthorizationStatus::Resolved);
        assert!(auth.tracked_items.is_empty());
        assert!(auth.unresolved_ids.is_empty());
        assert!(
            auth.subagent_prompt_preamble
                .contains("> rename the foo module to bar")
        );
    }

    /// `#runpresetunresolved`: the live session's `agent:run` marker, verbatim.
    const LIVE_RUN_PRESET: &str = "#spec-test-build-install-commit-push: approve all commit + integration + push: approve all worktree removals";

    fn run_doc(frontmatter: &str, preset: &str) -> String {
        format!(
            "---\nagent_doc_session: s\nagent_doc_format: template\n{frontmatter}---\n\n\
             ## Run <!-- agent:run id=\"main\" subagents preset=\"{preset}\" priority go -->\n\
             - do [#runpresetunresolved]\n<!-- /agent:run -->\n\n\
             ## Backlog\n\n<!-- agent:backlog -->\n\
             - [ ] [#runpresetunresolved] [P1] resolve inline run presets\n<!-- /agent:backlog -->\n"
        )
    }

    /// `#runpresetunresolved`: `preset="#name: body"` is an inline preset
    /// definition. It resolves exactly like a frontmatter entry, the brief
    /// quotes the operator's body verbatim, and the fail-closed block is gone.
    /// Before the fix every word token was looked up and every brief said
    /// AUTHORIZATION NOT RESOLVED.
    #[test]
    fn inline_run_preset_definition_resolves_and_quotes_the_body_verbatim() {
        let content = run_doc("", LIVE_RUN_PRESET);
        let auth = subagent_authorization("doc.md", &content, "do [#runpresetunresolved]");
        assert_eq!(auth.status, AuthorizationStatus::Resolved, "{auth:#?}");
        assert_eq!(auth.presets.len(), 1, "{:#?}", auth.presets);
        let preset = &auth.presets[0];
        assert_eq!(preset.name, "#spec-test-build-install-commit-push");
        assert_eq!(preset.source, PresetSource::QueueAttrInline);
        assert!(preset.resolved);
        assert_eq!(
            preset.body.as_deref(),
            Some("approve all commit + integration + push: approve all worktree removals")
        );
        assert!(auth.unresolved_ids.is_empty(), "{:?}", auth.unresolved_ids);
        let p = &auth.subagent_prompt_preamble;
        assert!(!p.contains(UNRESOLVED_AUTHORIZATION_MARKER), "{p}");
        assert!(
            p.contains(concat!(
                "  # source: queue preset, defined inline on the queue marker\n",
                "  \"#spec-test-build-install-commit-push\": |-\n",
                "    approve all commit + integration + push: approve all worktree removals\n",
            )),
            "{p}"
        );
        assert!(p.contains("current operator authorization for the work and actions they name"));
        assert!(p.contains("> [P1] resolve inline run presets"));
        // No word token of the free text is looked up as its own preset.
        for word in ["`approve`", "`all`", "`+`", "`removals`"] {
            assert!(!p.contains(word), "{word} leaked as a preset: {p}");
        }
        let json = serde_json::to_value(&auth).unwrap();
        assert_eq!(json["status"], "resolved");
        assert_eq!(json["presets"][0]["source"], "queue_attr_inline");
    }

    /// An inline definition shadows a frontmatter entry of the same name: the
    /// marker's literal text is the authorization, never the two merged.
    #[test]
    fn inline_run_preset_shadows_a_frontmatter_definition() {
        let content = run_doc(PRESETS, LIVE_RUN_PRESET);
        let auth = subagent_authorization("doc.md", &content, "do [#runpresetunresolved]");
        assert_eq!(auth.status, AuthorizationStatus::Resolved);
        assert_eq!(auth.presets.len(), 1);
        assert_eq!(
            auth.presets[0].body.as_deref(),
            Some("approve all commit + integration + push: approve all worktree removals")
        );
        assert!(
            !auth
                .subagent_prompt_preamble
                .contains("build + install for local testing")
        );
    }

    /// A bare unknown `#name` keeps meaning a lookup and still fails closed;
    /// the brief names the operator fix.
    #[test]
    fn bare_unknown_run_preset_still_fails_closed_and_names_the_fix() {
        let content = run_doc("", "#spec-test-build-install-commit-push");
        let auth = subagent_authorization("doc.md", &content, "do [#runpresetunresolved]");
        assert_eq!(auth.status, AuthorizationStatus::UnresolvedPreset);
        assert_eq!(auth.presets.len(), 1);
        assert_eq!(auth.presets[0].source, PresetSource::QueueAttr);
        assert!(!auth.presets[0].resolved);
        assert_eq!(auth.presets[0].body, None);
        let p = &auth.subagent_prompt_preamble;
        assert!(p.contains(UNRESOLVED_AUTHORIZATION_MARKER), "{p}");
        assert!(p.contains("`#spec-test-build-install-commit-push`"), "{p}");
        assert!(p.contains("Do not commit, push"), "{p}");
        assert!(
            p.contains(crate::prompt_component_attrs::UNRESOLVED_PRESET_FIX),
            "{p}"
        );

        // Free text with no `#name:` head still fails closed, token by token.
        let free = run_doc("", "approve all commit + push");
        let auth = subagent_authorization("doc.md", &free, "do [#runpresetunresolved]");
        assert_eq!(auth.status, AuthorizationStatus::UnresolvedPreset);
        assert!(
            auth.subagent_prompt_preamble
                .contains(UNRESOLVED_AUTHORIZATION_MARKER)
        );
    }

    #[test]
    fn missing_preset_fails_closed_without_inventing_a_body() {
        let content = doc(
            "",
            "subagents preset=\"#ship-it\"",
            &["do [#abc]"],
            &["[ ] [#abc] thing"],
        );
        let auth = subagent_authorization("doc.md", &content, "do [#abc]");
        assert_eq!(auth.status, AuthorizationStatus::UnresolvedPreset);
        assert_eq!(auth.presets.len(), 1);
        assert_eq!(auth.presets[0].name, "#ship-it");
        assert!(!auth.presets[0].resolved);
        assert_eq!(auth.presets[0].body, None);
        let p = &auth.subagent_prompt_preamble;
        assert!(p.contains(UNRESOLVED_AUTHORIZATION_MARKER), "{p}");
        assert!(p.contains("`#ship-it`"));
        assert!(p.contains("Do not commit, push"));
        assert!(!p.contains("Operator authorization, "));
        let json = serde_json::to_value(&auth).unwrap();
        assert_eq!(json["status"], "unresolved_preset");
        assert_eq!(json["presets"][0]["resolved"], false);
        assert!(json["presets"][0].get("body").is_none());
    }

    #[test]
    fn unknown_id_is_reported_not_invented() {
        let content = doc("", "subagents", &["do [#nope]"], &[]);
        let auth = subagent_authorization("doc.md", &content, "do [#nope]");
        assert_eq!(auth.unresolved_ids, vec!["nope".to_string()]);
        assert!(
            auth.subagent_prompt_preamble
                .contains("No backlog/review/todo item in the document carries #nope")
        );
    }

    #[test]
    fn authorization_context_round_trips_and_old_payload_defaults_absent() {
        let content = doc("", "subagents", &["do [#known]"], &["[ ] [#known] thing"]);
        let auth = subagent_authorization("doc.md", &content, "do [#known]");
        assert!(auth.prompt_context.is_some());
        let mut json = serde_json::to_value(&auth).unwrap();
        let round_trip: SubagentAuthorization = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(round_trip, auth);
        json.as_object_mut().unwrap().remove("prompt_context");
        let old: SubagentAuthorization = serde_json::from_value(json).unwrap();
        assert_eq!(old.prompt_context, None);
    }

    #[test]
    fn presets_alias_on_the_queue_resolves_like_preset() {
        let content = doc(
            PRESETS,
            "subagents presets=\"#spec-test-build-install-commit-push\"",
            &["do [#abc]"],
            &["[ ] [#abc] Add a way to post authorization"],
        );
        let auth = subagent_authorization("doc.md", &content, "do [#abc]");
        assert_eq!(auth.status, AuthorizationStatus::Resolved);
        assert!(auth.preset_attr_conflict.is_none());
        assert_eq!(auth.presets.len(), 1);
        assert_eq!(
            auth.presets[0].body.as_deref(),
            Some("update spec + tests. build + install for local testing. commit + push")
        );
    }

    #[test]
    fn identical_preset_and_presets_are_one_preset() {
        let fm = "prompt_presets:\n  \"#auth\": \"I authorize push\"\n";
        let content = doc(
            fm,
            "subagents preset=\"#auth\" presets=\"#auth\"",
            &["publish the release"],
            &[],
        );
        let auth = subagent_authorization("doc.md", &content, "publish the release");
        assert_eq!(auth.status, AuthorizationStatus::Resolved);
        assert_eq!(auth.presets.len(), 1);
        assert_eq!(auth.presets[0].name, "#auth");
        assert_eq!(auth.presets[0].body.as_deref(), Some("I authorize push"));
        assert!(auth.preset_attr_conflict.is_none());
    }

    #[test]
    fn conflicting_preset_attrs_fail_closed_without_either_body() {
        let fm =
            "prompt_presets:\n  \"#auth\": \"I authorize push\"\n  \"#ship\": \"commit + push\"\n";
        for marker in ["queue", "run", "loop"] {
            let content = format!(
                "---\nagent_doc_session: s\nagent_doc_format: template\n{fm}---\n\n\
                 <!-- agent:{marker} subagents preset=\"#auth\" presets=\"#ship\" go -->\n\
                 - publish the release\n\
                 <!-- /agent:{marker} -->\n"
            );
            let auth = subagent_authorization("doc.md", &content, "publish the release");
            assert_eq!(
                auth.status,
                AuthorizationStatus::ConflictingPresetAttrs,
                "{marker}: {auth:?}"
            );
            assert!(auth.presets.is_empty(), "{marker}: {:?}", auth.presets);
            let preamble = &auth.subagent_prompt_preamble;
            assert!(
                preamble.contains(UNRESOLVED_AUTHORIZATION_MARKER),
                "{marker}: {preamble}"
            );
            assert!(
                preamble.contains("#auth") && preamble.contains("#ship"),
                "{preamble}"
            );
            assert!(!preamble.contains("I authorize push"), "{preamble}");
            assert!(!preamble.contains("commit + push"), "{preamble}");
            assert!(!preamble.contains("No queue preset applies"), "{preamble}");
            assert!(
                !preamble.contains(SUBAGENT_RESOLVED_AUTHORIZATION_RULE),
                "{preamble}"
            );
            assert_eq!(
                auth.preset_attr_conflict.as_deref(),
                Some(
                    "`preset=\"#auth\"` and `presets=\"#ship\"` disagree; they alias one attribute, so neither value is applied"
                ),
                "{marker}"
            );
        }
    }

    #[test]
    fn unhashed_preset_attr_resolves_to_hashtag_key() {
        let fm = "prompt_presets:\n  \"#review\": \"review only, no commits\"\n";
        let content = doc(fm, "subagents preset=review", &["look at foo"], &[]);
        let auth = subagent_authorization("doc.md", &content, "look at foo");
        assert_eq!(auth.status, AuthorizationStatus::Resolved);
        assert_eq!(auth.presets[0].name, "#review");
        assert_eq!(
            auth.presets[0].body.as_deref(),
            Some("review only, no commits")
        );
    }

    #[test]
    fn multi_preset_attr_preserves_order_deduplicates_and_fails_closed() {
        let fm = concat!(
            "prompt_presets:\n",
            "  \"#push\": \"commit and push\"\n",
            "  \"#auth\":\n",
            "    prompt: \"use the release credentials\"\n",
            "    runbook: \"runbooks/release-auth.md\"\n",
        );
        let content = doc(
            fm,
            "subagents preset=\"#push '#auth' #push #missing\"",
            &["publish the release"],
            &[],
        );
        let auth = subagent_authorization("doc.md", &content, "publish the release");
        assert_eq!(auth.status, AuthorizationStatus::UnresolvedPreset);
        assert_eq!(
            auth.presets
                .iter()
                .map(|preset| preset.name.as_str())
                .collect::<Vec<_>>(),
            vec!["#push", "#auth", "#missing"]
        );
        assert!(auth.presets[..2].iter().all(|preset| preset.resolved));
        assert!(!auth.presets[2].resolved);
        assert_eq!(
            auth.presets[1].runbook.as_deref(),
            Some("runbooks/release-auth.md")
        );
        let preamble = &auth.subagent_prompt_preamble;
        let push = preamble.find("\"#push\": |-").unwrap();
        let auth_preset = preamble.find("\"#auth\": |-").unwrap();
        let missing = preamble.find("`#missing`").unwrap();
        assert!(push < auth_preset && auth_preset < missing, "{preamble}");
        assert!(preamble.contains("agent-doc runbook show 'doc.md' #auth"));
        assert!(preamble.contains(UNRESOLVED_AUTHORIZATION_MARKER));
    }

    #[test]
    fn body_level_preset_line_is_queue_scoped_authorization() {
        let content = concat!(
            "---\nagent_doc_session: s\nagent_doc_format: template\n",
            "prompt_presets:\n  \"#review\": \"review only, no commits\"\n---\n\n",
            "<!-- agent:queue subagents go -->\npreset #review\n- look at foo\n<!-- /agent:queue -->\n",
        );
        let auth = subagent_authorization("doc.md", content, "look at foo");
        assert_eq!(auth.status, AuthorizationStatus::Resolved, "{auth:?}");
        assert_eq!(auth.presets[0].source, PresetSource::QueueLine);
        assert!(auth.subagent_prompt_preamble.contains(
            "  # source: queue preset line\n  \"#review\": |-\n    review only, no commits"
        ));
    }

    #[test]
    fn authorization_carries_selected_queue_provenance_when_head_is_mirrored() {
        let content = concat!(
            "<!-- agent:queue id=main after=issues -->\n",
            "- do [#httpsgithubcom]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:queue id=issues subagents -->\n",
            "- do [#httpsgithubcom]\n",
            "<!-- /agent:queue -->\n",
        );
        let auth = subagent_authorization("doc.md", content, "do [#httpsgithubcom]");
        assert_eq!(auth.queue_id.as_deref(), Some("issues"));
        assert!(
            auth.subagent_prompt_preamble.contains(
                "agent-doc queue follow-up doc.md --id issues --after <origin> --item <text>..."
            ),
            "{}",
            auth.subagent_prompt_preamble
        );
    }

    /// A head that lives only in a later `agent:run` names that run, not the
    /// earlier selected run. Its submitted prompt keeps that section's subprompt.
    #[test]
    fn authorization_names_the_later_run_that_holds_the_subprompt() {
        let content = "\
<!-- agent:run id=\"api-prs\" subagents go -->
- https://example.com/api/pull/1
  - #rebase
<!-- /agent:run -->

<!-- agent:run id=\"contracts-prs\" subagents -->
- https://example.com/contracts/pull/9
  - #rebase
  - record the contracts note
<!-- /agent:run -->

<!-- agent:loop id=\"nightly\" subagents -->
- https://example.com/loop/pull/3
  - #check
    - check the nightly note
<!-- /agent:loop -->
";
        let contracts = "https://example.com/contracts/pull/9: #rebase";
        let nightly = "https://example.com/loop/pull/3: #check";
        let contracts_auth = subagent_authorization("doc.md", content, contracts);
        assert_eq!(contracts_auth.queue_id.as_deref(), Some("contracts-prs"));
        assert!(
            contracts_auth
                .subagent_prompt_preamble
                .contains("record the contracts note"),
            "{}",
            contracts_auth.subagent_prompt_preamble
        );
        assert!(
            contracts_auth.subagent_prompt_preamble.contains(
                "agent-doc queue follow-up doc.md --id contracts-prs --after <origin> --item <text>..."
            ),
            "{}",
            contracts_auth.subagent_prompt_preamble
        );
        let loop_auth = subagent_authorization("doc.md", content, nightly);
        assert_eq!(loop_auth.queue_id.as_deref(), Some("nightly"));
        assert!(
            loop_auth
                .subagent_prompt_preamble
                .contains("check the nightly note"),
            "{}",
            loop_auth.subagent_prompt_preamble
        );
    }

    #[test]
    fn every_status_restates_the_scratch_isolation_rule() {
        let resolved = doc(
            PRESETS,
            "subagents preset=\"#spec-test-build-install-commit-push\"",
            &["do [#abc]"],
            &["[ ] [#abc] x"],
        );
        let unresolved = doc("", "subagents preset=\"#nosuch\"", &["do [#abc]"], &[]);
        let item_only = doc("", "subagents", &["look at foo"], &[]);
        let bullet = format!("- {SUBAGENT_SCRATCH_ISOLATION_RULE}\n");
        for (content, item) in [
            (&resolved, "do [#abc]"),
            (&unresolved, "do [#abc]"),
            (&item_only, "look at foo"),
        ] {
            let p = subagent_authorization("doc.md", content, item).subagent_prompt_preamble;
            let rule_at = p.find(&bullet).unwrap_or_else(|| panic!("{p}"));
            // It sits inside the coordinator rules, before the commit/push rule.
            assert!(p.find("Coordinator rules").unwrap() < rule_at, "{p}");
            assert!(
                rule_at < p.find("- Commit and push only when").unwrap(),
                "{p}"
            );
            assert!(p.contains("explicit force-push directive"), "{p}");
            assert!(p.contains("overrides only that default"), "{p}");
        }
        assert!(SUBAGENT_SCRATCH_ISOLATION_RULE.contains("`git commit -F -`"));
        assert!(
            SUBAGENT_SCRATCH_ISOLATION_RULE.contains("never a file in a shared scratchpad or /tmp")
        );
        assert!(SUBAGENT_SCRATCH_ISOLATION_RULE.contains("unique `mktemp` path"));
    }

    /// `#worktreetmuxrouterskew`: every preamble forbids a flat worktree whose
    /// `../tmux-router` is the shared `~/worktrees/tmux-router` checkout.
    #[test]
    fn every_status_restates_the_paired_worktree_layout_rule() {
        let resolved = doc(
            PRESETS,
            "subagents preset=\"#spec-test-build-install-commit-push\"",
            &["do [#abc]"],
            &["[ ] [#abc] x"],
        );
        let unresolved = doc("", "subagents preset=\"#nosuch\"", &["do [#abc]"], &[]);
        let item_only = doc("", "subagents", &["look at foo"], &[]);
        let bullet = format!("- {SUBAGENT_WORKTREE_LAYOUT_RULE}\n");
        for (content, item) in [
            (&resolved, "do [#abc]"),
            (&unresolved, "do [#abc]"),
            (&item_only, "look at foo"),
        ] {
            let p = subagent_authorization("doc.md", content, item).subagent_prompt_preamble;
            let rule_at = p.find(&bullet).unwrap_or_else(|| panic!("{p}"));
            assert!(p.find("Coordinator rules").unwrap() < rule_at, "{p}");
            assert!(
                rule_at < p.find("- Commit and push only when").unwrap(),
                "{p}"
            );
            let own_worktree = p
                .find("Work only in your own git worktree")
                .expect("own-worktree bullet");
            assert!(own_worktree < rule_at, "{p}");
        }
        assert!(
            !crate::worktree_layout::resolves_shared_worktrees_tmux_router(
                &std::path::PathBuf::from("/home/brian/worktrees/label/agent-doc")
            )
        );
        assert!(
            crate::worktree_layout::resolves_shared_worktrees_tmux_router(
                &std::path::PathBuf::from("/home/brian/worktrees/agent-doc-flat")
            )
        );
        assert!(SUBAGENT_WORKTREE_LAYOUT_RULE.contains("verify the selected parent is writable"));
        assert!(
            SUBAGENT_WORKTREE_LAYOUT_RULE.contains("/tmp/agent-doc-worktrees/<label>/agent-doc")
        );
        assert!(SUBAGENT_WORKTREE_LAYOUT_RULE.contains("do not exit merely"));
    }

    /// `#tmuxkittyterminal`: every preamble tells the worker that `$TMUX`
    /// beats `TMUX_TMPDIR`, inside the coordinator rules, before commit/push.
    #[test]
    fn every_status_restates_the_live_tmux_isolation_rule() {
        let resolved = doc(
            PRESETS,
            "subagents preset=\"#spec-test-build-install-commit-push\"",
            &["do [#abc]"],
            &["[ ] [#abc] x"],
        );
        let unresolved = doc("", "subagents preset=\"#nosuch\"", &["do [#abc]"], &[]);
        let item_only = doc("", "subagents", &["look at foo"], &[]);
        let bullet = format!("- {SUBAGENT_TMUX_ISOLATION_RULE}\n");
        for (content, item) in [
            (&resolved, "do [#abc]"),
            (&unresolved, "do [#abc]"),
            (&item_only, "look at foo"),
        ] {
            let p = subagent_authorization("doc.md", content, item).subagent_prompt_preamble;
            let rule_at = p.find(&bullet).unwrap_or_else(|| panic!("{p}"));
            assert!(p.find("Coordinator rules").unwrap() < rule_at, "{p}");
            assert!(
                rule_at < p.find("- Commit and push only when").unwrap(),
                "{p}"
            );
        }
        for phrase in [
            "tmux reads `$TMUX` before `TMUX_TMPDIR`",
            "`TMUX_TMPDIR=<dir>` alone isolates nothing",
            "BOTH `env -u TMUX` and a private socket",
            "`env -u TMUX tmux -L <unique-name> ...`",
            "never `pkill`/`killall` tmux",
        ] {
            assert!(SUBAGENT_TMUX_ISOLATION_RULE.contains(phrase), "{phrase}");
        }
    }

    /// `#buildconcurrencygate`: every preamble names the build-slot gate,
    /// inside the coordinator rules, before commit/push.
    #[test]
    fn every_status_restates_the_build_slot_rule() {
        let resolved = doc(
            PRESETS,
            "subagents preset=\"#spec-test-build-install-commit-push\"",
            &["do [#abc]"],
            &["[ ] [#abc] x"],
        );
        let unresolved = doc("", "subagents preset=\"#nosuch\"", &["do [#abc]"], &[]);
        let item_only = doc("", "subagents", &["look at foo"], &[]);
        let bullet = format!("- {}\n", crate::build_slot::SUBAGENT_BUILD_SLOT_RULE);
        for (content, item) in [
            (&resolved, "do [#abc]"),
            (&unresolved, "do [#abc]"),
            (&item_only, "look at foo"),
        ] {
            let p = subagent_authorization("doc.md", content, item).subagent_prompt_preamble;
            let rule_at = p.find(&bullet).unwrap_or_else(|| panic!("{p}"));
            assert!(p.find("Coordinator rules").unwrap() < rule_at, "{p}");
            assert!(
                rule_at < p.find("- Commit and push only when").unwrap(),
                "{p}"
            );
        }
    }

    /// `#workerresponsestructure`: every preamble asks the worker for the
    /// labelled hand-back shape that `queue report-complete` parses, as the
    /// last coordinator rule, and that shape round-trips through the parser.
    #[test]
    fn every_status_asks_for_the_labelled_handback_shape() {
        let resolved = doc(
            PRESETS,
            "subagents preset=\"#spec-test-build-install-commit-push\"",
            &["do [#abc]"],
            &["[ ] [#abc] x"],
        );
        let unresolved = doc("", "subagents preset=\"#nosuch\"", &["do [#abc]"], &[]);
        let item_only = doc("", "subagents", &["look at foo"], &[]);
        let bullet = format!("- {SUBAGENT_HANDBACK_SHAPE_RULE}\n");
        for (content, item) in [
            (&resolved, "do [#abc]"),
            (&unresolved, "do [#abc]"),
            (&item_only, "look at foo"),
        ] {
            let p = subagent_authorization("doc.md", content, item).subagent_prompt_preamble;
            let rule_at = p.find(&bullet).unwrap_or_else(|| panic!("{p}"));
            assert!(
                p.find("- Commit and push only when").unwrap() < rule_at,
                "{p}"
            );
            assert!(
                rule_at < p.find("subagent operator request").unwrap(),
                "{p}"
            );
            assert!(!p.contains("- Report back: branch, commit SHA"), "{p}");
            assert!(p.contains("Do not hand back while a nested child task is still running"));
            assert!(p.contains("poll or resume that same task until it reaches a terminal state"));
        }
        for label in [
            "**Summary**",
            "**What changed**",
            "**Verification**",
            "**Action items**",
            "**Notes**",
            "**Next actions**",
        ] {
            assert!(SUBAGENT_HANDBACK_SHAPE_RULE.contains(label), "{label}");
        }
        let handback = "**Summary** done\n**What changed** a1b2c3\n**Verification** ok\n**Action items**\n- a gap\n**Notes**\n- a limit\n**Next actions** none";
        assert_eq!(
            agent_doc_document::worker_handback::action_items(handback),
            vec!["a gap".to_string()]
        );
    }

    #[test]
    fn every_status_carries_the_cross_harness_operator_request_protocol() {
        let resolved = doc(
            PRESETS,
            "subagents preset=\"#spec-test-build-install-commit-push\"",
            &["do [#abc]"],
            &["[ ] [#abc] x"],
        );
        let unresolved = doc("", "subagents preset=\"#nosuch\"", &["do [#abc]"], &[]);
        let item_only = doc("", "subagents", &["look at foo"], &[]);
        for (content, item) in [
            (&resolved, "do [#abc]"),
            (&unresolved, "do [#abc]"),
            (&item_only, "look at foo"),
        ] {
            let preamble = subagent_authorization("doc.md", content, item).subagent_prompt_preamble;
            assert!(
                preamble.contains(SUBAGENT_OPERATOR_REQUEST_MARKER),
                "{preamble}"
            );
            assert_eq!(
                preamble.matches(SUBAGENT_OPERATOR_REQUEST_MARKER).count(),
                1,
                "{preamble}"
            );
            assert!(!preamble.contains("[agent-doc] subagent operator request"));
            assert!(!preamble.contains("(#waypostrequest)"));
            assert!(
                preamble.contains(&format!(
                    "\n{SUBAGENT_OPERATOR_REQUEST_MARKER}\n- *queue_item:*\n  - {item}\n- *kind:*"
                )),
                "{preamble}"
            );
            assert!(preamble.contains("\n- *request:*\n  - <exact question or exact tool/action"));
            assert!(preamble.contains(
                "Use a different marker id for every request packet in the session document"
            ));
            assert!(
                preamble.contains("never reuse the legacy fixed `[#subagent-operator-request]` id")
            );
            assert!(preamble.contains("\n- *choices:*\n  - **<label>**: <effect>\n"));
            assert!(!preamble.contains("- *choices:*\n  - <label>: <effect>"));
            for key in ["queue_item", "request", "context"] {
                assert!(
                    !preamble.contains(&format!("- *{key}:* <")),
                    "the {key} value nests beneath its key line, never beside it: {preamble}"
                );
            }
            for harness in ["Claude Code", "Codex", "Grok Build", "OpenCode"] {
                assert!(preamble.contains(harness), "missing {harness}: {preamble}");
            }
        }
    }

    /// `#nestedoutwardauth`: a nested `push` under `#rebase`, including an
    /// effect marker, and a nested `publish` under `#review` are authorization.
    /// A negation and a struck child are not.
    #[test]
    fn nested_push_and_publish_are_operator_authorization() {
        let fm = concat!(
            "prompt_presets:\n",
            "  \"#rebase\": \"rebase + fix merge conflicts\"\n",
            "  \"#review\": \"Review using ~/.agents/skills/pr-review/SKILL.md\"\n",
        );
        let rebase = "#rebase [#subagent-operator-request-pr767-rebase-push]\n  - push ⟦effect:1⟧";
        let content = doc(fm, "subagents", &[rebase], &[]);
        let preamble = subagent_authorization("doc.md", &content, "push").subagent_prompt_preamble;
        assert!(preamble.contains("> push\n"), "{preamble}");
        assert!(
            !preamble.contains("> push ⟦effect:"),
            "the effect marker is not part of the directive: {preamble}"
        );
        assert!(
            preamble.contains(SUBAGENT_NESTED_OUTWARD_AUTHORIZATION),
            "{preamble}"
        );
        assert!(preamble.contains("not a chat reply"), "{preamble}");
        assert!(preamble.contains("`publish`"), "{preamble}");
        assert!(
            preamble.contains("Complete each authorized outward effect before handback")
                && preamble.contains("#effectbeforecloseout"),
            "resolved authorization must keep outward effects ahead of handback: {preamble}"
        );
        assert!(
            !preamble.contains("ask the coordinator for its text"),
            "a request annotation is not missing backlog text: {preamble}"
        );

        let parent = "https://github.com/haiven-dev/haiven-contracts/pull/162";
        let nested = format!("{parent}\n  - #rebase\n    - push ⟦effect:2⟧");
        let content = doc(fm, "subagents", &[&nested], &[]);
        let lineage = format!("{parent}: #rebase: push");
        let preamble =
            subagent_authorization("doc.md", &content, &lineage).subagent_prompt_preamble;
        assert!(
            preamble.contains(&format!("> {lineage}")),
            "the quote stays the lineage item: {preamble}"
        );
        assert!(
            preamble.contains("> push\n"),
            "the executable push leaf is operator authorization: {preamble}"
        );

        let review = "#review\n  - I don't see the review on github. publish";
        let content = doc(fm, "subagents", &[review], &[]);
        let review_leaf = "I don't see the review on github. publish";
        let preamble =
            subagent_authorization("doc.md", &content, review_leaf).subagent_prompt_preamble;
        assert!(
            preamble.contains("> I don't see the review on github. publish"),
            "{preamble}"
        );
        assert!(preamble.contains("plain comment"), "{preamble}");
        assert_eq!(
            subagent_authorization("doc.md", &content, review_leaf).status,
            AuthorizationStatus::Resolved
        );

        let unresolved = "#rebase\n  - push";
        let content = doc("", "subagents preset=\"#rebase\"", &[unresolved], &[]);
        let preamble = subagent_authorization("doc.md", &content, "push").subagent_prompt_preamble;
        assert!(
            preamble.contains(UNRESOLVED_AUTHORIZATION_MARKER),
            "{preamble}"
        );
        assert!(preamble.contains("> push\n"), "{preamble}");
        assert!(
            preamble.contains("including when another declared preset"),
            "{preamble}"
        );

        for inert in [
            "#rebase\n  - do not push",
            "#rebase\n  - ~~publish~~",
            "#review\n  - read the diff",
        ] {
            let content = doc(fm, "subagents", &[inert], &[]);
            let leaf = inert
                .lines()
                .nth(1)
                .unwrap()
                .trim()
                .trim_start_matches("- ");
            let preamble =
                subagent_authorization("doc.md", &content, leaf).subagent_prompt_preamble;
            assert!(
                !preamble.contains("effective directives"),
                "{inert}\n{preamble}"
            );
        }
    }

    #[test]
    fn operator_request_marker_requires_a_request_specific_id() {
        assert_eq!(
            subagent_operator_request_id(
                "[#subagent-operator-request-pr645] subagent operator request"
            ),
            Some("subagent-operator-request-pr645")
        );
        assert_eq!(
            subagent_operator_request_id(LEGACY_SUBAGENT_OPERATOR_REQUEST_MARKER),
            Some("subagent-operator-request")
        );
        assert_eq!(
            subagent_operator_request_id(SUBAGENT_OPERATOR_REQUEST_MARKER),
            None,
            "the protocol placeholder is not a concrete packet id"
        );
        assert_eq!(
            subagent_operator_request_id("[#subagent-operator-request-] subagent operator request"),
            None
        );
    }
}
