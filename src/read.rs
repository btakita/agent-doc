use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// A `read --component` name that is not present in the target document.
///
/// This is an operator usage error, not a failed Agent Doc turn. Keeping the
/// error typed lets the CLI suppress dogfood terminal-failure escalation even
/// when the target is the currently attached document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownReadComponent {
    pub requested: String,
    pub file: PathBuf,
    pub valid_components: Vec<String>,
}

impl std::fmt::Display for UnknownReadComponent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let valid = if self.valid_components.is_empty() {
            "(none)".to_string()
        } else {
            self.valid_components.join(", ")
        };
        write!(
            f,
            "component '{}' not found in {}; valid components: {valid}",
            self.requested,
            self.file.display()
        )
    }
}

impl std::error::Error for UnknownReadComponent {}

/// Print the full document or a single named component's body to stdout.
pub fn run(file: &Path, component: Option<&str>) -> Result<()> {
    let content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "read_command_document",
    )?;

    match component {
        None => {
            print!("{}", content);
        }
        Some(name) => {
            let components = agent_doc_element::element::parse(&content)
                .with_context(|| format!("failed to parse components in {}", file.display()))?;
            let Some(comp) = components.iter().find(|c| c.name == name) else {
                let mut valid_components = Vec::new();
                for component in &components {
                    if !valid_components.contains(&component.name) {
                        valid_components.push(component.name.clone());
                    }
                }
                return Err(anyhow::Error::new(UnknownReadComponent {
                    requested: name.to_string(),
                    file: file.to_path_buf(),
                    valid_components,
                }));
            };
            print!("{}", comp.content(&content));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_temp(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn read_full_file() {
        let content = "hello\nworld\n";
        let f = write_temp(content);
        // Just verify it runs without error — stdout capture not needed for correctness.
        run(f.path(), None).unwrap();
    }

    #[test]
    fn read_named_component() {
        let content = "<!-- agent:exchange -->\nbody text\n<!-- /agent:exchange -->\n";
        let f = write_temp(content);
        run(f.path(), Some("exchange")).unwrap();
    }

    #[test]
    fn read_missing_component_errors() {
        let content = concat!(
            "<!-- agent:exchange -->\nbody\n<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n<!-- /agent:queue -->\n",
        );
        let f = write_temp(content);
        let err = run(f.path(), Some("notexist")).unwrap_err();
        let usage = err.downcast_ref::<UnknownReadComponent>().unwrap();
        assert_eq!(usage.requested, "notexist");
        assert_eq!(usage.valid_components, ["exchange", "queue"]);
        assert!(
            err.to_string()
                .contains("valid components: exchange, queue")
        );
    }

    #[test]
    fn attached_and_other_documents_return_the_same_typed_usage_error() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join(".agent-doc")).unwrap();
        let attached = temp.path().join("attached.md");
        let other = temp.path().join("other.md");
        let content = concat!(
            "<!-- agent:exchange -->\nbody\n<!-- /agent:exchange -->\n",
            "<!-- agent:review -->\n<!-- /agent:review -->\n",
        );
        std::fs::write(&attached, content).unwrap();
        std::fs::write(&other, content).unwrap();
        agent_doc_test_support::seed_lazily_editor_registration_default(attached.to_str().unwrap());

        for (file, requested) in [(&attached, "nope-attached"), (&other, "nope-other")] {
            let err = run(file, Some(requested)).unwrap_err();
            let usage = err.downcast_ref::<UnknownReadComponent>().unwrap();
            assert_eq!(usage.requested, requested);
            assert_eq!(usage.valid_components, ["exchange", "review"]);
        }
    }

    #[test]
    fn read_missing_file_errors() {
        let err = run(Path::new("/tmp/does-not-exist-read-test.md"), None).unwrap_err();
        assert!(err.to_string().contains("read_command_document"));
    }
}
