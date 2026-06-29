//! # Module: ipc_corruption
//!
//! Pure realtime-document corruption forensics.
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
    /// A canonical structural component marker (e.g. `<!-- /agent:exchange -->`)
    /// appears more than once — the full-tail duplication shape where the
    /// post-exchange scaffold (close marker + Queue/Backlog/Icebox) is copied.
    ScaffoldDuplicated,
}

impl IpcCorruptionKind {
    /// Stable label for ops.log lines.
    pub fn label(&self) -> &'static str {
        match self {
            IpcCorruptionKind::ResponseDeleted => "response_deleted",
            IpcCorruptionKind::ResponseDuplicated => "response_duplicated",
            IpcCorruptionKind::ScaffoldDuplicated => "scaffold_duplicated",
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

/// Canonical structural component markers that must appear at most once in a
/// healthy template document. More than one — on its own line — is the full-tail
/// duplication shape of `#ipcfullprompt`: the editor copied the post-exchange
/// scaffold (close marker + Queue/Backlog/Icebox) around a live, in-progress
/// prompt, producing two `<!-- /agent:exchange -->` markers.
const STRUCTURAL_MARKERS: &[&str] = &[
    "<!-- /agent:exchange -->",
    "<!-- agent:queue -->",
    "<!-- /agent:queue -->",
    "<!-- agent:backlog -->",
    "<!-- /agent:backlog -->",
    "<!-- agent:icebox -->",
    "<!-- /agent:icebox -->",
];

/// Detect a duplicated structural scaffold in `current` (a self-check; no prior
/// needed). Counts only lines that, when trimmed, exactly equal a canonical
/// structural marker — so an inline mention inside response prose does not
/// inflate the count — and returns one finding per marker that appears more than
/// once. This is the `#ipcfullprompt` full-tail-duplication signature that
/// `detect_response_block_corruption` (which keys on `### Re:` headings) misses.
pub fn detect_duplicated_scaffold(current: &str) -> Vec<IpcCorruptionFinding> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for line in current.lines() {
        let trimmed = line.trim();
        for marker in STRUCTURAL_MARKERS {
            if trimmed == *marker {
                *counts.entry(*marker).or_insert(0) += 1;
            }
        }
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(marker, count)| IpcCorruptionFinding {
            kind: IpcCorruptionKind::ScaffoldDuplicated,
            heading: marker.to_string(),
            prior_count: 1,
            current_count: count,
        })
        .collect()
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
    let scaffold = findings
        .iter()
        .filter(|f| f.kind == IpcCorruptionKind::ScaffoldDuplicated)
        .count();
    let details: Vec<String> = findings
        .iter()
        .map(|f| {
            let mut h = f.heading.clone();
            if h.len() > 80 {
                // Floor to the nearest char boundary so a multi-byte glyph
                // (headings carry `—`/`→`/Unicode) straddling byte 80 cannot
                // panic `String::truncate`. UTF-8-safe per the route/IPC
                // diagnostic-trimming invariant.
                let mut end = 80;
                while end > 0 && !h.is_char_boundary(end) {
                    end -= 1;
                }
                h.truncate(end);
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
        "deleted={} duplicated={} scaffold_duplicated={} details=[{}]",
        deleted,
        duplicated,
        scaffold,
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
        let current =
            "<!-- agent:exchange -->\n### Re: x — opus-4-8\nhi\n<!-- /agent:exchange -->\n";
        assert!(detect_response_block_corruption(prior, current).is_empty());
    }

    // #ipcfullprompt-recur2: the full-tail duplication shape captured live in
    // tasks/professional/brandon-cinquegrana.md 2026-05-29 — the editor copied the
    // post-exchange scaffold around an in-progress prompt, leaving two
    // `<!-- /agent:exchange -->` markers. `detect_response_block_corruption` misses
    // this (no `### Re:` was duplicated); `detect_duplicated_scaffold` must catch it.
    #[test]
    fn detect_duplicated_scaffold_flags_two_exchange_close_markers() {
        let corrupted = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: prior — opus-4-8\nAnswer.\n",
            "<!-- agent:boundary:709a41ae -->\n",
            "Is the issue still happening?\nCan it be re\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "## Queue\n<!-- agent:queue -->\n<!-- /agent:queue -->\n",
            "## Backlog\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
            "## Icebox\n<!-- agent:icebox -->\n<!-- /agent:icebox -->\n",
            "Can it be rep11ro\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "## Queue\n<!-- agent:queue -->\n<!-- /agent:queue -->\n",
        );
        let findings = detect_duplicated_scaffold(corrupted);
        assert!(
            findings
                .iter()
                .any(|f| f.kind == IpcCorruptionKind::ScaffoldDuplicated
                    && f.heading == "<!-- /agent:exchange -->"
                    && f.current_count == 2),
            "two exchange close markers must be flagged: {findings:?}"
        );
        // The duplicated queue scaffold is also flagged.
        assert!(
            findings
                .iter()
                .any(|f| f.heading == "<!-- agent:queue -->" && f.current_count == 2)
        );
    }

    #[test]
    fn detect_duplicated_scaffold_clean_doc_has_no_findings() {
        let clean = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: x — opus-4-8\nAnswer.\n",
            "<!-- /agent:exchange -->\n",
            "## Queue\n<!-- agent:queue -->\n<!-- /agent:queue -->\n",
            "## Backlog\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
        );
        assert!(detect_duplicated_scaffold(clean).is_empty());
    }

    #[test]
    fn detect_duplicated_scaffold_ignores_inline_marker_mentions() {
        // A marker mentioned inline inside prose (not on its own line) must not
        // inflate the count — only standalone marker lines are structural.
        let doc = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: x — opus-4-8\n",
            "The close marker is written `<!-- /agent:exchange -->` inline here.\n",
            "<!-- /agent:exchange -->\n",
        );
        assert!(
            detect_duplicated_scaffold(doc).is_empty(),
            "inline mention must not count as a structural duplicate"
        );
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

    #[test]
    fn summarize_findings_truncates_multibyte_heading_without_panic() {
        // Regression: a long heading whose multi-byte glyph (`—`, em dash, 3
        // bytes) straddles byte 80 used to panic `String::truncate` with
        // `assertion failed: self.is_char_boundary(new_len)` during the live
        // IPC-drift corruption summary, aborting `finalize` recovery.
        // "### Re: " (8 bytes) + 71*'x' (-> byte 79) + "—" (3 bytes, 79..82) so
        // byte 80 lands inside the em dash, the worst-case truncation boundary.
        let heading = format!("### Re: {}— opus-4-8", "x".repeat(71));
        assert!(heading.len() > 80);
        assert!(!heading.is_char_boundary(80));
        let findings = vec![IpcCorruptionFinding {
            kind: IpcCorruptionKind::ResponseDeleted,
            heading,
            prior_count: 1,
            current_count: 0,
        }];
        let s = summarize_findings(&findings);
        assert!(s.contains("deleted=1"), "{s}");
        assert!(!s.contains('\n'), "summary must be single-line: {s}");
    }
}
