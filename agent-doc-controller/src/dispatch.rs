//! Pure controller dispatch admission helpers.

pub const DISPATCH_COALESCED_IN_FLIGHT_MARKER: &str = "failed_stage=coalesced_in_flight";
pub const DISPATCH_STALE_GENERATION_REDIRECT_MARKER: &str = "stale_generation_redirect";
pub const DISPATCH_SUPERVISOR_RESTART_REDIRECT_MARKER: &str = "supervisor_restart_redirect";
pub const STALE_QUEUE_PAUSE_INVARIANT_ID: &str = "stale_queue_pause";
pub const STALE_QUEUE_PAUSE_NEXT_ACTION: &str = "restart_supervisor_once_and_retry";

pub fn dispatch_should_coalesce_in_flight(
    in_flight_same_cycle: bool,
    operator_driven: bool,
) -> bool {
    in_flight_same_cycle && !operator_driven
}

pub fn dispatch_error_is_coalesced(message: &str) -> bool {
    message.contains(DISPATCH_COALESCED_IN_FLIGHT_MARKER)
}

pub fn dispatch_command_kind_is_operator_reopen(command_kind: &str) -> bool {
    matches!(command_kind, "managed_reopen" | "dispatch_only_reopen")
}

pub fn dispatch_error_stale_generation_redirect_target(message: &str) -> Option<u64> {
    if !message.contains(DISPATCH_STALE_GENERATION_REDIRECT_MARKER) {
        return None;
    }
    message.split("retry_generation=").nth(1).and_then(|rest| {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse::<u64>().ok()
    })
}

pub fn pause_reason_is_stale_supervisor_churn_stop(reason: &str) -> bool {
    let r = reason.to_ascii_lowercase();
    if r.contains("supervisor_binary_stale")
        || r.contains("stale supervisor")
        || r.contains("stale host supervisor")
        || r.contains("stale route-owned supervisor")
    {
        return true;
    }
    let is_churn_stop = r.contains("churn-stop") || r.contains("churn_stop");
    is_churn_stop && r.contains("needs operator recycle")
}

pub fn stale_supervisor_pid_from_pause_reason(reason: &str) -> Option<u32> {
    let lower = reason.to_ascii_lowercase();
    let rest = lower.split("pid").nth(1)?;
    let digits: String = rest
        .trim_start_matches([' ', '=', ':', '#'])
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

pub fn stale_queue_pause_pid_from_dispatch_error(message: &str) -> Option<u32> {
    if message.contains(DISPATCH_SUPERVISOR_RESTART_REDIRECT_MARKER) {
        let pid = message
            .split("stale_pid=")
            .nth(1)
            .map(|rest| {
                rest.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
            })
            .and_then(|digits| digits.parse::<u32>().ok())
            .unwrap_or(0);
        return Some(pid);
    }
    if message.contains("failed_stage=queue_paused")
        && pause_reason_is_stale_supervisor_churn_stop(message)
    {
        return Some(stale_supervisor_pid_from_pause_reason(message).unwrap_or(0));
    }
    None
}

pub fn spent_preset_id_from_pause_reason(reason: &str) -> Option<String> {
    let marker = " preset head is spent";
    let lower = reason.to_ascii_lowercase();
    if let Some(idx) = lower.find(marker) {
        let candidate = lower[..idx]
            .rsplit(|ch: char| ch.is_whitespace() || matches!(ch, ':' | ';' | ',' | '(' | '['))
            .next()?
            .trim()
            .trim_start_matches('#');
        if valid_preset_pause_id(candidate) {
            return Some(candidate.to_string());
        }
    }
    preset_token_unserviceable_id_from_pause_reason(&lower)
}

fn preset_token_unserviceable_id_from_pause_reason(lower_reason: &str) -> Option<String> {
    if !lower_reason.contains("preset-token") || !lower_reason.contains("un-drainable") {
        return None;
    }
    let (_, rest) = lower_reason.split_once("(#")?;
    let candidate: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect();
    if valid_preset_pause_id(&candidate) {
        Some(candidate)
    } else {
        None
    }
}

fn valid_preset_pause_id(candidate: &str) -> bool {
    !candidate.is_empty()
        && candidate
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_dispatch_coalesces_only_when_same_cycle_in_flight() {
        assert!(dispatch_should_coalesce_in_flight(true, false));
        assert!(!dispatch_should_coalesce_in_flight(true, true));
        assert!(!dispatch_should_coalesce_in_flight(false, false));
        assert!(!dispatch_should_coalesce_in_flight(false, true));
    }

    #[test]
    fn coalesced_error_marker_survives_wrapping() {
        let wrapped = format!(
            "project controller command `dispatch` failed: dispatch blocked: {}",
            DISPATCH_COALESCED_IN_FLIGHT_MARKER
        );

        assert!(dispatch_error_is_coalesced(&wrapped));
        assert!(!dispatch_error_is_coalesced(
            "dispatch blocked for x: failed_stage=queue_paused"
        ));
    }

    #[test]
    fn operator_reopen_command_kind_is_explicit() {
        assert!(dispatch_command_kind_is_operator_reopen("managed_reopen"));
        assert!(dispatch_command_kind_is_operator_reopen(
            "dispatch_only_reopen"
        ));
        assert!(!dispatch_command_kind_is_operator_reopen(
            "idle_queue_continuation"
        ));
        assert!(!dispatch_command_kind_is_operator_reopen("loop"));
    }

    #[test]
    fn stale_generation_redirect_extracts_retry_generation() {
        let wrapped = format!(
            "project controller command `dispatch` failed: {} retry_generation=42",
            DISPATCH_STALE_GENERATION_REDIRECT_MARKER
        );

        assert_eq!(
            dispatch_error_stale_generation_redirect_target(&wrapped),
            Some(42)
        );
        assert_eq!(
            dispatch_error_stale_generation_redirect_target("stale generation retry_generation=42"),
            None
        );
        assert_eq!(
            dispatch_error_stale_generation_redirect_target(&format!(
                "{} retry_generation=x",
                DISPATCH_STALE_GENERATION_REDIRECT_MARKER
            )),
            None
        );
    }

    #[test]
    fn stale_supervisor_churn_stop_classification_extracts_pid() {
        let reason =
            "churn-stop: head re-injected by stale supervisor pid1368698; needs operator recycle";
        assert!(pause_reason_is_stale_supervisor_churn_stop(reason));
        assert_eq!(
            stale_supervisor_pid_from_pause_reason(reason),
            Some(1368698)
        );

        let marked =
            "dispatch blocked: supervisor_restart_redirect stale_pid=42 failed_stage=queue_paused";
        assert_eq!(stale_queue_pause_pid_from_dispatch_error(marked), Some(42));

        let legacy =
            "dispatch blocked: failed_stage=queue_paused reason=stale host supervisor pid 9";
        assert_eq!(stale_queue_pause_pid_from_dispatch_error(legacy), Some(9));

        assert_eq!(
            stale_queue_pause_pid_from_dispatch_error("failed_stage=queue_paused reason=operator"),
            None
        );
    }

    #[test]
    fn spent_preset_pause_ids_are_extracted_from_supported_shapes() {
        assert_eq!(
            spent_preset_id_from_pause_reason("#abc-123 preset head is spent"),
            Some("abc-123".to_string())
        );
        assert_eq!(
            spent_preset_id_from_pause_reason("preset-token item is un-drainable (#review_queue)"),
            Some("review_queue".to_string())
        );
        assert_eq!(spent_preset_id_from_pause_reason("no preset here"), None);
    }
}
