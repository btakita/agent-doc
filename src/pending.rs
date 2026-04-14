//! # Module: pending
//!
//! Pure functions for parsing and mutating the `agent:pending` component body.
//!
//! Each pending item carries:
//! - a GFM task-list checkbox (`- [ ]` or `- [x]`)
//! - a 4-char hash id prefix rendered as `[#xxxx]`
//! - free-form text
//!
//! Canonical form: `- [ ] [#a3f2] refactor preflight commit path`
//!
//! This module is I/O-free. Callers (`pending_cmd.rs`, `preflight.rs`, `write.rs`)
//! handle reading/writing files, locking, and git commits.
//!
//! ## Spec
//! - Parser accepts legacy forms and normalizes via `backfill`.
//! - Hash ids are stable across edits/reorders; generated once, preserved thereafter.
//! - `reap` removes `- [x]` items. `detect_reorder` diffs id order between snapshot/current.
//! - Mutation ops (`op_add/done/edit/clear/reorder`) return a new body string, never mutate in place.

use anyhow::{Result, anyhow, bail};
use std::collections::HashSet;

/// Lifecycle state for a pending item, encoded by its GFM checkbox.
///
/// - `Open` (`[ ]`) — active or not started; default for new items.
/// - `Gated` (`[/]`) — code-complete, awaiting an external gate (release,
///   telemetry, field validation). Never auto-reaped.
/// - `Done` (`[x]`) — fully complete; reaped on the next preflight cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingState {
    Open,
    Gated,
    Done,
}

impl PendingState {
    /// Single character for the GFM checkbox body.
    pub fn box_char(self) -> char {
        match self {
            PendingState::Open => ' ',
            PendingState::Gated => '/',
            PendingState::Done => 'x',
        }
    }

    /// Parse the inside of a `[…]` checkbox. Accepts `[X]` as `Done`.
    /// Used by the upcoming `pending_cmd::gate/ungate` state-transition layer (Phase 2).
    #[allow(dead_code)]
    pub fn from_box_char(c: char) -> Option<PendingState> {
        match c {
            ' ' => Some(PendingState::Open),
            '/' => Some(PendingState::Gated),
            'x' | 'X' => Some(PendingState::Done),
            _ => None,
        }
    }
}

/// A parsed pending list item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingItem {
    /// 4+ char lowercase base32 hash id (no `#` prefix).
    pub id: String,
    /// Lifecycle state encoded by the GFM checkbox.
    pub state: PendingState,
    /// Bullet text after the hash prefix.
    pub text: String,
}

impl PendingItem {
    /// Render to canonical `- [<state>] [#id] text` form.
    pub fn render(&self) -> String {
        format!("- [{}] [#{}] {}", self.state.box_char(), self.id, self.text)
    }

    /// Convenience: true when state is `Done` (`[x]`).
    pub fn is_done(&self) -> bool {
        matches!(self.state, PendingState::Done)
    }
}

/// Parse the pending component body into (prelude, items, postlude).
///
/// - Prelude: leading non-list lines (whitespace, non `- ` bullets).
/// - Items: parsed `- ...` list entries (legacy or fully-migrated).
/// - Postlude: trailing non-list lines after the last item.
pub fn parse_items(body: &str) -> (String, Vec<PendingItem>, String) {
    let lines: Vec<&str> = body.lines().collect();

    // Find first list item line.
    let first_item = lines.iter().position(|l| is_item_line(l));
    let first_item = match first_item {
        Some(i) => i,
        None => return (body.to_string(), Vec::new(), String::new()),
    };

    // Find last list item line.
    let last_item = lines
        .iter()
        .rposition(|l| is_item_line(l))
        .unwrap_or(first_item);

    let prelude = join_lines(&lines[..first_item], has_trailing_newline(body) || first_item > 0);
    let postlude = if last_item + 1 < lines.len() {
        join_lines(&lines[last_item + 1..], has_trailing_newline(body))
    } else {
        String::new()
    };

    let mut items = Vec::new();
    for line in &lines[first_item..=last_item] {
        if let Some(item) = parse_item_line(line) {
            items.push(item);
        }
        // Non-item lines interleaved between items are dropped — callers must run
        // backfill first to normalize. Blank lines are treated the same.
    }

    (prelude, items, postlude)
}

