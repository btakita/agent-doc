//! `#queueblocked`: which live queue rows are blocked, and by what.
//!
//! A queue row is blocked when the tracked item it names (`do [#id]`) still
//! waits on other work that is open in this document:
//!
//! - a stays-open blocker clause in the item's text,
//!   `(stays open: blocked behind #a, #b)`, names `#a` / `#b`; or
//! - an `after=#id` edge (or a nested ordered-list predecessor in
//!   `agent:queue`) names a prerequisite.
//!
//! A cited id blocks only while it names an OPEN item in `agent:backlog`
//! (or legacy `agent:pending`), `agent:todo`, or `agent:review`. An id that
//! is closed, struck, archived, or absent from the document (a PR number such
//! as `#749`, an id moved to another document) does not block, so a row
//! unblocks as soon as its last open blocker closes. A clause that names no
//! open id therefore leaves the row runnable: the binary derives blocked
//! state only from ids it can watch close.
//!
//! The derivation is pure (document text in, map out). Preflight and the
//! projection paths paint `⛔ ⟦blocked by a, b⟧` from it, the drain gates treat
//! a blocked id as deferred, and subagent dispatch never offers it.

use std::collections::{BTreeMap, HashSet};

use agent_doc_element::{element, todo};
use agent_doc_element_backlog::backlog;
use agent_doc_element_queue::QueueItemIdentity;

/// The phrase that opens a stays-open clause, matched case-insensitively.
pub const STAYS_OPEN_CLAUSE_OPEN: &str = "(stays open:";

/// Ids cited by every stays-open BLOCKER clause in `text`, lowercased and
/// deduplicated in order of appearance.
///
/// A clause is `(stays open: ...)` with balanced parentheses; it is a blocker
/// clause only when its body says so (`blocked`, `blocker`, `blocking`). An id
/// is a `#` followed by `[A-Za-z0-9_-]`, not preceded by an alphanumeric
/// character, so `[#a]`, `#a,` and `(#a)` all cite `a` while `PR#2` cites
/// nothing. Whether a cited id is tracked work is the caller's question.
pub fn stays_open_blocker_ids(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let mut ids = Vec::new();
    let mut from = 0;
    while let Some(rel) = lower[from..].find(STAYS_OPEN_CLAUSE_OPEN) {
        let body_start = from + rel + 1;
        let mut depth = 1usize;
        let mut body_end = text.len();
        for (offset, ch) in text[body_start..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        body_end = body_start + offset;
                        break;
                    }
                }
                _ => {}
            }
        }
        if lower[body_start..body_end].contains("block") {
            for id in cited_hash_ids(&text[body_start..body_end]) {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        from = body_end.max(body_start);
    }
    ids
}

fn cited_hash_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut previous: Option<char> = None;
    for (index, ch) in text.char_indices() {
        if ch == '#' && !previous.is_some_and(|prev| prev.is_alphanumeric()) {
            let id: String = text[index + 1..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                .collect();
            let id = id.trim_end_matches(['-', '_']).to_ascii_lowercase();
            if id.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) {
                ids.push(id);
            }
        }
        previous = Some(ch);
    }
    ids
}

/// Blocked tracked items of one document: item id -> the open ids it waits on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueBlockers {
    by_item: BTreeMap<String, Vec<String>>,
    /// Items blocked by at least one open id their stays-open clause cites.
    /// Only these are folded into the drain deferral set: an `after=` edge
    /// already gates drain counts (`#dagdraingate`) and orders selection
    /// (auto-DAG), and a head already in progress must not be preempted by a
    /// dependency edit (GH #162).
    clause_blocked: HashSet<String>,
}

