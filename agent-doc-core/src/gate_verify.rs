//! # Module: gate_verify
//!
//! Typed proof/disproof predicates for gated `[/]` review items (`#optverify`).
//!
//! A gated review item often names an exact `ops.log` marker that PROVES it
//! (e.g. `live_prompt_drift_auto_recovered`) and an exact string that DISPROVES
//! it (e.g. `looks like a manual cleanup`). When the gate carries those as a
//! typed predicate, the binary can decide proof/disproof itself every time it
//! already reads `ops.log` — turning a standing human gate into an automatic
//! check (no dedicated live-verify session).
//!
//! ## Persistence (`#optv1`)
//!
//! The predicate is carried inline on the item text as an HTML-comment
//! annotation so it round-trips verbatim through the pending parser without
//! touching the `[/<gate_type>]` checkbox grammar:
//!
//! ```text
//! - [/] [#saev] early-ack ... <!-- gate-verify verify="early_ack_pending" disproof="false ack-timeout" set_at="1749526200" -->
//! ```
//!
//! Only markers emitted **at or after** `set_at` count, so a stale pre-gate
//! marker can never falsely prove a freshly re-opened gate.
//!
//! ## Scan (`#optv2`)
//!
//! [`scan_ops_log`] is a pure function over `(predicate, ops.log content)` →
//! [`VerifyOutcome`]. Disproof wins ties (a disproved gate must never
//! auto-resolve); neither marker → `Pending`.
//!
//! ## Prose exclusion (`#gng8`)
//!
//! Several ops.log entries embed *document content* (for example
//! `queue_diff_active_prompt_differs ... prompt_changes=["..."]`, route
//! `prompt={:?}`, session `tail={:?}`). Backlog or response text that merely
//! *mentions* a marker token would otherwise prove the gate from its own
//! description. Embedded content always arrives through `{:?}` debug
//! formatting, which wraps strings in double quotes — so the scan strips
//! double-quoted spans from each message before matching. Structured marker
//! emissions (`[claim] cross-session-reject pane_id=...`,
//! `[ipc-socket] early_ack_pending ...`, `[s760] clear-decision ...`) are
//! plain unquoted message text and must stay that way to remain provable.

use serde::{Deserialize, Serialize};

/// Marker used to delimit the inline predicate annotation.
const ANNOTATION_OPEN: &str = "<!-- gate-verify";
const ANNOTATION_CLOSE: &str = "-->";

/// Typed proof/disproof predicate carried inline on a gated `[/]` item.
///
/// `verify` / `disproof` are matched as plain substrings against `ops.log`
/// message bodies (the text after the `[<epoch>] ` prefix). `set_at` is the
/// gate-set time in epoch seconds; only markers at or after it are considered.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GatePredicate {
    /// ops.log substring that PROVES the gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify: Option<String>,
    /// ops.log substring that DISPROVES the gate (disproof wins ties).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disproof: Option<String>,
    /// Gate-set time as epoch seconds; only markers at/after this count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_at: Option<u64>,
}

impl GatePredicate {
    /// True when the predicate carries at least one usable matcher.
    pub fn is_actionable(&self) -> bool {
        self.verify.as_deref().is_some_and(|s| !s.is_empty())
            || self.disproof.as_deref().is_some_and(|s| !s.is_empty())
    }
}

/// Result of scanning `ops.log` for a gate predicate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum VerifyOutcome {
    /// Proof marker seen after the gate time, no disproof.
    Provable {
        /// The matched proof substring.
        marker: String,
        /// Epoch seconds of the earliest qualifying proof line.
        at: u64,
    },
    /// Disproof marker seen after the gate time (disproof wins).
    Failed {
        /// The matched disproof substring.
        marker: String,
        /// Epoch seconds of the earliest qualifying disproof line.
        at: u64,
    },
    /// Neither marker seen after the gate time.
    Pending,
}

