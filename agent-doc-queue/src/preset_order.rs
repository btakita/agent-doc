//! Temporal preset order (`#presetorder`).
//!
//! A queue item that names presets with `then` between them runs each preset
//! in that order. `#address then #adversarial-pr` runs address, then the
//! adversarial review. Settling the earlier preset, including
//! `NO-WRITTEN-FIX`, does not finish the row.
//!
//! A `#pipeprompt` statement orders its preset stages the same way:
//! `- #address |` over `- #adversarial-pr` is `#address then
//! #adversarial-pr`. Its stages come from
//! [`agent_doc_element_queue::pipe::statement_stage_pipelines`], so ordered
//! children, nested ` |` rows, a ` |+` deep pipe, and fan-out branches all
//! read in run order.

use agent_doc_document::queue_projection::strip_priority_markers;

/// Preset names `text` runs in temporal order: a `#pipeprompt` statement's
/// preset stages ([`pipe_preset_order`]), else presets joined with `then`.
///
/// A chain is known presets whose consecutive pairs have `then` between them.
/// Other words are gaps and do not break a `then`. A preset with no preceding
/// `then` starts a new chain. The longest chain of length at least two is
/// returned (the first chain on a tie). Inline code is masked, and a preset
/// already in the chain is kept once.
pub fn temporal_preset_order(text: &str, presets: &[(String, String)]) -> Vec<String> {
    let piped = pipe_preset_order(text, presets);
    if !piped.is_empty() {
        return piped;
    }
    then_preset_order(text, presets)
}

/// The preset stages of a ` |` / ` |+` statement (`#pipeprompt`), in run
/// order, when at least two stages name a preset.
///
/// Each stage contributes its own `then` chain, or else its first preset in
/// directive position (`#gh-fix <url>`, `<url>: #address`). A prose stage is
/// a gap. Fan-out branches all follow their parent's stage, so every branch's
/// preset is a later preset. An inline pipe row (`#address | #adversarial-pr`,
/// every segment led by a preset) is the same statement on one line, also
/// after a lineage prefix (`<url>: #address | #adversarial-pr`). A ` >`
/// fold, context children, and a prose ` | ` are not stages.
pub fn pipe_preset_order(text: &str, presets: &[(String, String)]) -> Vec<String> {
    let Some(text) = pipe_statement_scope(text) else {
        return Vec::new();
    };
    let mut order: Vec<String> = Vec::new();
    for stage in agent_doc_element_queue::pipe::statement_stage_pipelines(text)
        .iter()
        .flatten()
    {
        for name in stage_presets(stage, presets) {
            if !order.iter().any(|seen| seen.eq_ignore_ascii_case(&name)) {
                order.push(name);
            }
        }
    }
    if order.len() >= 2 { order } else { Vec::new() }
}

/// The ` |` statement `text` carries: `text` itself, or for a claimed or
/// submitted leaf whose nest vector leads with its lineage
/// (`<url>: #address | #adversarial-pr`), the inline pipe row after the last
/// lineage join. `None` when there is no statement.
fn pipe_statement_scope(text: &str) -> Option<&str> {
    let text = text.trim();
    if crate::document_queue::is_pipe_statement_text(text) {
        return Some(text);
    }
    let first = text.lines().next()?;
    first
        .match_indices(": ")
        .map(|(at, join)| text[at + join.len()..].trim_start())
        .find(|tail| {
            tail.lines()
                .next()
                .is_some_and(agent_doc_element_queue::pipe::is_inline_pipe)
        })
}

fn stage_presets(stage: &str, presets: &[(String, String)]) -> Vec<String> {
    let chain = then_preset_order(stage, presets);
    if !chain.is_empty() {
        return chain;
    }
    let text = strip_priority_markers(stage);
    let text = crate::queue_continuation::mask_directive_scan(&text);
    directive_tokens(&text)
        .find_map(|token| {
            presets
                .iter()
                .find(|(name, _)| token.eq_ignore_ascii_case(name))
                .map(|(name, _)| name.clone())
        })
        .into_iter()
        .collect()
}

fn directive_tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '#' | '-' | '_')))
        .filter(|token| !token.is_empty())
}

