//! Pure queue journal serialization and replay policy.
//!
//! This module owns content-only decisions for the crash-durable queue journal:
//! extracting prompts from a document, planning which prompts still need journal
//! entries, detecting prompts already represented by a queue, and merging
//! replayed entries back into queue content. Callers own filesystem paths,
//! durable reads/writes, and sidecar lifecycle.

use std::collections::HashSet;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::document_queue::{self, QueueEntry, QueuePrompt};

/// One journaled operator queue prompt. `text` is the canonical prompt text as
/// parsed by [`document_queue::parse`] (the `- ` bullet / fence markers stripped),
/// so it round-trips through [`QueuePrompt`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueJournalEntry {
    /// Canonical prompt text (matches [`QueuePrompt::text`]).
    pub text: String,
    /// Whether the prompt is a multiline (`--- ... ---`) queue entry.
    #[serde(default)]
    pub multiline: bool,
}

/// Parse the queue prompts (`Prompt` entries only, never `Completed`, presets,
/// dispatches, fences, or freeform noise) from a document's `agent:queue`
/// component. Returns an empty vec when there is no queue component.
pub fn queue_prompts(content: &str) -> Vec<QueuePrompt> {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return Vec::new();
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return Vec::new();
    };
    let body = &content[queue.open_end..queue.close_start];
    let Ok(entries) = document_queue::parse(body) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter_map(|entry| match entry {
            QueueEntry::Prompt(prompt) => Some(prompt),
            _ => None,
        })
        .collect()
}

/// Plan the journal entries still missing from an append-only journal.
///
/// Deduplication is by canonical prompt text: a prompt already present in the
/// journal, or repeated earlier in the same batch, is skipped.
pub fn plan_append_entries(
    existing: &[QueueJournalEntry],
    prompts: impl IntoIterator<Item = QueuePrompt>,
) -> Vec<QueueJournalEntry> {
    let mut seen: HashSet<String> = existing.iter().map(|entry| entry.text.clone()).collect();
    let mut planned = Vec::new();
    for prompt in prompts {
        if !seen.insert(prompt.text.clone()) {
            continue;
        }
        planned.push(QueueJournalEntry {
            text: prompt.text,
            multiline: prompt.multiline,
        });
    }
    planned
}

/// Return journal entries that are absent from the supplied live/durable queue
/// text set.
pub fn missing_entries(
    journal: impl IntoIterator<Item = QueueJournalEntry>,
    present_texts: &HashSet<String>,
) -> Vec<QueueJournalEntry> {
    journal
        .into_iter()
        .filter(|entry| !present_texts.contains(&entry.text))
        .collect()
}

/// All queue prompt texts currently represented in `content`, live `Prompt` AND
/// struck `Completed`, so a consumed/cancelled item is treated as present and is
/// never resurrected by replay.
pub fn present_queue_texts(content: &str) -> HashSet<String> {
    let mut texts = HashSet::new();
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return texts;
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return texts;
    };
    let body = &content[queue.open_end..queue.close_start];
    let Ok(entries) = document_queue::parse(body) else {
        return texts;
    };
    for entry in entries {
        match entry {
            QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => {
                texts.insert(prompt.text);
            }
            _ => {}
        }
    }
    texts
}

