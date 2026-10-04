//! Identity of the generated Agent Doc dashboard projection (`gvqv`).
//!
//! `agent-doc dashboard --write` and the project controller render a read-only
//! markdown view of the fleet board plus controller/supervisor liveness into
//! `.agent-doc/dashboard.md`. That file is a projection, never a session
//! document: it must not be opted in by `[documents] include` globs or the
//! `auto_session_for_all_md` escape hatch, must not be scanned by the fleet
//! board or `serve`, and must never be committed by the cross-document sweep.
//!
//! The first line of every rendered projection is [`DASHBOARD_MARKER_PREFIX`]
//! followed by its render parameters, so every classifier can recognise the
//! file from its content alone, wherever `--write` put it.

/// Project-relative default location. `.agent-doc/` is gitignored and every
/// session scanner skips hidden directories.
pub const DASHBOARD_DEFAULT_RELATIVE_PATH: &str = ".agent-doc/dashboard.md";

/// First-line marker of a rendered dashboard projection. Deliberately NOT an
/// `<!-- agent:` component marker, which would make every editor adapter
/// classify the projection as a session document.
pub const DASHBOARD_MARKER_PREFIX: &str = "<!-- agent-doc-dashboard ";

/// True when `content` is a generated dashboard projection.
pub fn is_dashboard_projection(content: &str) -> bool {
    content
        .trim_start_matches('\u{feff}')
        .starts_with(DASHBOARD_MARKER_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_projection_is_recognised_by_its_first_line_only() {
        assert!(is_dashboard_projection(
            "<!-- agent-doc-dashboard v1 scope=project -->\n# Agent Doc dashboard\n"
        ));
        assert!(!is_dashboard_projection(
            "# notes\n<!-- agent-doc-dashboard v1 scope=project -->\n"
        ));
        assert!(!is_dashboard_projection("<!-- agent:exchange -->\n"));
    }
}
