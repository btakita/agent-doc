//! Exact harness conversation identity across managed child replacement.

use serde::{Deserialize, Serialize};

/// Controller-observed conversation lineage for one document supervisor.
///
/// `initial_observed_id` fences an old ledger row when this supervisor was
/// explicitly started fresh. A later different projection is the hook event
/// emitted by the newly launched harness and becomes authoritative. Once an
/// active id is known, lagging or absent frontmatter cannot replace it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessSessionLineage {
    active_id: Option<String>,
    initial_observed_id: Option<String>,
}

impl HarnessSessionLineage {
    pub fn new(active_id: Option<String>, initial_observed_id: Option<String>) -> Self {
        Self {
            active_id,
            initial_observed_id,
        }
    }

    pub fn active_id(&self) -> Option<&str> {
        self.active_id.as_deref()
    }

    /// Replace the active lineage from an explicit operator Restart Agent.
    ///
    /// Ordinary child crashes and controller recycles must never call this:
    /// their lagging document projection cannot supersede a conversation that
    /// the running supervisor already observed. Restart Agent is different —
    /// it is the operator boundary that deliberately re-resolves current
    /// frontmatter, including a newly persisted exact `resume:` binding.
    pub fn replace_from_operator_restart(&mut self, id: String) -> bool {
        let id = id.trim();
        if id.is_empty() || self.active_id.as_deref() == Some(id) {
            return false;
        }
        self.active_id = Some(id.to_string());
        self.initial_observed_id = None;
        true
    }

    /// Adopt the conversation the exited harness child itself reported through
    /// its `SessionStart` hook (`#claudecrashresumeafterclear`).
    ///
    /// Only a [`claude_respawn_session_start_adoption`] proof may call this: the
    /// id comes from the very child this supervisor just lost, not from a
    /// lagging document projection, so it supersedes the in-memory lineage the
    /// supervisor minted or resumed before that child's `/clear`.
    pub fn adopt_exited_child_session_start(&mut self, id: &str) -> bool {
        let id = id.trim();
        if id.is_empty() || self.active_id.as_deref() == Some(id) {
            return false;
        }
        self.active_id = Some(id.to_string());
        self.initial_observed_id = None;
        true
    }

    /// Observe the durable hook/controller projection at a lifecycle edge.
    ///
    /// Returns true only when a fresh child has published a new exact id.
    pub fn observe_projected_id(&mut self, projected_id: Option<&str>) -> bool {
        if self.active_id.is_some() {
            return false;
        }
        let Some(projected_id) = projected_id.filter(|id| !id.trim().is_empty()) else {
            return false;
        };
        if self.initial_observed_id.as_deref() == Some(projected_id) {
            return false;
        }
        self.active_id = Some(projected_id.to_string());
        true
    }

    /// Retire only an id the harness proved missing.
    pub fn clear_proven_missing(&mut self, missing_id: &str) -> bool {
        if self.active_id.as_deref() != Some(missing_id) {
            return false;
        }
        self.active_id = None;
        self.initial_observed_id = Some(missing_id.to_string());
        true
    }

    /// Start an explicitly fresh lineage while fencing the current projection.
    pub fn begin_fresh(&mut self, current_projected_id: Option<String>) {
        self.active_id = None;
        self.initial_observed_id = current_projected_id;
    }
}

/// `document_runtime_state.state_kind` of the durable Claude `SessionStart`
/// capture (`#claudecrashresumeafterclear`).
pub const CLAUDE_SESSION_START_STATE_KIND: &str = "claude_session_start_capture";

/// Durable, provenance-bearing record of the conversation id a supervised
/// Claude harness reported through its `SessionStart` hook
/// (`#clauderesumeafterclear`).
///
/// The hook writes it only after proving, from `/proc`, that it ran under the
/// direct harness child of the document's current-generation supervisor. The
/// record names that exact child (`harness_pid` + its kernel start time), so a
/// supervisor respawning after that child crashed can tell the capture belongs
/// to the conversation it just lost — and not to an earlier child, a nested
/// `claude -p`, another supervisor, or a recycled pid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeSessionStartCapture {
    /// The conversation id the harness reported (`session_id`).
    pub session_id: String,
    /// `clear` or `startup`.
    pub source: String,
    /// The supervisor proven to be the harness's parent at hook time.
    pub supervisor_pid: u32,
    /// The document generation whose lease named that supervisor.
    pub generation: u64,
    /// The supervisor's direct harness child that ran the hook.
    pub harness_pid: u32,
    /// That child's kernel start time (`/proc/<pid>/stat` field 22), which
    /// makes `(harness_pid, harness_start_ticks)` immune to pid reuse.
    pub harness_start_ticks: u64,
    /// Wall-clock milliseconds when the hook recorded it.
    pub recorded_at_ms: u64,
}

