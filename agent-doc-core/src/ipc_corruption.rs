//! # Module: ipc_corruption
//!
//! ## Spec
//! Forensic detector for the `#ipcfullprompt` corruption shape: a full-document
//! editor-side IPC mutation (e.g. `PatchWatcher.setText`) that **deletes** or
//! **duplicates** a previously-committed assistant `### Re:` response block while
//! the user is typing a live prompt. The binary's IPC snapshot-adoption guards
//! fail closed against the *snapshot*, but the editor-visible buffer can still be
//! corrupted; this detector turns each occurrence into a durable forensic signal
//! (default-on capture) without changing any mutation behavior.
//!
//! - [`detect_response_block_corruption`] compares a `prior` document (the
//!   committed baseline) against a `current` document (the live editor buffer /
//!   candidate) and returns one finding per prior `### Re:` heading that is now
//!   absent (deleted) or appears more times than it did in `prior` (duplicated).
//! - [`summarize_findings`] renders findings as a compact, single-line,
//!   ops.log-friendly string.
//!
//! ## Agentic Contracts
//! - **Detection only.** This module never mutates a document or changes an
//!   adoption decision. A finding is a forensic log signal, not a block.
//! - Only headings that were present in `prior` are considered, so a brand-new
//!   response added this cycle (present in `current`, absent from `prior`) is
//!   never flagged — that is expected growth, not corruption.
//! - Heading identity ignores the working-tree-only ` (HEAD)` annotation and
//!   leading/trailing whitespace, matching how the rest of the binary treats the
//!   `(HEAD)` boundary artifact.
//!
//! ## Evals
//! - `detect_flags_deleted_prior_response`
//! - `detect_flags_duplicated_prior_response`
//! - `detect_ignores_new_response_block`
//! - `detect_ignores_head_annotation_difference`
//! - `detect_clean_document_has_no_findings`

use std::collections::BTreeMap;

/// The kind of `#ipcfullprompt` corruption detected for a response heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcCorruptionKind {
    /// A `### Re:` block present in `prior` is entirely absent from `current`.
    ResponseDeleted,
    /// A `### Re:` block appears more times in `current` than in `prior`.
    ResponseDuplicated,
}

impl IpcCorruptionKind {
    /// Stable label for ops.log lines.
    pub fn label(&self) -> &'static str {
        match self {
            IpcCorruptionKind::ResponseDeleted => "response_deleted",
            IpcCorruptionKind::ResponseDuplicated => "response_duplicated",
        }
    }
}

/// One detected corruption of a previously-committed response block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpcCorruptionFinding {
    pub kind: IpcCorruptionKind,
    /// The normalized `### Re:` heading text (no `(HEAD)` suffix, trimmed).
    pub heading: String,
    /// Occurrences in the prior/baseline document.
    pub prior_count: usize,
    /// Occurrences in the current/candidate document.
    pub current_count: usize,
}

/// Normalize a line to its `### Re:` heading identity, or `None` if it is not a
/// response heading. Strips the working-tree-only ` (HEAD)` annotation so a
/// freshly-annotated heading is not mistaken for a different block.
fn response_heading_identity(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with("### Re:") {
        return None;
    }
    let without_head = trimmed.strip_suffix(" (HEAD)").unwrap_or(trimmed);
    Some(without_head.trim_end().to_string())
}

/// Count `### Re:` heading occurrences per normalized identity in `doc`.
fn response_heading_counts(doc: &str) -> BTreeMap<String, usize> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for line in doc.lines() {
        if let Some(id) = response_heading_identity(line) {
            *counts.entry(id).or_insert(0) += 1;
        }
    }
    counts
}

/// Detect the `#ipcfullprompt` corruption shape by comparing the committed
/// `prior` document against the live `current` (candidate) document.
///
/// Returns one finding per prior `### Re:` heading whose count dropped to zero
/// (deleted) or rose above its prior count (duplicated). Headings that only
/// appear in `current` are ignored — that is the expected new response, not
/// corruption. Deterministic and side-effect free.
pub fn detect_response_block_corruption(prior: &str, current: &str) -> Vec<IpcCorruptionFinding> {
    let prior_counts = response_heading_counts(prior);
    if prior_counts.is_empty() {
        return Vec::new();
    }
    let current_counts = response_heading_counts(current);

    let mut findings = Vec::new();
    for (heading, &prior_count) in &prior_counts {
        let current_count = current_counts.get(heading).copied().unwrap_or(0);
        if current_count == 0 {
            findings.push(IpcCorruptionFinding {
                kind: IpcCorruptionKind::ResponseDeleted,
                heading: heading.clone(),
                prior_count,
                current_count,
            });
        } else if current_count > prior_count {
            findings.push(IpcCorruptionFinding {
                kind: IpcCorruptionKind::ResponseDuplicated,
                heading: heading.clone(),
                prior_count,
                current_count,
            });
        }
    }
    findings
}

