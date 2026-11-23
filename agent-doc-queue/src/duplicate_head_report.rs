//! A report, never a rewrite, of top-level queue rows that repeat more often
//! than the committed document holds them.
//!
//! A retained rebase that paired the operator's queue rows with an older base
//! repeated whole PR heads of a tracking queue (`- ### https://…/pull/380` six
//! times), each copy with children that later diverged. A byte-identical copy
//! is collapsed by self-repair ([`crate::self_repair::dedupe_live_rows`]); a
//! copy whose children differ cannot be told apart from an operator's own
//! repeated entry (the operator's last known-good queue repeats some heads on
//! purpose), and no per-row writer provenance survives the merge. So session
//! check names each suspected copy, with its line and the line of its first
//! occurrence, for the operator to delete in the editor.
//!
//! A head is suspected when the live queue component holds it more often than
//! the same component (by occurrence index) does in the committed document.
//! The first `committed` occurrences are presumed originals; every later one
//! is reported. Struck rows and blank rows are ignored.

use agent_doc_element::element;

/// One suspected copy of a top-level queue row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuspectedDuplicateHead {
    /// The row text after its `- ` bullet, trailing whitespace trimmed.
    pub head: String,
    /// 1-based document line of the suspected copy.
    pub line: usize,
    /// 1-based document line of the head's first occurrence.
    pub first_line: usize,
    /// How many copies the live component holds.
    pub live_copies: usize,
    /// How many copies the committed component holds.
    pub committed_copies: usize,
}

/// `(head, 1-based line)` of every top-level live row of each queue component
/// body, by component occurrence.
fn top_level_heads(document: &str) -> Option<Vec<Vec<(String, usize)>>> {
    let components = element::parse(document).ok()?;
    let mut out = Vec::new();
    for queue in components
        .iter()
        .filter(|component| component.name == "queue")
    {
        let first_line = document[..queue.open_end].matches('\n').count() + 1;
        let mut heads = Vec::new();
        let mut fence = false;
        for (offset, line) in queue.content(document).lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                fence = !fence;
                continue;
            }
            if fence || line.starts_with([' ', '\t']) {
                continue;
            }
            let Some(item) = line.strip_prefix("- ").map(str::trim_end) else {
                continue;
            };
            let bare = item.strip_prefix("### ").unwrap_or(item);
            if item.is_empty() || bare.starts_with("~~") {
                continue;
            }
            heads.push((item.to_string(), first_line + offset));
        }
        out.push(heads);
    }
    Some(out)
}

/// Every top-level queue row of `document` beyond its committed copy count.
/// Empty when either document does not parse or their queue components
/// differ in number.
pub fn suspected_duplicate_queue_heads(
    document: &str,
    committed: &str,
) -> Vec<SuspectedDuplicateHead> {
    let (Some(live), Some(base)) = (top_level_heads(document), top_level_heads(committed)) else {
        return Vec::new();
    };
    if live.len() != base.len() {
        return Vec::new();
    }
    let mut suspected = Vec::new();
    for (live, base) in live.iter().zip(&base) {
        let mut order: Vec<&str> = Vec::new();
        let mut lines: std::collections::HashMap<&str, Vec<usize>> =
            std::collections::HashMap::new();
        for (head, line) in live {
            let entry = lines.entry(head.as_str()).or_default();
            if entry.is_empty() {
                order.push(head.as_str());
            }
            entry.push(*line);
        }
        for head in order {
            let copies = &lines[head];
            let committed_copies = base.iter().filter(|(text, _)| text == head).count();
            if copies.len() <= committed_copies.max(1) {
                continue;
            }
            for line in &copies[committed_copies.max(1)..] {
                suspected.push(SuspectedDuplicateHead {
                    head: head.to_string(),
                    line: *line,
                    first_line: copies[0],
                    live_copies: copies.len(),
                    committed_copies,
                });
            }
        }
    }
    suspected.sort_by_key(|head| head.line);
    suspected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(queue: &str) -> String {
        format!(
            "---\nagent_doc_format: template\n---\n\n## PRs <!-- agent:run id=\"prs\" subagents -->\n{queue}<!-- /agent:run -->\n\n## Exchange <!-- agent:exchange -->\n<!-- /agent:exchange -->\n"
        )
    }

    const PR377: &str =
        "- ### 🔒 https://github.com/acme/widget-sdk/pull/377\n  - %% docs(sdk): quickstarts %%\n";
    const PR378: &str =
        "- ### https://github.com/acme/widget-sdk/pull/378\n  - %% feat(sdk): lifecycle %%\n";
    const PR380: &str = "- ### https://github.com/acme/widget-sdk/pull/380\n  - %% test(conformance): lifecycle %%\n";

    /// The committed queue repeats `pull/377` and `pull/378` on purpose; the
    /// live queue repeats the run of heads with diverged children. Only the
    /// copies beyond each head's committed count are reported.
    #[test]
    fn heads_beyond_the_committed_count_are_reported_with_their_first_line() {
        let committed = doc(&format!("{PR377}{PR378}{PR380}{PR377}{PR378}"));
        let live = doc(&format!(
            "{PR377}{PR378}{PR380}  - 🚧 [#subagent:rebase380#1] #rebase\n{PR377}  - 📥 #rebase\n{PR378}{PR380}  - 📥 squash and me\n{PR378}{PR380}- ~~https://github.com/acme/widget-sdk/pull/380~~\n"
        ));
        let suspected = suspected_duplicate_queue_heads(&live, &committed);
        let summary: Vec<(&str, usize, usize, usize, usize)> = suspected
            .iter()
            .map(|head| {
                (
                    head.head.as_str(),
                    head.line,
                    head.first_line,
                    head.live_copies,
                    head.committed_copies,
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (
                    "### https://github.com/acme/widget-sdk/pull/380",
                    18,
                    10,
                    3,
                    1
                ),
                (
                    "### https://github.com/acme/widget-sdk/pull/378",
                    21,
                    8,
                    3,
                    2
                ),
                (
                    "### https://github.com/acme/widget-sdk/pull/380",
                    23,
                    10,
                    3,
                    1
                ),
            ],
            "{live}"
        );
        assert!(suspected_duplicate_queue_heads(&committed, &committed).is_empty());
    }

    /// A head the committed document lacks is reported only from its second
    /// copy on; a document without the same queue layout reports nothing.
    #[test]
    fn a_new_head_is_reported_from_its_second_copy_and_a_layout_change_reports_nothing() {
        let committed = doc(PR377);
        let live = doc(&format!("{PR377}{PR380}{PR380}"));
        let suspected = suspected_duplicate_queue_heads(&live, &committed);
        assert_eq!(suspected.len(), 1, "{suspected:?}");
        assert_eq!(suspected[0].committed_copies, 0);
        assert!(suspected_duplicate_queue_heads(&live, "no components\n").is_empty());
    }
}
