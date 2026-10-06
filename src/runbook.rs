//! Project-local runbook catalog and authoring commands.

use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CatalogEntry {
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<String>,
    pub show_command: String,
}

pub fn list(file: &Path, json: bool) -> Result<()> {
    let entries = catalog(file)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else if entries.is_empty() {
        println!("(no project runbooks; create one with `agent-doc runbook create`)");
    } else {
        for entry in entries {
            let presets = if entry.presets.is_empty() {
                String::new()
            } else {
                format!(" [{}]", entry.presets.join(", "))
            };
            println!("{}{}", entry.path, presets);
            if let Some(description) = entry.description {
                println!("    {description}");
            }
            println!("    {}", entry.show_command);
        }
    }
    Ok(())
}

pub fn show(file: &Path, selector: &str, json: bool) -> Result<()> {
    let (root, fm) = document_context(file)?;
    let relative = if let Some(relative) = fm.prompt_presets.runbook(selector) {
        relative.to_string()
    } else if selector.contains('/') || selector.ends_with(".md") {
        selector.to_string()
    } else {
        let matches = catalog(file)?
            .into_iter()
            .filter(|entry| entry.name == selector)
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [entry] => entry.path.clone(),
            [] => bail!("no catalogued runbook named {selector:?}"),
            _ => bail!("runbook name {selector:?} is ambiguous; use a catalog path"),
        }
    };
    let path = resolve_existing_runbook(&root, &relative)?;
    let content = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    if json {
        let entry = catalog(file)?
            .into_iter()
            .find(|entry| entry.path == relative_path(&root, &path))
            .context("runbook is not in the project catalog")?;
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "runbook": entry,
                "content": content,
            }))?
        );
    } else {
        print!("{content}");
    }
    Ok(())
}

pub fn create(
    file: &Path,
    name: &str,
    preset: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    validate_name(name)?;
    let (root, mut fm) = document_context(file)?;
    if let Some(preset) = preset
        && !fm.prompt_presets.contains_key(preset)
    {
        bail!("unknown prompt preset {preset:?}");
    }
    let relative = format!("runbooks/{name}.md");
    let runbooks_dir = root.join("runbooks");
    fs::create_dir_all(&runbooks_dir)
        .with_context(|| format!("create {}", runbooks_dir.display()))?;
    let canonical_dir = runbooks_dir
        .canonicalize()
        .with_context(|| format!("canonicalize {}", runbooks_dir.display()))?;
    let canonical_root = root.canonicalize()?;
    if !canonical_dir.starts_with(&canonical_root) {
        bail!("runbooks directory escapes project root through a symlink");
    }
    let target = canonical_dir.join(format!("{name}.md"));
    let title = title_from_name(name);
    let description = description.unwrap_or("Describe when and how this procedure should be used.");
    let content = format!(
        "---\ndescription: {}\n---\n\n# {title}\n\n## When to use\n\n{description}\n\n## Procedure\n\n1. Replace this step with the first deterministic action.\n\n## Verification\n\n- Record the command and expected success evidence.\n",
        serde_yaml::to_string(description)?.trim()
    );

    let mut created = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .with_context(|| {
            format!(
                "create runbook {} (refusing to overwrite)",
                target.display()
            )
        })?;
    if let Err(error) = created.write_all(content.as_bytes()) {
        if let Err(remove_error) = fs::remove_file(&target) {
            eprintln!(
                "[runbook] warning: failed to remove partial {}: {remove_error}",
                target.display()
            );
        }
        return Err(error).with_context(|| format!("write {}", target.display()));
    }

    if let Some(preset) = preset {
        let association = (|| -> Result<()> {
            let original =
                fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
            let (_, body) = agent_doc_frontmatter::frontmatter::parse(&original)?;
            fm.prompt_presets.set_runbook(preset, relative.clone())?;
            let updated =
                agent_doc_frontmatter::frontmatter::write_preserving(&original, &fm, body)?;
            agent_doc_document_realtime_io::atomic_write_through_authority(file, &updated)?;
            Ok(())
        })();
        if let Err(error) = association {
            if let Err(remove_error) = fs::remove_file(&target) {
                eprintln!(
                    "[runbook] warning: failed to roll back {}: {remove_error}",
                    target.display()
                );
            }
            return Err(error).context("associate runbook with preset");
        }
    }
    println!("{}", relative_path(&root, &target));
    Ok(())
}

fn catalog(file: &Path) -> Result<Vec<CatalogEntry>> {
    let (root, fm) = document_context(file)?;
    let mut associations = BTreeMap::<String, Vec<String>>::new();
    for preset in fm.prompt_presets.keys() {
        if let Some(path) = fm.prompt_presets.runbook(preset) {
            let resolved = resolve_existing_runbook(&root, path)?;
            associations
                .entry(relative_path(&root, &resolved))
                .or_default()
                .push(preset.clone());
        }
    }
    let mut paths = Vec::new();
    for dir in [root.join("runbooks"), root.join(".agent-doc/runbooks")] {
        collect_markdown(&root, &dir, &mut paths)?;
    }
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .map(|path| {
            let relative = relative_path(&root, &path);
            let content = fs::read_to_string(&path)?;
            Ok(CatalogEntry {
                name: path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                description: runbook_description(&content),
                presets: associations.remove(&relative).unwrap_or_default(),
                show_command: format!("agent-doc runbook show '{}' '{}'", file.display(), relative),
                path: relative,
            })
        })
        .collect()
}

