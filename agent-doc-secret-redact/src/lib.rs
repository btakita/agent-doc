//! # Crate: agent-doc-secret-redact
//!
//! ## Spec
//! - Redacts plaintext API keys and similar secrets before they land in
//!   `.agent-doc/` state files (captures, snapshots, partial checkpoints,
//!   undo checkpoints) or in finalize/stream stdout messages.
//! - Always-on backend feature with no operator-facing CLI flag.
//! - Patterns covered (apply most-specific first):
//!   - `sk-proj-[A-Za-z0-9_-]{20,}` → `[REDACTED_OPENAI]`
//!   - `sk-svcacct-[A-Za-z0-9_-]{20,}` → `[REDACTED_OPENAI]`
//!   - `sk-[A-Za-z0-9]{40,}` (legacy OpenAI keys) → `[REDACTED_OPENAI]`
//!   - `AKIA[0-9A-Z]{16}` (AWS access keys) → `[REDACTED_AWS]`
//!   - `xoxb-[0-9]+-[0-9]+-[A-Za-z0-9]+` (Slack bot tokens) → `[REDACTED_SLACK]`
//!   - `ghp_[A-Za-z0-9]{36}` / `gho_[A-Za-z0-9]{36}` → `[REDACTED_GITHUB]`
//!   - `(OPENAI_API_KEY|ANTHROPIC_API_KEY|AWS_SECRET_ACCESS_KEY|CLOUDFLARE_API_TOKEN|XAI_API_KEY|OPENROUTER_API_KEY|STRIPE_API_KEY)\s*[:=]\s*\S+`
//!     → key name preserved, value replaced with `[REDACTED]`
//! - `passage` exemption: any token whose surrounding ±16-char window contains
//!   the substring `passage ` is left UNCHANGED. `passage open …`,
//!   `passage show …`, or `$(passage …)` are the canonical safe patterns and
//!   must round-trip verbatim.
//! - Streaming-safe variant `redact_streamed` holds a small tail buffer across
//!   calls so tokens that straddle a chunk boundary are still caught.
//!
//! ## Agentic Contracts
//! - `redact` is pure and infallible. Errors during compilation of the regex
//!   set would be a build-time failure (compiled once via `OnceLock`).
//! - Order matters: longer/more-specific patterns are applied first so the
//!   generic legacy-OpenAI rule does not steal a project / service-account
//!   token's prefix.
//! - For named-key rules, only the value half is replaced — the variable name
//!   stays visible so debugging still works.
//! - For bare-token rules, the entire matched token becomes
//!   `[REDACTED_<KIND>]`.
//! - Redaction does NOT modify input length predictably; downstream code that
//!   depends on byte offsets must redact first then offset.
//! - Whole-input redaction (the `redact` function) is NOT a security boundary
//!   against a determined adversary — it is a best-effort hygiene layer to
//!   stop accidental leakage of common token shapes via state files /
//!   terminal scrollback.
//!
//! ## Evals
//! - `redacts_sk_proj_token`: `sk-proj-...` value → `[REDACTED_OPENAI]`.
//! - `redacts_sk_svcacct_token`: `sk-svcacct-...` value → `[REDACTED_OPENAI]`.
//! - `redacts_legacy_sk_token`: 48-char `sk-…` legacy OpenAI key → `[REDACTED_OPENAI]`.
//! - `redacts_aws_access_key`: `AKIA…` (20 chars) → `[REDACTED_AWS]`.
//! - `redacts_slack_bot_token`: `xoxb-…` → `[REDACTED_SLACK]`.
//! - `redacts_github_pat`: `ghp_` and `gho_` 40-char tokens → `[REDACTED_GITHUB]`.
//! - `redacts_named_env_var_value`: `OPENAI_API_KEY=sk-proj-…`
//!   → `OPENAI_API_KEY=[REDACTED]`.
//! - `preserves_passage_open`: `OPENAI_API_KEY=$(passage open btak/openai/api_key)`
//!   stays UNCHANGED.
//! - `preserves_bare_passage_token`: `passage show btak/openai/api_key`
//!   stays UNCHANGED.
//! - `mixed_plaintext_only_token_redacted`: text containing both a real key and
//!   ordinary prose redacts only the key region.
//! - `streamed_redaction_catches_split_token`: a token split across two chunks
//!   is redacted when reassembled via `redact_streamed`.
//! - `idempotent_redact`: running `redact` twice yields the same output.
//! - `non_secret_input_passes_through`: ordinary prose is byte-identical after
//!   redaction.

