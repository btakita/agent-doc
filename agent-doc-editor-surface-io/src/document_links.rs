//! Resolve Markdown links from the exact document snapshot used by an Explorer.

use agent_doc_frontmatter::frontmatter::split_frontmatter_parts;
use percent_encoding::percent_decode_str;
use pulldown_cmark::{Event, Parser, Tag};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// Existing Markdown files linked by `source`, keyed by project-relative path.
/// Resolve symlinks before checking the project boundary.
pub fn linked_markdown_documents(
    source: &str,
    file: &Path,
    root: &Path,
) -> BTreeMap<String, PathBuf> {
    let (Ok(file), Ok(root)) = (file.canonicalize(), root.canonicalize()) else {
        return BTreeMap::new();
    };
    if !file.starts_with(&root) {
        return BTreeMap::new();
    }
    let Some(parent) = file.parent() else {
        return BTreeMap::new();
    };
    let (_, markdown) = split_frontmatter_parts(source);
    Parser::new(markdown)
        .filter_map(|event| {
            let Event::Start(Tag::Link { dest_url, .. }) = event else {
                return None;
            };
            let url_path = dest_url.split(['#', '?']).next()?;
            let decoded = percent_decode_str(url_path).decode_utf8().ok()?;
            // Local Markdown links use URL separators and have no scheme.
            if decoded.contains([':', '\\', '\0']) {
                return None;
            }
            let path = Path::new(decoded.as_ref());
            if path.is_absolute() || !is_markdown(path) {
                return None;
            }
            let target = parent.join(path).canonicalize().ok()?;
            if target == file || !is_markdown(&target) || !target.is_file() {
                return None;
            }
            let relative = target.strip_prefix(&root).ok()?;
            let name = relative
                .components()
                .map(|part| match part {
                    Component::Normal(name) => name.to_str(),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?
                .join("/");
            Some((name, target))
        })
        .collect()
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}
