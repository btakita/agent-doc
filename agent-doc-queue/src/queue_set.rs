//! Identified multi-queue parsing and dependency-aware scheduling.
//!
//! Queue blocks are independent scheduling nodes. The legacy single unkeyed
//! block is retained as the `default` queue; additional blocks use `id=` and
//! may wait for other blocks with `depends=id-a,id-b`.

use std::collections::{HashMap, HashSet};

use agent_doc_element::element::{self, Component};
use anyhow::{Context, Result, bail};

use crate::document_queue::{self, QueueEntry};

pub const DEFAULT_QUEUE_ID: &str = "default";

#[derive(Debug, Clone)]
pub struct QueueBlock<'a> {
    pub id: String,
    pub dependencies: Vec<String>,
    pub occurrence: usize,
    pub component: &'a Component,
    pub entries: Vec<QueueEntry>,
}

impl QueueBlock<'_> {
    pub fn live_prompt_count(&self) -> usize {
        document_queue::prompts(&self.entries).len()
            + self
                .entries
                .iter()
                .filter(|entry| {
                    matches!(entry, QueueEntry::Freeform(line) if !document_queue::is_noise_freeform_line(line))
                })
                .count()
    }

    pub fn is_drained(&self) -> bool {
        self.live_prompt_count() == 0
    }
}

#[derive(Debug, Clone)]
pub struct QueueSet<'a> {
    blocks: Vec<QueueBlock<'a>>,
    by_id: HashMap<String, usize>,
}

impl<'a> QueueSet<'a> {
    pub fn parse(content: &str, components: &'a [Component]) -> Result<Self> {
        let mut blocks = Vec::new();
        let mut by_id = HashMap::new();
        for (occurrence, component) in components
            .iter()
            .filter(|component| component.name == "queue")
            .enumerate()
        {
            let id = component
                .attrs
                .get("id")
                .map(|id| id.trim())
                .filter(|id| !id.is_empty() && *id != "true")
                .unwrap_or(DEFAULT_QUEUE_ID)
                .to_string();
            validate_id(&id)?;
            if by_id.insert(id.clone(), blocks.len()).is_some() {
                bail!("duplicate agent:queue id `{id}`; every queue block needs a unique `id=`");
            }
            let dependencies = parse_dependencies(component.attrs.get("depends"), &id)?;
            let entries = document_queue::parse(component.content(content))
                .with_context(|| format!("failed to parse agent:queue id `{id}`"))?;
            blocks.push(QueueBlock {
                id,
                dependencies,
                occurrence,
                component,
                entries,
            });
        }

        for block in &blocks {
            for dependency in &block.dependencies {
                if !by_id.contains_key(dependency) {
                    bail!(
                        "agent:queue id `{}` depends on missing queue id `{dependency}`",
                        block.id
                    );
                }
            }
        }
        validate_acyclic(&blocks, &by_id)?;
        Ok(Self { blocks, by_id })
    }

    pub fn from_document(content: &'a str, components: &'a [Component]) -> Result<Self> {
        Self::parse(content, components)
    }

    pub fn blocks(&self) -> &[QueueBlock<'a>] {
        &self.blocks
    }

    pub fn block(&self, id: &str) -> Option<&QueueBlock<'a>> {
        self.by_id.get(id).map(|index| &self.blocks[*index])
    }

    pub fn selected(&self) -> Option<&QueueBlock<'a>> {
        self.blocks.iter().find(|block| {
            !block.is_drained()
                && block
                    .dependencies
                    .iter()
                    .all(|dependency| self.block(dependency).is_some_and(QueueBlock::is_drained))
        })
    }

    pub fn all_drained(&self) -> bool {
        self.blocks.iter().all(QueueBlock::is_drained)
    }

    pub fn live_prompt_count(&self) -> usize {
        self.blocks.iter().map(QueueBlock::live_prompt_count).sum()
    }
}

pub fn parse_document(content: &str) -> Result<(Vec<Component>, Option<usize>)> {
    let components = element::parse(content)?;
    let selected_occurrence = QueueSet::parse(content, &components)?
        .selected()
        .map(|block| block.occurrence);
    Ok((components, selected_occurrence))
}

pub fn selected_component<'a>(
    content: &str,
    components: &'a [Component],
) -> Result<Option<&'a Component>> {
    Ok(QueueSet::parse(content, components)?
        .selected()
        .map(|block| block.component))
}

pub fn component_for_id<'a>(
    content: &str,
    components: &'a [Component],
    id: &str,
) -> Result<Option<&'a Component>> {
    Ok(QueueSet::parse(content, components)?
        .block(id)
        .map(|block| block.component))
}

fn validate_id(id: &str) -> Result<()> {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        bail!("agent:queue `id=` must not be empty");
    };
    if !first.is_ascii_alphanumeric()
        || !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        bail!(
            "invalid agent:queue id `{id}`; use an ASCII letter/digit followed by letters, digits, `.`, `_`, or `-`"
        );
    }
    Ok(())
}

