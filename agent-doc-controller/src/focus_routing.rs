//! Which controller owns an editor focus request (`#focuscrossroot`).
//!
//! A project controller answers focus from its own actor store, keyed on its
//! own `project_root`. An IDE whose project is the superproject publishes one
//! editor surface per IDE process, so selecting a tab that lives in a submodule
//! with its own `.agent-doc/` sends that document's focus intent to the
//! *superproject's* controller. That controller has no actor row for it — the
//! row is in the submodule's `state.db` — so it answered `missing_actor_record`
//! and nothing focused, while the submodule's own controller (holding a live,
//! correct binding) was never asked.
//!
//! `missing_actor_record` is the right answer for a document this controller
//! owns and has no actor for. It is the wrong answer for a document this
//! controller does not own: the request is simply addressed to the wrong
//! controller, and the fix is to re-address it rather than to refuse.
//!
//! This module owns only that decision. Resolving a document's project root and
//! talking to the other controller are the adapter's job.

use std::path::Path;

/// Where a focus request should actually be answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusOwner<'a> {
    /// This controller owns the document; answer it locally.
    Local,
    /// Another project root owns the document; re-address the request there.
    Delegate(&'a Path),
}

/// Decide who answers a focus request for `document_root` (the agent-doc project
/// root the document itself resolves to) on a controller rooted at
/// `controller_root`.
///
/// An unknown document root is never delegated: with no proof that some other
/// controller owns the document, the local answer — including a local
/// `missing_actor_record` — is still the honest one.
pub fn focus_owner<'a>(
    controller_root: &Path,
    document_root: Option<&'a Path>,
) -> FocusOwner<'a> {
    match document_root {
        Some(root) if root != controller_root => FocusOwner::Delegate(root),
        _ => FocusOwner::Local,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_document_under_this_controller_is_answered_locally() {
        let root = PathBuf::from("/w/agent-loop");
        assert_eq!(focus_owner(&root, Some(&root)), FocusOwner::Local);
    }

    #[test]
    fn a_submodule_document_delegates_to_the_submodule_controller() {
        let controller = PathBuf::from("/w/agent-loop");
        let submodule = PathBuf::from("/w/agent-loop/src/haiven-dev");
        assert_eq!(
            focus_owner(&controller, Some(&submodule)),
            FocusOwner::Delegate(submodule.as_path()),
            "the superproject controller has no actor row for a submodule document"
        );
    }

    #[test]
    fn an_unknown_document_root_stays_local() {
        let controller = PathBuf::from("/w/agent-loop");
        assert_eq!(
            focus_owner(&controller, None),
            FocusOwner::Local,
            "without proof of another owner, the local answer is the honest one"
        );
    }

    #[test]
    fn delegation_is_not_symmetric_back_to_the_superproject() {
        // The delegated call re-enters the decision on the submodule's own
        // controller, where the roots now match — so it terminates there.
        let submodule = PathBuf::from("/w/agent-loop/src/haiven-dev");
        assert_eq!(focus_owner(&submodule, Some(&submodule)), FocusOwner::Local);
    }
}