impl VerifyOutcome {
    /// Lowercase status discriminant (`provable` / `failed` / `pending`).
    pub fn status_str(&self) -> &'static str {
        match self {
            VerifyOutcome::Provable { .. } => "provable",
            VerifyOutcome::Failed { .. } => "failed",
            VerifyOutcome::Pending => "pending",
        }
    }
}

/// Parse `[<epoch>] <message>` into `(epoch, message)`. Returns `None` for
/// lines that do not start with a numeric epoch bracket.
fn parse_ops_line(line: &str) -> Option<(u64, &str)> {
    let rest = line.strip_prefix('[')?;
    let close = rest.find(']')?;
    let ts: u64 = rest[..close].trim().parse().ok()?;
    let msg = rest[close + 1..].trim_start();
    Some((ts, msg))
}

/// Strip double-quoted spans from an ops.log message (`#gng8`).
///
/// Content-logging entries embed document/prompt text via `{:?}` debug
/// formatting, which always wraps strings in double quotes with `\"` escapes.
/// A marker token inside such a span is embedded prose, not a structured
/// emission, so it must not prove or disprove a gate. An unterminated quote
/// drops the rest of the line (fail safe: malformed content stays excluded).
fn strip_quoted_spans(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    let mut chars = msg.chars();
    let mut in_quote = false;
    while let Some(c) = chars.next() {
        if in_quote {
            match c {
                '\\' => {
                    let _ = chars.next();
                }
                '"' => {
                    in_quote = false;
                    out.push('"');
                }
                _ => {}
            }
        } else {
            if c == '"' {
                in_quote = true;
            }
            out.push(c);
        }
    }
    out
}

/// Scan `ops.log` content for a gate predicate (`#optv2`, pure).
///
/// Decision matrix (only markers at/after `set_at` count):
/// - disproof substring seen anywhere → [`VerifyOutcome::Failed`] (disproof wins)
/// - else proof substring seen → [`VerifyOutcome::Provable`]
/// - else → [`VerifyOutcome::Pending`]
///
/// Both `Provable` and `Failed` report the earliest qualifying line so repeated
/// markers are stable. Markers are matched against the message with
/// double-quoted spans stripped ([`strip_quoted_spans`], `#gng8`) so embedded
/// document prose cannot prove a gate from its own description.
pub fn scan_ops_log(predicate: &GatePredicate, ops_log: &str) -> VerifyOutcome {
    let set_at = predicate.set_at.unwrap_or(0);
    let verify = predicate.verify.as_deref().filter(|s| !s.is_empty());
    let disproof = predicate.disproof.as_deref().filter(|s| !s.is_empty());

    let mut proof_hit: Option<u64> = None;
    let mut disproof_hit: Option<u64> = None;

    for line in ops_log.lines() {
        let Some((ts, msg)) = parse_ops_line(line) else {
            continue;
        };
        if ts < set_at {
            continue;
        }
        let msg = strip_quoted_spans(msg);
        if let Some(d) = disproof
            && msg.contains(d)
        {
            disproof_hit = Some(disproof_hit.map_or(ts, |t| t.min(ts)));
        }
        if let Some(v) = verify
            && msg.contains(v)
        {
            proof_hit = Some(proof_hit.map_or(ts, |t| t.min(ts)));
        }
    }

    if let Some(at) = disproof_hit {
        return VerifyOutcome::Failed {
            marker: disproof.unwrap_or_default().to_string(),
            at,
        };
    }
    if let Some(at) = proof_hit {
        return VerifyOutcome::Provable {
            marker: verify.unwrap_or_default().to_string(),
            at,
        };
    }
    VerifyOutcome::Pending
}

/// Parse the inline `<!-- gate-verify ... -->` annotation out of item text.
///
/// Returns `None` when no annotation is present. Attribute values are
/// double-quoted (`verify="..."`) so they may contain spaces.
pub fn parse_gate_predicate(text: &str) -> Option<GatePredicate> {
    let open = text.find(ANNOTATION_OPEN)?;
    let after_open = &text[open + ANNOTATION_OPEN.len()..];
    let close_rel = after_open.find(ANNOTATION_CLOSE)?;
    let inner = &after_open[..close_rel];

    let mut pred = GatePredicate::default();
    for (key, value) in parse_quoted_attrs(inner) {
        match key.as_str() {
            "verify" => pred.verify = Some(value),
            "disproof" => pred.disproof = Some(value),
            "set_at" => pred.set_at = value.trim().parse::<u64>().ok(),
            _ => {}
        }
    }
    Some(pred)
}