fn is_item_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("- ") || t == "-"
}

fn has_trailing_newline(body: &str) -> bool {
    body.ends_with('\n')
}

/// Rejoin lines with `\n`, preserving a trailing newline when `with_trailing` is true.
fn join_lines(lines: &[&str], with_trailing: bool) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut s = lines.join("\n");
    if with_trailing {
        s.push('\n');
    }
    s
}

/// Parse a single list item line into a `PendingItem` (id optional).
///
/// Returns `None` when the line is not a list item. When the id is missing,
/// the returned item has an empty id — callers must run `backfill` to assign one.
fn parse_item_line(line: &str) -> Option<PendingItem> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("- ")?;
    let rest = rest.trim_start();

    // Checkbox?
    let (state, after_box) = if let Some(r) = rest.strip_prefix("[ ]") {
        (PendingState::Open, r.trim_start())
    } else if let Some(r) = rest.strip_prefix("[/]") {
        (PendingState::Gated, r.trim_start())
    } else if let Some(r) = rest.strip_prefix("[x]") {
        (PendingState::Done, r.trim_start())
    } else if let Some(r) = rest.strip_prefix("[X]") {
        (PendingState::Done, r.trim_start())
    } else {
        (PendingState::Open, rest)
    };

    // Hash id?
    let (id, text) = if let Some(after_hash) = after_box.strip_prefix("[#") {
        if let Some(close) = after_hash.find(']') {
            let id_raw = &after_hash[..close];
            let tail = after_hash[close + 1..].trim_start();
            if is_valid_hash_id(id_raw) {
                (id_raw.to_lowercase(), tail.to_string())
            } else {
                (String::new(), after_box.to_string())
            }
        } else {
            (String::new(), after_box.to_string())
        }
    } else {
        (String::new(), after_box.to_string())
    };

    Some(PendingItem {
        id,
        state,
        text: text.trim_end().to_string(),
    })
}

fn is_valid_hash_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 8
        && s.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Serialize items back to a body string.