/// Re-insert lost operator queue prompts at the end of `content`'s `agent:queue`
/// component, returning the rewritten content.
///
/// Returns `Ok(None)` when nothing is missing or there is no queue component to
/// merge into. This is conservative: replay never fabricates a queue component.
pub fn merge_missing_into_content(
    missing: &[QueueJournalEntry],
    content: &str,
) -> Result<Option<String>> {
    if missing.is_empty() {
        return Ok(None);
    }
    let components = agent_doc_element::element::parse(content)?;
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return Ok(None);
    };
    let body = &content[queue.open_end..queue.close_start];
    let mut entries =
        document_queue::parse(body).context("queue_journal: failed to parse queue")?;
    let present = present_queue_texts(content);
    let mut added = false;
    for entry in missing {
        if present.contains(&entry.text) {
            continue;
        }
        entries.push(QueueEntry::Prompt(QueuePrompt {
            text: entry.text.clone(),
            multiline: entry.multiline,
        }));
        added = true;
    }
    if !added {
        return Ok(None);
    }
    let rendered = document_queue::render(&entries);
    let mut new_content = String::with_capacity(content.len() + rendered.len());
    new_content.push_str(&content[..queue.open_end]);
    if !rendered.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str(&rendered);
    new_content.push_str(&content[queue.close_start..]);
    Ok(Some(new_content))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_body(queue_lines: &[&str]) -> String {
        let mut body = String::from(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n## Queue\n\n<!-- agent:queue auto -->\n",
        );
        for line in queue_lines {
            body.push_str(line);
            body.push('\n');
        }
        body.push_str("<!-- /agent:queue -->\n");
        body
    }

    #[test]
    fn queue_prompts_returns_live_prompts_only() {
        let content = doc_body(&[
            "- do [#alpha]",
            "- ~~done [#beta]~~",
            "preset daily",
            "- do [#gamma]",
        ]);

        let prompts = queue_prompts(&content);

        assert_eq!(
            prompts
                .iter()
                .map(|prompt| prompt.text.as_str())
                .collect::<Vec<_>>(),
            vec!["do [#alpha]", "do [#gamma]"]
        );
    }

    #[test]
    fn present_queue_texts_includes_live_and_completed_prompts() {
        let content = doc_body(&["- do [#alpha]", "- ~~done [#beta]~~"]);

        let texts = present_queue_texts(&content);

        assert!(texts.contains("do [#alpha]"));
        assert!(texts.contains("done [#beta]"));
    }

    #[test]
    fn plan_append_entries_skips_existing_and_batch_duplicates() {
        let existing = vec![QueueJournalEntry {
            text: "do [#alpha]".to_string(),
            multiline: false,
        }];
        let prompts = vec![
            QueuePrompt {
                text: "do [#alpha]".to_string(),
                multiline: false,
            },
            QueuePrompt {
                text: "do [#beta]".to_string(),
                multiline: false,
            },
            QueuePrompt {
                text: "do [#beta]".to_string(),
                multiline: true,
            },
        ];

        let planned = plan_append_entries(&existing, prompts);

        assert_eq!(
            planned,
            vec![QueueJournalEntry {
                text: "do [#beta]".to_string(),
                multiline: false,
            }]
        );
    }

    #[test]
    fn missing_entries_filters_present_texts() {
        let journal = vec![
            QueueJournalEntry {
                text: "do [#alpha]".to_string(),
                multiline: false,
            },
            QueueJournalEntry {
                text: "do [#beta]".to_string(),
                multiline: false,
            },
        ];
        let present = HashSet::from(["do [#alpha]".to_string()]);

        let missing = missing_entries(journal, &present);

        assert_eq!(
            missing,
            vec![QueueJournalEntry {
                text: "do [#beta]".to_string(),
                multiline: false,
            }]
        );
    }

    #[test]
    fn merge_missing_into_content_appends_absent_prompts() {
        let content = doc_body(&["- do [#alpha]"]);
        let missing = vec![
            QueueJournalEntry {
                text: "do [#alpha]".to_string(),
                multiline: false,
            },
            QueueJournalEntry {
                text: "do [#beta]".to_string(),
                multiline: false,
            },
        ];

        let merged = merge_missing_into_content(&missing, &content)
            .unwrap()
            .expect("one absent prompt should be merged");

        assert!(merged.contains("- do [#alpha]"));
        assert!(merged.contains("- do [#beta]"));
        assert_eq!(merged.matches("- do [#alpha]").count(), 1);
    }

    #[test]
    fn merge_missing_into_content_is_noop_without_queue_component() {
        let missing = vec![QueueJournalEntry {
            text: "do [#alpha]".to_string(),
            multiline: false,
        }];

        assert!(
            merge_missing_into_content(&missing, "no components here")
                .unwrap()
                .is_none()
        );
    }
}
