//! Temporal preset order (`#presetorder`).
//!
//! A queue item that names presets with `then` between them runs each preset
//! in that order. `#address then #adversarial-pr` runs address, then the
//! adversarial review. Settling the earlier preset, including
//! `NO-WRITTEN-FIX`, does not finish the row.

use agent_doc_document::queue_projection::strip_priority_markers;

/// Preset names `text` joins with temporal `then`, in that order.
///
/// A chain is known presets whose consecutive pairs have `then` between them.
/// Other words are gaps and do not break a `then`. A preset with no preceding
/// `then` starts a new chain. The longest chain of length at least two is
/// returned (the first chain on a tie). Inline code is masked, and a preset
/// already in the chain is kept once.
pub fn temporal_preset_order(text: &str, presets: &[(String, String)]) -> Vec<String> {
    let text = strip_priority_markers(text);
    let text = crate::queue_continuation::mask_directive_scan(&text);
    let mut chains: Vec<Vec<String>> = Vec::new();
    let mut expect_continue = false;
    for token in
        text.split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '#' | '-' | '_')))
    {
        if token.is_empty() {
            continue;
        }
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

/// True when `item` has `then` and a later `#` token, so a brief can tell the
/// next worker that a settled earlier preset does not finish the row.
pub fn item_orders_a_later_preset(item: &str) -> bool {
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
         Do not start another address pass when the address half already settled. Order:",
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
}