impl QueueBlockers {
    /// Derive the blocked set from `content` (see the module docs).
    pub fn from_content(content: &str) -> Self {
        let Ok(components) = element::parse(content) else {
            return Self::default();
        };
        // Open tracked items and every text that can carry their clauses.
        let mut open: HashSet<String> = HashSet::new();
        let mut texts: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for comp in &components {
            if !(element::is_backlog_component(&comp.name)
                || element::is_review_component(&comp.name))
            {
                continue;
            }
            let (_, items, _) = backlog::parse_items(comp.content(content));
            for item in items.into_iter().filter(|item| !item.is_done()) {
                let id = backlog::normalize_pending_id(&item.id);
                if id.is_empty() {
                    continue;
                }
                open.insert(id.clone());
                let slot = texts.entry(id).or_default();
                slot.push(item.text);
                if !item.continuation.is_empty() {
                    slot.push(item.continuation);
                }
            }
        }
        for id in todo::open_todo_ids(content) {
            let id = backlog::normalize_pending_id(&id);
            if id.is_empty() {
                continue;
            }
            open.insert(id.clone());
            if let Some(item) = todo::todo_item_owning_id(content, &id) {
                texts.entry(id).or_default().push(item.text);
            }
        }
        if open.is_empty() {
            return Self::default();
        }
        let after_deps = crate::backlog_sync::collect_after_deps(&components, content);
        let mut by_item: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut add = |item: &str, dep: &str| -> bool {
            let dep = backlog::normalize_pending_id(dep);
            if dep.is_empty() || dep == item || !open.contains(&dep) {
                return false;
            }
            let slot = by_item.entry(item.to_string()).or_default();
            if !slot.contains(&dep) {
                slot.push(dep);
            }
            true
        };
        let mut clause_blocked = HashSet::new();
        for (id, item_texts) in &texts {
            for text in item_texts {
                for dep in stays_open_blocker_ids(text) {
                    if add(id, &dep) {
                        clause_blocked.insert(id.clone());
                    }
                }
            }
        }
        for (id, deps) in &after_deps {
            let id = backlog::normalize_pending_id(id);
            if !open.contains(&id) {
                continue;
            }
            for dep in deps {
                add(&id, dep);
            }
        }
        Self {
            by_item,
            clause_blocked,
        }
    }

    /// True when no tracked item is blocked.
    pub fn is_empty(&self) -> bool {
        self.by_item.is_empty()
    }

    /// The open ids blocking tracked item `id`, or `None` when it is runnable.
    pub fn blockers_for_id(&self, id: &str) -> Option<&[String]> {
        self.by_item
            .get(&backlog::normalize_pending_id(id))
            .map(Vec::as_slice)
    }

    /// The open ids blocking the tracked item a queue row names, or `None`.
    /// Free-text rows name no tracked item and are never blocked.
    pub fn blockers_for_row(&self, text: &str) -> Option<&[String]> {
        match crate::queue_claim::claim_identity(text) {
            QueueItemIdentity::Id(id) => self.blockers_for_id(&id),
            QueueItemIdentity::FreeText(_) => None,
        }
    }

    /// True when the queue row names a blocked tracked item.
    pub fn blocks_row(&self, text: &str) -> bool {
        self.blockers_for_row(text).is_some()
    }

    /// Every blocked tracked-item id, lowercased.
    pub fn blocked_ids(&self) -> HashSet<String> {
        self.by_item.keys().cloned().collect()
    }

    /// Blocked ids the drain gates defer and the skip pass exempts: items
    /// with an open stays-open blocker (see the `clause_blocked` field).
    pub fn deferred_ids(&self) -> HashSet<String> {
        self.clause_blocked.clone()
    }

    /// `(item, blockers)` pairs in id order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.by_item
            .iter()
            .map(|(id, deps)| (id.as_str(), deps.as_slice()))
    }
}

