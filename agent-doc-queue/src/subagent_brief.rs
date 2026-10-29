//! Operator authorization carried to a dispatched queue subagent
//! (`#waypostauthorization`).
//!
//! A background subagent dispatched for a `queue_subagent_dispatch` item sees
//! only the prompt its coordinator writes. Without help it gets none of the
//! operator's authorization: the queue's `preset=` expansion, the item's own
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
}

/// Where a preset applied to the item came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetSource {
    /// `<!-- agent:queue preset="#name" -->` on the selected queue.
    QueueAttr,
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
    /// Resolved harness model override for this dispatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Resolved harness reasoning-effort override for this dispatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Paste this verbatim at the top of the subagent's prompt.
    pub subagent_prompt_preamble: String,
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

/// Preset names the selected queue declares: every whitespace-separated
/// reference in its `preset=` attr, then any body-level `preset <name>` lines.
fn queue_preset_names(
    content: &str,
    components: &[element::Component],
) -> Vec<(String, PresetSource)> {
    let mut names = Vec::new();
    let Ok(Some(queue)) = crate::queue_set::selected_component(content, components) else {
        return names;
    };
    let attrs = crate::prompt_component_attrs::prompt_component_attrs_for(queue, "queue");
    for preset in attrs.preset_names() {
        names.push((preset.to_string(), PresetSource::QueueAttr));
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
    let submitted = crate::queue_response::submitted_queue_prompt(content, item);
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

    let status = if presets.iter().any(|p| !p.resolved) {
        AuthorizationStatus::UnresolvedPreset
    } else if presets.is_empty() {
        AuthorizationStatus::ItemTextOnly
    } else {
        AuthorizationStatus::Resolved
    };
    let queue_id = crate::queue_graph::QueueSet::parse(content, components)
        .ok()
        .and_then(|queues| queues.selected().map(|queue| queue.id.clone()));
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
        model,
        effort,
        subagent_prompt_preamble: String::new(),
    };
    let computed;
    let submitted = match submitted_prompt {
        Some(prompt) => prompt,
        None => {
            computed = crate::queue_response::submitted_queue_prompt(content, item);
            computed.as_str()
        }
    };
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
    "- *choices:*\n",
    "  - **<label>**: <effect>\n\n",
    "Replace `<unique-id>` with a short request-specific lowercase suffix using letters, digits, ",
    "and hyphens. Use a different marker id for every request packet in the session document; ",
    "never reuse the legacy fixed `[#subagent-operator-request]` id. ",
    "Put the `queue_item`, `request`, and `context` text in a nested list beneath its key line, ",
    "never on the key line itself; split the context into short bullets, one fact per bullet. ",
    "Keep the rendered `queue_item` value unchanged; it links this packet to the exact queue row. ",
    "Include every offered choice; write `- *choices:* []` only when the harness supplied none. ",
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
pub const SUBAGENT_RESOLVED_AUTHORIZATION_RULE: &str = concat!(
    "The queue item and resolved preset bodies above are current operator authorization for the ",
    "work and actions they name. If they say `go`, `push`, `publish`, `commit + push`, or carry an ",
    "authorizing preset such as `#auth`, do the work without asking the operator to repeat those ",
    "words, reconfirm, or invoke the preset again. This task authorization does not itself grant ",
    "a harness-native tool permission.\n"
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
    for preset in auth.presets.iter().filter(|p| p.resolved) {
        let origin = match preset.source {
            PresetSource::QueueAttr => "queue preset",
            PresetSource::QueueLine => "queue preset line",
            PresetSource::Item => "item preset",
        };
        out.push_str(&format!(
            "\nOperator authorization, {origin} `{}` (effective value):\n{}\n",
            preset.name,
            quote(preset.body.as_deref().unwrap_or_default())
        ));
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
    if auth.status != AuthorizationStatus::UnresolvedPreset {
        out.push('\n');
        out.push_str(SUBAGENT_RESOLVED_AUTHORIZATION_RULE);
    }
    match auth.status {
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
                 missing.\n",
                missing.join(", ")
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
         coordinator's checkout or a checkout another subagent uses.\n\
         - Write command output, logs, and scratch files inside your own worktree, never to a fixed \
         name (such as `check.log` or `msg.txt`) in a shared scratchpad or temp directory: sibling \
         subagents share it, and two runs truncating one file interleave each other's output, so \
         you would read another checkout's results as your own.\n\
         - Never run `make install`, a release build, a version bump, a tag, or a release. Where the \
         authorization says build + install, build and test in your worktree; the coordinator runs \
         the single `make install` after integrating.\n\
         - Never edit, write, or commit the agent-doc session document or its queue, and never run \
         `agent-doc` write/respond/queue commands against it; the coordinator owns the claim and \
         closes the item.\n",
    );
    out.push_str(concat!("- ", subagent_scratch_isolation_rule!(), "\n"));
    out.push_str(concat!("- ", subagent_tmux_isolation_rule!(), "\n"));
    out.push_str(
        "- Commit and push only when the authorization above includes it. Use your own branch and \
         fast-forward by default. An explicitly named target branch or explicit force-push directive \
         in the verbatim authorization overrides only that default: execute the exact authorized \
         scope, never broaden it, and route any harness-native permission through the request protocol.\n\
         - Report back: branch, commit SHA, files changed, and exact test results.\n",
    );
    out.push('\n');
    let queue_item = agent_doc_document::queue_projection::operator_request_origin_item(&auth.item);
    out.push_str(&SUBAGENT_OPERATOR_REQUEST_PROTOCOL.replace("<origin-queue-item>", &queue_item));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
            p.contains("> update spec + tests. build + install for local testing. commit + push")
        );
        assert!(p.contains("> do [#abc]"));
        assert!(p.contains("> Add a way to post authorization"));
        assert!(p.contains("Never run `make install`"));
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
            preamble.contains(
                "Submitted prompt (`#nestsubmit`), including parent lineage and the prompt tree:\n> https://github.com/haiven-dev/haiven-api/pull/736: #rebase\n"
            ),
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
            p.contains("Operator authorization, item preset `#auth`"),
            "{p}"
        );
        assert!(p.contains("> I authorize you to do this."), "{p}");
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
        let push = preamble.find("queue preset `#push`").unwrap();
        let auth_preset = preamble.find("queue preset `#auth`").unwrap();
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
        assert!(
            auth.subagent_prompt_preamble
                .contains("> review only, no commits")
        );
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
