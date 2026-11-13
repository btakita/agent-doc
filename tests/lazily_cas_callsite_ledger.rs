//! `#implementlazilyowned`: the Lazily CAS-effects migration ledger is exhaustive.
//!
//! `tasks/agent-doc/lazily-cas-callsite-ledger.tsv` must hold exactly one row for
//! every compare-and-swap / fence / lease / retry / supersession callsite in the
//! production Rust sources, as found by the audit below. A new callsite fails
//! this test until it is classified; a row whose callsite disappeared fails
//! until it is removed or marked `deleted`. Every `absent:` deletion proof is
//! re-verified against the current source, so a reintroduced bespoke loop fails.
//!
//! Audit definition (keep `tasks/agent-doc/plan-lazily-cas-effects.md` in sync):
//! - every `*.rs` file in this repository except `target/`, `tests/`,
//!   `benches/`, `examples/`, `fuzz/`, hidden directories, `editors/`,
//!   `python/`, `agent-doc-test-support/`, and `agent-doc-fuzz-harness/`;
//! - items under `#[cfg(test)]` / `#[test]` are excluded;
//! - a `fn`/`struct`/`enum`/`trait`/`type`/`const`/`static` definition whose
//!   identifier has a token in [`TOKENS`], a `supersed*` token, one of the
//!   token pairs in [`TOKEN_PAIRS`], or is named in [`PLAN_NAMED`];
//! - every `compare_exchange(` call and every `"update-ref"` argument.
//!
//! Regenerate missing rows with
//! `AGENT_DOC_UPDATE_LAZILY_CAS_LEDGER=1 cargo test --test lazily_cas_callsite_ledger`;
//! new rows get `triage` status with owning-crate defaults that must be reviewed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const LEDGER: &str = "tasks/agent-doc/lazily-cas-callsite-ledger.tsv";
const HEADER: &str = "path\tkind\tsymbol\toccurrence\tauthority_backend\tlazily_adapter\tparity_test\tstatus\tdeletion_proof";

const TOKENS: &[&str] = &[
    "attempts",
    "backoff",
    "cas",
    "fence",
    "fenced",
    "fences",
    "fencing",
    "lease",
    "leased",
    "leases",
    "retried",
    "retries",
    "retry",
    "retryable",
    "retrying",
];
const TOKEN_PAIRS: &[(&str, &str)] = &[
    ("compare", "and"),
    ("expected", "authority"),
    ("update", "ref"),
];
/// Symbols the migration plan names explicitly although no token matches.
const PLAN_NAMED: &[&str] = &["QueueMaintenanceEffectFamily"];
const CALL_MARKERS: &[(&str, &str)] = &[
    ("compare_exchange(", "compare_exchange"),
    ("\"update-ref\"", "update-ref"),
];
const SKIP_DIRS: &[&str] = &[
    "target",
    "tests",
    "benches",
    "examples",
    "fuzz",
    "node_modules",
    "editors",
    "python",
    "agent-doc-test-support",
    "agent-doc-fuzz-harness",
];
const STATUSES: &[&str] = &[
    // Orchestration now runs through `lazily::EffectCasCore`.
    "migrated",
    // Narrow backend adapter (`lazily::CompareAndApply`) or its atomic primitive.
    "lazily-adapter",
    // A matching seam whose bespoke orchestration is still live.
    "pending-migration",
    // Out of scope per the plan's "Do not migrate" list.
    "excluded",
    // Inventoried with owning-crate defaults; semantics not yet reviewed.
    "triage",
    // Bespoke orchestration removed; the callsite must be absent.
    "deleted",
];

type Key = (String, String, String, usize);

fn identifier_tokens(name: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for part in name.split('_') {
        let chars: Vec<char> = part.chars().collect();
        let mut current = String::new();
        for (index, &ch) in chars.iter().enumerate() {
            let boundary = !current.is_empty()
                && ((ch.is_ascii_uppercase()
                    && (current
                        .chars()
                        .last()
                        .is_some_and(|prev| !prev.is_ascii_uppercase())
                        || chars
                            .get(index + 1)
                            .is_some_and(|next| next.is_ascii_lowercase())))
                    || (ch.is_ascii_digit()
                        != current
                            .chars()
                            .last()
                            .is_some_and(|prev| prev.is_ascii_digit())));
            if boundary {
                tokens.push(current.to_ascii_lowercase());
                current.clear();
            }
            current.push(ch);
        }
        if !current.is_empty() {
            tokens.push(current.to_ascii_lowercase());
        }
    }
    tokens
}

