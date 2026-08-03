//! Pure whole-document and per-component authority hash projections.
//!
//! The whole-document hash remains the compatibility identity used by existing
//! convergence gates. Component hashes are diagnostic-only in this phase: they
//! explain which document regions diverge without changing any equality rule.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;

const UNSCOPED_NAME: &str = "@unscoped";
const STRUCTURE_NAME: &str = "@structure";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ComponentAuthorityKey {
    pub name: String,
    pub occurrence: usize,
}

impl ComponentAuthorityKey {
    fn new(name: impl Into<String>, occurrence: usize) -> Self {
        Self {
            name: name.into(),
            occurrence,
        }
    }

    fn label(&self) -> String {
        if self.occurrence == 0 {
            self.name.clone()
        } else {
            format!("{}#{}", self.name, self.occurrence + 1)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentAuthorityHash {
    pub key: ComponentAuthorityKey,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentAuthorityHashes {
    document_hash: String,
    components: Vec<ComponentAuthorityHash>,
}

impl DocumentAuthorityHashes {
    pub fn from_content(content: &str) -> Result<Self> {
        let parsed = agent_doc_element::element::parse(content)?;
        let mut occurrences = BTreeMap::<String, usize>::new();
        let mut components = Vec::with_capacity(parsed.len() + 2);
        let mut structure = Vec::with_capacity(parsed.len());

        for component in &parsed {
            let occurrence = occurrences.entry(component.name.clone()).or_default();
            let key = ComponentAuthorityKey::new(component.name.clone(), *occurrence);
            *occurrence += 1;
            structure.push(key.label());
            components.push(ComponentAuthorityHash {
                key,
                content_hash: agent_doc_hash::content_hash(component.content(content)),
            });
        }

        let mut top_level = Vec::new();
        for component in &parsed {
            if top_level
                .iter()
                .any(|parent: &&agent_doc_element::element::Component| {
                    parent.open_start <= component.open_start
                        && component.close_end <= parent.close_end
                })
            {
                continue;
            }
            top_level.push(component);
        }
        let mut unscoped = String::new();
        let mut cursor = 0;
        for component in top_level {
            unscoped.push_str(&content[cursor..component.open_start]);
            cursor = component.close_end;
        }
        unscoped.push_str(&content[cursor..]);
        components.push(ComponentAuthorityHash {
            key: ComponentAuthorityKey::new(UNSCOPED_NAME, 0),
            content_hash: agent_doc_hash::content_hash(&unscoped),
        });
        components.push(ComponentAuthorityHash {
            key: ComponentAuthorityKey::new(STRUCTURE_NAME, 0),
            content_hash: agent_doc_hash::content_hash(&structure.join("\n")),
        });

        Ok(Self {
            document_hash: agent_doc_hash::content_hash(content),
            components,
        })
    }

    pub fn document_hash(&self) -> &str {
        &self.document_hash
    }

    pub fn components(&self) -> &[ComponentAuthorityHash] {
        &self.components
    }

    pub fn divergences(&self, other: &Self) -> Vec<ComponentAuthorityDivergence> {
        let left = self
            .components
            .iter()
            .map(|component| (component.key.clone(), component.content_hash.as_str()))
            .collect::<BTreeMap<_, _>>();
        let right = other
            .components
            .iter()
            .map(|component| (component.key.clone(), component.content_hash.as_str()))
            .collect::<BTreeMap<_, _>>();
        left.keys()
            .chain(right.keys())
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|key| {
                let authority_hash = left.get(&key).copied();
                let disk_hash = right.get(&key).copied();
                (authority_hash != disk_hash).then(|| ComponentAuthorityDivergence {
                    key,
                    authority_hash: authority_hash.map(str::to_string),
                    disk_hash: disk_hash.map(str::to_string),
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentAuthorityDivergence {
    pub key: ComponentAuthorityKey,
    pub authority_hash: Option<String>,
    pub disk_hash: Option<String>,
}

fn short(hash: Option<&str>) -> &str {
    hash.map(|value| &value[..value.len().min(12)])
        .unwrap_or("missing")
}

/// Render a bounded, deterministic authority/disk component divergence list.
///
/// Parsing failures are diagnostic data, not a new convergence gate. Callers
/// continue to decide equality from the original whole-document bytes.
pub fn format_authority_disk_component_divergence(
    authority_content: &str,
    disk_content: &str,
) -> String {
    let authority = match DocumentAuthorityHashes::from_content(authority_content) {
        Ok(projection) => projection,
        Err(error) => return format!("unavailable(authority_parse={error:#})"),
    };
    let disk = match DocumentAuthorityHashes::from_content(disk_content) {
        Ok(projection) => projection,
        Err(error) => return format!("unavailable(disk_parse={error:#})"),
    };
    let divergences = authority.divergences(&disk);
    if divergences.is_empty() {
        return "none".to_string();
    }
    divergences
        .iter()
        .map(|divergence| {
            format!(
                "{}:{}->{}",
                divergence.key.label(),
                short(divergence.authority_hash.as_deref()),
                short(divergence.disk_hash.as_deref()),
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::{DocumentAuthorityHashes, format_authority_disk_component_divergence};

    #[test]
    fn whole_document_hash_preserves_the_existing_content_hash() {
        let document = "<!-- agent:exchange -->\nhello\n<!-- /agent:exchange -->\n";
        let projection = DocumentAuthorityHashes::from_content(document).unwrap();
        assert_eq!(
            projection.document_hash(),
            agent_doc_hash::content_hash(document)
        );
    }

    #[test]
    fn reports_only_the_changed_component_when_unscoped_text_is_stable() {
        let authority = concat!(
            "# Sample\n\n",
            "<!-- agent:exchange -->\nnew response\n<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n- do work\n<!-- /agent:queue -->\n",
        );
        let disk = authority.replace("new response", "old response");
        let summary = format_authority_disk_component_divergence(authority, &disk);
        assert!(summary.starts_with("exchange:"));
        assert!(!summary.contains("queue:"));
        assert!(!summary.contains("@unscoped:"));
    }

    #[test]
    fn reports_unscoped_and_structure_drift_without_hiding_whole_hash_drift() {
        let first = concat!(
            "# First\n",
            "<!-- agent:exchange -->\na\n<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\nb\n<!-- /agent:queue -->\n",
        );
        let unscoped = first.replacen("# First", "# Second", 1);
        assert!(
            format_authority_disk_component_divergence(first, &unscoped).starts_with("@unscoped:")
        );

        let reordered = concat!(
            "# First\n",
            "<!-- agent:queue -->\nb\n<!-- /agent:queue -->\n",
            "<!-- agent:exchange -->\na\n<!-- /agent:exchange -->\n",
        );
        let summary = format_authority_disk_component_divergence(first, reordered);
        assert!(summary.contains("@structure:"));
    }

    #[test]
    fn duplicate_component_names_have_stable_occurrence_labels() {
        let authority = concat!(
            "<!-- agent:item -->\na\n<!-- /agent:item -->\n",
            "<!-- agent:item -->\nb\n<!-- /agent:item -->\n",
        );
        let disk = authority.replacen("\nb\n", "\nc\n", 1);
        assert!(
            format_authority_disk_component_divergence(authority, &disk).starts_with("item#2:")
        );
    }
}