/// Parse `key="value"` pairs out of an attribute string. Values are everything
/// between the quotes (spaces allowed); keys are unquoted leading identifiers.
fn parse_quoted_attrs(input: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip to the next key character.
        while i < bytes.len() && !(bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        let key_start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        if key_start == i {
            break;
        }
        let key = input[key_start..i].to_string();
        // Skip whitespace then expect `=`.
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            continue;
        }
        i += 1; // consume '='
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'"' {
            continue;
        }
        i += 1; // consume opening quote
        let val_start = i;
        while i < bytes.len() && bytes[i] != b'"' {
            i += 1;
        }
        let value = input[val_start..i].to_string();
        i += 1; // consume closing quote
        out.push((key, value));
    }
    out
}

/// Render the predicate to its canonical inline annotation. Empty fields are
/// omitted. Returns an empty string when the predicate has no fields at all.
pub fn render_annotation(predicate: &GatePredicate) -> String {
    let mut parts = String::new();
    if let Some(v) = predicate.verify.as_deref().filter(|s| !s.is_empty()) {
        parts.push_str(&format!(" verify=\"{}\"", v));
    }
    if let Some(d) = predicate.disproof.as_deref().filter(|s| !s.is_empty()) {
        parts.push_str(&format!(" disproof=\"{}\"", d));
    }
    if let Some(t) = predicate.set_at {
        parts.push_str(&format!(" set_at=\"{}\"", t));
    }
    if parts.is_empty() {
        return String::new();
    }
    format!("{}{} {}", ANNOTATION_OPEN, parts, ANNOTATION_CLOSE)
}

/// Strip any existing `<!-- gate-verify ... -->` annotation from item text,
/// trimming the trailing whitespace it leaves behind.
pub fn strip_annotation(text: &str) -> String {
    let Some(open) = text.find(ANNOTATION_OPEN) else {
        return text.to_string();
    };
    let after_open = &text[open + ANNOTATION_OPEN.len()..];
    let Some(close_rel) = after_open.find(ANNOTATION_CLOSE) else {
        return text.to_string();
    };
    let close_abs = open + ANNOTATION_OPEN.len() + close_rel + ANNOTATION_CLOSE.len();
    let mut out = String::with_capacity(text.len());
    out.push_str(text[..open].trim_end());
    let tail = &text[close_abs..];
    if !tail.trim().is_empty() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(tail.trim_start());
    }
    out
}

/// Replace (or insert) the gate-verify annotation in item text. Existing
/// annotations are removed first so the operation is idempotent.
pub fn upsert_annotation(text: &str, predicate: &GatePredicate) -> String {
    let base = strip_annotation(text);
    let annotation = render_annotation(predicate);
    if annotation.is_empty() {
        return base;
    }
    if base.trim().is_empty() {
        return annotation;
    }
    format!("{} {}", base.trim_end(), annotation)
}