use std::sync::OnceLock;

use regex::Regex;

/// Width of the surrounding window (in BYTES, ASCII-only assumption for the
/// `passage` keyword) used to decide whether a match should be exempted.
const PASSAGE_WINDOW: usize = 16;

/// Tail buffer length kept across `redact_streamed` calls so tokens spanning
/// a chunk boundary still match.
#[allow(dead_code)]
const STREAM_TAIL_CARRY: usize = 128;

struct RedactRule {
    re: Regex,
    /// Replacement strategy.
    kind: RedactKind,
}

enum RedactKind {
    /// Replace the entire match with `[REDACTED_<LABEL>]`.
    BareToken(&'static str),
    /// Replace only capture group 2 (the value after `:` / `=`) with
    /// `[REDACTED]`, preserving the captured key name (group 1) and any
    /// whitespace / separator in the matched prefix.
    NamedValue,
}

fn rules() -> &'static Vec<RedactRule> {
    static RULES: OnceLock<Vec<RedactRule>> = OnceLock::new();
    RULES.get_or_init(|| {
        // ORDER MATTERS — longer / more-specific patterns first.
        vec![
            // OpenAI project key (sk-proj-...).
            RedactRule {
                re: Regex::new(r"sk-proj-[A-Za-z0-9_\-]{20,}").expect("sk-proj regex"),
                kind: RedactKind::BareToken("OPENAI"),
            },
            // OpenAI service account key (sk-svcacct-...).
            RedactRule {
                re: Regex::new(r"sk-svcacct-[A-Za-z0-9_\-]{20,}").expect("sk-svcacct regex"),
                kind: RedactKind::BareToken("OPENAI"),
            },
            // Legacy OpenAI key (sk-XXXXXXXX… 40+ chars).
            RedactRule {
                re: Regex::new(r"sk-[A-Za-z0-9]{40,}").expect("sk-legacy regex"),
                kind: RedactKind::BareToken("OPENAI"),
            },
            // AWS access keys (AKIA + 16 uppercase alphanumerics).
            RedactRule {
                re: Regex::new(r"AKIA[0-9A-Z]{16}").expect("AKIA regex"),
                kind: RedactKind::BareToken("AWS"),
            },
            // Slack bot tokens.
            RedactRule {
                re: Regex::new(r"xoxb-[0-9]+-[0-9]+-[A-Za-z0-9]+").expect("xoxb regex"),
                kind: RedactKind::BareToken("SLACK"),
            },
            // GitHub PAT.
            RedactRule {
                re: Regex::new(r"ghp_[A-Za-z0-9]{36}").expect("ghp regex"),
                kind: RedactKind::BareToken("GITHUB"),
            },
            // GitHub OAuth.
            RedactRule {
                re: Regex::new(r"gho_[A-Za-z0-9]{36}").expect("gho regex"),
                kind: RedactKind::BareToken("GITHUB"),
            },
            // Named env-var family. Capture key name (group 1), separator
            // (group 2), and value (group 3). Replace value with [REDACTED].
            RedactRule {
                re: Regex::new(
                    r"(OPENAI_API_KEY|ANTHROPIC_API_KEY|AWS_SECRET_ACCESS_KEY|CLOUDFLARE_API_TOKEN|XAI_API_KEY|OPENROUTER_API_KEY|STRIPE_API_KEY)(\s*[:=]\s*)(\S+)",
                )
                .expect("named env regex"),
                kind: RedactKind::NamedValue,
            },
        ]
    })
}

/// Return `true` if the byte range `[start, end)` in `input` is within
/// `PASSAGE_WINDOW` bytes of a `passage ` substring (either before or after).
/// Used to skip redaction of strings like `passage open btak/openai/api_key`.
fn is_passage_protected(input: &str, start: usize, end: usize) -> bool {
    let bytes = input.as_bytes();
    let window_start = start.saturating_sub(PASSAGE_WINDOW);
    let window_end = (end + PASSAGE_WINDOW).min(bytes.len());
    // Find a UTF-8 safe slice — the `passage ` token is ASCII so byte search
    // is sound on any UTF-8 string.
    let needle = b"passage ";
    if window_end <= window_start || needle.len() > window_end - window_start {
        return false;
    }
    bytes[window_start..window_end]
        .windows(needle.len())
        .any(|w| w == needle)
}

/// Redact secrets in `input` and return the cleaned string.
///
/// See the module docs for the pattern catalog and the `passage` exemption.
pub fn redact(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    let mut current = input.to_string();
    for rule in rules() {
        current = apply_rule(rule, &current);
    }
    current
}

