//! `#strayclosemarker` commit-gate regression tests.

use super::guard_committable_document_content;
use std::path::Path;

/// `#strayclosemarker`: the 2026-11-04 fpe.md refusal was
/// `commit refused structurally corrupt document ... at initial_authority
/// (non_standalone_component_marker:/agent:run:close:line48)`. A stray
/// keystroke before a closing marker must reach git at every commit stage
/// (the document commit and the parent-pointer commit both pass here).
#[test]
fn commit_accepts_stray_text_before_a_close_marker() {
    let document = concat!(
        "<!-- agent:exchange -->\n",
        "### Re: prior — gpt-5\n",
        "body\n",
        "<!-- /agent:exchange -->\n\n",
        "## fpe-prs <!-- agent:run id=\"fpe-prs\" subagents priority go -->\n",
        "- https://github.com/haiven-dev/fpe/pull/217\n",
        "  - \u{1f4e5} #adversarial-pr\n",
        "t<!-- /agent:run -->\n",
    );
    for stage in ["initial_authority", "pre_stage"] {
        guard_committable_document_content(Path::new("/tmp/fpe.md"), document, stage)
            .unwrap_or_else(|error| panic!("stage {stage} must commit: {error:#}"));
    }
}