fn document_context(
    file: &Path,
) -> Result<(PathBuf, agent_doc_frontmatter::frontmatter::Frontmatter)> {
    let canonical = file
        .canonicalize()
        .with_context(|| format!("canonicalize session document {}", file.display()))?;
    let root = agent_doc_fs::find_project_root(&canonical)
        .with_context(|| format!("find project root for {}", canonical.display()))?;
    let content = fs::read_to_string(&canonical)?;
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content)?;
    Ok((root.canonicalize()?, fm))
}

fn resolve_existing_runbook(root: &Path, relative: &str) -> Result<PathBuf> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("runbook path must be project-relative and may not traverse parents: {relative}");
    }
    let allowed = relative_path.starts_with("runbooks")
        || relative_path.starts_with(Path::new(".agent-doc/runbooks"));
    if !allowed || relative_path.extension().and_then(|v| v.to_str()) != Some("md") {
        bail!(
            "runbook path must be a Markdown file under runbooks/ or .agent-doc/runbooks/: {relative}"
        );
    }
    let canonical = root
        .join(relative_path)
        .canonicalize()
        .with_context(|| format!("resolve runbook {relative}"))?;
    if !canonical.starts_with(root) {
        bail!("runbook path escapes project root through a symlink: {relative}");
    }
    Ok(canonical)
}

fn collect_markdown(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let canonical_dir = dir.canonicalize()?;
    if !canonical_dir.starts_with(root) {
        bail!(
            "runbook catalog directory escapes project root: {}",
            dir.display()
        );
    }
    let mut entries = fs::read_dir(&canonical_dir)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            let canonical = entry.path().canonicalize()?;
            if !canonical.starts_with(root) {
                bail!(
                    "runbook catalog symlink escapes project root: {}",
                    entry.path().display()
                );
            }
        }
        if file_type.is_dir() {
            collect_markdown(root, &entry.path(), out)?;
        } else if entry.path().extension().and_then(|v| v.to_str()) == Some("md") {
            let canonical = entry.path().canonicalize()?;
            if !canonical.starts_with(root) {
                bail!("runbook escapes project root: {}", entry.path().display());
            }
            out.push(canonical);
        }
    }
    Ok(())
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name.split('-').all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
    {
        bail!("runbook name must be lowercase kebab-case: {name:?}");
    }
    Ok(())
}

fn title_from_name(name: &str) -> String {
    name.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn runbook_description(content: &str) -> Option<String> {
    let yaml = agent_doc_frontmatter::frontmatter::raw_frontmatter_yaml(content)?;
    serde_yaml::from_str::<serde_yaml::Value>(yaml)
        .ok()?
        .as_mapping()?
        .get(serde_yaml::Value::from("description"))?
        .as_str()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        fs::write(
            &doc,
            "---\npresets:\n  '#release': release + publish\n---\n\n# Session\n",
        )
        .unwrap();
        (dir, doc)
    }

    #[test]
    fn create_associates_scalar_preset_and_list_show_json_work() {
        let (dir, doc) = project();
        create(
            &doc,
            "release-check",
            Some("#release"),
            Some("Ship safely."),
        )
        .unwrap();
        let runbook = dir.path().join("runbooks/release-check.md");
        assert!(runbook.exists());
        let current = fs::read_to_string(&doc).unwrap();
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&current).unwrap();
        assert_eq!(
            fm.prompt_presets.get("#release").unwrap(),
            "release + publish"
        );
        assert_eq!(
            fm.prompt_presets.runbook("#release"),
            Some("runbooks/release-check.md")
        );
        let entries = catalog(&doc).unwrap();
        assert_eq!(entries[0].presets, vec!["#release"]);
        assert!(
            serde_json::to_string(&entries)
                .unwrap()
                .contains("show_command")
        );
        assert!(list(&doc, true).is_ok());
        assert!(show(&doc, "#release", true).is_ok());
        assert!(show(&doc, "release-check", true).is_ok());
    }

    #[test]
    fn traversal_and_symlink_escape_are_rejected() {
        let (dir, doc) = project();
        assert!(resolve_existing_runbook(dir.path(), "runbooks/../outside.md").is_err());
        fs::create_dir(dir.path().join("runbooks")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.md"), "secret").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), dir.path().join("runbooks/link")).unwrap();
            assert!(show(&doc, "runbooks/link/secret.md", false).is_err());
        }
    }

    #[test]
    fn create_refuses_name_and_path_collisions() {
        let (dir, doc) = project();
        assert!(create(&doc, "../bad", None, None).is_err());
        assert!(create(&doc, "unknown", Some("#missing"), None).is_err());
        assert!(
            !dir.path().join("runbooks/unknown.md").exists(),
            "unknown preset must not create a scaffold"
        );
        assert!(
            !dir.path().join("runbooks").exists(),
            "unknown preset must not create a runbook catalog"
        );
        create(&doc, "safe", None, None).unwrap();
        assert!(create(&doc, "safe", None, None).is_err());
    }
}
