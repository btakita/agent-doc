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
//! A clause that cites a KNOWN id (an item of this document, open or closed,
//! in `agent:backlog` / legacy `agent:pending`, `agent:todo`, `agent:review`,
//! `agent:icebox` or `agent:done`, or an id in the document's external done
//! archive) is a tracked clause: its cited ids block only while they name an
//! OPEN item, so the row unblocks as soon as its last open blocker closes. An
//! id a tracked clause cites that is absent (a PR number such as `#749`, an id
//! moved to another document) does not block on its own.
//!
//! `#gatedheadpending`: a blocker clause that cites NO known id records an
//! external or unknown blocker (`(stays open: blocked behind #vendorfix; ...)`).
//! No agent can advance such a row until that blocker resolves, so it blocks by
//! the ids it cites (all-digit ids such as PR numbers are references, never
//! blockers). It stays blocked until the clause changes, the cited id appears
//! as tracked work, the operator records a disregard (`#blockerdisregard`), or
//! the decision graph records the id settled
//! (`<!-- agent-doc:blocker-settled id -->`). A settlement releases only an
//! external id. An open tracked item of the same id still blocks.
//! A closed blocker the done archive moved out of the document is still known,
//! so archival never turns a resolved tracked blocker into an external one:
//! [`QueueBlockers::from_content_with_archived`] reads the archive ids an IO
//! caller passes in. The text-only [`QueueBlockers::from_content`] cannot see
//! an external archive, so when the document declares one it keeps only the
//! external blockers the IO projection already painted on the item's row
//! (`⛔ ⟦blocked by …⟧`); without an external archive the document holds
//! every closed item and the text-only derivation is exact.
//!
//! The derivation is pure (document text in, map out). Preflight and the
//! projection paths paint `⛔ ⟦blocked by a, b⟧` from it, the drain gates treat
//! a blocked id as deferred, the closeout gate treats a blocked `📥` row as
//! quiescent (`#stopquiescentrows`), and subagent dispatch never offers it.

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
    let mut ids = Vec::new();
    for clause in stays_open_blocker_clauses(text) {
        for id in clause {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

/// The ids each stays-open BLOCKER clause in `text` cites, one list per
/// clause in order (see [`stays_open_blocker_ids`]). A blocker clause that
/// cites no id yields an empty list.
pub fn stays_open_blocker_clauses(text: &str) -> Vec<Vec<String>> {
    stays_open_clause_bodies(text)
        .into_iter()
        .filter(|body| body.to_ascii_lowercase().contains("block"))
        .map(|body| {
            let mut ids: Vec<String> = Vec::new();
            for id in cited_hash_ids(body) {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            ids
        })
        .collect()
}

/// True when `id` is all digits: a pull request or issue number, a reference
/// rather than a tracked-work id.
fn is_reference_number(id: &str) -> bool {
    id.chars().all(|c| c.is_ascii_digit())
}

/// The body of every `(stays open: ...)` clause in `text`, in order, with
/// balanced parentheses and the leading `stays open:` phrase removed
/// (`#ackstrikeopenbacklog` reads the first one as the visible stays-open
/// reason). An unclosed clause runs to the end of `text`.
pub fn stays_open_clause_bodies(text: &str) -> Vec<&str> {
    let lower = text.to_ascii_lowercase();
    let mut bodies = Vec::new();
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
        let phrase_end = (from + rel + STAYS_OPEN_CLAUSE_OPEN.len()).min(body_end);
        bodies.push(&text[phrase_end..body_end]);
        from = body_end.max(body_start);
    }
    bodies
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
    /// `#gatedheadpending`: the text-only derivation of a document with an
    /// external done archive found a blocker clause citing only ids absent
    /// from the document, which an archive read could resolve. IO callers
    /// rederive with [`Self::from_content_with_archived`] when this is set.
    archive_lookup_needed: bool,
}

/// What the derivation knows about closed items outside the document text.
#[derive(Clone, Copy)]
enum ArchiveKnowledge<'a> {
    /// Text only: trust the painted `⛔` for clauses an archive could resolve.
    TextOnly,
    /// The own ids of the document's external done archive.
    Archived(&'a HashSet<String>),
}

impl QueueBlockers {
    /// Derive the blocked set from `content` alone (see the module docs).
    pub fn from_content(content: &str) -> Self {
        Self::derive(content, ArchiveKnowledge::TextOnly)
    }

    /// Derive the blocked set from `content` with the own ids of its external
    /// done archive (`agent:done archive`), read by the caller. A clause that
    /// cites an archived id is tracked work that closed, never an external
    /// blocker.
    pub fn from_content_with_archived(content: &str, archived: &HashSet<String>) -> Self {
        Self::derive(content, ArchiveKnowledge::Archived(archived))
    }

    /// True when the text-only derivation left an external-blocker question an
    /// archive read answers (see the `archive_lookup_needed` field).
    pub fn archive_lookup_needed(&self) -> bool {
        self.archive_lookup_needed
    }

    fn derive(content: &str, archive: ArchiveKnowledge<'_>) -> Self {
        let Ok(components) = element::parse(content) else {
            return Self::default();
        };
        // Open tracked items and every text that can carry their clauses.
        let mut open: HashSet<String> = HashSet::new();
        let mut texts: BTreeMap<String, Vec<String>> = BTreeMap::new();
        // `#gatedheadpending`: every id this document tracks or tracked, open
        // or closed. A blocker clause that cites none of them is external.
        let mut known: HashSet<String> = HashSet::new();
        let mut external_archive = false;
        for comp in &components {
            if element::is_backlog_done_component(&comp.name) {
                external_archive |= comp.attrs.contains_key("archive");
            }
            if element::is_tracked_work_component(&comp.name)
                || element::is_backlog_done_component(&comp.name)
                || todo::is_todo_component(&comp.name)
            {
                known.extend(backlog::extract_done_item_own_ids(comp.content(content)));
            }
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
        // `#preflightqueuescale`: one todo parse for every open id, not one
        // whole-document parse per open todo.
        let todos = todo::OwnedTodoIndex::from_components(content, &components);
        for id in todos.open_ids() {
            let id = backlog::normalize_pending_id(id);
            if id.is_empty() {
                continue;
            }
            open.insert(id.clone());
            if let Some(item) = todos.owning(&id) {
                texts.entry(id).or_default().push(item.text.clone());
            }
        }
        if open.is_empty() {
            return Self::default();
        }
        known.extend(open.iter().cloned());
        if let ArchiveKnowledge::Archived(archived) = archive {
            known.extend(archived.iter().map(|id| backlog::normalize_pending_id(id)));
        }
        // `#blockerdisregard`: an operator record names the exact invented
        // edges to ignore. Parsing the comment does not call back into this
        // derivation. Edges the comment does not name still block.
        let disregarded = crate::blocker_disregard::QueueDisregards::from_content(content);
        // External ids the decision graph has already settled. Tracked open
        // ids are not consulted here; `add` still requires the dep to be open.
        let settled = settled_external_ids(content);
        let after_deps = crate::backlog_sync::collect_after_deps(&components, content);
        let mut by_item: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut add = |item: &str, dep: &str| -> bool {
            let dep = backlog::normalize_pending_id(dep);
            if dep.is_empty() || dep == item || !open.contains(&dep) {
                return false;
            }
            if disregarded.ignores(item, &dep) {
                return false;
            }
            let slot = by_item.entry(item.to_string()).or_default();
            if !slot.contains(&dep) {
                slot.push(dep);
            }
            true
        };
        let mut clause_blocked = HashSet::new();
        // `#gatedheadpending`: external clauses, resolved after the tracked
        // edges because the text-only resolution reads the painted rows.
        let mut external: Vec<(String, Vec<String>)> = Vec::new();
        for (id, item_texts) in &texts {
            for text in item_texts {
                for clause in stays_open_blocker_clauses(text) {
                    let cited: Vec<String> = clause
                        .iter()
                        .map(|dep| backlog::normalize_pending_id(dep))
                        .filter(|dep| !dep.is_empty() && dep != id)
                        .collect();
                    if cited.iter().any(|dep| known.contains(dep)) {
                        for dep in &cited {
                            if add(id, dep) {
                                clause_blocked.insert(id.clone());
                            }
                        }
                    } else {
                        let unknown: Vec<String> = cited
                            .into_iter()
                            .filter(|dep| !is_reference_number(dep))
                            .collect();
                        if !unknown.is_empty() {
                            external.push((id.clone(), unknown));
                        }
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
        let mut archive_lookup_needed = false;
        let painted = match archive {
            ArchiveKnowledge::TextOnly if external_archive && !external.is_empty() => {
                archive_lookup_needed = true;
                Some(painted_blocker_ids(content, &components))
            }
            _ => None,
        };
        for (id, deps) in external {
            for dep in deps {
                // Text only, with an external archive: the cited id may be a
                // closed item the archive holds, so only the IO projection's
                // painted `⛔` keeps it blocking until a rederivation.
                if painted
                    .as_ref()
                    .is_some_and(|painted| !painted.get(&id).is_some_and(|ids| ids.contains(&dep)))
                {
                    continue;
                }
                if disregarded.ignores(&id, &dep) || settled.contains(&dep) {
                    continue;
                }
                let slot = by_item.entry(id.clone()).or_default();
                if !slot.contains(&dep) {
                    slot.push(dep);
                }
                clause_blocked.insert(id.clone());
            }
        }
        Self {
            by_item,
            clause_blocked,
            archive_lookup_needed,
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

/// External blocker ids the decision graph has settled. The marker is written
/// by preflight after a linked pull request closes or a decision names the id.
fn settled_external_ids(content: &str) -> HashSet<String> {
    const PREFIX: &str = "<!-- agent-doc:blocker-settled";
    let mut ids = HashSet::new();
    for line in content.lines() {
        let Some(rest) = line.trim().strip_prefix(PREFIX) else {
            continue;
        };
        let Some(body) = rest.split("-->").next() else {
            continue;
        };
        for id in body.split_whitespace() {
            let id = backlog::normalize_pending_id(id.trim_matches('#'));
            if !id.is_empty() && !is_reference_number(&id) {
                ids.insert(id);
            }
        }
    }
    ids
}

/// Blocker ids painted on live queue rows (`⛔ ⟦blocked by a, b⟧ do [#x]`),
/// by the tracked item each row names.
fn painted_blocker_ids(
    content: &str,
    components: &[element::Component],
) -> BTreeMap<String, HashSet<String>> {
    let mut painted: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    for comp in components.iter().filter(|comp| comp.name == "queue") {
        let Ok(entries) = crate::document_queue::parse(comp.content(content)) else {
            continue;
        };
        for entry in &entries {
            let crate::document_queue::QueueEntry::Prompt(prompt) = entry else {
                continue;
            };
            let ids = agent_doc_document::queue_projection::blocked_marker_ids(&prompt.text);
            if ids.is_empty() {
                continue;
            }
            if let QueueItemIdentity::Id(item) = crate::queue_claim::claim_identity(&prompt.text) {
                painted
                    .entry(backlog::normalize_pending_id(&item))
                    .or_default()
                    .extend(ids.iter().map(|id| backlog::normalize_pending_id(id)));
            }
        }
    }
    painted
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
    fn a_recorded_disregard_drops_only_the_named_edge() {
        let doc = format!(
            "<!-- agent-doc:blocker-disregard queue=bossroom-sbx-provision:sbx-deploy-749 -->\n{}",
            sdk_demo_like()
        );
        let blockers = queue_blockers(&doc);
        assert_eq!(blockers.blockers_for_id("bossroom-sbx-provision"), None);
        assert_eq!(
            blockers.blockers_for_id("bossroom-stg-provision"),
            Some(&["bossroom-sbx-provision".to_string()][..])
        );
        assert!(
            !blockers.deferred_ids().contains("bossroom-sbx-provision"),
            "the disregarded stays-open edge is no longer a drain deferral"
        );
        assert!(blockers.deferred_ids().contains("bossroom-stg-provision"));
    }

    /// `#gatedheadpending` (an sdk session, 2026-11-14): a worker handed back
    /// `(stays open: blocked behind #vendorfix; ...)` naming a blocker the
    /// document does not track. The head stayed a drainable `📥` that no
    /// agent could advance. A clause citing no known id is an external
    /// blocker: it blocks by the ids it cites until the clause changes.
    fn external_doc(done: &str) -> String {
        format!(
            concat!(
                "<!-- agent:queue -->\n",
                "- 📥 do [#licensefix]\n",
                "- 📥 do [#prnumber]\n",
                "- 📥 do [#plain]\n",
                "<!-- /agent:queue -->\n",
                "<!-- agent:backlog -->\n",
                "- [ ] [#licensefix] Remove the stale sentence. (stays open: blocked behind #vendorfix; the vendor PR still lacks the file)\n",
                "- [ ] [#prnumber] Land the bump. (stays open: blocked behind PR #12)\n",
                "- [ ] [#plain] Runnable work.\n",
                "<!-- /agent:backlog -->\n",
                "{}",
            ),
            done
        )
    }

    #[test]
    fn a_clause_naming_only_unknown_ids_is_an_external_blocker() {
        let doc = external_doc("");
        let blockers = queue_blockers(&doc);
        assert_eq!(
            blockers.blockers_for_id("licensefix"),
            Some(&["vendorfix".to_string()][..])
        );
        assert!(blockers.deferred_ids().contains("licensefix"));
        assert!(blockers.blocks_row("📥 do [#licensefix]"));
        assert!(
            !blockers.archive_lookup_needed(),
            "without an external archive the text holds every closed item"
        );
        // A pull request number is a reference, never a blocker id.
        assert_eq!(blockers.blockers_for_id("prnumber"), None);
        // An item with no blocker clause still runs.
        assert_eq!(blockers.blockers_for_id("plain"), None);

        // Dropping the clause, or recording a disregard, releases the row.
        let cleared = doc.replace(
            " (stays open: blocked behind #vendorfix; the vendor PR still lacks the file)",
            "",
        );
        assert!(queue_blockers(&cleared).is_empty());
        let disregarded =
            format!("<!-- agent-doc:blocker-disregard queue=licensefix:vendorfix -->\n{doc}");
        assert_eq!(
            queue_blockers(&disregarded).blockers_for_id("licensefix"),
            None
        );
    }

    /// A settled external id is not a wait. The same id, once it is an open
    /// tracked item, still blocks: the marker does not override real work.
    #[test]
    fn a_settlement_marker_releases_only_an_external_blocker() {
        let doc = external_doc("");
        let settled = format!("<!-- agent-doc:blocker-settled vendorfix -->\n{doc}");
        assert_eq!(queue_blockers(&settled).blockers_for_id("licensefix"), None);
        assert!(
            !queue_blockers(&settled)
                .deferred_ids()
                .contains("licensefix")
        );
        let open = format!(
            "<!-- agent-doc:blocker-settled vendorfix -->\n{}",
            external_doc("<!-- agent:todo -->\n- [ ] [#vendorfix] land it\n<!-- /agent:todo -->\n")
        );
        assert_eq!(
            queue_blockers(&open).blockers_for_id("licensefix"),
            Some(&["vendorfix".to_string()][..])
        );
    }

    #[test]
    fn a_closed_or_archived_blocker_is_resolved_not_external() {
        // Closed in place, or reaped into the inline `agent:done`.
        let closed =
            external_doc("<!-- agent:todo -->\n- [x] [#vendorfix] merged\n<!-- /agent:todo -->\n");
        assert_eq!(queue_blockers(&closed).blockers_for_id("licensefix"), None);
        let inline_done = external_doc(
            "<!-- agent:done -->\n- 2026-11-14 [#vendorfix] merged\n<!-- /agent:done -->\n",
        );
        assert_eq!(
            queue_blockers(&inline_done).blockers_for_id("licensefix"),
            None
        );
        // An open tracked blocker blocks by that id, as before.
        let open =
            external_doc("<!-- agent:todo -->\n- [ ] [#vendorfix] land it\n<!-- /agent:todo -->\n");
        assert_eq!(
            queue_blockers(&open).blockers_for_id("licensefix"),
            Some(&["vendorfix".to_string()][..])
        );

        // With an external archive the text cannot tell an archived blocker
        // from an unknown one: the archive ids decide.
        let archived_doc = external_doc(
            "<!-- agent:done archive -->\n<!-- completed work archived in x.done.md -->\n<!-- /agent:done -->\n",
        );
        let text_only = queue_blockers(&archived_doc);
        assert!(text_only.archive_lookup_needed());
        assert_eq!(
            text_only.blockers_for_id("licensefix"),
            None,
            "unpainted and unresolved, the text-only read keeps the row runnable"
        );
        let in_archive: HashSet<String> = ["vendorfix".to_string()].into();
        assert_eq!(
            QueueBlockers::from_content_with_archived(&archived_doc, &in_archive)
                .blockers_for_id("licensefix"),
            None
        );
        let elsewhere: HashSet<String> = ["unrelated".to_string()].into();
        let resolved = QueueBlockers::from_content_with_archived(&archived_doc, &elsewhere);
        assert_eq!(
            resolved.blockers_for_id("licensefix"),
            Some(&["vendorfix".to_string()][..])
        );
        assert!(!resolved.archive_lookup_needed());

        // Once the IO projection painted the row, text-only readers (drain
        // gates, dispatch facts) agree with it.
        let painted = archived_doc.replace(
            "- 📥 do [#licensefix]",
            "- ⛔ ⟦blocked by vendorfix⟧ do [#licensefix]",
        );
        assert_eq!(
            queue_blockers(&painted).blockers_for_id("licensefix"),
            Some(&["vendorfix".to_string()][..])
        );
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