fn audited_identifier(name: &str) -> bool {
    if PLAN_NAMED.contains(&name) {
        return true;
    }
    let tokens = identifier_tokens(name);
    tokens
        .iter()
        .any(|token| TOKENS.contains(&token.as_str()) || token.starts_with("supersed"))
        || tokens
            .windows(2)
            .any(|pair| TOKEN_PAIRS.contains(&(pair[0].as_str(), pair[1].as_str())))
}

/// `(kind, identifier)` of an item definition line.
fn definition(line: &str) -> Option<(&'static str, String)> {
    let mut rest = line.trim_start();
    if let Some(after) = rest.strip_prefix("pub") {
        rest = after.trim_start();
        if rest.starts_with('(') {
            let close = rest.find(')')?;
            rest = rest[close + 1..].trim_start();
        }
    }
    for qualifier in ["async ", "unsafe ", "extern \"C\" "] {
        if let Some(after) = rest.strip_prefix(qualifier) {
            rest = after.trim_start();
        }
    }
    if rest.starts_with("const fn ") || rest.starts_with("const unsafe fn ") {
        rest = rest["const ".len()..].trim_start();
        if let Some(after) = rest.strip_prefix("unsafe ") {
            rest = after.trim_start();
        }
    }
    for kind in ["fn", "struct", "enum", "trait", "type", "const", "static"] {
        let Some(after) = rest.strip_prefix(kind) else {
            continue;
        };
        if !after.starts_with(char::is_whitespace) {
            continue;
        }
        let ident: String = after
            .trim_start()
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .collect();
        let first = ident.chars().next()?;
        if first.is_ascii_digit() {
            return None;
        }
        return Some((kind, ident));
    }
    None
}

/// Brace delta of a line with comments, string literals, and char literals blanked.
fn brace_delta(line: &str) -> i64 {
    let chars: Vec<char> = line.chars().collect();
    let mut delta = 0;
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '/' if chars.get(index + 1) == Some(&'/') => break,
            '"' => {
                index += 1;
                while index < chars.len() && chars[index] != '"' {
                    if chars[index] == '\\' {
                        index += 1;
                    }
                    index += 1;
                }
            }
            '\'' if chars.get(index + 2) == Some(&'\'') => index += 2,
            '\'' if chars.get(index + 1) == Some(&'\\') && chars.get(index + 3) == Some(&'\'') => {
                index += 3
            }
            '{' => delta += 1,
            '}' => delta -= 1,
            _ => {}
        }
        index += 1;
    }
    delta
}

fn scan_file(rel: &str, source: &str, rows: &mut Vec<(String, String, String)>) {
    let mut depth: i64 = 0;
    // Depth at which a skipped `#[cfg(test)]` / `#[test]` block closes.
    let mut skip_until: Option<i64> = None;
    // Inside the header of a test-only item, before its `{` or `;`.
    let mut pending_test = false;
    for line in source.lines() {
        let trimmed = line.trim();
        let delta = brace_delta(line);
        let starts_block = delta > 0 || (delta == 0 && line.contains('{') && line.contains('}'));
        if skip_until.is_none() {
            if trimmed.starts_with("#[cfg(test)]")
                || trimmed.starts_with("#[test]")
                || trimmed.starts_with("#[cfg(all(test")
            {
                pending_test = true;
            } else if pending_test {
                if !trimmed.starts_with('#') && !trimmed.starts_with("//") {
                    if delta > 0 {
                        skip_until = Some(depth);
                        pending_test = false;
                    } else if starts_block || trimmed.ends_with(';') {
                        pending_test = false;
                    }
                }
            } else {
                if let Some((kind, ident)) = definition(line)
                    && audited_identifier(&ident)
                {
                    rows.push((rel.to_string(), kind.to_string(), ident));
                }
                if !trimmed.starts_with("//") {
                    for (marker, label) in CALL_MARKERS {
                        if line.contains(marker) {
                            rows.push((rel.to_string(), "call".to_string(), label.to_string()));
                        }
                    }
                }
            }
        }
        depth += delta;
        if skip_until.is_some_and(|until| depth <= until) {
            skip_until = None;
        }
    }
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if path.is_dir() {
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            walk(&path, files);
        } else if name.ends_with(".rs") {
            files.push(path);
        }
    }
}

