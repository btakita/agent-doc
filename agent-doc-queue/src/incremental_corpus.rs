//! Test corpus for `#incrementalreconcile`: the session fixtures, a
//! session-document generator in the shape of a large live session (a dozen
//! run queues, two of them large, tracked-work components, then thousands of
//! `agent:exchange` lines), and random document edits.

/// Every session fixture, by name.
pub(crate) const FIXTURES: &[(&str, &str)] = &[
    (
        "child_row_followups_session",
        include_str!("../tests/fixtures/child_row_followups_session.md"),
    ),
    (
        "child_row_struck_answer_session",
        include_str!("../tests/fixtures/child_row_struck_answer_session.md"),
    ),
    (
        "claim_label_residue_session",
        include_str!("../tests/fixtures/claim_label_residue_session.md"),
    ),
    (
        "closed_by_nest_session",
        include_str!("../tests/fixtures/closed_by_nest_session.md"),
    ),
    (
        "followup_moved_state_session",
        include_str!("../tests/fixtures/followup_moved_state_session.md"),
    ),
    (
        "human_followup_rows",
        include_str!("../tests/fixtures/human_followup_rows.md"),
    ),
    (
        "loose_ends_session",
        include_str!("../tests/fixtures/loose_ends_session.md"),
    ),
    (
        "open_count_session",
        include_str!("../tests/fixtures/open_count_session.md"),
    ),
    (
        "pr412_adversarial_handback",
        include_str!("../tests/fixtures/pr412_adversarial_handback.md"),
    ),
    (
        "response_remaining_agent_entry_answered",
        include_str!("../tests/fixtures/response_remaining_agent_entry_answered.md"),
    ),
    (
        "response_remaining_answered_entries",
        include_str!("../tests/fixtures/response_remaining_answered_entries.md"),
    ),
    (
        "response_remaining_detached_tally",
        include_str!("../tests/fixtures/response_remaining_detached_tally.md"),
    ),
    (
        "response_remaining_session",
        include_str!("../tests/fixtures/response_remaining_session.md"),
    ),
    (
        "response_tree_slot_session",
        include_str!("../tests/fixtures/response_tree_slot_session.md"),
    ),
    (
        "subtree_guidance_session",
        include_str!("../tests/fixtures/subtree_guidance_session.md"),
    ),
    (
        "waiting_tree_operator_replies",
        include_str!("../tests/fixtures/waiting_tree_operator_replies.md"),
    ),
    (
        "waiting_tree_session",
        include_str!("../tests/fixtures/waiting_tree_session.md"),
    ),
    (
        "waiting_tree_unmatched_replies",
        include_str!("../tests/fixtures/waiting_tree_unmatched_replies.md"),
    ),
];

/// The fixture named `name`.
pub(crate) fn fixture(name: &str) -> &'static str {
    FIXTURES
        .iter()
        .find(|(fixture, _)| *fixture == name)
        .map(|(_, text)| *text)
        .unwrap_or_else(|| panic!("no fixture {name}"))
}

/// One `#[test]` per session fixture, each calling `$check(name)`, so the
/// corpus runs in parallel under nextest.
macro_rules! per_fixture_tests {
    ($check:ident) => {
        $crate::incremental_corpus::per_fixture_tests!(@each $check,
            child_row_followups_session => "child_row_followups_session",
            child_row_struck_answer_session => "child_row_struck_answer_session",
            claim_label_residue_session => "claim_label_residue_session",
            closed_by_nest_session => "closed_by_nest_session",
            followup_moved_state_session => "followup_moved_state_session",
            human_followup_rows => "human_followup_rows",
            loose_ends_session => "loose_ends_session",
            open_count_session => "open_count_session",
            pr412_adversarial_handback => "pr412_adversarial_handback",
            response_remaining_agent_entry_answered => "response_remaining_agent_entry_answered",
            response_remaining_answered_entries => "response_remaining_answered_entries",
            response_remaining_detached_tally => "response_remaining_detached_tally",
            response_remaining_session => "response_remaining_session",
            response_tree_slot_session => "response_tree_slot_session",
            subtree_guidance_session => "subtree_guidance_session",
            waiting_tree_operator_replies => "waiting_tree_operator_replies",
            waiting_tree_session => "waiting_tree_session",
            waiting_tree_unmatched_replies => "waiting_tree_unmatched_replies",
        );
    };
    (@each $check:ident, $($test:ident => $name:literal,)*) => {
        $(
            #[test]
            fn $test() {
                $check($name);
            }
        )*
    };
}
pub(crate) use per_fixture_tests;

/// `(name, attrs, body)` of every top-level component of `document`.
fn components_of(document: &str) -> Vec<(String, String, String)> {
    let Ok(components) = agent_doc_element::element::parse(document) else {
        return Vec::new();
    };
    components
        .iter()
        .map(|component| {
            let opener = &document[component.open_start..component.open_end];
            let attrs = opener
                .trim()
                .trim_start_matches("<!--")
                .trim_end_matches("-->")
                .trim()
                .split_once(char::is_whitespace)
                .map(|(_, attrs)| attrs.trim().to_string())
                .unwrap_or_default();
            (
                component.name.clone(),
                attrs,
                component.content(document).to_string(),
            )
        })
        .collect()
}

