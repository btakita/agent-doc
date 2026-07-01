//! Pure backlog guard policy and reference projection.
//!
//! This module owns the tracked-work decisions/messages that do not require
//! filesystem, git, or session adapters. Orchestration supplies document content
//! and any already-resolved id sets, then translates [`BacklogGuardOutcome`] at
//! its boundary.

use crate::backlog::{
    DroppedBacklogReport, MalformedTrackedItemRef, ShadowPendingReport,
    detect_dropped_from_history_with_extra_current_ids, detect_shadow_open_items,
    format_dropped_refs, format_shadow_refs, malformed_tracked_item_interruption_message,
    malformed_tracked_item_refs_in_components,
};
use agent_doc_element::element;
use anyhow::Result;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BacklogGuardOutcome {
    Pass,
    Warn(Vec<String>),
    Interrupt(String),
}

pub fn shadow_backlog_guard(doc: &str) -> Result<BacklogGuardOutcome> {
    Ok(shadow_backlog_report_guard(&detect_shadow_open_items(doc)?))
}

pub fn shadow_backlog_report_guard(report: &ShadowPendingReport) -> BacklogGuardOutcome {
    if !report.shadow_only.is_empty() {
        return BacklogGuardOutcome::Interrupt(format!(
            "[session-check] INTERRUPTED: open backlog item(s) exist only outside live agent:backlog: {}. Re-run preflight/repair after restoring them to the live backlog or marking them complete",
            format_shadow_refs(&report.shadow_only)
        ));
    }
    if !report.duplicated_in_live_backlog.is_empty() {
        return BacklogGuardOutcome::Warn(vec![format!(
            "[session-check] warning: open backlog item(s) also appear outside live agent:backlog: {}",
            format_shadow_refs(&report.duplicated_in_live_backlog)
        )]);
    }
    BacklogGuardOutcome::Pass
}

pub fn malformed_tracked_item_guard(
    content: &str,
    components: &[element::Component],
) -> BacklogGuardOutcome {
    let refs = malformed_tracked_item_reference_strings(malformed_tracked_item_refs_in_components(
        content, components,
    ));
    if refs.is_empty() {
        return BacklogGuardOutcome::Pass;
    }
    BacklogGuardOutcome::Interrupt(malformed_tracked_item_interruption_message(&refs))
}

pub fn malformed_tracked_item_reference_strings(
    refs: impl IntoIterator<Item = MalformedTrackedItemRef>,
) -> Vec<String> {
    refs.into_iter().map(|item| item.reference()).collect()
}

pub fn dropped_from_history_guard(
    current_doc: &str,
    baseline_doc: &str,
    done_ids: &HashSet<String>,
    extra_current_ids: &HashSet<String>,
) -> Result<BacklogGuardOutcome> {
    Ok(dropped_from_history_report_guard(
        &detect_dropped_from_history_with_extra_current_ids(
            current_doc,
            baseline_doc,
            done_ids,
            extra_current_ids,
        )?,
    ))
}

pub fn dropped_from_history_report_guard(report: &DroppedBacklogReport) -> BacklogGuardOutcome {
    if report.dropped.is_empty() {
        return BacklogGuardOutcome::Pass;
    }
    BacklogGuardOutcome::Interrupt(format!(
        "[session-check] INTERRUPTED: open backlog item(s) from recent history are completely absent from the document: {}. Restore them to the live backlog, move them to icebox, or mark them done",
        format_dropped_refs(&report.dropped)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backlog::{
        ShadowPendingItem, detect_dropped_from_history, format_dropped_refs, format_shadow_refs,
    };

    #[test]
    fn format_shadow_refs_joins_references() {
        let refs = format_shadow_refs(&[
            ShadowPendingItem {
                id: "one1".to_string(),
                text: "first".to_string(),
                line: 7,
            },
            ShadowPendingItem {
                id: "two2".to_string(),
                text: "second".to_string(),
                line: 9,
            },
        ]);

        assert_eq!(refs, "#one1 (line 7), #two2 (line 9)");
    }

    #[test]
    fn shadow_guard_interrupts_shadow_only_items() {
        let doc = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#live1] Keep in backlog\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- parked copy\n",
            "- [ ] [#lost1] Drifted out of backlog\n",
            "-->\n"
        );

        let outcome = shadow_backlog_guard(doc).unwrap();

        assert!(matches!(outcome, BacklogGuardOutcome::Interrupt(message)
            if message.contains("#lost1") && message.contains("only outside live agent:backlog")));
    }

    #[test]
    fn shadow_guard_warns_duplicate_shadow_items() {
        let doc = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#live1] Keep in backlog\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- parked copy\n",
            "- [ ] [#live1] Keep in backlog\n",
            "-->\n"
        );

        let outcome = shadow_backlog_guard(doc).unwrap();

        assert!(matches!(outcome, BacklogGuardOutcome::Warn(lines)
            if lines.len() == 1 && lines[0].contains("#live1") && lines[0].contains("also appear")));
    }

    #[test]
    fn malformed_guard_interrupts_with_projected_refs() {
        let doc = concat!(
            "<!-- agent:backlog -->\n",
            "_- [ ] [#bad1] damaged backlog\n",
            "<!-- /agent:backlog -->\n"
        );
        let components = element::parse(doc).unwrap();

        let outcome = malformed_tracked_item_guard(doc, &components);

        assert!(matches!(outcome, BacklogGuardOutcome::Interrupt(message)
            if message.contains("backlog #bad1 (line 1)") && message.contains("malformed tracked checklist item")));
    }

    #[test]
    fn dropped_history_guard_interrupts_with_missing_ids() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#gone1] Was open in baseline\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!("<!-- agent:backlog -->\n", "<!-- /agent:backlog -->\n");

        let outcome =
            dropped_from_history_guard(current, baseline, &HashSet::new(), &HashSet::new())
                .unwrap();

        assert!(matches!(outcome, BacklogGuardOutcome::Interrupt(message)
            if message.contains("#gone1") && message.contains("recent history")));
    }

    #[test]
    fn dropped_refs_match_backlog_detection_projection() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#gone1] Was open in baseline\n",
            "- [ ] [#gone2] Also open in baseline\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!("<!-- agent:backlog -->\n", "<!-- /agent:backlog -->\n");
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();

        assert_eq!(format_dropped_refs(&report.dropped), "#gone1, #gone2");
    }
}