pub fn render_items(prelude: &str, items: &[PendingItem], postlude: &str) -> String {
    let mut out = String::new();
    out.push_str(prelude);
    if !prelude.is_empty() && !prelude.ends_with('\n') {
        out.push('\n');
    }
    for item in items {
        out.push_str(&item.render());
        out.push('\n');
    }
    if !postlude.is_empty() {
        out.push_str(postlude);
        if !postlude.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// Generate a stable 4-char base32 hash from `(text, doc_id, counter)`.
///
/// Uses SHA-256 (already a dependency) and crockford-ish base32 (lowercase, no padding)
/// on the first 4 chars of the alphabet. Collisions are the caller's responsibility —
/// see `backfill`, which retries with the counter incremented.
pub fn generate_hash(text: &str, doc_id: &str, counter: u64) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hasher.update(b":");
    hasher.update(doc_id.as_bytes());
    hasher.update(b":");
    hasher.update(counter.to_le_bytes());
    let digest = hasher.finalize();

    // Base32 lowercase alphabet (no 0/1/8/9 per crockford would complicate collisions;
    // stick with full a-z/0-9 subset for simplicity — we only need 20 bits).
    const ALPHABET: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz"; // 32 chars
    // Take first 20 bits of the digest → 4 base32 chars.
    let b0 = digest[0] as u32;
    let b1 = digest[1] as u32;
    let b2 = digest[2] as u32;
    let v: u32 = (b0 << 16) | (b1 << 8) | b2; // 24 bits; use top 20
    let c0 = (v >> 15) & 0x1f;
    let c1 = (v >> 10) & 0x1f;
    let c2 = (v >> 5) & 0x1f;
    let c3 = v & 0x1f;
    let mut out = String::with_capacity(4);
    out.push(ALPHABET[c0 as usize] as char);
    out.push(ALPHABET[c1 as usize] as char);
    out.push(ALPHABET[c2 as usize] as char);
    out.push(ALPHABET[c3 as usize] as char);
    out
}

/// Lazy backfill: ensure every item has a hash id and a checkbox.
///
/// - Items missing a hash get a new one (guaranteed unique within the component).
/// - Checkboxes are normalized (default `[ ]`).
/// - Returns `(new_body, changed)`. `changed = false` when the body was already canonical.
pub fn backfill(body: &str, doc_id: &str, existing_ids: &HashSet<String>) -> (String, bool) {
    let (prelude, items, postlude) = parse_items(body);
    let mut taken: HashSet<String> = existing_ids.clone();
    for item in &items {
        if !item.id.is_empty() {
            taken.insert(item.id.clone());
        }
    }

    let mut changed = false;
    let mut new_items = Vec::with_capacity(items.len());
    for item in items {
        if item.id.is_empty() {
            // Assign a new id — retry on collision.
            let mut counter = 0u64;
            let mut id = generate_hash(&item.text, doc_id, counter);
            while taken.contains(&id) {
                counter += 1;
                id = generate_hash(&item.text, doc_id, counter);
            }
            taken.insert(id.clone());
            changed = true;
            new_items.push(PendingItem { id, ..item });
        } else {
            new_items.push(item);
        }
    }

    let new_body = render_items(&prelude, &new_items, &postlude);

    // Also mark as changed when the canonical render differs from the input
    // (e.g., legacy whitespace / missing checkbox normalization).
    if new_body != body {
        changed = true;
    }
    (new_body, changed)
}

/// Reap `[x]` items. `[/]` (gated) items are never reaped.
/// Returns `(new_body, removed_ids)`.
pub fn reap(body: &str) -> (String, Vec<String>) {
    let (prelude, items, postlude) = parse_items(body);
    let mut removed = Vec::new();
    let mut kept = Vec::new();
    for item in items {
        if item.is_done() {
            if !item.id.is_empty() {
                removed.push(item.id.clone());
            }
        } else {
            kept.push(item);
        }
    }
    if removed.is_empty() {
        return (body.to_string(), removed);
    }
    let new_body = render_items(&prelude, &kept, &postlude);
    (new_body, removed)
}

/// Detect reorder: returns `Some(current_order)` when id-sets match but order differs.
/// Returns `None` when id-sets differ or order is identical.
pub fn detect_reorder(snapshot_body: &str, current_body: &str) -> Option<Vec<String>> {
    let (_, snap_items, _) = parse_items(snapshot_body);
    let (_, cur_items, _) = parse_items(current_body);

    let snap_ids: Vec<String> = snap_items
        .iter()
        .filter(|i| !i.id.is_empty())
        .map(|i| i.id.clone())
        .collect();
    let cur_ids: Vec<String> = cur_items
        .iter()
        .filter(|i| !i.id.is_empty())
        .map(|i| i.id.clone())
        .collect();

    if snap_ids.len() != cur_ids.len() {
        return None;
    }
    let snap_set: HashSet<&String> = snap_ids.iter().collect();
    let cur_set: HashSet<&String> = cur_ids.iter().collect();
    if snap_set != cur_set {
        return None;
    }
    if snap_ids == cur_ids {
        return None;
    }
    Some(cur_ids)
}

/// Append a new item to the body. Binary assigns hash and `[ ]`.
pub fn op_add(body: &str, text: &str, doc_id: &str) -> Result<String> {
    let text = text.trim();
    if text.is_empty() {
        bail!("pending add: text must be non-empty");
    }
    let (prelude, mut items, postlude) = parse_items(body);
    let mut taken: HashSet<String> = items
        .iter()
        .filter(|i| !i.id.is_empty())
        .map(|i| i.id.clone())
        .collect();

    let mut counter = 0u64;
    let mut id = generate_hash(text, doc_id, counter);
    while taken.contains(&id) {
        counter += 1;
        id = generate_hash(text, doc_id, counter);
    }
    taken.insert(id.clone());

    items.push(PendingItem {
        id,
        state: PendingState::Open,
        text: text.to_string(),
    });
    Ok(render_items(&prelude, &items, &postlude))
}

/// Mark an item `[x]` by id. Phase 1: state-machine validation lives in
/// the upcoming `pending_cmd` layer; this primitive forces Done unconditionally.
pub fn op_done(body: &str, id: &str) -> Result<String> {
    let id = id.trim().to_lowercase();
    let (prelude, mut items, postlude) = parse_items(body);
    let item = items
        .iter_mut()
        .find(|i| i.id == id)
        .ok_or_else(|| anyhow!("pending done: no item with id [#{}]", id))?;
    item.state = PendingState::Done;
    Ok(render_items(&prelude, &items, &postlude))
}

/// Edit an item's text, preserving its hash id.
pub fn op_edit(body: &str, id: &str, new_text: &str) -> Result<String> {
    let new_text = new_text.trim();
    if new_text.is_empty() {
        bail!("pending edit: text must be non-empty");
    }
    let id = id.trim().to_lowercase();
    let (prelude, mut items, postlude) = parse_items(body);
    let item = items
        .iter_mut()
        .find(|i| i.id == id)
        .ok_or_else(|| anyhow!("pending edit: no item with id [#{}]", id))?;
    item.text = new_text.to_string();
    Ok(render_items(&prelude, &items, &postlude))
}

/// Clear all items from the body. Prelude/postlude are preserved.
pub fn op_clear(body: &str) -> Result<String> {
    let (prelude, _items, postlude) = parse_items(body);
    Ok(render_items(&prelude, &[], &postlude))
}

/// Reorder items by id. Listed ids come first (in the given order); unlisted ids
/// keep their relative order and follow.
pub fn op_reorder(body: &str, ids: &[String]) -> Result<String> {
    let (prelude, items, postlude) = parse_items(body);
    let requested: Vec<String> = ids.iter().map(|s| s.trim().to_lowercase()).collect();
    for id in &requested {
        if !items.iter().any(|i| i.id == *id) {
            bail!("pending reorder: no item with id [#{}]", id);
        }
    }
    let mut remaining: Vec<PendingItem> = items.clone();
    let mut ordered: Vec<PendingItem> = Vec::new();
    for id in &requested {
        if let Some(pos) = remaining.iter().position(|i| i.id == *id) {
            ordered.push(remaining.remove(pos));
        }
    }
    ordered.extend(remaining);
    Ok(render_items(&prelude, &ordered, &postlude))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC_ID: &str = "test-doc";

    fn ids() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn parse_empty_body() {
        let (p, items, post) = parse_items("");
        assert_eq!(p, "");
        assert!(items.is_empty());
        assert_eq!(post, "");
    }

    #[test]
    fn parse_fully_migrated() {
        let body = "- [ ] [#a3f2] first\n- [x] [#b1c4] second\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "a3f2");
        assert_eq!(items[0].state, PendingState::Open);
        assert_eq!(items[0].text, "first");
        assert_eq!(items[1].id, "b1c4");
        assert_eq!(items[1].state, PendingState::Done);
    }

    #[test]
    fn parse_gated_state() {
        let body = "- [/] [#eg0w] CommitLock — gate: v0.32.5\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].state, PendingState::Gated);
        assert_eq!(items[0].id, "eg0w");
        assert_eq!(items[0].text, "CommitLock — gate: v0.32.5");
    }

    #[test]
    fn parse_all_three_states() {
        let body = "- [ ] [#a3f2] open\n- [/] [#b1c4] gated\n- [x] [#c9e0] done\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items[0].state, PendingState::Open);
        assert_eq!(items[1].state, PendingState::Gated);
        assert_eq!(items[2].state, PendingState::Done);
    }

    #[test]
    fn parse_checkbox_only_no_id() {
        let body = "- [ ] just text\n- [x] done item\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "");
        assert_eq!(items[0].text, "just text");
        assert_eq!(items[1].state, PendingState::Done);
    }

    #[test]
    fn parse_legacy_no_checkbox() {
        let body = "- legacy one\n- legacy two\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text, "legacy one");
        assert_eq!(items[0].state, PendingState::Open);
        assert_eq!(items[0].id, "");
    }

    #[test]
    fn parse_mixed() {
        let body = "- [ ] [#a3f2] migrated\n- [ ] partial\n- legacy\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].id, "a3f2");
        assert_eq!(items[1].id, "");
        assert_eq!(items[1].text, "partial");
        assert_eq!(items[2].text, "legacy");
    }

    #[test]
    fn render_roundtrip_canonical() {
        let body = "- [ ] [#a3f2] first\n- [x] [#b1c4] second\n";
        let (p, items, post) = parse_items(body);
        let out = render_items(&p, &items, &post);
        assert_eq!(out, body);
    }

    #[test]
    fn render_roundtrip_all_three_states() {
        let body = "- [ ] [#a3f2] open\n- [/] [#b1c4] gated — gate: v0.32.5\n- [x] [#c9e0] done\n";
        let (p, items, post) = parse_items(body);
        let out = render_items(&p, &items, &post);
        assert_eq!(out, body);
    }

    #[test]
    fn render_emits_slash_for_gated() {
        let item = PendingItem {
            id: "eg0w".to_string(),
            state: PendingState::Gated,
            text: "CommitLock".to_string(),
        };
        assert_eq!(item.render(), "- [/] [#eg0w] CommitLock");
    }

    #[test]
    fn backfill_adds_hashes() {
        let body = "- legacy one\n- legacy two\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items.len(), 2);
        assert!(!items[0].id.is_empty());
        assert!(!items[1].id.is_empty());
        assert_ne!(items[0].id, items[1].id);
        assert!(new_body.contains("- [ ] [#"));
    }

    #[test]
    fn backfill_idempotent() {
        let body = "- [ ] [#a3f2] first\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(!changed, "fully-migrated body should not change");
        assert_eq!(new_body, body);
    }

    #[test]
    fn backfill_normalizes_checkbox_only() {
        let body = "- [ ] no id here\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        assert!(new_body.contains("[#"));
    }

    #[test]
    fn backfill_never_inserts_gated() {
        // Legacy items with no checkbox must default to Open `[ ]`,
        // never Gated `[/]`. Gated state is always operator-explicit.
        let body = "- legacy item awaiting v0.32.5\n";
        let (new_body, _) = backfill(body, DOC_ID, &ids());
        assert!(new_body.contains("- [ ] "));
        assert!(!new_body.contains("- [/] "));
    }

    #[test]
    fn backfill_preserves_existing_gated() {
        let body = "- [/] [#eg0w] CommitLock — gate: v0.32.5\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(!changed);
        assert_eq!(new_body, body);
    }

    #[test]
    fn reap_skips_gated() {
        let body = "- [/] [#eg0w] gated\n- [x] [#c9e0] done\n";
        let (new_body, removed) = reap(body);
        assert_eq!(removed, vec!["c9e0"]);
        assert!(new_body.contains("[#eg0w]"));
        assert!(!new_body.contains("[#c9e0]"));
    }

    #[test]
    fn reap_removes_checked() {
        let body = "- [ ] [#a3f2] keep\n- [x] [#b1c4] drop\n- [ ] [#c5d6] keep2\n";
        let (new_body, removed) = reap(body);
        assert_eq!(removed, vec!["b1c4"]);
        assert!(new_body.contains("a3f2"));
        assert!(!new_body.contains("b1c4"));
        assert!(new_body.contains("c5d6"));
    }

    #[test]
    fn reap_noop_when_none_checked() {
        let body = "- [ ] [#a3f2] keep\n";
        let (new_body, removed) = reap(body);
        assert!(removed.is_empty());
        assert_eq!(new_body, body);
    }

    #[test]
    fn detect_reorder_same_set_different_order() {
        let snap = "- [ ] [#a1b2] one\n- [ ] [#c3d4] two\n";
        let cur = "- [ ] [#c3d4] two\n- [ ] [#a1b2] one\n";
        let result = detect_reorder(snap, cur);
        assert_eq!(result, Some(vec!["c3d4".to_string(), "a1b2".to_string()]));
    }

    #[test]
    fn detect_reorder_none_when_sets_differ() {
        let snap = "- [ ] [#a1b2] one\n";
        let cur = "- [ ] [#a1b2] one\n- [ ] [#c3d4] two\n";
        assert_eq!(detect_reorder(snap, cur), None);
    }

    #[test]
    fn detect_reorder_none_when_same_order() {
        let snap = "- [ ] [#a1b2] one\n- [ ] [#c3d4] two\n";
        assert_eq!(detect_reorder(snap, snap), None);
    }

    #[test]
    fn op_add_appends_new_item_with_hash() {
        let body = "";
        let new_body = op_add(body, "first task", DOC_ID).unwrap();
        assert!(new_body.contains("- [ ] [#"));
        assert!(new_body.contains("first task"));
    }

    #[test]
    fn op_add_rejects_empty() {
        assert!(op_add("", "   ", DOC_ID).is_err());
    }

    #[test]
    fn op_done_marks_checked() {
        let body = "- [ ] [#a1b2] task\n";
        let new_body = op_done(body, "a1b2").unwrap();
        assert!(new_body.contains("[x]"));
    }

    #[test]
    fn op_done_unknown_id_errors() {
        let body = "- [ ] [#a1b2] task\n";
        assert!(op_done(body, "zzzz").is_err());
    }

    #[test]
    fn op_edit_preserves_hash() {
        let body = "- [ ] [#a1b2] original\n";
        let new_body = op_edit(body, "a1b2", "updated").unwrap();
        assert!(new_body.contains("[#a1b2]"));
        assert!(new_body.contains("updated"));
        assert!(!new_body.contains("original"));
    }

    #[test]
    fn op_clear_empties_items() {
        let body = "- [ ] [#a1b2] one\n- [ ] [#c3d4] two\n";
        let new_body = op_clear(body).unwrap();
        assert!(!new_body.contains("[#"));
    }

    #[test]
    fn op_reorder_reorders_by_id() {
        let body = "- [ ] [#a1b2] first\n- [ ] [#c3d4] second\n- [ ] [#e5f6] third\n";
        let new_body =
            op_reorder(body, &["e5f6".to_string(), "a1b2".to_string()]).unwrap();
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items[0].id, "e5f6");
        assert_eq!(items[1].id, "a1b2");
        assert_eq!(items[2].id, "c3d4");
    }

    #[test]
    fn op_reorder_unknown_id_errors() {
        let body = "- [ ] [#a1b2] one\n";
        assert!(op_reorder(body, &["zzzz".to_string()]).is_err());
    }

    #[test]
    fn generate_hash_deterministic_and_short() {
        let h = generate_hash("text", "doc", 0);
        assert_eq!(h.len(), 4);
        assert_eq!(h, generate_hash("text", "doc", 0));
        assert_ne!(h, generate_hash("text", "doc", 1));
    }
}
