use anyhow::Result;
use std::path::Path;

pub trait PipelineFrontmatterEffects {
    fn read_current_document_content(&self, file: &Path, source: &str) -> Result<String>;

    fn converge_or_disk_write(
        &self,
        file: &Path,
        current_content: &str,
        target_content: &str,
        reason: &str,
    ) -> Result<()>;

    fn log_op(&self, file: &Path, message: &str);
}

/// #22a8 (Phase 5b write-side): mirror the live cycle phase into the session
/// document's `agent_doc_pipeline:` frontmatter block so any later invocation or
/// editor can read where the pipeline is without replaying the state ledger.
///
/// Best-effort and non-fatal. The write is byte-precise and goes through the
/// editor-aware convergence path so a live IDE buffer does not raise a file
/// cache conflict.
pub fn mirror_pipeline_frontmatter(
    effects: &impl PipelineFrontmatterEffects,
    file: &Path,
    state: &crate::CycleState,
) {
    if let Err(e) = (|| -> Result<()> {
        let content = effects.read_current_document_content(file, "pipeline_mirror")?;
        let updated = agent_doc_frontmatter::frontmatter::splice_pipeline_block(
            &content,
            &state.to_pipeline(),
        )?;
        if updated != content {
            effects.converge_or_disk_write(file, &content, &updated, "pipeline_mirror")?;
        }
        Ok(())
    })() {
        effects.log_op(
            file,
            &format!("pipeline_mirror_failed file={} err={}", file.display(), e),
        );
    }
}

/// Clear the live `agent_doc_pipeline:` frontmatter mirror after a terminal
/// cycle transition through the injected editor-aware document convergence port.
pub fn clear_pipeline_frontmatter(effects: &impl PipelineFrontmatterEffects, file: &Path) {
    if let Err(e) = (|| -> Result<()> {
        let content = effects.read_current_document_content(file, "pipeline_clear")?;
        clear_pipeline_frontmatter_from_content(effects, file, &content)?;
        Ok(())
    })() {
        effects.log_op(
            file,
            &format!("pipeline_clear_failed file={} err={}", file.display(), e),
        );
    }
}

/// Clear the pipeline mirror from caller-resolved current document content.
///
/// Terminal closeout already resolves the realtime document once for cycle and
/// capture state. Reusing that authority snapshot avoids a second controller
/// round trip while the convergence port still rebases concurrent operator edits
/// instead of replacing them.
pub fn clear_pipeline_frontmatter_from_content(
    effects: &impl PipelineFrontmatterEffects,
    file: &Path,
    content: &str,
) -> Result<()> {
    if !content.contains("agent_doc_pipeline:") {
        return Ok(());
    }
    let updated =
        agent_doc_frontmatter::frontmatter::splice_pipeline_block(content, &Default::default())?;
    if updated != content {
        effects.converge_or_disk_write(file, content, &updated, "pipeline_clear")?;
    }
    Ok(())
}

pub fn mark_committed(
    effects: &impl PipelineFrontmatterEffects,
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<crate::CycleState> {
    let state = crate::mark_committed(file, event, snapshot_content, file_content)?;
    if let Some(content) = file_content {
        if let Err(e) = clear_pipeline_frontmatter_from_content(effects, file, content) {
            effects.log_op(
                file,
                &format!("pipeline_clear_failed file={} err={}", file.display(), e),
            );
        }
    } else {
        clear_pipeline_frontmatter(effects, file);
    }
    Ok(state)
}

pub fn mark_abandoned(
    effects: &impl PipelineFrontmatterEffects,
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<crate::CycleState> {
    let state = crate::mark_abandoned(file, event, snapshot_content, file_content)?;
    if let Some(content) = file_content {
        if let Err(e) = clear_pipeline_frontmatter_from_content(effects, file, content) {
            effects.log_op(
                file,
                &format!("pipeline_clear_failed file={} err={}", file.display(), e),
            );
        }
    } else {
        clear_pipeline_frontmatter(effects, file);
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingEffects {
        target: Mutex<Option<String>>,
    }

    impl PipelineFrontmatterEffects for RecordingEffects {
        fn read_current_document_content(&self, _file: &Path, _source: &str) -> Result<String> {
            panic!("caller-resolved cleanup must not resolve the document again")
        }

        fn converge_or_disk_write(
            &self,
            _file: &Path,
            current_content: &str,
            target_content: &str,
            reason: &str,
        ) -> Result<()> {
            assert!(current_content.contains("agent_doc_pipeline:"));
            assert_eq!(reason, "pipeline_clear");
            *self.target.lock().unwrap() = Some(target_content.to_string());
            Ok(())
        }

        fn log_op(&self, _file: &Path, _message: &str) {}
    }

    #[test]
    fn caller_resolved_pipeline_cleanup_avoids_a_second_authority_read() {
        let base = "---\nsession: test\n---\n\nbody\n";
        let content = agent_doc_frontmatter::frontmatter::set_pipeline_state(
            base,
            Some("run-1"),
            Some("committing"),
            None,
            None,
        )
        .unwrap();
        let effects = RecordingEffects::default();

        clear_pipeline_frontmatter_from_content(
            &effects,
            Path::new("/tmp/agent-doc-pipeline-fast-path.md"),
            &content,
        )
        .unwrap();

        let target = effects.target.lock().unwrap().clone().unwrap();
        assert!(!target.contains("agent_doc_pipeline:"));
        assert!(target.ends_with("\nbody\n"));
    }
}