/// Parse a stored [`ClaudeSessionStartCapture`]. Malformed is absent.
pub fn parse_claude_session_start_capture(payload: &str) -> Option<ClaudeSessionStartCapture> {
    serde_json::from_str(payload).ok()
}

/// Kernel identity of a harness child a supervisor spawned or adopted, sampled
/// while the child was alive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessChildIdentity {
    pub pid: u32,
    pub start_ticks: u64,
}

/// Decide whether a supervisor respawning its Claude harness must resume the
/// conversation the exited child reported through `SessionStart`
/// (`#claudecrashresumeafterclear`).
///
/// The supervisor's in-memory lineage is the id it minted or resumed at launch.
/// An operator `/clear` inside that child starts a new conversation the
/// supervisor never sees, so a crash restart would otherwise resume the
/// pre-clear conversation. The capture is adopted only when every link of the
/// hook's provenance still holds at the respawn edge — as strict as the hook's
/// own proof:
///
/// - the capture names THIS supervisor (`capture_from_other_supervisor`);
/// - this supervisor still holds the document's current-generation lease, the
///   same lease the hook proved against (`supervisor_not_current_lease_owner`);
/// - the exited child's kernel identity was sampled while it lived
///   (`exited_child_identity_unknown`), and the capture names exactly that
///   child — pid AND start time (`capture_from_other_harness_child`).
///
/// `Err` is a stable skip reason; `no_capture` is the common silent case.
pub fn claude_respawn_session_start_adoption(
    capture: Option<&ClaudeSessionStartCapture>,
    supervisor_pid: u32,
    current_lease_supervisor_pid: Option<u32>,
    exited_child: Option<HarnessChildIdentity>,
) -> Result<&str, &'static str> {
    let capture = capture.ok_or("no_capture")?;
    if capture.supervisor_pid != supervisor_pid {
        return Err("capture_from_other_supervisor");
    }
    if current_lease_supervisor_pid != Some(supervisor_pid) {
        return Err("supervisor_not_current_lease_owner");
    }
    let exited_child = exited_child.ok_or("exited_child_identity_unknown")?;
    if capture.harness_pid != exited_child.pid
        || capture.harness_start_ticks != exited_child.start_ticks
    {
        return Err("capture_from_other_harness_child");
    }
    let id = capture.session_id.trim();
    if id.is_empty() {
        return Err("capture_session_id_empty");
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projected_id_becomes_stable_restart_authority() {
        let mut lineage = HarnessSessionLineage::new(None, Some("old-thread".into()));
        assert!(!lineage.observe_projected_id(Some("old-thread")));
        assert!(lineage.observe_projected_id(Some("orchard-thread")));
        assert_eq!(lineage.active_id(), Some("orchard-thread"));
        assert!(!lineage.observe_projected_id(Some("other-thread")));
        assert_eq!(lineage.active_id(), Some("orchard-thread"));
    }

    #[test]
    fn proven_missing_id_is_fenced_until_a_new_projection_arrives() {
        let mut lineage =
            HarnessSessionLineage::new(Some("orchard-thread".into()), Some("old-thread".into()));
        assert!(lineage.clear_proven_missing("orchard-thread"));
        assert!(!lineage.observe_projected_id(Some("orchard-thread")));
        assert!(lineage.observe_projected_id(Some("replacement-thread")));
    }

    #[test]
    fn explicit_operator_restart_can_replace_stable_lineage() {
        let mut lineage =
            HarnessSessionLineage::new(Some("stale-thread".into()), Some("older-thread".into()));
        assert!(lineage.replace_from_operator_restart("restored-thread".into()));
        assert_eq!(lineage.active_id(), Some("restored-thread"));
        assert!(!lineage.replace_from_operator_restart("restored-thread".into()));
        assert!(!lineage.replace_from_operator_restart("   ".into()));
        assert_eq!(lineage.active_id(), Some("restored-thread"));
    }

    fn capture(
        id: &str,
        supervisor_pid: u32,
        harness_pid: u32,
        ticks: u64,
    ) -> ClaudeSessionStartCapture {
        ClaudeSessionStartCapture {
            session_id: id.into(),
            source: "clear".into(),
            supervisor_pid,
            generation: 3,
            harness_pid,
            harness_start_ticks: ticks,
            recorded_at_ms: 1,
        }
    }

    const CHILD: HarnessChildIdentity = HarnessChildIdentity {
        pid: 2909304,
        start_ticks: 77,
    };

    /// `#claudecrashresumeafterclear`: a capture from the very child that just
    /// exited, under the supervisor that still owns the document, is adopted.
    #[test]
    fn respawn_adopts_only_the_exited_childs_own_session_start() {
        let own = capture("e62ea89f", 1605424, CHILD.pid, CHILD.start_ticks);
        assert_eq!(
            claude_respawn_session_start_adoption(Some(&own), 1605424, Some(1605424), Some(CHILD)),
            Ok("e62ea89f")
        );
        assert_eq!(
            claude_respawn_session_start_adoption(None, 1605424, Some(1605424), Some(CHILD)),
            Err("no_capture")
        );
        // Another supervisor's capture (an earlier process, or a different doc
        // owner) proves nothing about this supervisor's child.
        assert_eq!(
            claude_respawn_session_start_adoption(Some(&own), 42, Some(42), Some(CHILD)),
            Err("capture_from_other_supervisor")
        );
        // This supervisor no longer holds the current-generation lease.
        assert_eq!(
            claude_respawn_session_start_adoption(Some(&own), 1605424, Some(9), Some(CHILD)),
            Err("supervisor_not_current_lease_owner")
        );
        assert_eq!(
            claude_respawn_session_start_adoption(Some(&own), 1605424, None, Some(CHILD)),
            Err("supervisor_not_current_lease_owner")
        );
        // The exited child's identity was never sampled: fail closed.
        assert_eq!(
            claude_respawn_session_start_adoption(Some(&own), 1605424, Some(1605424), None),
            Err("exited_child_identity_unknown")
        );
        // An earlier child's capture (different pid), or a recycled pid (same
        // pid, different kernel start time), is not this child's conversation.
        let earlier = capture("old", 1605424, 111, CHILD.start_ticks);
        assert_eq!(
            claude_respawn_session_start_adoption(
                Some(&earlier),
                1605424,
                Some(1605424),
                Some(CHILD)
            ),
            Err("capture_from_other_harness_child")
        );
        let recycled = capture("old", 1605424, CHILD.pid, CHILD.start_ticks + 1);
        assert_eq!(
            claude_respawn_session_start_adoption(
                Some(&recycled),
                1605424,
                Some(1605424),
                Some(CHILD)
            ),
            Err("capture_from_other_harness_child")
        );
        let blank = capture("  ", 1605424, CHILD.pid, CHILD.start_ticks);
        assert_eq!(
            claude_respawn_session_start_adoption(
                Some(&blank),
                1605424,
                Some(1605424),
                Some(CHILD)
            ),
            Err("capture_session_id_empty")
        );
    }

    /// `#claudecrashresumeafterclear`: the adoption supersedes the pre-clear
    /// in-memory lineage, and is a no-op when the lineage already names it.
    #[test]
    fn exited_child_session_start_supersedes_pre_clear_lineage() {
        let mut lineage = HarnessSessionLineage::new(Some("ba846c4c".into()), None);
        assert!(lineage.adopt_exited_child_session_start("e62ea89f"));
        assert_eq!(lineage.active_id(), Some("e62ea89f"));
        assert!(!lineage.adopt_exited_child_session_start("e62ea89f"));
        assert!(!lineage.adopt_exited_child_session_start(" "));
        assert_eq!(lineage.active_id(), Some("e62ea89f"));
        // A later projection still cannot displace the adopted id.
        assert!(!lineage.observe_projected_id(Some("other")));
        assert_eq!(lineage.active_id(), Some("e62ea89f"));
    }

    #[test]
    fn session_start_capture_round_trips_and_rejects_malformed() {
        let own = capture("e62ea89f", 1, 2, 3);
        let json = serde_json::to_string(&own).unwrap();
        assert_eq!(parse_claude_session_start_capture(&json), Some(own));
        assert_eq!(
            parse_claude_session_start_capture("{\"session_id\":1}"),
            None
        );
    }
}