fn then_preset_order(text: &str, presets: &[(String, String)]) -> Vec<String> {
    let text = strip_priority_markers(text);
    let text = crate::queue_continuation::mask_directive_scan(&text);
    let mut chains: Vec<Vec<String>> = Vec::new();
    let mut expect_continue = false;
    for token in directive_tokens(&text) {
        if token.eq_ignore_ascii_case("then") {
            if chains.last().is_some_and(|chain| !chain.is_empty()) {
                expect_continue = true;
            }
            continue;
        }
        let Some(name) = presets
            .iter()
            .find(|(name, _)| token.eq_ignore_ascii_case(name))
            .map(|(name, _)| name.clone())
        else {
            continue;
        };
        if expect_continue && let Some(chain) = chains.last_mut() {
            expect_continue = false;
            if !chain.iter().any(|seen| seen.eq_ignore_ascii_case(&name)) {
                chain.push(name);
            }
            continue;
        }
        expect_continue = false;
        chains.push(vec![name]);
    }
    chains
        .into_iter()
        .filter(|chain| chain.len() >= 2)
        .reduce(|best, chain| {
            if chain.len() > best.len() {
                chain
            } else {
                best
            }
        })
        .unwrap_or_default()
}

/// True when `item` has `then` and a later `#` token, or is a `#pipeprompt`
/// statement with a `#` directive in a stage after its first, so a brief can
/// tell the next worker that a settled earlier preset does not finish the row.
pub fn item_orders_a_later_preset(item: &str) -> bool {
    if let Some(statement) = pipe_statement_scope(item)
        && agent_doc_element_queue::pipe::statement_stage_pipelines(statement)
            .iter()
            .any(|stages| {
                stages.iter().skip(1).any(|stage| {
                    let text = strip_priority_markers(stage);
                    crate::queue_continuation::mask_directive_scan(&text).contains('#')
                })
            })
    {
        return true;
    }
    let text = strip_priority_markers(item);
    let text = crate::queue_continuation::mask_directive_scan(&text);
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let lowered = collapsed.to_ascii_lowercase();
    let Some(at) = lowered.find(" then ") else {
        return false;
    };
    lowered[at + " then ".len()..].contains('#')
}

/// Binding closeout for a temporal preset order (`#presetorder`).
pub fn ordered_preset_rule(order: &[String], presets: &[(String, String)]) -> String {
    if order.len() < 2 {
        return String::new();
    }
    let mut out = String::from(
        "Ordered presets (#presetorder): this item names presets in temporal order. \
         Run each preset to its own completion before the next one. Do not stop after an earlier preset. \
         A settled earlier preset is not permission to skip a later preset. \
         `NO-WRITTEN-FIX head=<sha>` settles only `#address` and does not finish the row. \
         After that receipt, run the next preset in this same row. \
         Do not start another address pass when the address half already settled. \
         A receipt from the previous hand-back of this same row still counts when \
         this hand-back settles the other half and both name the same head \
         (`#pipestagereceipt`). Order:",
    );
    for (index, name) in order.iter().enumerate() {
        out.push_str(&format!(" {}. {name}", index + 1));
    }
    out.push('.');
    let later_has_states = order.iter().skip(1).any(|name| {
        preset_body(name, presets)
            .is_some_and(|body| !crate::goal_preset_terminal::preset_final_states(body).is_empty())
    });
    let later_without_contract = order.iter().skip(1).any(|name| {
        !name.eq_ignore_ascii_case("#address")
            && preset_body(name, presets).is_none_or(|body| {
                crate::goal_preset_terminal::preset_final_states(body).is_empty()
            })
    });
    if later_has_states {
        out.push_str(
            " When a later preset declares final states, finish only with one of those states.",
        );
    }
    if later_without_contract {
        out.push_str(
            " When a later preset declares no final states, finish with one line `PRESET-DONE <preset>` after that preset has run. Do not emit `PRESET-DONE` for a preset that did not run.",
        );
    }
    out
}

/// True when any preset after the first in `order` is not settled by `response`.
pub fn later_ordered_preset_unmet(
    order: &[String],
    response: &str,
    presets: &[(String, String)],
) -> bool {
    first_unmet_later_preset(order, response, presets).is_some()
}

