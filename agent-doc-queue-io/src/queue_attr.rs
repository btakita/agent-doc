//! Exact-id queue opening-marker attribute mutation.
//!
//! The command is a read/plan/CAS-write transaction: resolution and validation
//! are pure, while [`edit_with_effects`] holds the queue mutation lease and
//! submits the exact source text to the existing editor/CRDT convergence path.

use std::collections::HashSet;
use std::path::Path;

use agent_doc_element::element;
use agent_doc_queue::queue_graph::QueueGraph;
use anyhow::{Context, Result, bail};

use crate::queue_cmd::QueueCommandEffects;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueAttrSet {
    pub key: String,
    pub value: Option<String>,
}

impl QueueAttrSet {
    pub fn parse(raw: &str) -> Result<Self> {
        let (key, value) = match raw.split_once('=') {
            Some((key, value)) => (key, Some(value.to_string())),
            None => (raw, None),
        };
        let key = key.trim();
        if key.is_empty() {
            bail!("queue attr: empty attribute name in `--set {raw}`");
        }
        Ok(Self {
            key: key.to_string(),
            value,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueAttrEditPlan {
    pub target_content: String,
    pub changed: bool,
    pub occurrence: usize,
}

fn validate_mutation_key(key: &str) -> Result<()> {
    if agent_doc_queue::component_attrs::is_queue_component_attr(key) {
        return Ok(());
    }
    bail!(
        "queue attr: `{key}` is not a recognized agent:queue attribute; refusing an attribute that would be silently ignored"
    )
}

fn validate_set(set: &QueueAttrSet) -> Result<()> {
    validate_mutation_key(&set.key)?;
    let value = set.value.as_deref();
    match set.key.as_str() {
        "auto" | "start" | "go" | "stop" | "priority" | "disable-auto-clear" => {
            if let Some(value) = value {
                bail!(
                    "queue attr: `{}` is a bare flag; use `--set {}` instead of `--set {}={value}`",
                    set.key,
                    set.key,
                    set.key,
                );
            }
        }
        "subagents" | "fan-out" => {
            if let Some(value) = value {
                agent_doc_queue::subagent_intent::parse_queue_subagents_value(value).map_err(
                    |reason| anyhow::anyhow!("queue attr: `{}={value}`: {reason}", set.key),
                )?;
            }
        }
        "loop" => {
            // Schedule validation below owns the complete loop vocabulary.
        }
        _ => {
            if value.is_none_or(|value| value.trim().is_empty()) {
                bail!(
                    "queue attr: `{}` requires a nonempty value (`--set {}=VALUE`)",
                    set.key,
                    set.key,
                );
            }
        }
    }
    Ok(())
}

/// Derive one validated successor while preserving every byte outside the
/// selected queue's opening marker.
pub fn plan_queue_attr_edit(
    current: &str,
    queue_id: &str,
    sets: &[QueueAttrSet],
    removes: &[String],
) -> Result<QueueAttrEditPlan> {
    if sets.is_empty() && removes.is_empty() {
        bail!("queue attr: provide at least one `--set KEY[=VALUE]` or `--remove KEY`");
    }
    if queue_id.trim().is_empty() {
        bail!("queue attr: `--id` must be nonempty");
    }

    let mut seen = HashSet::new();
    for set in sets {
        validate_set(set)?;
        if !seen.insert(set.key.as_str()) {
            bail!(
                "queue attr: attribute `{}` is mutated more than once",
                set.key
            );
        }
    }
    for key in removes {
        let key = key.trim();
        validate_mutation_key(key)?;
        if !seen.insert(key) {
            bail!("queue attr: attribute `{key}` is mutated more than once");
        }
    }

    let components = element::parse(current).context("queue attr: parse document components")?;
    for component in components
        .iter()
        .filter(|component| component.name == "queue")
    {
        let marker = &current[component.open_start..component.open_end];
        let id_tokens = element::open_marker_attr_tokens(marker)
            .context("queue attr: parse queue opening marker attributes")?
            .into_iter()
            .filter(|(key, _)| key == "id")
            .collect::<Vec<_>>();
        if id_tokens.len() > 1 {
            bail!("queue attr: duplicate `id` attributes on one agent:queue marker");
        }
        if id_tokens
            .first()
            .is_some_and(|(_, value)| value.as_deref().is_none_or(|value| value.trim().is_empty()))
        {
            bail!("queue attr: malformed bare or empty agent:queue `id` attribute");
        }
    }
    let graph = QueueGraph::parse(current, &components).map_err(anyhow::Error::new)?;
    let block = graph
        .block(queue_id)
        .ok_or_else(|| anyhow::anyhow!("queue attr: agent:queue id `{queue_id}` not found"))?;
    let occurrence = block.occurrence;
    let marker = &current[block.component.open_start..block.component.open_end];
    let mut next_marker = marker.to_string();
    for key in removes {
        next_marker = element::unset_open_marker_attr(&next_marker, key.trim())
            .with_context(|| format!("queue attr: remove `{}`", key.trim()))?;
    }
    for set in sets {
        next_marker = element::set_open_marker_attr(&next_marker, &set.key, set.value.as_deref())
            .with_context(|| format!("queue attr: set `{}`", set.key))?;
    }
    if next_marker == marker {
        return Ok(QueueAttrEditPlan {
            target_content: current.to_string(),
            changed: false,
            occurrence,
        });
    }

    let mut target_content = String::with_capacity(current.len() + next_marker.len());
    target_content.push_str(&current[..block.component.open_start]);
    target_content.push_str(&next_marker);
    target_content.push_str(&current[block.component.open_end..]);

    // The successor itself is the validation evidence. This rejects an id
    // removal/rename that makes a multi-queue graph ambiguous, duplicates an
    // id, breaks an `after=` edge, or introduces a cycle.
    let next_components =
        element::parse(&target_content).context("queue attr: parse successor components")?;
    let next_graph =
        QueueGraph::parse(&target_content, &next_components).map_err(anyhow::Error::new)?;
    let next_block = next_graph
        .blocks()
        .iter()
        .find(|block| block.occurrence == occurrence)
        .ok_or_else(|| anyhow::anyhow!("queue attr: selected queue disappeared from successor"))?;
    let (_, schedule_issues) =
        agent_doc_queue::queue_schedule::queue_schedule_spec_for_attrs(&next_block.component.attrs);
    let fatal = schedule_issues
        .iter()
        .filter(|issue| issue.fatal)
        .map(|issue| format!("{}: {}", issue.attr, issue.message))
        .collect::<Vec<_>>();
    if !fatal.is_empty() {
        bail!(
            "queue attr: invalid schedule on queue id `{}`: {}",
            next_block.id,
            fatal.join("; ")
        );
    }

    Ok(QueueAttrEditPlan {
        target_content,
        changed: true,
        occurrence,
    })
}

/// Apply an exact-id attribute edit through the queue's serialized, CAS-safe
/// editor/CRDT write boundary.
pub fn edit_with_effects(
    effects: &impl QueueCommandEffects,
    file: &Path,
    queue_id: &str,
    sets: &[QueueAttrSet],
    removes: &[String],
) -> Result<bool> {
    let _queue_edit_guard = crate::queue_edit_owner::QueueEditGuard::acquire(file);
    let current = effects.current_document_content(file, "queue_attr")?;
    let plan = plan_queue_attr_edit(&current, queue_id, sets, removes)?;
    if !plan.changed {
        return Ok(false);
    }
    effects.converge_document_or_disk(file, &plan.target_content, &current, "queue_attr")?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_attr_applied queue_id={queue_id} occurrence={} source_hash={} target_hash={}",
            plan.occurrence,
            agent_doc_hash::content_hash(&current),
            agent_doc_hash::content_hash(&plan.target_content),
        ),
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const TWO_QUEUES: &str = "<!-- agent:queue id=main -->\n- main\n<!-- /agent:queue -->\n\n<!-- agent:queue id=issues interval=\"1 hour\" loop -->\n- issues\n<!-- /agent:queue -->\n";

    #[test]
    fn exact_id_edit_preserves_sibling_and_body_bytes() {
        let set = QueueAttrSet::parse("interval=1h").unwrap();
        let plan = plan_queue_attr_edit(TWO_QUEUES, "issues", &[set], &[]).unwrap();
        assert_eq!(
            plan.target_content,
            "<!-- agent:queue id=main -->\n- main\n<!-- /agent:queue -->\n\n<!-- agent:queue id=issues interval=1h loop -->\n- issues\n<!-- /agent:queue -->\n"
        );
        assert_eq!(plan.occurrence, 1);
    }

    #[test]
    fn remove_is_idempotent_and_preserves_other_attributes() {
        let valid = TWO_QUEUES.replace("interval=\"1 hour\"", "interval=1h");
        let first = plan_queue_attr_edit(&valid, "issues", &[], &["loop".into()]).unwrap();
        assert!(first.changed);
        assert!(first.target_content.contains("id=issues interval=1h -->"));
        let second =
            plan_queue_attr_edit(&first.target_content, "issues", &[], &["loop".into()]).unwrap();
        assert!(!second.changed);
    }

    #[test]
    fn missing_duplicate_and_malformed_ids_refuse() {
        assert!(
            plan_queue_attr_edit(
                TWO_QUEUES,
                "missing",
                &[QueueAttrSet::parse("interval=1h").unwrap()],
                &[]
            )
            .is_err()
        );
        let duplicate = TWO_QUEUES.replace("id=issues", "id=main");
        assert!(
            plan_queue_attr_edit(
                &duplicate,
                "main",
                &[QueueAttrSet::parse("interval=1h").unwrap()],
                &[]
            )
            .is_err()
        );
        let malformed = TWO_QUEUES.replace("id=issues", "id='bad id'");
        assert!(
            plan_queue_attr_edit(
                &malformed,
                "main",
                &[QueueAttrSet::parse("interval=1h").unwrap()],
                &[]
            )
            .is_err()
        );
        for malformed in [
            "<!-- agent:queue id -->\n- item\n<!-- /agent:queue -->\n",
            "<!-- agent:queue id= -->\n- item\n<!-- /agent:queue -->\n",
            "<!-- agent:queue id=a id=b -->\n- item\n<!-- /agent:queue -->\n",
        ] {
            assert!(
                plan_queue_attr_edit(
                    malformed,
                    "default",
                    &[QueueAttrSet::parse("interval=1h").unwrap()],
                    &[]
                )
                .is_err(),
                "single queue with malformed id must refuse: {malformed}"
            );
        }
    }

    #[test]
    fn invalid_successor_schedule_refuses() {
        let error = plan_queue_attr_edit(
            TWO_QUEUES,
            "issues",
            &[QueueAttrSet::parse("interval=still bad").unwrap()],
            &[],
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("invalid schedule"));
    }

    #[test]
    fn unknown_and_conflicting_mutations_refuse() {
        assert!(
            plan_queue_attr_edit(
                TWO_QUEUES,
                "issues",
                &[QueueAttrSet::parse("intervaal=1h").unwrap()],
                &[]
            )
            .is_err()
        );
        assert!(
            plan_queue_attr_edit(
                TWO_QUEUES,
                "issues",
                &[QueueAttrSet::parse("interval=1h").unwrap()],
                &["interval".into()]
            )
            .is_err()
        );
    }

    struct CasEffects {
        current: Mutex<String>,
        writes: Mutex<Vec<(String, String, String)>>,
        diverge_after_read: bool,
    }

    impl QueueCommandEffects for CasEffects {
        fn current_document_content(&self, _file: &Path, _source: &str) -> Result<String> {
            let mut current = self.current.lock().unwrap();
            let observed = current.clone();
            if self.diverge_after_read {
                current.push_str("\noperator edit\n");
            }
            Ok(observed)
        }

        fn converge_document_or_disk(
            &self,
            _file: &Path,
            target_content: &str,
            source_content: &str,
            reason: &str,
        ) -> Result<()> {
            let mut current = self.current.lock().unwrap();
            if current.as_str() != source_content {
                bail!("stale source generation");
            }
            *current = target_content.to_string();
            self.writes.lock().unwrap().push((
                source_content.to_string(),
                target_content.to_string(),
                reason.to_string(),
            ));
            Ok(())
        }

        fn consume_free_text_queue_prompts_force_disk(
            &self,
            _file: &Path,
            _count: usize,
        ) -> Result<Option<crate::queue_cmd::QueueCommandConsumeOutcome>> {
            unreachable!()
        }
        fn consume_free_text_queue_prompts_with_outcome(
            &self,
            _file: &Path,
            _count: usize,
        ) -> Result<Option<crate::queue_cmd::QueueCommandConsumeOutcome>> {
            unreachable!()
        }
        fn strike_orphan_id_backed_queue_head(&self, _file: &Path, _id: &str) -> Result<bool> {
            unreachable!()
        }
        fn acknowledge_open_id_backed_queue_head(&self, _file: &Path, _id: &str) -> Result<bool> {
            unreachable!()
        }
        fn acknowledge_paused_free_text_queue_head(
            &self,
            _file: &Path,
            _expected_head: &str,
        ) -> Result<Option<crate::queue_cmd::QueueCommandConsumeOutcome>> {
            unreachable!()
        }
        fn prune_noise_queue_heads(&self, _file: &Path) -> Result<usize> {
            unreachable!()
        }
    }

    #[test]
    fn io_path_submits_exact_read_generation_to_cas_boundary() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("doc.md");
        std::fs::write(&file, TWO_QUEUES).unwrap();
        let effects = CasEffects {
            current: Mutex::new(TWO_QUEUES.to_string()),
            writes: Mutex::new(Vec::new()),
            diverge_after_read: false,
        };
        assert!(
            edit_with_effects(
                &effects,
                &file,
                "issues",
                &[QueueAttrSet::parse("interval=1h").unwrap()],
                &[],
            )
            .unwrap()
        );
        let writes = effects.writes.lock().unwrap();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, TWO_QUEUES);
        assert_eq!(writes[0].2, "queue_attr");
    }

    #[test]
    fn stale_authority_generation_is_rejected_without_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("doc.md");
        std::fs::write(&file, TWO_QUEUES).unwrap();
        let effects = CasEffects {
            current: Mutex::new(TWO_QUEUES.to_string()),
            writes: Mutex::new(Vec::new()),
            diverge_after_read: true,
        };
        let error = edit_with_effects(
            &effects,
            &file,
            "issues",
            &[QueueAttrSet::parse("interval=1h").unwrap()],
            &[],
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("stale source generation"));
        assert!(effects.writes.lock().unwrap().is_empty());
        assert!(effects.current.lock().unwrap().ends_with("operator edit\n"));
    }
}
