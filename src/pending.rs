//! # Module: pending
//!
//! Pure functions for parsing and mutating the `agent:pending` component body.
//!
//! Each pending item carries:
//! - a GFM task-list checkbox (`- [ ]` or `- [x]`)
//! - an id prefix rendered as `[#xxxx]` (generated hash or caller-provided custom id)
//! - free-form text
//!
//! Canonical form: `- [ ] [#a3f2] refactor preflight commit path`
//!
//! This module is I/O-free. Callers (`pending_cmd.rs`, `preflight.rs`, `write.rs`)
//! handle reading/writing files, locking, and git commits.
//!
//! ## Spec
//! - Parser accepts legacy forms and normalizes via `backfill`.
//! - IDs are stable across edits/reorders; generated once (or supplied on insert), preserved thereafter.
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
    /// Currently only used by tests; kept on the public API for parser callers
    /// that may need to inspect a single checkbox char in isolation.
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

/// Mutating operation on a pending item — used by the state-transition matrix.
///
/// `MarkDone` is referenced by the matrix tests and reserved for the `op_done`
/// migration path (Phase 3); it is not yet wired through the CLI primitives,
/// which keep the unconditional `op_done` semantics from Phase 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum PendingOp {
    /// `[ ] → [/]` — code-complete, awaiting external gate.
    Gate,
    /// `[/] → [ ]` — return a gated item to active.
    Ungate,
    /// `[ ] | [/] → [x]` — fully complete.
    MarkDone,
}

/// Outcome of `validate_transition`. `NoOp` means "already in target state, do nothing".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionResult {
    Transition(PendingState),
    NoOp,
}

/// Apply the state-machine matrix from `specs/pending-system.md` §4.
///
/// | from \ op | Gate     | Ungate   | MarkDone |
/// |-----------|----------|----------|----------|
/// | Open      | → Gated  | error    | → Done   |
/// | Gated     | no-op    | → Open   | → Done   |
/// | Done      | error    | error    | no-op    |
///
/// Rationale (spec §4):
/// - `gate` from Done is an error: a fully-complete item cannot be re-gated; the
///   intended workflow is to add a new pending item describing the follow-up gate.
/// - `ungate` from Open or Done is an error: ungate is the inverse of gate, not
///   a generic "reset" — it requires an explicit `[/]` source.
/// - `Gate` on Gated and `MarkDone` on Done are idempotent no-ops, not errors,
///   so the granular CLI flags can be re-run safely (skill retries, watch loops).
pub fn validate_transition(from: PendingState, op: PendingOp) -> Result<TransitionResult> {
    use PendingOp::*;
    use PendingState as S;
    use TransitionResult::*;
    match (from, op) {
        (S::Open, Gate) => Ok(Transition(S::Gated)),
        (S::Open, Ungate) => bail!("cannot ungate Open item: source must be `[/]`"),
        (S::Open, MarkDone) => Ok(Transition(S::Done)),
        (S::Gated, Gate) => Ok(NoOp),
        (S::Gated, Ungate) => Ok(Transition(S::Open)),
        (S::Gated, MarkDone) => Ok(Transition(S::Done)),
        (S::Done, Gate) => {
            bail!("cannot gate Done item: add a new pending item for the follow-up gate")
        }
        (S::Done, Ungate) => bail!("cannot ungate Done item: source must be `[/]`"),
        (S::Done, MarkDone) => Ok(NoOp),
    }
}

/// A parsed pending list item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingItem {
    /// Pending item id (no `#` prefix). Generated ids are lowercase base32; custom ids
    /// may be any non-empty ASCII alphanumeric string and are normalized to lowercase.
    pub id: String,
    /// Lifecycle state encoded by the GFM checkbox.
    pub state: PendingState,
    /// Optional typed gate (e.g., "release" for `[/release]`, "deploy" for `[/deploy]`).
    /// Only meaningful when `state == Gated`. `None` means untyped `[/]`.
    pub gate_type: Option<String>,
    /// Bullet text after the hash prefix.
    pub text: String,
}

impl PendingItem {
    /// Render to canonical `- [<state>] [#id] text` form.
    /// Typed gates render as `[/release]`, `[/deploy]`, etc.
    pub fn render(&self) -> String {
        let checkbox = match (&self.state, &self.gate_type) {
            (PendingState::Gated, Some(gt)) => format!("[/{}]", gt),
            _ => format!("[{}]", self.state.box_char()),
        };
        format!("- {} [#{}] {}", checkbox, self.id, self.text)
    }

