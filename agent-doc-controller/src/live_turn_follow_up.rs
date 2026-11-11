//! One composer follow-up for a running owner turn (`#steercoalesce`).
//!
//! A hookless harness queues every Enter. Pool-refill pastes and a steering
//! paste are the same follow-up: the latest pool snapshot and the steering
//! text share one payload, and a second Enter is not a second queue row.

/// Marker on a payload that carries both slots. A one-slot payload stays the
/// original text so an existing compact-token consumer still reads it.
pub const LIVE_TURN_FOLLOW_UP_MARKER: &str = "[agent-doc] live-turn follow-up (#steercoalesce)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveTurnFollowUpSlot {
    /// Latest running-turn pool refill. A full snapshot, not a delta.
    Pool,
    /// Operator steering for this turn. Chunks accumulate until consumed.
    Steering,
}

/// Fold `incoming` into steering already stored for this turn.
///
/// A later route-sync diff is only the delta after the baseline moves, so it
/// is appended. An identical chunk, or a snapshot that already contains the
/// other, does not grow the payload.
pub fn merge_steering(existing: Option<&str>, incoming: &str) -> String {
    let incoming = incoming.trim();
    let Some(existing) = existing.map(str::trim).filter(|text| !text.is_empty()) else {
        return incoming.to_string();
    };
    if existing == incoming || incoming.contains(existing) {
        return incoming.to_string();
    }
    if existing.contains(incoming) {
        return existing.to_string();
    }
    format!("{existing}\n\n{incoming}")
}

/// Payload behind the one compact reference for this turn.
///
/// One slot is stored as itself. Both slots become one body with steering
/// before the pool snapshot, so a queued agent-prompt item is not a second
/// prompt in front of steering.
pub fn live_turn_follow_up_payload(pool: Option<&str>, steering: Option<&str>) -> Option<String> {
    let pool = pool.map(str::trim).filter(|text| !text.is_empty());
    let steering = steering.map(str::trim).filter(|text| !text.is_empty());
    match (steering, pool) {
        (None, None) => None,
        (Some(steering), None) => Some(steering.to_string()),
        (None, Some(pool)) => Some(pool.to_string()),
        (Some(steering), Some(pool)) => Some(format!(
            "{LIVE_TURN_FOLLOW_UP_MARKER}\n\
             Agent prompt items and operator steering for this running turn are one follow-up.\n\
             \n\
             {steering}\n\
             \n\
             {pool}\n"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_slot_stays_the_original_text() {
        assert_eq!(
            live_turn_follow_up_payload(None, Some(" secret payload ")).as_deref(),
            Some("secret payload")
        );
        assert_eq!(
            live_turn_follow_up_payload(Some("claim the head"), None).as_deref(),
            Some("claim the head")
        );
    }

    #[test]
    fn pool_and_steering_are_one_payload_with_steering_first() {
        let payload = live_turn_follow_up_payload(Some("pool body"), Some("steer body")).unwrap();
        assert!(payload.starts_with(LIVE_TURN_FOLLOW_UP_MARKER));
        let steer = payload.find("steer body").unwrap();
        let pool = payload.find("pool body").unwrap();
        assert!(steer < pool, "{payload}");
    }

    #[test]
    fn a_later_steering_delta_is_appended_once() {
        let first = merge_steering(None, "full diff");
        let again = merge_steering(Some(&first), "full diff");
        assert_eq!(again, "full diff");
        let delta = merge_steering(Some(&again), "only the new line");
        assert_eq!(delta, "full diff\n\nonly the new line");
        assert_eq!(
            merge_steering(Some(&delta), "full diff"),
            delta,
            "a snapshot that is already contained does not duplicate"
        );
    }
}