/// A session document of about `target_lines` lines, shaped like a large live
/// session: status, loose ends and active, `copies` rounds of every fixture's
/// run queues (each queue under its own id), the tracked-work components,
/// then `agent:exchange` filled from the fixtures' exchange cells.
pub(crate) fn session_shaped(copies: usize, target_lines: usize) -> String {
    let mut queues = Vec::new();
    let mut tracked: Vec<(String, String)> = Vec::new();
    let mut exchange = Vec::new();
    for (name, fixture) in FIXTURES {
        for (index, (component, _, body)) in components_of(fixture).into_iter().enumerate() {
            match component.as_str() {
                "queue" => queues.push((format!("{name}-{index}"), body)),
                "todo" | "backlog" => tracked.push((component, body)),
                "exchange" if !body.trim().is_empty() => exchange.push(body),
                _ => {}
            }
        }
    }
    let mut out = String::from(
        "---\nagent_doc_session: incremental-shape\nagent_doc_format: template\n---\n\n\
         ## Status <!-- agent:status patch=replace -->\n<!-- /agent:status -->\n\n\
         ## Active <!-- agent:active priority -->\n<!-- /agent:active -->\n\n",
    );
    for copy in 0..copies {
        for (id, body) in &queues {
            out.push_str(&format!(
                "## {id} {copy} <!-- agent:run id=\"{id}-{copy}\" subagents go -->\n{body}<!-- /agent:run -->\n\n"
            ));
        }
    }
    for (name, body) in &tracked {
        out.push_str(&format!(
            "## {name} <!-- agent:{name} -->\n{body}<!-- /agent:{name} -->\n\n"
        ));
    }
    out.push_str("## Exchange <!-- agent:exchange patch=append -->\n");
    let mut lines = out.lines().count();
    let mut round = 0;
    while lines < target_lines && !exchange.is_empty() {
        let cell = &exchange[round % exchange.len()];
        out.push_str(cell);
        if !cell.ends_with('\n') {
            out.push('\n');
        }
        lines += cell.lines().count();
        round += 1;
    }
    out.push_str("<!-- /agent:exchange -->\n");
    out
}

/// A small deterministic generator (xorshift64*), so a failing sequence
/// replays from its seed.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub(crate) fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub(crate) fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Lines an edit may insert: queue rows, struck rows, response-tree rows,
/// follow-up rows, claim rows, and syntax that changes how a component
/// parses (fences, markers, comments, raw HTML).
const INSERTS: &[&str] = &[
    "- new operator row",
    "- ~~struck operator row~~",
    "  - child reply",
    "  - ~~Done with that~~",
    "- 🚧 [#subagent:probe#1] running probe work",
    "- 🙋 follow up on an action item from queue item “probe”: operator: decide the probe. [operator-verify] [#q-0123456789ab]",
    "- ### ~~do [#probe]~~",
    "  - **Response [#r-0123456789ab]**",
    "    - **State:** Complete.",
    "    - **Guidance:** 🗑️ Safe to delete this tree.",
    "    - **Summary:** Probe done.",
    "```",
    "~~~",
    "",
    "    indented text",
    "%% operator comment %%",
    "> quoted line",
    "<!-- agent:run id=\"probe\" -->",
    "<!-- /agent:run -->",
    "<pre>",
    "### Re: probe response",
];

/// One random edit of `document`: insert, delete, duplicate or strike a
/// line, or append an exchange line.
pub(crate) fn random_edit(document: &str, rng: &mut Rng) -> String {
    let lines: Vec<&str> = document.split_inclusive('\n').collect();
    if lines.is_empty() {
        return format!("{}\n", INSERTS[rng.below(INSERTS.len())]);
    }
    let at = rng.below(lines.len());
    let mut out: Vec<String> = lines.iter().map(|line| line.to_string()).collect();
    match rng.below(6) {
        0 | 1 => out.insert(at, format!("{}\n", INSERTS[rng.below(INSERTS.len())])),
        2 => {
            out.remove(at);
        }
        3 => {
            let copy = out[at].clone();
            out.insert(at, copy);
        }
        4 => {
            let line = out[at].trim_end_matches('\n').to_string();
            let indent = line.len() - line.trim_start().len();
            if let Some(body) = line.trim_start().strip_prefix("- ") {
                out[at] = format!("{}- ~~{body}~~\n", " ".repeat(indent));
            }
        }
        _ => {
            let close = document.rfind("<!-- /agent:exchange -->");
            if let Some(close) = close {
                let mut appended = document.to_string();
                appended.insert_str(close, "### Re: appended cell\n> **Queue prompt:** probe\n");
                return appended;
            }
        }
    }
    out.concat()
}