/// Render findings as a compact single-line summary for ops.log. Truncates each
/// heading so a long topic cannot blow up the log line.
pub fn summarize_findings(findings: &[IpcCorruptionFinding]) -> String {
    let deleted = findings
        .iter()
        .filter(|f| f.kind == IpcCorruptionKind::ResponseDeleted)
        .count();
    let duplicated = findings
        .iter()
        .filter(|f| f.kind == IpcCorruptionKind::ResponseDuplicated)
        .count();
    let details: Vec<String> = findings
        .iter()
        .map(|f| {
            let mut h = f.heading.clone();
            if h.len() > 80 {
                h.truncate(80);
            }
            format!(
                "{}({}:{}->{})",
                f.kind.label(),
                h,
                f.prior_count,
                f.current_count
            )
        })
        .collect();
    format!(
        "deleted={} duplicated={} details=[{}]",
        deleted,
        duplicated,
        details.join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIOR: &str = concat!(
        "<!-- agent:exchange -->\n",
        "### Re: first topic — opus-4-8\n",
        "First answer.\n",
        "### Re: second topic — opus-4-8\n",
        "Second answer.\n",
        "<!-- /agent:exchange -->\n",
    );

    #[test]
    fn detect_flags_deleted_prior_response() {
        // The second response block is gone from the live buffer.
        let current = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: first topic — opus-4-8\n",
            "First answer.\n",
            "<!-- /agent:exchange -->\n",
        );
        let findings = detect_response_block_corruption(PRIOR, current);
        assert_eq!(findings.len(), 1, "exactly one deletion: {findings:?}");
        assert_eq!(findings[0].kind, IpcCorruptionKind::ResponseDeleted);
        assert_eq!(findings[0].heading, "### Re: second topic — opus-4-8");
        assert_eq!(findings[0].prior_count, 1);
        assert_eq!(findings[0].current_count, 0);
    }

    #[test]
    fn detect_flags_duplicated_prior_response() {
        // The first response block appears twice — the full-tail duplication shape.
        let current = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: first topic — opus-4-8\n",
            "First answer.\n",
            "### Re: first topic — opus-4-8\n",
            "First answer.\n",
            "### Re: second topic — opus-4-8\n",
            "Second answer.\n",
            "<!-- /agent:exchange -->\n",
        );
        let findings = detect_response_block_corruption(PRIOR, current);
        assert_eq!(findings.len(), 1, "exactly one duplication: {findings:?}");
        assert_eq!(findings[0].kind, IpcCorruptionKind::ResponseDuplicated);
        assert_eq!(findings[0].heading, "### Re: first topic — opus-4-8");
        assert_eq!(findings[0].prior_count, 1);
        assert_eq!(findings[0].current_count, 2);
    }

    #[test]
    fn detect_ignores_new_response_block() {
        // A brand-new response added this cycle is expected growth, not corruption.
        let current = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: first topic — opus-4-8\n",
            "First answer.\n",
            "### Re: second topic — opus-4-8\n",
            "Second answer.\n",
            "### Re: third topic — opus-4-8\n",
            "Third answer.\n",
            "<!-- /agent:exchange -->\n",
        );
        assert!(
            detect_response_block_corruption(PRIOR, current).is_empty(),
            "new response heading must not be flagged"
        );
    }

    #[test]
    fn detect_ignores_head_annotation_difference() {
        // The live buffer marks the newest heading with `(HEAD)`; identity must
        // ignore it so no spurious delete+add pair is reported.
        let current = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: first topic — opus-4-8\n",
            "First answer.\n",
            "### Re: second topic — opus-4-8 (HEAD)\n",
            "Second answer.\n",
            "<!-- /agent:exchange -->\n",
        );
        assert!(
            detect_response_block_corruption(PRIOR, current).is_empty(),
            "(HEAD) annotation must not read as corruption"
        );
    }

    #[test]
    fn detect_clean_document_has_no_findings() {
        assert!(detect_response_block_corruption(PRIOR, PRIOR).is_empty());
    }

    #[test]
    fn detect_no_prior_headings_is_empty() {
        // No prior response blocks → nothing to protect, no findings.
        let prior = "<!-- agent:exchange -->\njust a prompt\n<!-- /agent:exchange -->\n";
        let current = "<!-- agent:exchange -->\n### Re: x — opus-4-8\nhi\n<!-- /agent:exchange -->\n";
        assert!(detect_response_block_corruption(prior, current).is_empty());
    }

    #[test]
    fn summarize_is_compact_and_truncates() {
        let findings = vec![
            IpcCorruptionFinding {
                kind: IpcCorruptionKind::ResponseDeleted,
                heading: "### Re: a".to_string(),
                prior_count: 1,
                current_count: 0,
            },
            IpcCorruptionFinding {
                kind: IpcCorruptionKind::ResponseDuplicated,
                heading: "### Re: b".to_string(),
                prior_count: 1,
                current_count: 3,
            },
        ];
        let s = summarize_findings(&findings);
        assert!(s.contains("deleted=1"), "{s}");
        assert!(s.contains("duplicated=1"), "{s}");
        assert!(s.contains("response_deleted(### Re: a:1->0)"), "{s}");
        assert!(s.contains("response_duplicated(### Re: b:1->3)"), "{s}");
        assert!(!s.contains('\n'), "summary must be single-line: {s}");
    }
}