/// The first preset after the first in `order` that `response` does not settle.
pub fn first_unmet_later_preset(
    order: &[String],
    response: &str,
    presets: &[(String, String)],
) -> Option<String> {
    if order.len() < 2 {
        return None;
    }
    order
        .iter()
        .skip(1)
        .find(|name| !preset_phase_settled(name, response, presets))
        .cloned()
}

/// True when `response` settles preset `name`.
///
/// `#address` settles only with `FIX-ON-HEAD` or `NO-WRITTEN-FIX`. A preset
/// that declares final states settles only when the hand-back completes that
/// goal. Any other preset settles with one line `PRESET-DONE <preset>` after
/// it has run.
pub fn preset_phase_settled(name: &str, response: &str, presets: &[(String, String)]) -> bool {
    if name.eq_ignore_ascii_case("#address") {
        return crate::address_fix_on_head::address_run_lands_written_fix(response);
    }
    let Some(body) = preset_body(name, presets) else {
        return false;
    };
    let states = crate::goal_preset_terminal::preset_final_states(body);
    if !states.is_empty() {
        let bodies = if states.iter().any(|state| state == "BLOCKED") {
            vec![body.to_string()]
        } else {
            Vec::new()
        };
        return crate::goal_preset_terminal::hand_back_completes_goal_loop(
            response, &states, &bodies,
        );
    }
    response.lines().any(|line| line_preset_done(line, name))
}

fn preset_body<'a>(name: &str, presets: &'a [(String, String)]) -> Option<&'a str> {
    presets
        .iter()
        .find(|(preset, _)| preset.eq_ignore_ascii_case(name))
        .map(|(_, body)| body.as_str())
}

/// The previous hand-back of an ordered row (`#pipestagereceipt`).
///
/// Exclusive briefs ask one attempt for `FIX-ON-HEAD` and the next for
/// `REVIEW-CLEAN`. Each half stays settled when the other arrives later.
pub struct PriorOrderedHandoff<'a> {
    pub response: &'a str,
    pub address_fix_pending: bool,
    pub address_effect_unproven: bool,
    pub address_effect_unknown: bool,
}

impl PriorOrderedHandoff<'_> {
    fn carries_address(&self) -> bool {
        !self.address_fix_pending
            && !self.address_effect_unproven
            && !self.address_effect_unknown
            && crate::address_fix_on_head::address_run_lands_written_fix(self.response)
    }
}

/// True when `name` is settled by `response`, or by `prior` when this
/// hand-back does not unsay that settlement and both name the same `head=`.
pub fn preset_phase_settled_with_prior(
    name: &str,
    response: &str,
    prior: Option<&PriorOrderedHandoff<'_>>,
    presets: &[(String, String)],
) -> bool {
    if preset_phase_settled(name, response, presets) {
        return true;
    }
    let Some(prior) = prior else {
        return false;
    };
    if !ordered_heads_agree(response, prior.response) {
        return false;
    }
    if name.eq_ignore_ascii_case("#address") {
        return prior.carries_address() && !address_receipt_unsays(response);
    }
    if !preset_phase_settled(name, prior.response, presets) {
        return false;
    }
    !goal_phase_unsays(name, response, presets)
}

/// The first preset after the first in `order` that neither `response` nor
/// `prior` settles.
pub fn first_unmet_later_preset_with_prior(
    order: &[String],
    response: &str,
    prior: Option<&PriorOrderedHandoff<'_>>,
    presets: &[(String, String)],
) -> Option<String> {
    if order.len() < 2 {
        return None;
    }
    order
        .iter()
        .skip(1)
        .find(|name| !preset_phase_settled_with_prior(name, response, prior, presets))
        .cloned()
}

/// True when `prior_response` already completed the goal and `response` landed
/// the address half on that same head.
///
/// A `REVIEW-CLEAN` that this hand-back opens but does not complete (a withheld
/// independent review) does not keep the prior completion.
pub fn prior_goal_completion_carries(
    response: &str,
    prior_response: &str,
    states: &[String],
    bodies: &[String],
    current_settles_address: bool,
) -> bool {
    if !current_settles_address || states.is_empty() {
        return false;
    }
    if !ordered_heads_agree(response, prior_response) {
        return false;
    }
    if crate::goal_preset_terminal::hand_back_begins_with_final_state(response, states)
        && !crate::goal_preset_terminal::hand_back_completes_goal_loop(response, states, bodies)
    {
        return false;
    }
    crate::goal_preset_terminal::hand_back_completes_goal_loop(prior_response, states, bodies)
}

