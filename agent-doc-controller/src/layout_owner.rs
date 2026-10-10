//! Which controller owns a shared tmux main-window layout (`#crossrootcolumnflip`).
//!
//! A superproject (`agent-loop`) and a nested project root with its own
//! `.agent-doc/` (`src/haiven-dev`) each run a project controller. When the
//! superproject's IDE shows a superproject document beside a submodule
//! document, both documents' panes live in ONE tmux `agent-doc` window, but
//! each controller kept its own desired-layout source and its own generation
//! counter. An editor route for the submodule document published its layout
//! on the submodule controller (gen 21, `[agent-doc-bugs, contracts]`) while
//! the superproject controller's editor-surface churn published gens 32-36
//! ending `[agent-doc-bugs, api.md]`. Two arbiters projecting into one window
//! means "whichever projects last wins", so a column could flip between
//! documents after an IDE restart and never converge to what the editor shows.
//!
//! The rule this module owns: a layout's main window is owned by the
//! controller of the **outermost project root that contains the publishing
//! controller and any column's document**. A nested controller whose layout
//! names a document from an enclosing root does not publish that layout; it
//! re-addresses the publication to the enclosing controller, which merges it
//! into its single generation sequence beside its own editor-surface
//! publications. Within one arbiter the later editor observation wins, so
//! post-restart churn converges to the editor's real visible set.
//!
//! Resolving each document's project root and talking to the other controller
//! are the adapter's job.

use std::path::Path;

/// Who publishes a layout into the shared main window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutOwner<'a> {
    /// This controller owns the window for this layout; publish locally.
    Local,
    /// An enclosing project root owns the window; re-address the publication.
    Delegate(&'a Path),
}

/// Decide who publishes a layout whose column documents resolve to
/// `column_roots` (`None` = the document's project root is unknown), on a
/// controller rooted at `controller_root`.
///
/// Only a **strictly enclosing** root triggers delegation, and the outermost
/// one wins, so the decision terminates on the owner (where no column root
/// encloses it). A column from a nested root (the superproject showing a
/// submodule document) stays local: the enclosing controller already owns
/// that window. A sibling root has no containment order and stays local, and
/// an unknown root is never delegated — without proof of an enclosing owner the
/// local publication is still the honest one.
pub fn main_window_layout_owner<'a>(
    controller_root: &Path,
    column_roots: impl IntoIterator<Item = Option<&'a Path>>,
) -> LayoutOwner<'a> {
    column_roots
        .into_iter()
        .flatten()
        .filter(|root| *root != controller_root && controller_root.starts_with(root))
        .min_by_key(|root| root.components().count())
        .map_or(LayoutOwner::Local, LayoutOwner::Delegate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn superproject() -> PathBuf {
        PathBuf::from("/w/agent-loop")
    }

    fn submodule() -> PathBuf {
        PathBuf::from("/w/agent-loop/src/haiven-dev")
    }

    #[test]
    fn a_nested_controller_delegates_a_layout_naming_a_superproject_document() {
        // The observed gen-21 publication: [agent-doc-bugs (agent-loop),
        // contracts (haiven-dev)] published by the haiven-dev controller.
        let superproject = superproject();
        let submodule = submodule();
        assert_eq!(
            main_window_layout_owner(
                &submodule,
                [Some(superproject.as_path()), Some(submodule.as_path())]
            ),
            LayoutOwner::Delegate(superproject.as_path()),
        );
    }

    #[test]
    fn the_superproject_controller_owns_a_layout_naming_submodule_documents() {
        let superproject = superproject();
        let submodule = submodule();
        assert_eq!(
            main_window_layout_owner(
                &superproject,
                [Some(superproject.as_path()), Some(submodule.as_path())]
            ),
            LayoutOwner::Local,
            "delegation must terminate on the owner"
        );
    }

    #[test]
    fn a_layout_entirely_inside_the_nested_root_stays_local() {
        let submodule = submodule();
        assert_eq!(
            main_window_layout_owner(&submodule, [Some(submodule.as_path())]),
            LayoutOwner::Local,
        );
    }

    #[test]
    fn the_outermost_enclosing_root_wins() {
        let outer = PathBuf::from("/w");
        let middle = superproject();
        let inner = submodule();
        assert_eq!(
            main_window_layout_owner(&inner, [Some(middle.as_path()), Some(outer.as_path())]),
            LayoutOwner::Delegate(outer.as_path()),
        );
    }

    #[test]
    fn sibling_and_unknown_roots_never_delegate() {
        let submodule = submodule();
        let sibling = PathBuf::from("/w/agent-loop/src/tsift");
        let prefix_lookalike = PathBuf::from("/w/agent-loop/src/haiven");
        assert_eq!(
            main_window_layout_owner(
                &submodule,
                [
                    Some(sibling.as_path()),
                    Some(prefix_lookalike.as_path()),
                    None
                ]
            ),
            LayoutOwner::Local,
            "only a path-component ancestor encloses the controller"
        );
    }
}