/// [`QueueBlockers::from_content`].
pub fn queue_blockers(content: &str) -> QueueBlockers {
    QueueBlockers::from_content(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shapes observed in haiven-dev `tasks/sdk/sdk-demo.md` on
    /// 2026-10-27: `#749` is a PR number, `#template-api-create` lives in
    /// another document, and only the ids open here block.
    fn sdk_demo_like() -> String {
        concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Run <!-- agent:run priority subagents go -->\n",
            "- ⏭\u{fe0f} do [#bossroom-stg-provision]\n",
            "- 📥 do [#bossroom-sbx-provision]\n",
            "- 📥 do #sbx-deploy-749\n",
            "- 📥 do [#runnable]\n",
            "<!-- /agent:run -->\n\n",
            "## Todo <!-- agent:todo -->\n",
            "- [ ] [#sbx-deploy-749] Pick how haiven-api #749 reaches SBX.\n",
            "<!-- /agent:todo -->\n\n",
            "## Backlog <!-- agent:backlog queue -->\n",
            "- [ ] [#bossroom-sbx-provision] Apply the template in SBX. (stays open: blocked behind #template-api-create, #template-sdk-manifest and the haiven-api dev deploy to SBX, #sbx-deploy-749) (todos: [#sbx-deploy-749])\n",
            "- [ ] [#bossroom-stg-provision] Repeat #bossroom-sbx-provision in STG. (stays open: blocked behind #bossroom-sbx-provision and a haiven-api release from main containing the template work and #749; release PR #752 has changes requested) (todos: [#sbx-deploy-749])\n",
            "- [ ] [#runnable] Unblocked work.\n",
            "<!-- /agent:backlog -->\n",
        )
        .to_string()
    }

    #[test]
    fn clause_ids_are_parsed_with_nested_parentheses() {
        assert_eq!(
            stays_open_blocker_ids(
                "x (stays open: blocked behind #a (see PR #12), [#b] and #c-d.) tail #e"
            ),
            vec!["a", "12", "b", "c-d"]
        );
        assert_eq!(
            stays_open_blocker_ids("x (Stays Open: Blocked behind #A)"),
            vec!["a"]
        );
        // Not a blocker clause, not a stays-open clause, or no `#` boundary.
        assert!(stays_open_blocker_ids("(stays open: waiting for the operator on #a)").is_empty());
        assert!(stays_open_blocker_ids("blocked behind #a").is_empty());
        assert_eq!(
            stays_open_blocker_ids("(stays open: blocked behind PR#2 and #b"),
            vec!["b"]
        );
    }

    #[test]
    fn only_ids_open_in_the_document_block() {
        let blockers = queue_blockers(&sdk_demo_like());
        assert_eq!(
            blockers.blockers_for_id("bossroom-sbx-provision"),
            Some(&["sbx-deploy-749".to_string()][..])
        );
        assert_eq!(
            blockers.blockers_for_id("bossroom-stg-provision"),
            Some(&["bossroom-sbx-provision".to_string()][..])
        );
        assert_eq!(blockers.blockers_for_id("runnable"), None);
        assert_eq!(
            blockers.deferred_ids(),
            ["bossroom-sbx-provision", "bossroom-stg-provision"]
                .map(String::from)
                .into()
        );
        assert_eq!(blockers.blockers_for_id("sbx-deploy-749"), None);
        assert!(blockers.blocks_row("⏭\u{fe0f} do [#bossroom-stg-provision]"));
        assert!(blockers.blocks_row("#subagents do [#bossroom-sbx-provision]"));
        assert!(!blockers.blocks_row("do #sbx-deploy-749"));
        assert!(!blockers.blocks_row("free text about #bossroom-sbx-provision"));
    }

    #[test]
    fn a_row_unblocks_when_its_last_open_blocker_closes() {
        let closed = sdk_demo_like().replace(
            "- [ ] [#sbx-deploy-749] Pick",
            "- [x] [#sbx-deploy-749] Pick",
        );
        let blockers = queue_blockers(&closed);
        assert_eq!(blockers.blockers_for_id("bossroom-sbx-provision"), None);
        // stg still waits on sbx, which stays open on its own.
        assert!(blockers.blockers_for_id("bossroom-stg-provision").is_some());

        let removed = closed.replace(
            "- [ ] [#bossroom-sbx-provision]",
            "- [x] [#bossroom-sbx-provision]",
        );
        assert!(queue_blockers(&removed).is_empty());
    }

    #[test]
    fn after_edges_and_review_items_block_while_open() {
        let doc = concat!(
            "<!-- agent:run -->\n- do [#dep]\n<!-- /agent:run -->\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#dep] after=#pre dependent work\n",
            "- [ ] [#gate-dep] after=#gated needs the gated item merged\n",
            "<!-- /agent:backlog -->\n",
            "<!-- agent:review -->\n",
            "- [/] [#pre] waiting for verification\n",
            "- [/] [#gated] PR open\n",
            "<!-- /agent:review -->\n",
        );
        let blockers = queue_blockers(doc);
        assert_eq!(
            blockers.blockers_for_id("dep"),
            Some(&["pre".to_string()][..])
        );
        assert_eq!(
            blockers.blockers_for_id("gate-dep"),
            Some(&["gated".to_string()][..])
        );
        assert!(
            blockers.deferred_ids().is_empty(),
            "after= edges paint ⛔ but stay with the auto-DAG drain gate"
        );
        let merged = doc.replace("- [/] [#pre]", "- [x] [#pre]");
        assert_eq!(queue_blockers(&merged).blockers_for_id("dep"), None);
    }

    #[test]
    fn self_citation_never_blocks() {
        let doc = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#a] (stays open: blocked behind #a) after=#a\n",
            "<!-- /agent:backlog -->\n",
        );
        assert!(queue_blockers(doc).is_empty());
    }
}
