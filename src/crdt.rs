use anyhow::{Context, Result};
use yrs::updates::decoder::Decode;
use yrs::{Doc, GetString, ReadTxn, Text, TextRef, Transact, Update};

const TEXT_KEY: &str = "content";

/// CRDT document wrapping a Yjs `Doc` for conflict-free merging.
pub struct CrdtDoc {
    doc: Doc,
}

impl CrdtDoc {
    /// Create a new CRDT document initialized with the given text content.
    pub fn from_text(content: &str) -> Self {
        let doc = Doc::new();
        let text = doc.get_or_insert_text(TEXT_KEY);
        let mut txn = doc.transact_mut();
        text.insert(&mut txn, 0, content);
        drop(txn);
        CrdtDoc { doc }
    }

    /// Extract the current text content from the CRDT document.
    pub fn to_text(&self) -> String {
        let text = self.doc.get_or_insert_text(TEXT_KEY);
        let txn = self.doc.transact();
        text.get_string(&txn)
    }

    /// Apply a local edit: delete `delete_len` chars at `offset`, then insert `insert` there.
    #[allow(dead_code)] // Used in tests and Phase 4 stream write-back
    pub fn apply_edit(&self, offset: u32, delete_len: u32, insert: &str) {
        let text = self.doc.get_or_insert_text(TEXT_KEY);
        let mut txn = self.doc.transact_mut();
        if delete_len > 0 {
            text.remove_range(&mut txn, offset, delete_len);
        }
        if !insert.is_empty() {
            text.insert(&mut txn, offset, insert);
        }
    }

    /// Encode the full document state (for persistence).
    pub fn encode_state(&self) -> Vec<u8> {
        let txn = self.doc.transact();
        txn.encode_state_as_update_v1(&yrs::StateVector::default())
    }

    /// Decode a previously encoded state into a new CrdtDoc.
    pub fn decode_state(bytes: &[u8]) -> Result<Self> {
        let doc = Doc::new();
        let update = Update::decode_v1(bytes)
            .map_err(|e| anyhow::anyhow!("failed to decode CRDT state: {}", e))?;
        let mut txn = doc.transact_mut();
        txn.apply_update(update)
            .map_err(|e| anyhow::anyhow!("failed to apply CRDT update: {}", e))?;
        drop(txn);
        Ok(CrdtDoc { doc })
    }
}