fn parse_dependencies(raw: Option<&String>, owner: &str) -> Result<Vec<String>> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    if raw.trim().is_empty() || raw == "true" {
        bail!("agent:queue id `{owner}` has bare `depends`; use `depends=queue-id`");
    }
    let mut seen = HashSet::new();
    let mut dependencies = Vec::new();
    for dependency in raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        validate_id(dependency)?;
        if dependency == owner {
            bail!("agent:queue id `{owner}` cannot depend on itself");
        }
        if seen.insert(dependency.to_string()) {
            dependencies.push(dependency.to_string());
        }
    }
    if dependencies.is_empty() {
        bail!("agent:queue id `{owner}` has an empty `depends=` list");
    }
    Ok(dependencies)
}

fn validate_acyclic(blocks: &[QueueBlock<'_>], by_id: &HashMap<String, usize>) -> Result<()> {
    fn visit(
        index: usize,
        blocks: &[QueueBlock<'_>],
        by_id: &HashMap<String, usize>,
        visiting: &mut HashSet<usize>,
        visited: &mut HashSet<usize>,
    ) -> Result<()> {
        if visited.contains(&index) {
            return Ok(());
        }
        if !visiting.insert(index) {
            bail!(
                "agent:queue dependency cycle includes id `{}`",
                blocks[index].id
            );
        }
        for dependency in &blocks[index].dependencies {
            visit(by_id[dependency], blocks, by_id, visiting, visited)?;
        }
        visiting.remove(&index);
        visited.insert(index);
        Ok(())
    }

    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for index in 0..blocks.len() {
        visit(index, blocks, by_id, &mut visiting, &mut visited)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_single_unkeyed_queue_is_default() {
        let doc = "<!-- agent:queue -->\n- first\n<!-- /agent:queue -->\n";
        let components = element::parse(doc).unwrap();
        let queues = QueueSet::parse(doc, &components).unwrap();
        let selected = queues.selected().unwrap();
        assert_eq!(selected.id, DEFAULT_QUEUE_ID);
        assert_eq!(selected.occurrence, 0);
    }

    #[test]
    fn selects_first_ready_id_and_preserves_scoped_attrs() {
        let doc = concat!(
            "<!-- agent:queue id=review subagents=2 -->\n",
            "- review now\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=release preset=release depends=review -->\n",
            "- publish later\n",
            "<!-- /agent:queue -->\n",
        );
        let components = element::parse(doc).unwrap();
        let queues = QueueSet::parse(doc, &components).unwrap();
        assert_eq!(queues.selected().unwrap().id, "review");
        assert_eq!(
            queues.block("review").unwrap().component.attrs["subagents"],
            "2"
        );
        assert_eq!(
            queues.block("release").unwrap().component.attrs["preset"],
            "release"
        );
    }

    #[test]
    fn dependent_release_queue_unblocks_after_predecessor_drains() {
        let doc = concat!(
            "<!-- agent:queue id=release-a -->\n",
            "~~- shipped A~~\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=release-b depends=release-a -->\n",
            "- organize B\n",
            "<!-- /agent:queue -->\n",
        );
        let components = element::parse(doc).unwrap();
        let queues = QueueSet::parse(doc, &components).unwrap();
        assert_eq!(queues.selected().unwrap().id, "release-b");
        assert_eq!(queues.selected().unwrap().occurrence, 1);
    }

    #[test]
    fn unowned_bare_hash_head_keeps_queue_runnable() {
        let doc = "<!-- agent:queue -->\n- #unknown-preset\n<!-- /agent:queue -->\n";
        let components = element::parse(doc).unwrap();
        let queues = QueueSet::parse(doc, &components).unwrap();
        assert_eq!(queues.blocks()[0].live_prompt_count(), 1);
        assert!(queues.selected().is_some());
    }

    #[test]
    fn nonempty_dependent_queue_stays_blocked() {
        let doc = concat!(
            "<!-- agent:queue id=release-a -->\n",
            "- finish A\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=release-b depends=release-a -->\n",
            "- organize B\n",
            "<!-- /agent:queue -->\n",
        );
        let components = element::parse(doc).unwrap();
        let queues = QueueSet::parse(doc, &components).unwrap();
        assert_eq!(queues.selected().unwrap().id, "release-a");
    }

    #[test]
    fn rejects_duplicate_ids_missing_dependencies_and_cycles() {
        for (doc, expected) in [
            (
                concat!(
                    "<!-- agent:queue id=a -->\n<!-- /agent:queue -->\n",
                    "<!-- agent:queue id=a -->\n<!-- /agent:queue -->\n",
                ),
                "duplicate agent:queue id",
            ),
            (
                "<!-- agent:queue id=b depends=a -->\n- B\n<!-- /agent:queue -->\n",
                "depends on missing queue id",
            ),
            (
                concat!(
                    "<!-- agent:queue id=a depends=b -->\n- A\n<!-- /agent:queue -->\n",
                    "<!-- agent:queue id=b depends=a -->\n- B\n<!-- /agent:queue -->\n",
                ),
                "dependency cycle",
            ),
        ] {
            let components = element::parse(doc).unwrap();
            let error = QueueSet::parse(doc, &components).unwrap_err().to_string();
            assert!(error.contains(expected), "{error}");
        }
    }
}