fn audited_callsites(root: &Path) -> Vec<Key> {
    let mut files = Vec::new();
    walk(root, &mut files);
    let mut rows = Vec::new();
    for file in files {
        let rel = file
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let source = String::from_utf8_lossy(&std::fs::read(&file).unwrap()).to_string();
        scan_file(&rel, &source, &mut rows);
    }
    let mut occurrences: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    rows.into_iter()
        .map(|(path, kind, symbol)| {
            let occurrence = occurrences
                .entry((path.clone(), kind.clone(), symbol.clone()))
                .or_default();
            *occurrence += 1;
            (path, kind, symbol, *occurrence)
        })
        .collect()
}

/// Owning-crate defaults for a new `triage` row: (authority backend, adapter).
fn triage_defaults(path: &str) -> (&'static str, &'static str) {
    let krate = path.split('/').next().unwrap_or(path);
    let crate_name = if krate == "src" { "agent-doc" } else { krate };
    let has = |needles: &[&str]| needles.iter().any(|needle| crate_name.contains(needle));
    if has(&["git", "commit", "compact", "response-replay"]) {
        ("git-ref", "GitUpdateRefAdapter (planned)")
    } else if has(&[
        "document",
        "crdt",
        "sync",
        "write",
        "op-capture",
        "editor",
        "merge",
    ]) {
        (
            "document-editor-authority",
            "DocumentAuthorityAdapter (planned)",
        )
    } else if has(&["queue", "preflight", "element"]) {
        ("queue-maintenance", "QueueMaintenance EffectPlan (planned)")
    } else if has(&["route", "turn-executor", "start", "tmux"]) {
        (
            "tmux-layout-projection",
            "LayoutProjectionAdapter (planned)",
        )
    } else if has(&[
        "controller",
        "supervisor",
        "sqlite",
        "cycle-state",
        "session",
        "turn",
        "answer-ledger",
        "state-backbone",
        "process-owner",
        "lease",
        "repair",
        "closeout",
    ]) {
        (
            "controller-ledger-sqlite",
            "ControllerLedgerTxnAdapter (planned)",
        )
    } else if has(&["ipc", "codex", "hooks", "agent-doc"]) {
        ("process-ipc-transport", "none (transport, review)")
    } else {
        ("local-pure", "none (no external authority, review)")
    }
}

struct Row {
    fields: Vec<String>,
}

impl Row {
    fn key(&self) -> Key {
        (
            self.fields[0].clone(),
            self.fields[1].clone(),
            self.fields[2].clone(),
            self.fields[3].parse().expect("occurrence is a number"),
        )
    }
}

fn parse_ledger(text: &str) -> Vec<Row> {
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some(HEADER), "{LEDGER}: header changed");
    lines
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let fields: Vec<String> = line.split('\t').map(str::to_string).collect();
            assert_eq!(fields.len(), 9, "{LEDGER}: malformed row {line:?}");
            for (index, field) in fields.iter().enumerate() {
                assert!(
                    !field.trim().is_empty(),
                    "{LEDGER}: empty column {index} in {line:?}"
                );
            }
            Row { fields }
        })
        .collect()
}