/// Merge two concurrent text versions against a common base using CRDT.
///
/// Creates three CRDT actors: base, ours, theirs.
/// Applies each side's edits as diffs from the base, then merges updates.
/// Returns the merged text (conflict-free).
///
/// **Stale base detection:** If the CRDT base text doesn't match either ours
/// or theirs as a prefix/substring, the base is stale. In that case, we use
/// `ours_text` as the base to prevent duplicate insertions.
pub fn merge(base_state: Option<&[u8]>, ours_text: &str, theirs_text: &str) -> Result<String> {
    // Short-circuit: if both sides are identical, no merge needed
    if ours_text == theirs_text {
        eprintln!("[crdt] ours == theirs, skipping merge");
        return Ok(ours_text.to_string());
    }

    // Bootstrap base doc from state or empty
    let base_doc = if let Some(bytes) = base_state {
        CrdtDoc::decode_state(bytes)
            .context("failed to decode base CRDT state")?
    } else {
        CrdtDoc::from_text("")
    };
    let mut base_text = base_doc.to_text();

    eprintln!(
        "[crdt] merge: base_len={} ours_len={} theirs_len={}",
        base_text.len(),
        ours_text.len(),
        theirs_text.len()
    );

    // Stale base detection: if the base text doesn't share a common prefix
    // with both sides, it's stale. Use ours as the base instead.
    // This prevents duplicate insertions when both sides contain text
    // that the stale base doesn't have.
    let ours_common = common_prefix_len(&base_text, ours_text);
    let theirs_common = common_prefix_len(&base_text, theirs_text);
    let base_len = base_text.len();

    if base_len > 0
        && (ours_common as f64 / base_len as f64) < 0.5
        && (theirs_common as f64 / base_len as f64) < 0.5
    {
        eprintln!(
            "[crdt] Stale CRDT base detected (common prefix: ours={}%, theirs={}%). Using ours as base.",
            (ours_common * 100) / base_len,
            (theirs_common * 100) / base_len
        );
        base_text = ours_text.to_string();
    }

    // Advance base to the common prefix of ours and theirs when it extends
    // beyond the current base.
    //
    // When both ours and theirs independently added the same text beyond the
    // stale base (e.g., both contain a user prompt that the base doesn't have),
    // the CRDT treats each insertion as independent and includes both, causing
    // duplication. Fix: use the common prefix of ours and theirs as the effective
    // base, so shared additions are not treated as independent insertions.
    //
    // This handles the common pattern where:
    //   base   = "old content"
    //   ours   = "old content + user prompt + agent response"
    //   theirs = "old content + user prompt + small edit"
    // Without fix: user prompt appears twice (from both sides).
    // With fix: base advances to "old content + user prompt", ours' diff is
    //           just the agent response, theirs' diff is just the small edit.
    let mutual_prefix = common_prefix_len(ours_text, theirs_text);
    if mutual_prefix > base_text.len() {
        // Snap to a line boundary to avoid splitting mid-line
        let snap = &ours_text[..mutual_prefix];
        let snapped = match snap.rfind('\n') {
            Some(pos) if pos >= base_text.len() => pos + 1,
            _ => mutual_prefix,
        };
        if snapped > base_text.len() {
            eprintln!(
                "[crdt] Advancing base to shared prefix (base_len={} → {})",
                base_text.len(),
                snapped
            );
            base_text = ours_text[..snapped].to_string();
        }
    }

    // Compute diffs from base to each side
    let ours_ops = compute_edit_ops(&base_text, ours_text);
    let theirs_ops = compute_edit_ops(&base_text, theirs_text);

    // Create two independent docs from the base state.
    // If base was overridden (stale detection), rebuild from the new base_text.
    let base_encoded = if base_text == base_doc.to_text() {
        base_doc.encode_state()
    } else {
        CrdtDoc::from_text(&base_text).encode_state()
    };

    let ours_doc = Doc::with_client_id(1);
    {
        let update = Update::decode_v1(&base_encoded)
            .map_err(|e| anyhow::anyhow!("decode error: {}", e))?;
        let mut txn = ours_doc.transact_mut();
        txn.apply_update(update)
            .map_err(|e| anyhow::anyhow!("apply error: {}", e))?;
    }

    let theirs_doc = Doc::with_client_id(2);
    {
        let update = Update::decode_v1(&base_encoded)
            .map_err(|e| anyhow::anyhow!("decode error: {}", e))?;
        let mut txn = theirs_doc.transact_mut();
        txn.apply_update(update)
            .map_err(|e| anyhow::anyhow!("apply error: {}", e))?;
    }

    // Apply ours edits
    {
        let text = ours_doc.get_or_insert_text(TEXT_KEY);
        let mut txn = ours_doc.transact_mut();
        apply_ops(&text, &mut txn, &ours_ops);
    }

    // Apply theirs edits
    {
        let text = theirs_doc.get_or_insert_text(TEXT_KEY);
        let mut txn = theirs_doc.transact_mut();
        apply_ops(&text, &mut txn, &theirs_ops);
    }

    // Merge: apply theirs' changes into ours
    let ours_sv = {
        let txn = ours_doc.transact();
        txn.state_vector()
    };
    let theirs_update = {
        let txn = theirs_doc.transact();
        txn.encode_state_as_update_v1(&ours_sv)
    };
    {
        let update = Update::decode_v1(&theirs_update)
            .map_err(|e| anyhow::anyhow!("decode error: {}", e))?;
        let mut txn = ours_doc.transact_mut();
        txn.apply_update(update)
            .map_err(|e| anyhow::anyhow!("apply error: {}", e))?;
    }

    // Read merged result
    let text = ours_doc.get_or_insert_text(TEXT_KEY);
    let txn = ours_doc.transact();
    Ok(text.get_string(&txn))
}

/// Compact a CRDT state by re-encoding (GC tombstones where possible).
pub fn compact(state: &[u8]) -> Result<Vec<u8>> {
    let doc = CrdtDoc::decode_state(state)?;
    Ok(doc.encode_state())
}

/// Count the number of bytes in the common prefix of two strings.
fn common_prefix_len(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}

/// Edit operation for replaying diffs onto a CRDT text.
#[derive(Debug)]
enum EditOp {
    Retain(u32),
    Delete(u32),
    Insert(String),
}

/// Compute edit operations to transform `from` into `to` using `similar` diff.
fn compute_edit_ops(from: &str, to: &str) -> Vec<EditOp> {
    use similar::{ChangeTag, TextDiff};

    let diff = TextDiff::from_chars(from, to);
    let mut ops = Vec::new();

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                let len = change.value().len() as u32;
                if let Some(EditOp::Retain(n)) = ops.last_mut() {
                    *n += len;
                } else {
                    ops.push(EditOp::Retain(len));
                }
            }
            ChangeTag::Delete => {
                let len = change.value().len() as u32;
                if let Some(EditOp::Delete(n)) = ops.last_mut() {
                    *n += len;
                } else {
                    ops.push(EditOp::Delete(len));
                }
            }
            ChangeTag::Insert => {
                let s = change.value().to_string();
                if let Some(EditOp::Insert(existing)) = ops.last_mut() {
                    existing.push_str(&s);
                } else {
                    ops.push(EditOp::Insert(s));
                }
            }
        }
    }

    ops
}

