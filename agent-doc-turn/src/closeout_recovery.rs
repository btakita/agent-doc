//! Pure closeout recovery policy.
//!
//! Orchestration owns file, git, and sidecar mutation effects. This module owns
//! action-independent turn recovery decisions that can be proven from document
//! content facts.

/// Which side of a metadata-only drift is authoritative for closeout recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataDriftAuthority {
    /// The local side (snapshot for queue metadata drift, visible file for
    /// sidecar-visible drift) is authoritative and can be committed forward.
    Local,
    /// HEAD is authoritative and the local side should be restored from it.
    Head,
    /// Neither side is provably authoritative; recovery must fail closed.
    Ambiguous,
}

/// Decide the authoritative side of a content-equal metadata-only drift between
/// a `local` document string (the candidate to commit) and the committed `head`.
///
/// The decision turns on the live auto-queue continuation signal
/// (`#recovery-drift-authoritative-side`). Because the caller has already proven
/// the content components are byte-identical, the only durable state the diff can
/// destroy is an active queue continuation. Legitimate consumption of a queue
/// head always shows up as response/content drift, so a continuation that exists
/// in HEAD but is gone or re-headed in a metadata-only local drift cannot have
/// been legitimately consumed.
pub fn metadata_drift_authority(local: &str, head: &str) -> MetadataDriftAuthority {
    let local_head = agent_doc_queue::queue_continuation::live_continuation_head(local);
    let head_head = agent_doc_queue::queue_continuation::live_continuation_head(head);
    match (local_head, head_head) {
        // HEAD carries a live continuation that the local side dropped entirely
        // (deactivated / drained / fenced) with no consuming response.
        (None, Some(_)) => MetadataDriftAuthority::Head,
        // Both sides carry a live continuation but with different ready heads,
        // and content equality proves no response consumed the old head.
        (Some(local_id), Some(head_id)) if local_id != head_id => MetadataDriftAuthority::Ambiguous,
        // Same live head, HEAD has no live continuation at risk, or neither side
        // does.
        _ => MetadataDriftAuthority::Local,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_drift_authority_head_when_local_drops_live_continuation() {
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "<!-- agent:queue -->\n- do [#a]\n- do [#b]\n<!-- /agent:queue -->\n",
        );
        let local = head.replace("queue_active: true", "queue_active: false");
        assert_eq!(
            metadata_drift_authority(&local, head),
            MetadataDriftAuthority::Head
        );
    }

    #[test]
    fn metadata_drift_authority_local_when_no_live_head_continuation() {
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let local = head.replace("- do [#a]\n", "- do [#a]\n- do [#b]\n");
        assert_eq!(
            metadata_drift_authority(&local, head),
            MetadataDriftAuthority::Local
        );
    }

    #[test]
    fn metadata_drift_authority_ambiguous_when_live_heads_diverge() {
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let local = head.replace("- do [#a]", "- do [#z]");
        assert_eq!(
            metadata_drift_authority(&local, head),
            MetadataDriftAuthority::Ambiguous
        );
    }
}
