//! Exact harness conversation identity across managed child replacement.

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
}