/// True when both hand-backs name at least one `head=<sha>` in common.
pub fn ordered_heads_agree(current: &str, prior: &str) -> bool {
    let current = named_head_shas(current);
    let prior = named_head_shas(prior);
    if current.is_empty() || prior.is_empty() {
        return false;
    }
    current
        .iter()
        .any(|sha| prior.iter().any(|other| other.eq_ignore_ascii_case(sha)))
}

fn named_head_shas(response: &str) -> Vec<String> {
    let mut shas: Vec<String> = Vec::new();
    let mut rest = response;
    while let Some(at) = rest.find("head=") {
        rest = &rest[at + "head=".len()..];
        let sha: String = rest
            .chars()
            .take_while(|ch| ch.is_ascii_hexdigit())
            .collect();
        if (7..=40).contains(&sha.len()) && !shas.iter().any(|seen| seen.eq_ignore_ascii_case(&sha))
        {
            shas.push(sha);
        }
    }
    shas
}

fn address_receipt_unsays(response: &str) -> bool {
    if crate::address_fix_on_head::address_response_admits_fix_absent(response) {
        return true;
    }
    let fixes = !crate::address_fix_on_head::address_fix_receipts(response).is_empty();
    let none = !crate::address_fix_on_head::no_written_fix_receipts(response).is_empty();
    fixes && none
}

fn goal_phase_unsays(name: &str, response: &str, presets: &[(String, String)]) -> bool {
    let Some(body) = preset_body(name, presets) else {
        return false;
    };
    let states = crate::goal_preset_terminal::preset_final_states(body);
    if states.is_empty() {
        return false;
    }
    let bodies = if states.iter().any(|state| state == "BLOCKED") {
        vec![body.to_string()]
    } else {
        Vec::new()
    };
    crate::goal_preset_terminal::hand_back_begins_with_final_state(response, &states)
        && !crate::goal_preset_terminal::hand_back_completes_goal_loop(response, &states, &bodies)
}