#[test]
fn lazily_cas_callsite_ledger_is_exhaustive_and_deletion_proofs_hold() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ledger_path = root.join(LEDGER);
    let observed = audited_callsites(root);
    let observed_set: BTreeSet<Key> = observed.iter().cloned().collect();
    let text = std::fs::read_to_string(&ledger_path).unwrap_or_else(|_| format!("{HEADER}\n"));
    let rows = parse_ledger(&text);

    if std::env::var_os("AGENT_DOC_UPDATE_LAZILY_CAS_LEDGER").is_some() {
        let known: BTreeMap<Key, &Row> = rows.iter().map(|row| (row.key(), row)).collect();
        let mut out = format!("{HEADER}\n");
        for key in &observed {
            match known.get(key) {
                Some(row) => out.push_str(&row.fields.join("\t")),
                None => {
                    let (backend, adapter) = triage_defaults(&key.0);
                    out.push_str(&format!(
                        "{}\t{}\t{}\t{}\t{backend}\t{adapter}\tpending\ttriage\tnone",
                        key.0, key.1, key.2, key.3
                    ));
                }
            }
            out.push('\n');
        }
        for row in rows.iter().filter(|row| row.fields[7] == "deleted") {
            if !observed_set.contains(&row.key()) {
                out.push_str(&row.fields.join("\t"));
                out.push('\n');
            }
        }
        std::fs::write(&ledger_path, out).unwrap();
    }

    let text = std::fs::read_to_string(&ledger_path).unwrap();
    let rows = parse_ledger(&text);
    let mut seen = BTreeSet::new();
    let mut failures = Vec::new();
    for row in &rows {
        let key = row.key();
        if !seen.insert(key.clone()) {
            failures.push(format!("duplicate ledger row {key:?}"));
        }
        let status = row.fields[7].as_str();
        if !STATUSES.contains(&status) {
            failures.push(format!("unknown status {status:?} for {key:?}"));
        }
        let present = observed_set.contains(&key);
        if status == "deleted" && present {
            failures.push(format!("{key:?} is marked deleted but still present"));
        }
        if status != "deleted" && !present {
            failures.push(format!(
                "{key:?} is no longer an audited callsite; remove the row or mark it deleted"
            ));
        }
        if matches!(status, "migrated" | "deleted") && row.fields[6] == "pending" {
            failures.push(format!("{key:?} is {status} without a parity test"));
        }
        let proof = row.fields[8].as_str();
        if matches!(status, "migrated" | "deleted") && proof == "none" {
            failures.push(format!("{key:?} is {status} without a deletion proof"));
        }
        for clause in proof.split(';').map(str::trim) {
            if let Some(pattern) = clause.strip_prefix("absent:") {
                let source = std::fs::read_to_string(root.join(&key.0)).unwrap_or_default();
                if source.contains(pattern) {
                    failures.push(format!(
                        "{key:?}: deletion proof violated, {:?} reappeared in {}",
                        pattern, key.0
                    ));
                }
            }
        }
    }
    for key in &observed {
        if !seen.contains(key) {
            let (backend, adapter) = triage_defaults(&key.0);
            failures.push(format!(
                "unledgered callsite, add: {}\t{}\t{}\t{}\t{backend}\t{adapter}\tpending\ttriage\tnone",
                key.0, key.1, key.2, key.3
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{LEDGER} is out of date ({} problems):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn audit_tokenizer_matches_snake_and_camel_tokens_but_not_substrings() {
    assert!(audited_identifier("BackgroundQueueMaintenanceLease"));
    assert!(audited_identifier("fence_queue_maintenance_effect"));
    assert!(audited_identifier("QUEUE_MAINTENANCE_CAS_ATTEMPTS"));
    assert!(audited_identifier("QueueMaintenanceAuthoritySuperseded"));
    assert!(audited_identifier("expected_authority_hash"));
    assert!(audited_identifier("compare_and_swap"));
    assert!(audited_identifier("QueueMaintenanceEffectFamily"));
    assert!(!audited_identifier("release_lock"));
    assert!(!audited_identifier("PluginRelease"));
    assert!(!audited_identifier("cascade"));
    assert!(!audited_identifier("case_label"));
    assert_eq!(
        identifier_tokens("HTTPRetryCASFence2"),
        vec!["http", "retry", "cas", "fence", "2"]
    );
}

#[test]
fn audit_skips_test_items_but_not_production_items() {
    let source = concat!(
        "fn production_fence() {}\n",
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    fn test_only_fence() { let x = \"{\"; }\n",
        "}\n",
        "#[test]\n",
        "fn lone_test_fence(\n",
        "    arg: u8,\n",
        ") {\n",
        "    fn nested_fence() {}\n",
        "}\n",
        "#[cfg(test)]\n",
        "const TEST_RETRY: u8 = 1;\n",
        "const fn const_fence() {}\n",
        "pub(crate) const RETRY_BACKOFF: u64 = 1;\n",
        "fn call() { flag.compare_exchange(false, true); }\n",
    );
    let mut rows = Vec::new();
    scan_file("x.rs", source, &mut rows);
    let symbols: Vec<&str> = rows.iter().map(|row| row.2.as_str()).collect();
    assert_eq!(
        symbols,
        vec![
            "production_fence",
            "const_fence",
            "RETRY_BACKOFF",
            "compare_exchange"
        ]
    );
}
