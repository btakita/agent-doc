use anyhow::{Context, Result};
use std::path::Path;

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
            let comp = components
                .iter()
                .find(|c| c.name == name)
                .with_context(|| format!("component '{}' not found in {}", name, file.display()))?;
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
        let content = "<!-- agent:exchange -->\nbody\n<!-- /agent:exchange -->\n";
        let f = write_temp(content);
        let err = run(f.path(), Some("notexist")).unwrap_err();
        assert!(err.to_string().contains("notexist"));
    }

    #[test]
    fn read_missing_file_errors() {
        let err = run(Path::new("/tmp/does-not-exist-read-test.md"), None).unwrap_err();
        assert!(err.to_string().contains("read_command_document"));
    }
}