/// Parse a `verify=...;disproof=...` predicate spec (the value half of a CLI
/// `--pending-set-verify id=<spec>` argument) into a [`GatePredicate`].
///
/// Accepts `ops_log:` prefixes on the values (stripped) so the operator can
/// write `verify=ops_log:marker` matching the plan vocabulary. `set_at` is not
/// accepted here — it is stamped by the caller at write time.
pub fn parse_predicate_spec(spec: &str) -> GatePredicate {
    let mut pred = GatePredicate::default();
    for clause in spec.split(';') {
        let clause = clause.trim();
        let Some((key, raw)) = clause.split_once('=') else {
            continue;
        };
        let value = raw
            .trim()
            .strip_prefix("ops_log:")
            .unwrap_or_else(|| raw.trim())
            .trim()
            .to_string();
        match key.trim() {
            "verify" => pred.verify = Some(value).filter(|s| !s.is_empty()),
            "disproof" => pred.disproof = Some(value).filter(|s| !s.is_empty()),
            _ => {}
        }
    }
    pred
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ops_line_extracts_epoch_and_message() {
        assert_eq!(
            parse_ops_line("[1749526200] early_ack_pending emitted"),
            Some((1749526200, "early_ack_pending emitted"))
        );
        assert_eq!(parse_ops_line("no bracket"), None);
        assert_eq!(parse_ops_line("[notnum] x"), None);
    }

    #[test]
    fn scan_provable_when_marker_after_gate() {
        let pred = GatePredicate {
            verify: Some("early_ack_pending".to_string()),
            disproof: None,
            set_at: Some(100),
        };
        let log = "[90] early_ack_pending stale\n[150] early_ack_pending emitted\n";
        assert_eq!(
            scan_ops_log(&pred, log),
            VerifyOutcome::Provable {
                marker: "early_ack_pending".to_string(),
                at: 150
            }
        );
    }

    #[test]
    fn scan_ignores_marker_before_gate_time() {
        let pred = GatePredicate {
            verify: Some("early_ack_pending".to_string()),
            disproof: None,
            set_at: Some(200),
        };
        let log = "[150] early_ack_pending emitted\n";
        assert_eq!(scan_ops_log(&pred, log), VerifyOutcome::Pending);
    }

    #[test]
    fn scan_disproof_wins_over_proof() {
        let pred = GatePredicate {
            verify: Some("ok".to_string()),
            disproof: Some("manual cleanup".to_string()),
            set_at: Some(0),
        };
        let log = "[10] ok happened\n[20] looks like a manual cleanup\n";
        assert_eq!(
            scan_ops_log(&pred, log),
            VerifyOutcome::Failed {
                marker: "manual cleanup".to_string(),
                at: 20
            }
        );
    }

    #[test]
    fn scan_pending_when_no_markers() {
        let pred = GatePredicate {
            verify: Some("never".to_string()),
            disproof: Some("nope".to_string()),
            set_at: Some(0),
        };
        assert_eq!(scan_ops_log(&pred, "[10] unrelated\n"), VerifyOutcome::Pending);
    }

    #[test]
    fn scan_reports_earliest_qualifying_line() {
        let pred = GatePredicate {
            verify: Some("hit".to_string()),
            disproof: None,
            set_at: Some(0),
        };
        let log = "[30] hit\n[10] hit\n[20] hit\n";
        assert_eq!(
            scan_ops_log(&pred, log),
            VerifyOutcome::Provable {
                marker: "hit".to_string(),
                at: 10
            }
        );
    }

    #[test]
    fn scan_ignores_marker_inside_quoted_prose() {
        // #gng8: queue_diff_active_prompt_differs logs document content via
        // {:?}, so a backlog item *describing* the marker must not prove it.
        let pred = GatePredicate {
            verify: Some("cross-session-reject".to_string()),
            disproof: None,
            set_at: Some(0),
        };
        let log = "[10] queue_diff_active_prompt_differs file=doc.md prompt_changes=[\"- [ ] [#4wxr] claim emits cross-session-reject pane_id=..\"] queue_head=\"[#x]\"\n";
        assert_eq!(scan_ops_log(&pred, log), VerifyOutcome::Pending);
    }

    #[test]
    fn scan_ignores_disproof_inside_quoted_prose() {
        let pred = GatePredicate {
            verify: Some("ok marker".to_string()),
            disproof: Some("manual cleanup".to_string()),
            set_at: Some(0),
        };
        let log = "[5] route_dispatch_queued prompt=Some(\"discussing the manual cleanup path\")\n[10] ok marker emitted\n";
        assert_eq!(
            scan_ops_log(&pred, log),
            VerifyOutcome::Provable {
                marker: "ok marker".to_string(),
                at: 10
            }
        );
    }

    #[test]
    fn scan_structured_marker_proves_despite_prose_mentions() {
        let pred = GatePredicate {
            verify: Some("cross-session-reject".to_string()),
            disproof: None,
            set_at: Some(0),
        };
        let log = concat!(
            "[10] queue_diff_active_prompt_differs prompt_changes=[\"mentions cross-session-reject in prose\"]\n",
            "[20] [claim] cross-session-reject pane_id=%43 pane_session=5 configured=0\n",
        );
        assert_eq!(
            scan_ops_log(&pred, log),
            VerifyOutcome::Provable {
                marker: "cross-session-reject".to_string(),
                at: 20
            }
        );
    }

    #[test]
    fn strip_quoted_spans_handles_escaped_quotes_and_unterminated() {
        assert_eq!(
            strip_quoted_spans("a=\"text \\\"inner\\\" more\" tail marker"),
            "a=\"\" tail marker"
        );
        // Unterminated quote drops the rest of the line (fail safe).
        assert_eq!(strip_quoted_spans("a=\"unterminated marker"), "a=\"");
        assert_eq!(strip_quoted_spans("no quotes marker"), "no quotes marker");
    }

    #[test]
    fn annotation_round_trips_with_spaces() {
        let pred = GatePredicate {
            verify: Some("early_ack_pending".to_string()),
            disproof: Some("false ack-timeout".to_string()),
            set_at: Some(1749526200),
        };
        let text = format!("early-ack live verify {}", render_annotation(&pred));
        let parsed = parse_gate_predicate(&text).unwrap();
        assert_eq!(parsed, pred);
    }

    #[test]
    fn parse_returns_none_without_annotation() {
        assert_eq!(parse_gate_predicate("plain item text"), None);
    }

    #[test]
    fn strip_annotation_removes_and_trims() {
        let pred = GatePredicate {
            verify: Some("m".to_string()),
            disproof: None,
            set_at: Some(5),
        };
        let text = format!("body text {}", render_annotation(&pred));
        assert_eq!(strip_annotation(&text), "body text");
    }

    #[test]
    fn upsert_is_idempotent() {
        let pred = GatePredicate {
            verify: Some("m".to_string()),
            disproof: None,
            set_at: Some(5),
        };
        let once = upsert_annotation("body", &pred);
        let twice = upsert_annotation(&once, &pred);
        assert_eq!(once, twice);
        assert!(twice.starts_with("body "));
        assert_eq!(parse_gate_predicate(&twice).unwrap(), pred);
    }

    #[test]
    fn upsert_replaces_existing_predicate() {
        let first = GatePredicate {
            verify: Some("old".to_string()),
            disproof: None,
            set_at: Some(1),
        };
        let second = GatePredicate {
            verify: Some("new".to_string()),
            disproof: Some("bad".to_string()),
            set_at: Some(2),
        };
        let text = upsert_annotation(&upsert_annotation("body", &first), &second);
        let parsed = parse_gate_predicate(&text).unwrap();
        assert_eq!(parsed, second);
        // Only one annotation present.
        assert_eq!(text.matches(ANNOTATION_OPEN).count(), 1);
    }

    #[test]
    fn parse_predicate_spec_strips_ops_log_prefix() {
        let pred = parse_predicate_spec("verify=ops_log:early_ack_pending;disproof=ops_log:false ack-timeout");
        assert_eq!(pred.verify.as_deref(), Some("early_ack_pending"));
        assert_eq!(pred.disproof.as_deref(), Some("false ack-timeout"));
        assert_eq!(pred.set_at, None);
    }

    #[test]
    fn parse_predicate_spec_verify_only() {
        let pred = parse_predicate_spec("verify=marker_x");
        assert_eq!(pred.verify.as_deref(), Some("marker_x"));
        assert_eq!(pred.disproof, None);
    }

    #[test]
    fn is_actionable_requires_a_matcher() {
        assert!(!GatePredicate::default().is_actionable());
        assert!(GatePredicate {
            verify: Some("x".to_string()),
            ..Default::default()
        }
        .is_actionable());
    }
}