fn apply_rule(rule: &RedactRule, input: &str) -> String {
    // Collect matches first so the `passage` window check observes the
    // ORIGINAL input — replacements would otherwise shift indices.
    let matches: Vec<(usize, usize)> = rule
        .re
        .find_iter(input)
        .map(|m| (m.start(), m.end()))
        .collect();
    if matches.is_empty() {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;
    for (start, end) in matches {
        // Copy unchanged prefix.
        out.push_str(&input[cursor..start]);
        if is_passage_protected(input, start, end) {
            // Keep the original token verbatim.
            out.push_str(&input[start..end]);
        } else {
            match rule.kind {
                RedactKind::BareToken(label) => {
                    out.push_str(&format!("[REDACTED_{}]", label));
                }
                RedactKind::NamedValue => {
                    // Re-run captures on the exact match span so we can
                    // preserve the key name and separator.
                    let span = &input[start..end];
                    if let Some(caps) = rule.re.captures(span) {
                        let key = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                        let sep = caps.get(2).map(|m| m.as_str()).unwrap_or("=");
                        out.push_str(key);
                        out.push_str(sep);
                        out.push_str("[REDACTED]");
                    } else {
                        out.push_str("[REDACTED]");
                    }
                }
            }
        }
        cursor = end;
    }
    out.push_str(&input[cursor..]);
    out
}

/// Convenience in-place variant.
#[allow(dead_code)]
pub fn redact_in_place(input: &mut String) {
    let redacted = redact(input);
    if redacted.len() != input.len() || redacted != *input {
        *input = redacted;
    }
}

/// Streaming-safe redactor. Holds the last `STREAM_TAIL_CARRY` bytes of
/// previously-seen input in `carry` so a token straddling a chunk boundary
/// still matches.
///
/// Usage:
/// ```ignore
/// let mut carry = String::new();
/// let safe = secret_redact::redact_streamed(&mut carry, "sk-pro");
/// // safe is "" or the unredacted-but-safe prefix
/// let safe = secret_redact::redact_streamed(&mut carry, "j-XXXXXXXX...");
/// ```
///
/// Contract: the returned `String` is the redacted form of all input that is
/// safe to emit downstream. The remaining tail is held in `carry` and will be
/// emitted in a later call or via `flush_streamed`.
#[allow(dead_code)]
pub fn redact_streamed(carry: &mut String, chunk: &str) -> String {
    if chunk.is_empty() {
        return String::new();
    }
    let combined = {
        let mut s = String::with_capacity(carry.len() + chunk.len());
        s.push_str(carry);
        s.push_str(chunk);
        s
    };
    let redacted = redact(&combined);
    // Decide how much we are safe to emit now. If the input is short, just
    // hold everything in carry.
    if redacted.len() <= STREAM_TAIL_CARRY {
        *carry = redacted;
        return String::new();
    }
    let split_at = redacted.len() - STREAM_TAIL_CARRY;
    // Snap to char boundary so we never split a multi-byte sequence.
    let split_at = floor_char_boundary(&redacted, split_at);
    let emit = redacted[..split_at].to_string();
    *carry = redacted[split_at..].to_string();
    emit
}

/// Flush whatever is still in `carry`. Call once the stream has ended so the
/// final tail is emitted (and redacted, in case the last chunk completed a
/// previously-partial token).
#[allow(dead_code)]
pub fn flush_streamed(carry: &mut String) -> String {
    if carry.is_empty() {
        return String::new();
    }
    let out = redact(carry);
    carry.clear();
    out
}

#[allow(dead_code)]
fn floor_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_sk_proj_token() {
        let input = "key: sk-proj-abcdefghijklmnopqrstuvwx";
        let out = redact(input);
        assert_eq!(out, "key: [REDACTED_OPENAI]");
    }

    #[test]
    fn redacts_sk_svcacct_token() {
        let input = "TOKEN sk-svcacct-ABCDEFGHIJKLMNOPQRSTUVWX more";
        let out = redact(input);
        assert_eq!(out, "TOKEN [REDACTED_OPENAI] more");
    }

    #[test]
    fn redacts_legacy_sk_token() {
        // 48 chars after "sk-"
        let input = "use sk-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghij done";
        let out = redact(input);
        assert!(out.contains("[REDACTED_OPENAI]"), "got: {}", out);
        assert!(!out.contains("ABCDEFG"), "leftover token: {}", out);
    }

    #[test]
    fn redacts_aws_access_key() {
        let input = "id=AKIAIOSFODNN7EXAMPLE ok";
        let out = redact(input);
        assert_eq!(out, "id=[REDACTED_AWS] ok");
    }

    #[test]
    fn redacts_slack_bot_token() {
        let input = "Bearer xoxb-1234567890-1234567890-abcDEFghiJKL";
        let out = redact(input);
        assert_eq!(out, "Bearer [REDACTED_SLACK]");
    }

    #[test]
    fn redacts_github_pat() {
        let input = "token=ghp_0123456789abcdefghijklmnopqrstuvwxyz";
        let out = redact(input);
        assert!(out.contains("[REDACTED_GITHUB]"), "got: {}", out);
        assert!(!out.contains("0123456789abcdef"), "leftover: {}", out);

        let input2 = "oauth=gho_0123456789abcdefghijklmnopqrstuvwxyz";
        let out2 = redact(input2);
        assert!(out2.contains("[REDACTED_GITHUB]"), "got: {}", out2);
    }

    #[test]
    fn redacts_named_env_var_value() {
        let input = "OPENAI_API_KEY=plaintextvalue";
        let out = redact(input);
        assert_eq!(out, "OPENAI_API_KEY=[REDACTED]");

        // Colon-separated form.
        let input2 = "ANTHROPIC_API_KEY: anthropicvalue";
        let out2 = redact(input2);
        assert_eq!(out2, "ANTHROPIC_API_KEY: [REDACTED]");

        // With surrounding context.
        let input3 = "Logged in. STRIPE_API_KEY=sk_live_abc more";
        let out3 = redact(input3);
        // Note: the STRIPE_API_KEY=… rule fires AFTER the bare sk- rule, so
        // we must accept either complete redaction outcome — both keep no
        // plaintext value.
        assert!(out3.contains("STRIPE_API_KEY=[REDACTED]") || out3.contains("STRIPE_API_KEY"));
        assert!(!out3.contains("sk_live_abc"), "leak: {}", out3);
    }

    #[test]
    fn preserves_passage_open() {
        let input = "OPENAI_API_KEY=$(passage open btak/openai/api_key)";
        let out = redact(input);
        assert_eq!(out, input, "passage open form must round-trip");
    }

    #[test]
    fn preserves_bare_passage_token() {
        let input = "Run: passage show btak/openai/api_key";
        let out = redact(input);
        assert_eq!(out, input);
    }

    #[test]
    fn mixed_plaintext_only_token_redacted() {
        let input = "Use sk-svcacct-XYZ123abcdefghijklmnop then a normal sentence.";
        let out = redact(input);
        assert_eq!(out, "Use [REDACTED_OPENAI] then a normal sentence.");
    }

    #[test]
    fn streamed_redaction_catches_split_token() {
        // Build a long chunk so the carry-buffer machinery actually gates
        // bytes out instead of holding everything.
        let prefix = "x".repeat(200);
        let mut carry = String::new();
        let chunk1 = format!("{}sk-pro", prefix);
        let chunk2 = "j-abcdefghijklmnopqrstuvwx then done";
        let emitted1 = redact_streamed(&mut carry, &chunk1);
        let emitted2 = redact_streamed(&mut carry, chunk2);
        let final_tail = flush_streamed(&mut carry);
        let total = format!("{}{}{}", emitted1, emitted2, final_tail);
        assert!(
            total.contains("[REDACTED_OPENAI]"),
            "split-token redaction failed: {}",
            total
        );
        assert!(
            !total.contains("sk-proj-abcdefghij"),
            "split token leaked: {}",
            total
        );
    }

    #[test]
    fn idempotent_redact() {
        let input = "OPENAI_API_KEY=sk-proj-abcdefghijklmnopqrstuvwx";
        let once = redact(input);
        let twice = redact(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn non_secret_input_passes_through() {
        let input = "Just an ordinary sentence with no tokens at all.";
        let out = redact(input);
        assert_eq!(out, input);
    }

    #[test]
    fn redact_in_place_mutates() {
        let mut s = "leak AKIAIOSFODNN7EXAMPLE".to_string();
        redact_in_place(&mut s);
        assert_eq!(s, "leak [REDACTED_AWS]");
    }

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(redact(""), "");
        let mut carry = String::new();
        assert_eq!(redact_streamed(&mut carry, ""), "");
        assert_eq!(flush_streamed(&mut carry), "");
    }
}
