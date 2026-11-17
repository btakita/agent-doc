//! Focused writes for `agent:place`.
//!
//! `place set` replaces only the bytes inside `place:out` and records the
//! prompt hash it answered. The write is attributed to the agent baseline so
//! it does not come back as operator input. It does not capture, seal, or
//! commit the response cycle.

use std::collections::BTreeMap;
use std::path::Path;

use agent_doc_document::place::{self, PlaceAnswer};
use anyhow::{Context, Result};

use crate::{RUNTIME_STATUS_WRITE_EFFECTS, StatusWriteEffects};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceRepairOutcome {
    AlreadyWellFormed,
    Repaired,
}

/// `agent-doc place set <FILE> --id <id> --text <answer>`.
pub fn set_place(file: &Path, id: &str, text: &str, force_disk: bool) -> Result<()> {
    set_place_with(&RUNTIME_STATUS_WRITE_EFFECTS, file, id, text, force_disk)
}

pub fn set_place_with<E: StatusWriteEffects + ?Sized>(
    effects: &E,
    file: &Path,
    id: &str,
    text: &str,
    force_disk: bool,
) -> Result<()> {
    let current = read(effects, file, force_disk, "place_set")?;
    let updated = agent_doc_element::place::replace_output(&current, id, text)
        .with_context(|| format!("place set id `{id}`"))?;
    if updated != current {
        land(effects, file, &current, &updated, force_disk, "place_set")?;
    } else {
        effects.log_op(
            file,
            &format!(
                "place_set file={} id={} outcome=unchanged",
                file.display(),
                id
            ),
        );
    }
    let assessed = place::assess(&updated, &BTreeMap::new())
        .map_err(|error| anyhow::anyhow!("{error}"))
        .context("place set could not read the place it wrote")?;
    let contract = assessed
        .contracts
        .iter()
        .find(|contract| contract.id == id)
        .with_context(|| format!("place set lost id `{id}`"))?;
    agent_doc_snapshot_io::record_place_answer(
        file,
        id,
        &PlaceAnswer {
            prompt_hash: contract.prompt_hash.clone(),
            output: contract.output.clone(),
        },
    )
    .context("record place answer")?;
    effects.log_op(
        file,
        &format!(
            "place_answer_recorded file={} id={} prompt_hash={}",
            file.display(),
            id,
            contract.prompt_hash
        ),
    );
    Ok(())
}

/// `agent-doc place repair <FILE>`.
pub fn repair_place(file: &Path, force_disk: bool) -> Result<PlaceRepairOutcome> {
    repair_place_with(&RUNTIME_STATUS_WRITE_EFFECTS, file, force_disk)
}

pub fn repair_place_with<E: StatusWriteEffects + ?Sized>(
    effects: &E,
    file: &Path,
    force_disk: bool,
) -> Result<PlaceRepairOutcome> {
    let current = read(effects, file, force_disk, "place_repair")?;
    let Some(updated) = agent_doc_element::place::repair(&current).context("place repair")? else {
        effects.log_op(
            file,
            &format!(
                "place_repair file={} outcome=already_well_formed",
                file.display()
            ),
        );
        return Ok(PlaceRepairOutcome::AlreadyWellFormed);
    };
    land(
        effects,
        file,
        &current,
        &updated,
        force_disk,
        "place_repair",
    )?;
    Ok(PlaceRepairOutcome::Repaired)
}

fn read<E: StatusWriteEffects + ?Sized>(
    effects: &E,
    file: &Path,
    force_disk: bool,
    op: &str,
) -> Result<String> {
    if force_disk {
        effects.force_disk_document_content(file, op)
    } else {
        effects.current_document_content(file, op)
    }
    .context("failed to read document")
}

fn land<E: StatusWriteEffects + ?Sized>(
    effects: &E,
    file: &Path,
    before: &str,
    after: &str,
    force_disk: bool,
    op: &str,
) -> Result<()> {
    if force_disk {
        std::fs::write(file, after)
            .with_context(|| format!("{op}: failed to write {}", file.display()))?;
        effects.record_document_write_provenance(file, after);
        effects.log_op(
            file,
            &format!(
                "{op}_writeback file={} transport=disk_force len={} hash={}",
                file.display(),
                after.len(),
                agent_doc_hash::content_hash(after)
            ),
        );
    } else {
        effects.converge_or_disk_write(file, before, after, op)?;
    }
    effects.attribute_agent_write_to_baseline(file, before, after);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RuntimeStatusWriteEffects;

    fn effects() -> RuntimeStatusWriteEffects {
        RuntimeStatusWriteEffects
    }

    #[test]
    fn set_place_replaces_the_output_and_records_the_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.md");
        std::fs::write(
            &file,
            "\
<!-- agent:place id=\"step-1\" -->
Rewrite the paragraph above.
<!-- place:out -->
<!-- /place:out -->
<!-- /agent:place -->
",
        )
        .unwrap();
        set_place_with(&effects(), &file, "step-1", "1. Read the log.", true).unwrap();
        let landed = std::fs::read_to_string(&file).unwrap();
        assert!(landed.contains("1. Read the log.\n"));
        assert!(landed.contains("Rewrite the paragraph above."));
        let answers = agent_doc_snapshot_io::load_place_answers(&file).unwrap();
        assert_eq!(answers["step-1"].output, "1. Read the log.");
        let settled = place::assess(&landed, &answers).unwrap();
        assert!(!settled.needs_turn());
    }
}