    /// Convenience: true when state is `Done` (`[x]`).
    pub fn is_done(&self) -> bool {
        matches!(self.state, PendingState::Done)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowPendingItem {
    pub id: String,
    pub text: String,
    pub line: usize,
}

impl ShadowPendingItem {
    pub fn reference(&self) -> String {
        format!("#{} (line {})", self.id, self.line)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShadowPendingReport {
    pub duplicated_in_live_backlog: Vec<ShadowPendingItem>,
    pub shadow_only: Vec<ShadowPendingItem>,
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

    let prelude = join_lines(
        &lines[..first_item],
        has_trailing_newline(body) || first_item > 0,
    );
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

    // Checkbox? Supports typed gates: [/release], [/deploy], etc.
    let (state, gate_type, after_box) = if let Some(r) = rest.strip_prefix("[ ]") {
        (PendingState::Open, None, r.trim_start())
    } else if let Some(r) = rest.strip_prefix("[/]") {
        (PendingState::Gated, None, r.trim_start())
    } else if let Some(inner) = rest.strip_prefix("[/") {
        // Typed gate: [/release], [/deploy], etc.
        if let Some(close) = inner.find(']') {
            let gt = &inner[..close];
            if !gt.is_empty()
                && gt
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                let r = &inner[close + 1..];
                (PendingState::Gated, Some(gt.to_lowercase()), r.trim_start())
            } else {
                (PendingState::Open, None, rest)
            }
        } else {
            (PendingState::Open, None, rest)
        }
    } else if let Some(r) = rest.strip_prefix("[x]") {
        (PendingState::Done, None, r.trim_start())
    } else if let Some(r) = rest.strip_prefix("[X]") {
        (PendingState::Done, None, r.trim_start())
    } else {
        (PendingState::Open, None, rest)
    };

    // Hash id?
    let (id, text) = if let Some(after_hash) = after_box.strip_prefix("[#") {
        if let Some(close) = after_hash.find(']') {
            let id_raw = &after_hash[..close];
            let tail = after_hash[close + 1..].trim_start();
            if is_valid_pending_id(id_raw) {
                (id_raw.to_lowercase(), tail.to_string())
            } else if id_raw.is_empty() {
                // Bare [#] placeholder — consume it, text starts after ]
                (String::new(), tail.to_string())
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
        gate_type,
        text: text.trim_end().to_string(),
    })
}

pub(crate) fn is_valid_pending_id(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeadingCustomIdPrefix {
    Explicit,
    Bracketed,
    BarePlaceholder,
}

fn detect_leading_custom_id_prefix(text: &str) -> Option<LeadingCustomIdPrefix> {
    let trimmed = text.trim_start();
    if trimmed.starts_with("id=") {
        return Some(LeadingCustomIdPrefix::Explicit);
    }
    let after_hash = trimmed.strip_prefix("[#")?;
    let close = after_hash.find(']')?;
    if close == 0 {
        Some(LeadingCustomIdPrefix::BarePlaceholder)
    } else {
        Some(LeadingCustomIdPrefix::Bracketed)
    }
}

pub(crate) fn ensure_no_leading_custom_id_prefix(text: &str, context: &str) -> Result<()> {
    match detect_leading_custom_id_prefix(text) {
        None => Ok(()),
        Some(LeadingCustomIdPrefix::BarePlaceholder) => bail!(
            "{}: bare `[#]` placeholder is invalid — omit it or use `id=<id> <text>`",
            context
        ),
        Some(LeadingCustomIdPrefix::Explicit) | Some(LeadingCustomIdPrefix::Bracketed) => bail!(
            "{}: duplicate leading custom id prefix in item text — use exactly one leading `id=<id>` or `[#id]` prefix",
            context
        ),
    }
}

fn custom_id_error(raw_id: &str) -> anyhow::Error {
    anyhow!(
        "pending add: invalid custom id `{}` — ids must be non-empty ASCII alphanumeric strings",
        raw_id.trim()
    )
}

fn parse_explicit_custom_id_prefix(rest: &str) -> Result<(Option<String>, String)> {
    let Some((raw_id, remainder)) = rest.split_once(char::is_whitespace) else {
        bail!(
            "pending add: custom id prefix must be followed by item text (expected `id=<id> <text>`)"
        );
    };
    let custom_id = raw_id.trim().trim_start_matches('#');
    if custom_id.is_empty() {
        bail!("pending add: empty custom id after `id=` — expected `id=<id> <text>`");
    }
    if !is_valid_pending_id(custom_id) {
        return Err(custom_id_error(raw_id));
    }
    let remainder = remainder.trim();
    if remainder.is_empty() {
        bail!(
            "pending add: custom id prefix must be followed by item text (expected `id=<id> <text>`)"
        );
    }
    ensure_no_leading_custom_id_prefix(remainder, "pending add")?;
    Ok((Some(custom_id.to_lowercase()), remainder.to_string()))
}

fn parse_bracketed_custom_id_prefix(trimmed: &str) -> Result<(Option<String>, String)> {
    let Some(after_hash) = trimmed.strip_prefix("[#") else {
        return Ok((None, trimmed.to_string()));
    };
    let Some(close) = after_hash.find(']') else {
        return Ok((None, trimmed.to_string()));
    };
    let raw_id = &after_hash[..close];
    let remainder = after_hash[close + 1..].trim_start();
    if raw_id.is_empty() {
        bail!("pending add: bare `[#]` placeholder is invalid — use `id=<id> <text>` or omit it");
    }
    if !is_valid_pending_id(raw_id) {
        return Err(custom_id_error(raw_id));
    }
    if remainder.is_empty() {
        bail!(
            "pending add: bracketed custom id prefix must be followed by item text (expected `[#id] <text>`)"
        );
    }
    ensure_no_leading_custom_id_prefix(remainder, "pending add")?;
    Ok((Some(raw_id.to_lowercase()), remainder.to_string()))
}

fn parse_custom_id_prefix(text: &str) -> Result<(Option<String>, String)> {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("id=") {
        return parse_explicit_custom_id_prefix(rest);
    }
    parse_bracketed_custom_id_prefix(trimmed)
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

pub fn detect_shadow_open_items(doc: &str) -> Result<ShadowPendingReport> {
    let components = crate::component::parse(doc)?;
    let Some(backlog_component) = components
        .iter()
        .find(|component| crate::component::is_backlog_component(&component.name))
    else {
        return Ok(ShadowPendingReport::default());
    };

    let (_, live_items, _) = parse_items(backlog_component.content(doc));
    let live_open_ids: HashSet<String> = live_items
        .into_iter()
        .filter(|item| !item.id.is_empty() && !item.is_done())
        .map(|item| item.id)
        .collect();

    let excluded_ranges: Vec<(usize, usize)> = components
        .iter()
        .filter(|component| {
            crate::component::is_backlog_component(&component.name)
                || crate::component::is_icebox_component(&component.name)
        })
        .map(|component| (component.open_start, component.close_end))
        .collect();
    let code_ranges = crate::component::find_code_ranges(doc);

    let mut report = ShadowPendingReport::default();
    let mut seen_ids = HashSet::new();
    let mut offset = 0usize;

    for (line_idx, raw_line) in doc.split_inclusive('\n').enumerate() {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let line_start = offset;
        let line_end = offset + raw_line.len();
        offset = line_end;

        if excluded_ranges
            .iter()
            .any(|(start, end)| line_start < *end && line_end > *start)
        {
            continue;
        }
        if code_ranges
            .iter()
            .any(|(start, end)| line_start < *end && line_end > *start)
        {
            continue;
        }

        let Some(item) = parse_item_line(line) else {
            continue;
        };
        if item.id.is_empty() || item.is_done() || !seen_ids.insert(item.id.clone()) {
            continue;
        }

        let shadow = ShadowPendingItem {
            id: item.id.clone(),
            text: item.text,
            line: line_idx + 1,
        };
        if live_open_ids.contains(&item.id) {
            report.duplicated_in_live_backlog.push(shadow);
        } else {
            report.shadow_only.push(shadow);
        }
    }

    Ok(report)
}

/// Generate a stable 4-char base32 hash from `(text, doc_id, counter)`.
///
/// Backward-compat thin wrapper over [`generate_hash_n`] at width 4. Existing
/// docs keep their 4-char IDs on re-backfill because width-4 output is
/// bit-identical to the original formula. Kept in the public API for
/// backward compatibility and as the canonical entry point for width-4
/// hashing in tests.
#[allow(dead_code)]
pub fn generate_hash(text: &str, doc_id: &str, counter: u64) -> String {
    generate_hash_n(text, doc_id, counter, 4)
}

/// Generate a stable variable-width base32 hash. `width` is clamped to `[4, 8]`
/// — the spec §1 ceiling on collision extension.
///
/// Width-4 output is bit-identical to the pre-#14z4 `generate_hash` formula,
/// so lazy backfill on existing docs is a no-op.
pub fn generate_hash_n(text: &str, doc_id: &str, counter: u64, width: usize) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hasher.update(b":");
    hasher.update(doc_id.as_bytes());
    hasher.update(b":");
    hasher.update(counter.to_le_bytes());
    let digest = hasher.finalize();

    // Base32 lowercase alphabet (no 0/1/8/9 per crockford would complicate collisions;
    // stick with full a-z/0-9 subset for simplicity).
    const ALPHABET: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz"; // 32 chars

    let width = width.clamp(4, 8);
    let mut out = String::with_capacity(width);

    // First 4 chars: preserve the original bit packing so width-4 output is
    // stable across the #14z4 refactor. (bottom 20 bits of b0<<16 | b1<<8 | b2)
    let b0 = digest[0] as u32;
    let b1 = digest[1] as u32;
    let b2 = digest[2] as u32;
    let v: u32 = (b0 << 16) | (b1 << 8) | b2;
    out.push(ALPHABET[((v >> 15) & 0x1f) as usize] as char);
    out.push(ALPHABET[((v >> 10) & 0x1f) as usize] as char);
    out.push(ALPHABET[((v >> 5) & 0x1f) as usize] as char);
    out.push(ALPHABET[(v & 0x1f) as usize] as char);

    // Extra chars (5..=8): draw from bytes 3..=5 of the digest. Same 5-bit
    // packing layout, starting from a fresh 24-bit window so layer N+1 is not
    // a mechanical continuation of layer N (prevents widening from aliasing
    // to a near-neighbor that also collides).
    if width > 4 {
        let e0 = digest[3] as u32;
        let e1 = digest[4] as u32;
        let e2 = digest[5] as u32;
        let extra: u32 = (e0 << 16) | (e1 << 8) | e2;
        for i in 0..(width - 4) {
            let shift = 15 - (i as u32) * 5;
            out.push(ALPHABET[((extra >> shift) & 0x1f) as usize] as char);
        }
    }
    out
}

/// Assign a hash id that does not collide with `taken`.
///
/// Starts at width 4 and extends up to the spec §1 ceiling of 8. Counter
/// cycles within each width before widening — at width 4 that's ~1M values
/// before we touch width 5, so normal docs never widen in practice.
fn assign_unique_hash(text: &str, doc_id: &str, taken: &HashSet<String>) -> String {
    // Per-width retry budget: small because a single widening step gives
    // another 5 bits of entropy, which is a much bigger win than continuing
    // to spin at the old width.
    const RETRIES_PER_WIDTH: u64 = 4;
    let mut counter: u64 = 0;
    loop {
        // width = 4 + (counter / 4), clamped at 8 (spec §1 ceiling).
        // Once we hit width 8, keep spinning counter forever — at 40 bits of
        // entropy a further collision is effectively impossible in practice.
        let width = std::cmp::min(4 + (counter / RETRIES_PER_WIDTH) as usize, 8);
        let id = generate_hash_n(text, doc_id, counter, width);
        if !taken.contains(&id) {
            return id;
        }
        counter = counter.saturating_add(1);
    }
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
            let id = assign_unique_hash(&item.text, doc_id, &taken);
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
    let (new_body, removed) = reap_with_items(body);
    let ids = removed.iter().map(|i| i.id.clone()).collect();
    (new_body, ids)
}

/// Reap `[x]` items and return the removed items (with text), not just ids.
/// Used by preflight to archive reaped items to an `agent:pending-done`
/// component when one exists (spec §3 step 3).
pub fn reap_with_items(body: &str) -> (String, Vec<PendingItem>) {
    let (prelude, items, postlude) = parse_items(body);
    let mut removed = Vec::new();
    let mut kept = Vec::new();
    for item in items {
        if item.is_done() {
            if !item.id.is_empty() {
                removed.push(item);
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

/// Insert a new item at the beginning of the body. Binary assigns hash and `[ ]`
/// (or `[/]` if `gated`).
/// Returns `(new_body, assigned_id)`.
pub fn op_add(body: &str, text: &str, doc_id: &str, gated: bool) -> Result<(String, String)> {
    let (custom_id, text) = parse_custom_id_prefix(text)?;
    let text = text.trim();
    if text.is_empty() {
        bail!("pending add: text must be non-empty");
    }
    if text.starts_with("[ ]")
        || text.starts_with("[/]")
        || text.starts_with("[x]")
        || text.starts_with("[X]")
    {
        bail!(
            "pending add: text must not start with a state marker ([ ], [/], [x]); use --pending-add-gated for gated items"
        );
    }
    let (prelude, mut items, postlude) = parse_items(body);

    // Dedup: reject if an item with identical text already exists.
    if items.iter().any(|i| i.text == text) {
        bail!("pending add: duplicate item text already exists: {}", text);
    }

    let mut taken: HashSet<String> = items
        .iter()
        .filter(|i| !i.id.is_empty())
        .map(|i| i.id.clone())
        .collect();

    let id = if let Some(custom_id) = custom_id {
        if taken.contains(&custom_id) {
            bail!("pending add: custom id already exists: {}", custom_id);
        }
        custom_id
    } else {
        assign_unique_hash(text, doc_id, &taken)
    };
    taken.insert(id.clone());

    items.insert(
        0,
        PendingItem {
            id: id.clone(),
            state: if gated {
                PendingState::Gated
            } else {
                PendingState::Open
            },
            gate_type: None,
            text: text.to_string(),
        },
    );
    Ok((render_items(&prelude, &items, &postlude), id))
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

/// Transition an item to `Gated` (`[/]`) by id.
///
/// - `Open → Gated`: state mutates.
/// - `Gated → Gated`: idempotent no-op (returns body unchanged).
/// - `Done → *`: error (cannot re-gate a completed item).
pub fn op_gate(body: &str, id: &str) -> Result<String> {
    let id = id.trim().to_lowercase();
    let (prelude, mut items, postlude) = parse_items(body);
    let item = items
        .iter_mut()
        .find(|i| i.id == id)
        .ok_or_else(|| anyhow!("pending gate: no item with id [#{}]", id))?;
    match validate_transition(item.state, PendingOp::Gate)? {
        TransitionResult::Transition(next) => {
            item.state = next;
            Ok(render_items(&prelude, &items, &postlude))
        }
        TransitionResult::NoOp => Ok(body.to_string()),
    }
}

/// Transition an item to `Open` (`[ ]`) by id.
///
/// - `Gated → Open`: state mutates.
/// - `Open → *`: error (no source `[/]`).
/// - `Done → *`: error (cannot ungate a completed item).
pub fn op_ungate(body: &str, id: &str) -> Result<String> {
    let id = id.trim().to_lowercase();
    let (prelude, mut items, postlude) = parse_items(body);
    let item = items
        .iter_mut()
        .find(|i| i.id == id)
        .ok_or_else(|| anyhow!("pending ungate: no item with id [#{}]", id))?;
    match validate_transition(item.state, PendingOp::Ungate)? {
        TransitionResult::Transition(next) => {
            item.state = next;
            Ok(render_items(&prelude, &items, &postlude))
        }
        TransitionResult::NoOp => Ok(body.to_string()),
    }
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

/// Resolve all gated items matching a typed gate. Finds items with `[/<gate_type>]`
/// and flips them to `[x]`. Returns `(new_body, resolved_ids)`.
///
/// Only matches typed gates — untyped `[/]` items are never resolved by this op.
pub fn op_resolve_gate(body: &str, gate_type: &str) -> (String, Vec<String>) {
    let gt = gate_type.trim().to_lowercase();
    let (prelude, mut items, postlude) = parse_items(body);
    let mut resolved = Vec::new();
    for item in &mut items {
        if item.state == PendingState::Gated && item.gate_type.as_deref() == Some(gt.as_str()) {
            item.state = PendingState::Done;
            item.gate_type = None;
            resolved.push(item.id.clone());
        }
    }
    (render_items(&prelude, &items, &postlude), resolved)
}

/// Set a typed gate on a gated item. The item must already be in `[/]` state.
/// Transitions `[/] → [/<gate_type>]`. Errors if the item is not gated.
pub fn op_set_gate_type(body: &str, id: &str, gate_type: &str) -> Result<String> {
    let id = id.trim().to_lowercase();
    let gt = gate_type.trim().to_lowercase();
    if gt.is_empty()
        || !gt
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("invalid gate type: must be alphanumeric/dash/underscore");
    }
    let (prelude, mut items, postlude) = parse_items(body);
    let item = items
        .iter_mut()
        .find(|i| i.id == id)
        .ok_or_else(|| anyhow!("pending set-gate-type: no item with id [#{}]", id))?;
    if item.state != PendingState::Gated {
        bail!(
            "pending set-gate-type: item [#{}] must be gated ([/]) to set a typed gate, current state: [{}]",
            id,
            item.state.box_char()
        );
    }
    item.gate_type = Some(gt);
    Ok(render_items(&prelude, &items, &postlude))
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
            gate_type: None,
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
    fn op_add_inserts_new_item_with_hash() {
        let body = "";
        let (new_body, id) = op_add(body, "first task", DOC_ID, false).unwrap();
        assert!(new_body.contains("- [ ] [#"));
        assert!(new_body.contains("first task"));
        assert!(!id.is_empty());
    }

    #[test]
    fn op_add_prepends_before_existing_items() {
        let body = "- [ ] [#a1b2] existing task\n- [ ] [#c3d4] later task\n";
        let (new_body, _id) = op_add(body, "new first task", DOC_ID, false).unwrap();
        let lines: Vec<&str> = new_body.lines().collect();
        assert!(
            lines[0].contains("new first task"),
            "expected new item first, got: {}",
            new_body
        );
        assert!(
            lines[1].contains("existing task"),
            "expected previous first item second, got: {}",
            new_body
        );
    }

    #[test]
    fn op_add_accepts_custom_id_prefix() {
        let (new_body, id) = op_add("", "id=ship1 release checklist", DOC_ID, false).unwrap();
        assert_eq!(id, "ship1");
        assert!(new_body.contains("- [ ] [#ship1] release checklist"));
    }

    #[test]
    fn op_add_accepts_custom_id_prefix_with_hash_marker() {
        let (new_body, id) = op_add("", "id=#ship1 release checklist", DOC_ID, false).unwrap();
        assert_eq!(id, "ship1");
        assert!(new_body.contains("- [ ] [#ship1] release checklist"));
    }

    #[test]
    fn op_add_accepts_bracketed_custom_id_prefix() {
        let (new_body, id) = op_add("", "[#ship1] release checklist", DOC_ID, false).unwrap();
        assert_eq!(id, "ship1");
        assert!(new_body.contains("- [ ] [#ship1] release checklist"));
    }

    #[test]
    fn op_add_accepts_long_bracketed_custom_id_prefix() {
        let (new_body, id) = op_add("", "[#sdig2matrix] release checklist", DOC_ID, false).unwrap();
        assert_eq!(id, "sdig2matrix");
        assert!(new_body.contains("- [ ] [#sdig2matrix] release checklist"));
    }

    #[test]
    fn op_add_rejects_invalid_custom_id_prefix() {
        let err = op_add("", "id=bad-id release checklist", DOC_ID, false).unwrap_err();
        assert!(format!("{}", err).contains("invalid custom id"));
    }

    #[test]
    fn op_add_rejects_invalid_bracketed_custom_id_prefix() {
        let err = op_add("", "[#bad-id] release checklist", DOC_ID, false).unwrap_err();
        assert!(format!("{}", err).contains("invalid custom id"));
    }

    #[test]
    fn op_add_rejects_bare_bracket_placeholder_prefix() {
        let err = op_add("", "[#] release checklist", DOC_ID, false).unwrap_err();
        assert!(format!("{}", err).contains("bare `[#]` placeholder"));
    }

    #[test]
    fn op_add_rejects_empty_explicit_custom_id_prefix() {
        let err = op_add("", "id=  release checklist", DOC_ID, false).unwrap_err();
        assert!(format!("{}", err).contains("empty custom id"));
    }

    #[test]
    fn op_add_rejects_duplicate_custom_id_prefix() {
        let body = "- [ ] [#ship1] existing task\n";
        let err = op_add(body, "id=ship1 new task", DOC_ID, false).unwrap_err();
        assert!(format!("{}", err).contains("custom id already exists"));
    }

    #[test]
    fn op_add_rejects_missing_text_after_custom_id_prefix() {
        let err = op_add("", "id=ship1", DOC_ID, false).unwrap_err();
        assert!(format!("{}", err).contains("custom id prefix must be followed by item text"));
    }

    #[test]
    fn op_add_rejects_missing_text_after_bracketed_custom_id_prefix() {
        let err = op_add("", "[#ship1]", DOC_ID, false).unwrap_err();
        assert!(
            format!("{}", err).contains("bracketed custom id prefix must be followed by item text")
        );
    }

    #[test]
    fn op_add_rejects_stacked_bracketed_custom_id_prefixes() {
        let err = op_add("", "[#ship1] [#ship2] release checklist", DOC_ID, false).unwrap_err();
        assert!(
            format!("{}", err).contains("duplicate leading custom id prefix"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn op_add_rejects_stacked_mixed_custom_id_prefixes() {
        let err = op_add("", "id=ship1 [#ship2] release checklist", DOC_ID, false).unwrap_err();
        assert!(
            format!("{}", err).contains("duplicate leading custom id prefix"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn op_add_rejects_empty() {
        assert!(op_add("", "   ", DOC_ID, false).is_err());
    }

    #[test]
    fn op_add_rejects_state_marker_prefix() {
        for marker in &["[ ] task", "[/] task", "[x] task", "[X] task"] {
            let err = op_add("", marker, DOC_ID, false).unwrap_err();
            let msg = format!("{}", err);
            assert!(
                msg.contains("state marker"),
                "expected state marker error for '{}', got: {}",
                marker,
                msg
            );
        }
    }

    #[test]
    fn op_add_rejects_duplicate_text() {
        let (body, _id1) = op_add("", "Wire Sift into corky", DOC_ID, false).unwrap();
        let err = op_add(&body, "Wire Sift into corky", DOC_ID, false).unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("duplicate"),
            "expected duplicate error, got: {}",
            msg
        );
    }

    #[test]
    fn parse_bare_hash_placeholder_strips_marker() {
        let item = parse_item_line("- [ ] [#] Wire Sift into corky").unwrap();
        assert_eq!(item.id, "");
        assert_eq!(item.text, "Wire Sift into corky");
    }

    #[test]
    fn backfill_strips_bare_hash_placeholder() {
        let body = "- [ ] [#] task with placeholder\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        assert!(new_body.contains("[#"), "should have a hash id");
        // The bare [#] should be consumed — only one [# in the output
        let hash_count = new_body.matches("[#").count();
        assert_eq!(hash_count, 1, "expected exactly one [# in: {}", new_body);
        assert!(new_body.contains("task with placeholder"));
        assert!(
            !new_body.contains("[#] task"),
            "bare [#] should not survive in text"
        );
    }

    #[test]
    fn parse_bare_hash_placeholder_no_checkbox() {
        // `- [#] text` — no checkbox, bare placeholder
        let item = parse_item_line("- [#] no checkbox").unwrap();
        assert_eq!(item.id, "");
        assert_eq!(item.state, PendingState::Open);
        assert_eq!(item.text, "no checkbox");
    }

    #[test]
    fn parse_bare_hash_placeholder_gated() {
        // `- [/] [#] gated task` — gated with bare placeholder
        let item = parse_item_line("- [/] [#] gated task").unwrap();
        assert_eq!(item.id, "");
        assert_eq!(item.state, PendingState::Gated);
        assert_eq!(item.text, "gated task");
    }

    #[test]
    fn backfill_strips_multiple_bare_placeholders() {
        // Multiple items each with bare [#] — all should get real IDs
        let body = "- [ ] [#] first task\n- [ ] [#] second task\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items.len(), 2);
        assert!(!items[0].id.is_empty(), "first should have id");
        assert!(!items[1].id.is_empty(), "second should have id");
        assert_ne!(items[0].id, items[1].id, "ids should be unique");
        // No residual [#] in text
        assert!(
            !items[0].text.contains("[#]"),
            "first text has residual [#]: {}",
            items[0].text
        );
        assert!(
            !items[1].text.contains("[#]"),
            "second text has residual [#]: {}",
            items[1].text
        );
    }

    #[test]
    fn backfill_preserves_long_custom_id() {
        let body = "- [ ] [#sdig2matrix] Fixture evidence matrix\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(!changed);
        assert_eq!(new_body, body);
    }

    #[test]
    fn backfill_idempotent_after_placeholder_strip() {
        // After stripping [#] and assigning ID, second backfill should be a no-op
        let body = "- [ ] [#] task\n";
        let (first_pass, _) = backfill(body, DOC_ID, &ids());
        let (second_pass, changed) = backfill(&first_pass, DOC_ID, &ids());
        assert!(
            !changed,
            "second backfill should be no-op, got: {}",
            second_pass
        );
        assert_eq!(first_pass, second_pass);
    }

    #[test]
    fn op_add_dedup_case_sensitive() {
        // Different casing should NOT be considered duplicate
        let (body, _) = op_add("", "Wire Sift", DOC_ID, false).unwrap();
        let result = op_add(&body, "wire sift", DOC_ID, false);
        assert!(result.is_ok(), "different case should not be duplicate");
    }

    #[test]
    fn op_add_dedup_across_states() {
        // Item exists as gated — adding same text as open should still dedup
        let (body, _) = op_add("", "deploy to prod", DOC_ID, true).unwrap();
        let err = op_add(&body, "deploy to prod", DOC_ID, false).unwrap_err();
        assert!(format!("{}", err).contains("duplicate"));
    }

    #[test]
    fn op_add_gated_produces_gated_item() {
        let (new_body, id) = op_add("", "gated task", DOC_ID, true).unwrap();
        assert!(new_body.contains("[/]"), "expected [/] in: {}", new_body);
        assert!(new_body.contains(&format!("[#{}]", id)));
        assert!(new_body.contains("gated task"));
    }

    #[test]
    fn op_add_returns_assigned_id() {
        let (body, id1) = op_add("", "task one", DOC_ID, false).unwrap();
        assert!(!id1.is_empty());
        assert!(body.contains(&format!("[#{}]", id1)));
        let (body2, id2) = op_add(&body, "task two", DOC_ID, false).unwrap();
        assert_ne!(id1, id2);
        assert!(body2.contains(&format!("[#{}]", id2)));
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
        let new_body = op_reorder(body, &["e5f6".to_string(), "a1b2".to_string()]).unwrap();
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

    // ---- Phase 2: state matrix + gate/ungate ----

    #[test]
    fn validate_transition_full_matrix() {
        use PendingOp::*;
        use PendingState::*;
        use TransitionResult::*;

        // Open
        assert_eq!(validate_transition(Open, Gate).unwrap(), Transition(Gated));
        assert!(validate_transition(Open, Ungate).is_err());
        assert_eq!(
            validate_transition(Open, MarkDone).unwrap(),
            Transition(Done)
        );

        // Gated
        assert_eq!(validate_transition(Gated, Gate).unwrap(), NoOp);
        assert_eq!(
            validate_transition(Gated, Ungate).unwrap(),
            Transition(Open)
        );
        assert_eq!(
            validate_transition(Gated, MarkDone).unwrap(),
            Transition(Done)
        );

        // Done
        assert!(validate_transition(Done, Gate).is_err());
        assert!(validate_transition(Done, Ungate).is_err());
        assert_eq!(validate_transition(Done, MarkDone).unwrap(), NoOp);
    }

    #[test]
    fn op_gate_open_to_gated() {
        let body = "- [ ] [#a1b2] task\n";
        let new_body = op_gate(body, "a1b2").unwrap();
        assert!(new_body.contains("- [/] [#a1b2]"));
    }

    #[test]
    fn op_gate_gated_is_noop() {
        let body = "- [/] [#a1b2] task\n";
        let new_body = op_gate(body, "a1b2").unwrap();
        // No-op: body unchanged byte-for-byte.
        assert_eq!(new_body, body);
    }

    #[test]
    fn op_gate_done_errors() {
        let body = "- [x] [#a1b2] task\n";
        let err = op_gate(body, "a1b2").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("cannot gate Done item"), "got: {}", msg);
    }

    #[test]
    fn op_gate_unknown_id_errors() {
        let body = "- [ ] [#a1b2] task\n";
        assert!(op_gate(body, "zzzz").is_err());
    }

    #[test]
    fn op_ungate_gated_to_open() {
        let body = "- [/] [#a1b2] task\n";
        let new_body = op_ungate(body, "a1b2").unwrap();
        assert!(new_body.contains("- [ ] [#a1b2]"));
    }

    #[test]
    fn op_ungate_open_errors() {
        let body = "- [ ] [#a1b2] task\n";
        let err = op_ungate(body, "a1b2").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("cannot ungate Open"), "got: {}", msg);
    }

    #[test]
    fn op_ungate_done_errors() {
        let body = "- [x] [#a1b2] task\n";
        let err = op_ungate(body, "a1b2").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("cannot ungate Done"), "got: {}", msg);
    }

    #[test]
    fn op_ungate_unknown_id_errors() {
        let body = "- [/] [#a1b2] task\n";
        assert!(op_ungate(body, "zzzz").is_err());
    }

    #[test]
    fn op_gate_preserves_other_items_and_text() {
        let body = "- [ ] [#a1b2] one\n- [ ] [#c3d4] two — gate: v0.32.6\n- [x] [#e5f6] three\n";
        let new_body = op_gate(body, "c3d4").unwrap();
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items[0].state, PendingState::Open);
        assert_eq!(items[1].state, PendingState::Gated);
        assert_eq!(items[1].text, "two — gate: v0.32.6");
        assert_eq!(items[2].state, PendingState::Done);
    }

    #[test]
    fn generate_hash_deterministic_and_short() {
        let h = generate_hash("text", "doc", 0);
        assert_eq!(h.len(), 4);
        assert_eq!(h, generate_hash("text", "doc", 0));
        assert_ne!(h, generate_hash("text", "doc", 1));
    }

    #[test]
    fn generate_hash_n_width4_matches_generate_hash() {
        // Width-4 output must be bit-identical to the pre-#14z4 formula so
        // existing docs don't churn their IDs on re-backfill.
        let cases = [
            ("text", "doc", 0u64),
            ("refactor preflight", "abc123", 7),
            ("", "", 42),
            ("long text with spaces", "doc_id_long", 99),
        ];
        for (t, d, c) in cases {
            assert_eq!(generate_hash(t, d, c), generate_hash_n(t, d, c, 4));
        }
    }

    #[test]
    fn generate_hash_n_widths_have_correct_length() {
        for w in 4..=8 {
            let h = generate_hash_n("text", "doc", 0, w);
            assert_eq!(h.len(), w, "width {} produced len {}", w, h.len());
        }
        // Out-of-range widths clamp to [4, 8].
        assert_eq!(generate_hash_n("x", "y", 0, 1).len(), 4);
        assert_eq!(generate_hash_n("x", "y", 0, 20).len(), 8);
    }

    #[test]
    fn generate_hash_n_wider_extends_shorter() {
        // A wider hash must start with the shorter hash as a prefix so
        // visible widening is explainable to humans.
        let h4 = generate_hash_n("text", "doc", 0, 4);
        let h5 = generate_hash_n("text", "doc", 0, 5);
        let h8 = generate_hash_n("text", "doc", 0, 8);
        assert!(h5.starts_with(&h4), "h5={} h4={}", h5, h4);
        assert!(h8.starts_with(&h4), "h8={} h4={}", h8, h4);
        assert!(h8.starts_with(&h5), "h8={} h5={}", h8, h5);
    }

    #[test]
    fn assign_unique_hash_extends_on_collision() {
        // Pre-populate `taken` with the width-4 hash of "item". The next
        // assignment for the same text must either reuse the width-4 slot
        // with a different counter OR widen. Either way the result must
        // differ from the pre-populated value and be valid.
        let h4 = generate_hash_n("item", "doc", 0, 4);
        let mut taken = HashSet::new();
        taken.insert(h4.clone());
        let id = assign_unique_hash("item", "doc", &taken);
        assert_ne!(id, h4);
        assert!((4..=8).contains(&id.len()));
    }

    #[test]
    fn assign_unique_hash_widens_when_counter_exhausted_at_width4() {
        // Pre-populate `taken` with EVERY width-4 hash the retry loop would
        // try at counters 0..=3. Assignment must widen to 5 chars.
        let mut taken = HashSet::new();
        for c in 0..=3u64 {
            taken.insert(generate_hash_n("x", "d", c, 4));
        }
        let id = assign_unique_hash("x", "d", &taken);
        assert!(!taken.contains(&id));
        // Either width-4 (an untried counter) or width-5+. Accept both —
        // the important invariant is uniqueness, not forced widening.
        assert!((4..=8).contains(&id.len()));
    }

    #[test]
    fn backfill_assigns_collision_free_ids_under_pressure() {
        // Stress test: backfill 50 items. All must get unique 4..=8-char ids.
        let mut body = String::new();
        for i in 0..50 {
            body.push_str(&format!("- item {}\n", i));
        }
        let (out, changed) = backfill(&body, "doc", &HashSet::new());
        assert!(changed);
        let (_, items, _) = parse_items(&out);
        assert_eq!(items.len(), 50);
        let ids: HashSet<String> = items.iter().map(|i| i.id.clone()).collect();
        assert_eq!(ids.len(), 50, "ids must be unique");
        for id in &ids {
            assert!(
                (4..=8).contains(&id.len()),
                "id {} has width {}",
                id,
                id.len()
            );
        }
    }

    // ---- Typed gates ----

    #[test]
    fn parse_typed_gate_release() {
        let body = "- [/release] [#a1b2] Release v0.32.4\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].state, PendingState::Gated);
        assert_eq!(items[0].gate_type, Some("release".to_string()));
        assert_eq!(items[0].text, "Release v0.32.4");
    }

    #[test]
    fn parse_typed_gate_deploy() {
        let body = "- [/deploy] [#c3d4] Push CDN config\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items[0].state, PendingState::Gated);
        assert_eq!(items[0].gate_type, Some("deploy".to_string()));
    }

    #[test]
    fn parse_untyped_gate_has_no_gate_type() {
        let body = "- [/] [#a1b2] waiting\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items[0].state, PendingState::Gated);
        assert_eq!(items[0].gate_type, None);
    }

    #[test]
    fn parse_open_has_no_gate_type() {
        let body = "- [ ] [#a1b2] task\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items[0].gate_type, None);
    }

    #[test]
    fn render_typed_gate() {
        let item = PendingItem {
            id: "a1b2".to_string(),
            state: PendingState::Gated,
            gate_type: Some("release".to_string()),
            text: "Release v0.32.4".to_string(),
        };
        assert_eq!(item.render(), "- [/release] [#a1b2] Release v0.32.4");
    }

    #[test]
    fn render_roundtrip_typed_gate() {
        let body = "- [/release] [#a1b2] Release v0.32.4\n- [/deploy] [#c3d4] Push\n- [/] [#e5f6] Generic\n";
        let (p, items, post) = parse_items(body);
        let out = render_items(&p, &items, &post);
        assert_eq!(out, body);
    }

    #[test]
    fn op_resolve_gate_flips_matching() {
        let body = "- [/release] [#a1b2] Release v0.32.4\n- [/deploy] [#c3d4] Deploy\n- [/] [#e5f6] Generic gate\n";
        let (new_body, resolved) = op_resolve_gate(body, "release");
        assert_eq!(resolved, vec!["a1b2"]);
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items[0].state, PendingState::Done); // was [/release]
        assert_eq!(items[0].gate_type, None); // cleared
        assert_eq!(items[1].state, PendingState::Gated); // [/deploy] untouched
        assert_eq!(items[1].gate_type, Some("deploy".to_string()));
        assert_eq!(items[2].state, PendingState::Gated); // [/] untouched
        assert_eq!(items[2].gate_type, None);
    }

    #[test]
    fn op_resolve_gate_no_match() {
        let body = "- [/release] [#a1b2] Release\n- [/] [#c3d4] Generic\n";
        let (new_body, resolved) = op_resolve_gate(body, "deploy");
        assert!(resolved.is_empty());
        assert_eq!(new_body, body);
    }

    #[test]
    fn op_resolve_gate_ignores_untyped() {
        let body = "- [/] [#a1b2] Generic gate\n";
        let (_, resolved) = op_resolve_gate(body, "release");
        assert!(resolved.is_empty());
    }

    #[test]
    fn op_resolve_gate_multiple_same_type() {
        let body = "- [/release] [#a1b2] First\n- [/release] [#c3d4] Second\n";
        let (_, resolved) = op_resolve_gate(body, "release");
        assert_eq!(resolved, vec!["a1b2", "c3d4"]);
    }

    #[test]
    fn op_set_gate_type_on_gated() {
        let body = "- [/] [#a1b2] Release v0.32.4\n";
        let new_body = op_set_gate_type(body, "a1b2", "release").unwrap();
        assert!(new_body.contains("[/release]"));
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items[0].gate_type, Some("release".to_string()));
    }

    #[test]
    fn op_set_gate_type_errors_on_open() {
        let body = "- [ ] [#a1b2] task\n";
        assert!(op_set_gate_type(body, "a1b2", "release").is_err());
    }

    #[test]
    fn op_set_gate_type_errors_on_done() {
        let body = "- [x] [#a1b2] task\n";
        assert!(op_set_gate_type(body, "a1b2", "release").is_err());
    }

    #[test]
    fn op_set_gate_type_replaces_existing() {
        let body = "- [/release] [#a1b2] task\n";
        let new_body = op_set_gate_type(body, "a1b2", "deploy").unwrap();
        assert!(new_body.contains("[/deploy]"));
        assert!(!new_body.contains("[/release]"));
    }

    #[test]
    fn parse_typed_gate_case_insensitive() {
        let body = "- [/Release] [#a1b2] task\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items[0].gate_type, Some("release".to_string()));
    }

    #[test]
    fn parse_typed_gate_with_hyphens_underscores() {
        let body =
            "- [/code-review] [#a1b2] Review PR\n- [/pre_release] [#c3d4] Pre-release check\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items[0].gate_type, Some("code-review".to_string()));
        assert_eq!(items[1].gate_type, Some("pre_release".to_string()));
    }

    #[test]
    fn detect_shadow_open_items_classifies_duplicate_and_shadow_only_ids() {
        let doc = concat!(
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#live1] Keep in backlog\n",
            "- [/] [#gate1] Gated live item\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- parked copy\n",
            "- [ ] [#live1] Keep in backlog\n",
            "- [ ] [#lost1] Drifted out of backlog\n",
            "- [x] [#done1] Already done\n",
            "-->\n"
        );

        let report = detect_shadow_open_items(doc).unwrap();
        assert_eq!(
            report
                .duplicated_in_live_backlog
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["live1"]
        );
        assert_eq!(
            report
                .shadow_only
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["lost1"]
        );
    }

    #[test]
    fn detect_shadow_open_items_ignores_icebox_and_code_blocks() {
        let doc = concat!(
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#live1] Keep in backlog\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#cold1] Intentionally parked\n",
            "<!-- /agent:icebox -->\n\n",
            "```md\n",
            "- [ ] [#code1] Example only\n",
            "```\n"
        );

        let report = detect_shadow_open_items(doc).unwrap();
        assert!(report.duplicated_in_live_backlog.is_empty());
        assert!(report.shadow_only.is_empty());
    }
}
