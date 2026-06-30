//! Pure cross-document security-review policy for shared documents.

use crate::frontmatter::{CollaborationMode, Frontmatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityReviewSubject {
    Source,
    Target,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossDocumentSecurityReviewDecision {
    pub missing: Vec<SecurityReviewSubject>,
}

impl CrossDocumentSecurityReviewDecision {
    pub fn allowed() -> Self {
        Self {
            missing: Vec::new(),
        }
    }

    pub fn is_allowed(&self) -> bool {
        self.missing.is_empty()
    }
}

pub fn cross_document_security_review_decision(
    same_document: bool,
    source: &Frontmatter,
    target: Option<&Frontmatter>,
) -> CrossDocumentSecurityReviewDecision {
    if same_document {
        return CrossDocumentSecurityReviewDecision::allowed();
    }

    let mut missing = Vec::new();
    if source.collaboration_mode() == CollaborationMode::Shared && !source.has_security_review() {
        missing.push(SecurityReviewSubject::Source);
    }
    if let Some(target) = target
        && target.collaboration_mode() == CollaborationMode::Shared
        && !target.has_security_review()
    {
        missing.push(SecurityReviewSubject::Target);
    }

    CrossDocumentSecurityReviewDecision { missing }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared(review: Option<&str>) -> Frontmatter {
        Frontmatter {
            collaboration: Some(CollaborationMode::Shared),
            security_review: review.map(ToOwned::to_owned),
            ..Default::default()
        }
    }

    fn private() -> Frontmatter {
        Frontmatter::default()
    }

    #[test]
    fn same_document_bypasses_cross_document_review() {
        let decision = cross_document_security_review_decision(true, &shared(None), None);
        assert!(decision.is_allowed());
    }

    #[test]
    fn shared_source_missing_review_blocks() {
        let decision = cross_document_security_review_decision(false, &shared(None), None);
        assert_eq!(decision.missing, vec![SecurityReviewSubject::Source]);
    }

    #[test]
    fn shared_target_missing_review_blocks() {
        let decision =
            cross_document_security_review_decision(false, &private(), Some(&shared(None)));
        assert_eq!(decision.missing, vec![SecurityReviewSubject::Target]);
    }

    #[test]
    fn shared_source_and_target_missing_review_blocks_both() {
        let decision =
            cross_document_security_review_decision(false, &shared(None), Some(&shared(None)));
        assert_eq!(
            decision.missing,
            vec![SecurityReviewSubject::Source, SecurityReviewSubject::Target]
        );
    }

    #[test]
    fn private_or_default_documents_allow_without_review() {
        let decision = cross_document_security_review_decision(false, &private(), Some(&private()));
        assert!(decision.is_allowed());
    }

    #[test]
    fn non_empty_trimmed_review_allows_shared_document() {
        let decision =
            cross_document_security_review_decision(false, &shared(Some(" sec-2026 ")), None);
        assert!(decision.is_allowed());
    }

    #[test]
    fn blank_review_is_missing() {
        let decision = cross_document_security_review_decision(false, &shared(Some("  ")), None);
        assert_eq!(decision.missing, vec![SecurityReviewSubject::Source]);
    }
}