/// Apply edit operations to a Yrs text type within a transaction.
fn apply_ops(text: &TextRef, txn: &mut yrs::TransactionMut<'_>, ops: &[EditOp]) {
    let mut cursor: u32 = 0;
    for op in ops {
        match op {
            EditOp::Retain(n) => cursor += n,
            EditOp::Delete(n) => {
                text.remove_range(txn, cursor, *n);
                // cursor stays — content shifted left
            }
            EditOp::Insert(s) => {
                text.insert(txn, cursor, s);
                cursor += s.len() as u32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_text() {
        let content = "Hello, world!\nLine two.\n";
        let doc = CrdtDoc::from_text(content);
        assert_eq!(doc.to_text(), content);
    }

    #[test]
    fn roundtrip_encode_decode() {
        let content = "Some document content.\n";
        let doc = CrdtDoc::from_text(content);
        let encoded = doc.encode_state();
        let decoded = CrdtDoc::decode_state(&encoded).unwrap();
        assert_eq!(decoded.to_text(), content);
    }

    #[test]
    fn apply_edit_insert() {
        let doc = CrdtDoc::from_text("Hello world");
        doc.apply_edit(5, 0, ",");
        assert_eq!(doc.to_text(), "Hello, world");
    }

    #[test]
    fn apply_edit_delete() {
        let doc = CrdtDoc::from_text("Hello, world");
        doc.apply_edit(5, 1, "");
        assert_eq!(doc.to_text(), "Hello world");
    }

    #[test]
    fn apply_edit_replace() {
        let doc = CrdtDoc::from_text("Hello world");
        doc.apply_edit(6, 5, "Rust");
        assert_eq!(doc.to_text(), "Hello Rust");
    }

    #[test]
    fn concurrent_append_merge_no_conflict() {
        let base = "# Document\n\nBase content.\n";
        let base_doc = CrdtDoc::from_text(base);
        let base_state = base_doc.encode_state();

        let ours = format!("{base}## Agent\n\nAgent response.\n");
        let theirs = format!("{base}## User\n\nUser addition.\n");

        let merged = merge(Some(&base_state), &ours, &theirs).unwrap();

        // Both additions should be present
        assert!(merged.contains("Agent response."), "missing agent text");
        assert!(merged.contains("User addition."), "missing user text");
        assert!(merged.contains("Base content."), "missing base text");
        // No conflict markers
        assert!(!merged.contains("<<<<<<<"));
        assert!(!merged.contains(">>>>>>>"));
    }

    #[test]
    fn concurrent_insert_same_position() {
        let base = "Line 1\nLine 3\n";
        let base_doc = CrdtDoc::from_text(base);
        let base_state = base_doc.encode_state();

        let ours = "Line 1\nAgent line\nLine 3\n";
        let theirs = "Line 1\nUser line\nLine 3\n";

        let merged = merge(Some(&base_state), ours, theirs).unwrap();

        // Both insertions preserved, no conflict
        assert!(merged.contains("Agent line"), "missing agent insertion");
        assert!(merged.contains("User line"), "missing user insertion");
        assert!(merged.contains("Line 1"), "missing line 1");
        assert!(merged.contains("Line 3"), "missing line 3");
    }

    #[test]
    fn merge_no_base_state() {
        // When no base state exists, bootstrap from empty
        let ours = "Agent wrote this.\n";
        let theirs = "User wrote this.\n";

        let merged = merge(None, ours, theirs).unwrap();

        assert!(merged.contains("Agent wrote this."));
        assert!(merged.contains("User wrote this."));
    }

    #[test]
    fn compact_preserves_content() {
        let doc = CrdtDoc::from_text("Hello");
        doc.apply_edit(5, 0, " world");
        doc.apply_edit(11, 0, "!");

        let state = doc.encode_state();
        let compacted = compact(&state).unwrap();
        let restored = CrdtDoc::decode_state(&compacted).unwrap();

        assert_eq!(restored.to_text(), "Hello world!");
        assert!(compacted.len() <= state.len());
    }

    #[test]
    fn compact_reduces_size_after_edits() {
        let doc = CrdtDoc::from_text("aaaa");
        // Many small edits to build up tombstones
        for i in 0..20 {
            let c = ((b'a' + (i % 26)) as char).to_string();
            doc.apply_edit(0, 1, &c);
        }
        let state = doc.encode_state();
        let compacted = compact(&state).unwrap();
        let restored = CrdtDoc::decode_state(&compacted).unwrap();
        assert_eq!(restored.to_text(), doc.to_text());
    }

    #[test]
    fn empty_document() {
        let doc = CrdtDoc::from_text("");
        assert_eq!(doc.to_text(), "");

        let encoded = doc.encode_state();
        let decoded = CrdtDoc::decode_state(&encoded).unwrap();
        assert_eq!(decoded.to_text(), "");
    }

    #[test]
    fn decode_invalid_bytes_errors() {
        let result = CrdtDoc::decode_state(&[0xff, 0xfe, 0xfd]);
        assert!(result.is_err());
    }

    #[test]
    fn merge_identical_texts() {
        let base = "Same content.\n";
        let base_doc = CrdtDoc::from_text(base);
        let state = base_doc.encode_state();

        let merged = merge(Some(&state), base, base).unwrap();
        assert_eq!(merged, base);
    }

    #[test]
    fn merge_one_side_unchanged() {
        let base = "Original.\n";
        let base_doc = CrdtDoc::from_text(base);
        let state = base_doc.encode_state();

        let ours = "Original.\nAgent added.\n";
        let merged = merge(Some(&state), ours, base).unwrap();
        assert_eq!(merged, ours);
    }

    /// Regression test: CRDT merge should not duplicate user prompt when both
    /// ours and theirs contain the same text added since the base state.
    ///
    /// Scenario (brookebrodack-dev.md duplication bug):
    /// 1. CRDT base = exchange content from a previous cycle (no user prompt)
    /// 2. User adds prompt to exchange → saved as baseline
    /// 3. Agent generates response, content_ours = baseline + response (has user prompt)
    /// 4. User makes a small edit during response generation → content_current (has user prompt too)
    /// 5. CRDT merge: both ours and theirs have the user prompt relative to stale base
    /// 6. BUG: user prompt appears twice in merged output
    #[test]
    fn merge_stale_base_no_duplicate_user_prompt() {
        // CRDT base from a previous cycle — does NOT have the user's current prompt
        let base_content = "\
## Assistant

Previous response content.

Committed and pushed.

";
        let base_doc = CrdtDoc::from_text(base_content);
        let base_state = base_doc.encode_state();

        // User adds prompt after base was saved
        let user_prompt = "\
Opening a video a shows video a.
Closing video a then opening video b start video b but video b is hidden.
Closing video b then reopening video b starts and shows video b. video b is visible.
";

        // content_ours: base + user prompt + agent response (from run_stream with full exchange)
        let ours = format!("\
{}{}### Re: Close A → Open B still hidden

Added explicit height and visibility reset.

Committed and pushed.

", base_content, user_prompt);

        // content_current: base + user prompt + minor user edit (e.g., added a blank line)
        let theirs = format!("\
{}{}
", base_content, user_prompt);

        let merged = merge(Some(&base_state), &ours, &theirs).unwrap();

        // User prompt should appear exactly ONCE
        let prompt_count = merged.matches("Opening a video a shows video a.").count();
        assert_eq!(
            prompt_count, 1,
            "User prompt duplicated! Appeared {} times in:\n{}",
            prompt_count, merged
        );

        // Agent response should be present
        assert!(
            merged.contains("### Re: Close A → Open B still hidden"),
            "Agent response missing from merge:\n{}", merged
        );
    }

    /// Regression test: When CRDT base is stale and both sides added the same text
    /// at the same position, the merge should not duplicate it.
    #[test]
    fn merge_stale_base_same_insertion_both_sides() {
        let base_content = "Line 1\nLine 2\n";
        let base_doc = CrdtDoc::from_text(base_content);
        let base_state = base_doc.encode_state();

        // Both sides added the same text (user prompt) + ours adds more
        let shared_addition = "User typed this.\n";
        let ours = format!("{}{}Agent response.\n", base_content, shared_addition);
        let theirs = format!("{}{}", base_content, shared_addition);

        let merged = merge(Some(&base_state), &ours, &theirs).unwrap();

        let count = merged.matches("User typed this.").count();
        assert_eq!(
            count, 1,
            "Shared text duplicated! Appeared {} times in:\n{}",
            count, merged
        );
        assert!(merged.contains("Agent response."), "Agent text missing:\n{}", merged);
    }
}