fn line_preset_done(line: &str, name: &str) -> bool {
    let mut rest = line.trim();
    loop {
        let before = rest;
        rest = rest.trim_start_matches('>').trim_start();
        for opener in ["- ", "* ", "+ "] {
            if let Some(stripped) = rest.strip_prefix(opener) {
                rest = stripped.trim_start();
            }
        }
        rest = rest.trim_start_matches(['*', '_', '`']);
        if rest == before {
            break;
        }
    }
    let Some(rest) = rest.strip_prefix("PRESET-DONE") else {
        return false;
    };
    if !rest.starts_with(|ch: char| ch.is_whitespace()) {
        return false;
    }
    let Some(token) = rest.split_whitespace().next() else {
        return false;
    };
    let token = token.trim_end_matches([',', '.', ';', ')', '`']);
    token.eq_ignore_ascii_case(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presets() -> Vec<(String, String)> {
        vec![
            ("#address".to_string(), "Address the review.".to_string()),
            (
                "#adversarial-pr".to_string(),
                "Post one independent COMMENTED review.".to_string(),
            ),
            ("#rebase".to_string(), "Rebase onto the base branch.".to_string()),
            ("#ship".to_string(), "Publish the branch.".to_string()),
            ("#note".to_string(), "Write the note.".to_string()),
            ("#gh-issue".to_string(), "File a GitHub issue.".to_string()),
            ("#upgrade".to_string(), "Upgrade agent-doc.".to_string()),
            (
                "#goal".to_string(),
                "/goal Finish the review. Your final message must begin with exactly one of these lines: \
                 - `REVIEW-CLEAN <url> head=<sha>` \
                 - `BLOCKED <action> head=<sha>`."
                    .to_string(),
            ),
        ]
    }

    #[test]
    fn temporal_preset_order_follows_then_and_ignores_prose() {
        let presets = presets();
        assert_eq!(
            temporal_preset_order("#address then #adversarial-pr", &presets),
            vec!["#address".to_string(), "#adversarial-pr".to_string()]
        );
        assert_eq!(
            temporal_preset_order("#rebase then #address then #adversarial-pr", &presets),
            vec![
                "#rebase".to_string(),
                "#address".to_string(),
                "#adversarial-pr".to_string()
            ]
        );
        assert_eq!(
            temporal_preset_order("#Address AND THEN #Adversarial-pr", &presets),
            vec!["#address".to_string(), "#adversarial-pr".to_string()]
        );
        assert_eq!(
            temporal_preset_order("#address, then #adversarial-pr", &presets),
            vec!["#address".to_string(), "#adversarial-pr".to_string()]
        );
        assert_eq!(
            temporal_preset_order("#address then please run #adversarial-pr", &presets),
            vec!["#address".to_string(), "#adversarial-pr".to_string()]
        );
        assert!(temporal_preset_order("#address #adversarial-pr", &presets).is_empty());
        assert!(temporal_preset_order("#address then update the title", &presets).is_empty());
        assert_eq!(
            temporal_preset_order("#gh-issue then #upgrade, then #gh-issue again", &presets),
            vec!["#gh-issue".to_string(), "#upgrade".to_string()]
        );
        assert_eq!(
            temporal_preset_order(
                "#ship then #note. #rebase then #address then #adversarial-pr",
                &presets,
            ),
            vec![
                "#rebase".to_string(),
                "#address".to_string(),
                "#adversarial-pr".to_string()
            ]
        );
        assert_eq!(
            temporal_preset_order("#ship then #note. #rebase then #address", &presets),
            vec!["#ship".to_string(), "#note".to_string()]
        );
        assert_eq!(
            temporal_preset_order("#address then `#adversarial-pr`", &presets),
            Vec::<String>::new()
        );
        assert!(item_orders_a_later_preset("#address then #adversarial-pr"));
        assert!(!item_orders_a_later_preset("#address"));
        assert!(!item_orders_a_later_preset(
            "`#address then #adversarial-pr`"
        ));
    }

    /// `#pipeprompt` × `#presetorder`: a pipe statement's preset stages are a
    /// temporal preset order. `- #address |` over `- #adversarial-pr` runs
    /// address, then the adversarial review, exactly like `then`.
    #[test]
    fn pipe_statement_preset_stages_are_a_temporal_order() {
        let presets = presets();
        let address_then_review = vec!["#address".to_string(), "#adversarial-pr".to_string()];
        for statement in [
            "#address |\n  - #adversarial-pr",
            "https://github.com/sample-org/sample-repo/pull/12: #address |\n  - #adversarial-pr",
            "🚧 [#subagent:pipe#2] #address | [#q-0123456789ab]\n  - #adversarial-pr",
            "📥 #address |\n  - ▶️ #adversarial-pr",
            "#address |+\n  - #adversarial-pr",
            "#address |\n  1. #adversarial-pr",
            "#address |\n  - %% a list comment %%\n  - #adversarial-pr",
        ] {
            assert_eq!(
                temporal_preset_order(statement, &presets),
                address_then_review,
                "{statement}"
            );
            assert!(item_orders_a_later_preset(statement), "{statement}");
            let order = temporal_preset_order(statement, &presets);
            let head = "fa3a3f449986d565f21aeb6e0f792323bc8d1d58";
            assert_eq!(
                first_unmet_later_preset(
                    &order,
                    &format!("NO-WRITTEN-FIX head={head}\n"),
                    &presets
                )
                .as_deref(),
                Some("#adversarial-pr"),
                "{statement}"
            );
        }
        // Chains: an ordered list, nested pipes, and a deep pipe keep reading order.
        let three = vec![
            "#rebase".to_string(),
            "#address".to_string(),
            "#adversarial-pr".to_string(),
        ];
        for statement in [
            "#rebase |\n  1. #address\n  2. #adversarial-pr",
            "#rebase |\n  - #address |\n    - #adversarial-pr",
            "#rebase |+\n  - #address\n    - #adversarial-pr",
            "#rebase |\n  - #address then #adversarial-pr",
            "#rebase |\n  - summarize the diff |\n    - #address |\n      - #adversarial-pr",
        ] {
            assert_eq!(
                temporal_preset_order(statement, &presets),
                three,
                "{statement}"
            );
        }
        // Fan-out: every branch runs after the parent's stage.
        assert_eq!(
            temporal_preset_order("#address |\n  - #adversarial-pr\n  - #note", &presets),
            vec![
                "#address".to_string(),
                "#adversarial-pr".to_string(),
                "#note".to_string()
            ]
        );
        // A preset with a gh-fix style argument is still that stage's preset.
        assert_eq!(
            temporal_preset_order(
                "#gh-issue https://github.com/sample-org/sample-repo/issues/7 |\n  - #upgrade",
                &presets
            ),
            vec!["#gh-issue".to_string(), "#upgrade".to_string()]
        );
        // `#pipeprompt` inline form: the same statement on one row, also
        // after a lineage prefix, a hand-off chain the worker runs whole, and
        // a stage argument.
        for statement in [
            "#address | #adversarial-pr",
            "🚧 [#subagent:pipe#2] #address | #adversarial-pr [#q-0123456789ab]",
            "https://github.com/sample-org/sample-repo/pull/12: #address | #adversarial-pr",
            "#address (see https://github.com/sample-org/sample-repo/pull/12#discussion_r1) |> #adversarial-pr",
            "#address | #adversarial-pr |\n  - under 200 words",
        ] {
            assert_eq!(
                temporal_preset_order(statement, &presets),
                address_then_review,
                "{statement}"
            );
            assert!(item_orders_a_later_preset(statement), "{statement}");
        }
        assert_eq!(
            temporal_preset_order(
                "https://github.com/sample-org/sample-repo/pull/933: #rebase |> #address |> #adversarial-pr",
                &presets
            ),
            three
        );
        // `#directiveposition` per segment: a middle segment's preset is a
        // directive even with prose arguments on both sides of it.
        assert_eq!(
            temporal_preset_order(
                "#address (see the thread) | #note the fix please | #adversarial-pr",
                &presets
            ),
            vec![
                "#address".to_string(),
                "#note".to_string(),
                "#adversarial-pr".to_string()
            ]
        );
        // Not an order: no pipe (context children), one preset stage, a prose
        // or quoted mention, a code span, or a prose ` | ` segment.
        for text in [
            "#address\n  - #adversarial-pr",
            "#address |\n  - update the title",
            "Collect the open issues |\n  - #address",
            "#address |\n  - explain why the #adversarial-pr review was stale",
            "#address |\n  - `#adversarial-pr`",
            "#address |\n  - \"#adversarial-pr\"",
            "#address | review it with #adversarial-pr",
            "`#address | #adversarial-pr`",
            "#address >\n  - #adversarial-pr",
        ] {
            assert!(temporal_preset_order(text, &presets).is_empty(), "{text}");
        }
        assert!(!item_orders_a_later_preset(
            "#address |\n  - update the title"
        ));
        assert!(!item_orders_a_later_preset("#address\n  - #adversarial-pr"));
        assert!(!item_orders_a_later_preset(
            "#address |\n  - `#adversarial-pr`"
        ));
    }

    #[test]
    fn a_no_change_address_does_not_settle_the_later_preset() {
        let presets = presets();
        let order = temporal_preset_order("#address then #adversarial-pr", &presets);
        let head = "fa3a3f449986d565f21aeb6e0f792323bc8d1d58";
        let address_only = format!("NO-WRITTEN-FIX head={head}\n");
        assert_eq!(
            first_unmet_later_preset(&order, &address_only, &presets).as_deref(),
            Some("#adversarial-pr")
        );
        let done = format!("{address_only}PRESET-DONE #adversarial-pr\n");
        assert!(first_unmet_later_preset(&order, &done, &presets).is_none());
        assert!(!line_preset_done(
            "PRESET-DONE #adversarial",
            "#adversarial-pr"
        ));

        let goal = temporal_preset_order("#rebase then #goal", &presets);
        let clean = "REVIEW-CLEAN https://github.com/sample-org/sample-repo/pull/12#pullrequestreview-1 head=fa3a3f449986d565f21aeb6e0f792323bc8d1d58\n";
        assert!(first_unmet_later_preset(&goal, clean, &presets).is_none());
        assert!(first_unmet_later_preset(&goal, &address_only, &presets).is_some());

        let both = format!(
            "FIX-ON-HEAD https://github.com/sample-org/sample-repo/pull/12#pullrequestreview-1 head={head}\n\
             NO-WRITTEN-FIX head={head}\n"
        );
        let address_tail = temporal_preset_order("#rebase then #address", &presets);
        assert_eq!(
            first_unmet_later_preset(&address_tail, &both, &presets).as_deref(),
            Some("#address")
        );
    }

    fn goal_presets() -> Vec<(String, String)> {
        vec![
            (
                "#address".to_string(),
                "Address every actionable review item on the exact current head.".to_string(),
            ),
            (
                "#adversarial-pr".to_string(),
                "/goal Post exactly one plain COMMENTED review on the exact current remote head. \
                 Your final message must begin with exactly one of these lines: \
                 - `REVIEW-CLEAN <final review URL> base=<sha> head=<sha> cycles=<n>` \
                 - `BLOCKED <single operator action> base=<sha> head=<sha> cycles=<n>`."
                    .to_string(),
            ),
        ]
    }

    /// `#pipestagereceipt`: each attempt emits one half. The previous hand-back
    /// supplies the other when both name the same head.
    #[test]
    fn a_prior_half_settles_the_other_stage_on_the_same_head() {
        let presets = goal_presets();
        let head = "fa3a3f449986d565f21aeb6e0f792323bc8d1d58";
        let other = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let fix = format!(
            "FIX-ON-HEAD https://github.com/sample-org/sample-repo/pull/12#pullrequestreview-1 head={head}\n\
             INLINE-THREADS-LOADED 0 head={head}\n"
        );
        let clean = format!(
            "REVIEW-CLEAN https://github.com/sample-org/sample-repo/pull/12#pullrequestreview-2 \
             base={other} head={head} cycles=1\n\
             The plain commented review is already on the exact head.\n\
             INLINE-THREADS-LOADED 0 head={head}\n"
        );
        let prior_fix = PriorOrderedHandoff {
            response: &fix,
            address_fix_pending: false,
            address_effect_unproven: false,
            address_effect_unknown: false,
        };
        assert!(preset_phase_settled_with_prior(
            "#address",
            &clean,
            Some(&prior_fix),
            &presets
        ));
        assert!(preset_phase_settled_with_prior(
            "#adversarial-pr",
            &clean,
            Some(&prior_fix),
            &presets
        ));
        let prior_clean = PriorOrderedHandoff {
            response: &clean,
            address_fix_pending: true,
            address_effect_unproven: false,
            address_effect_unknown: false,
        };
        assert!(preset_phase_settled_with_prior(
            "#adversarial-pr",
            &fix,
            Some(&prior_clean),
            &presets
        ));
        assert!(prior_goal_completion_carries(
            &fix,
            &clean,
            &["REVIEW-CLEAN".to_string(), "BLOCKED".to_string()],
            &[presets[1].1.clone()],
            true
        ));
        let withheld = format!(
            "REVIEW-CLEAN https://github.com/sample-org/sample-repo/pull/12#pullrequestreview-2 \
             head={head} cycles=1\n\
             posted nothing\n\
             INLINE-THREADS-LOADED 0 head={head}\n"
        );
        assert!(!preset_phase_settled_with_prior(
            "#adversarial-pr",
            &withheld,
            Some(&prior_fix),
            &presets
        ));
        assert!(!prior_goal_completion_carries(
            &withheld,
            &clean,
            &["REVIEW-CLEAN".to_string(), "BLOCKED".to_string()],
            &[presets[1].1.clone()],
            true
        ));
        let clean_other = clean.replace(head, other);
        assert!(!preset_phase_settled_with_prior(
            "#address",
            &clean_other,
            Some(&prior_fix),
            &presets
        ));
        let unproven = PriorOrderedHandoff {
            response: &fix,
            address_fix_pending: false,
            address_effect_unproven: true,
            address_effect_unknown: false,
        };
        assert!(!preset_phase_settled_with_prior(
            "#address",
            &clean,
            Some(&unproven),
            &presets
        ));
        let both = format!("{fix}NO-WRITTEN-FIX head={head}\n");
        assert!(!preset_phase_settled_with_prior(
            "#address",
            &both,
            Some(&prior_fix),
            &presets
        ));
    }
}
