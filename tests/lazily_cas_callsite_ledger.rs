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
//!   `benches/`, `examples/`, `fuzz/`, `tmp/` (gitignored scratch), hidden
//!   directories, `editors/`, `python/`, `agent-doc-test-support/`, and
//!   `agent-doc-fuzz-harness/`;
//! - every production editor-plugin source under `editors/` (`*.rs`, `*.kt`,
//!   `*.java`, `*.ts`; test sources, `*.d.ts`, and build output excluded),
//!   whose Kotlin/Java/TypeScript definitions are matched by keyword (see
//!   [`editor_definition`]) plus `.compareAndSet(` / `.compareAndExchange(` /
//!   `Atomics.compareExchange(` calls;
//! - items under `#[cfg(test)]` / `#[test]` are excluded;
//! - a `fn`/`struct`/`enum`/`trait`/`type`/`const`/`static` definition whose
//!   identifier has a token in [`TOKENS`], a `supersed*` token, one of the
//!   token pairs in [`TOKEN_PAIRS`], or is named in [`PLAN_NAMED`];
//! - every `compare_exchange(` call and every `"update-ref"` argument.
//!
//! Regenerate missing rows with
//! `AGENT_DOC_UPDATE_LAZILY_CAS_LEDGER=1 cargo test --test lazily_cas_callsite_ledger`;
//! new rows get `triage` status with owning-crate defaults, and a `triage` row
//! fails the audit until it is reviewed into another status.

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
    "tmp",
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
    // Only the regeneration mode writes it, and the audit fails on it.
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
                    } else if starts_block || trimmed.ends_with(';') || delta < 0 {
                        // A `}` closing the enclosing block ends a test-only
                        // struct or struct-literal field (`#[cfg(test)] f: v,`),
                        // which has no `{` or `;` of its own. Without this the
                        // pending attribute swallowed the next item's body.
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

/// Editor-plugin call markers (`#implementlazilyowned` final audit): the JVM
/// and JS compare-and-set primitives are the editor-side analog of
/// `compare_exchange(`.
const EDITOR_CALL_MARKERS: &[(&str, &str)] = &[
    (".compareAndSet(", "compareAndSet"),
    (".compareAndExchange(", "compareAndExchange"),
    ("Atomics.compareExchange(", "Atomics.compareExchange"),
];

/// Modifiers stripped before an editor-plugin definition keyword.
const EDITOR_MODIFIERS: &[&str] = &[
    "public",
    "private",
    "protected",
    "internal",
    "override",
    "open",
    "abstract",
    "final",
    "suspend",
    "inline",
    "data",
    "sealed",
    "lateinit",
    "companion",
    "inner",
    "value",
    "tailrec",
    "operator",
    "infix",
    "external",
    "static",
    "export",
    "default",
    "async",
    "declare",
    "readonly",
    "synchronized",
    "volatile",
    "transient",
];

/// Definition keywords of the editor-plugin languages (Kotlin, Java, TypeScript).
const EDITOR_KINDS: &[&str] = &[
    "fun",
    "class",
    "object",
    "interface",
    "val",
    "var",
    "typealias",
    "function",
    "type",
    "const",
    "let",
    "enum",
];

/// `(kind, identifier)` of a Kotlin/Java/TypeScript definition line. Java
/// methods carry no keyword and are not matched; their fields and classes are.
fn editor_definition(line: &str) -> Option<(&'static str, String)> {
    let mut rest = line.trim_start();
    if rest.starts_with("//") || rest.starts_with('*') || rest.starts_with("/*") {
        return None;
    }
    while rest.starts_with('@') {
        // Skip an annotation (`@Volatile`, `@JvmStatic`, `@Suppress("x")`).
        let end = rest
            .find(|ch: char| ch.is_whitespace())
            .unwrap_or(rest.len());
        rest = rest[end..].trim_start();
    }
    loop {
        let mut stripped = false;
        // Kotlin `const val` / `enum class`; a bare TypeScript `const`/`enum`
        // is the definition keyword itself.
        for prefix in ["const val ", "enum class "] {
            if rest.starts_with(prefix) {
                rest = rest[prefix.find(' ').unwrap()..].trim_start();
                stripped = true;
            }
        }
        for modifier in EDITOR_MODIFIERS {
            if let Some(after) = rest.strip_prefix(modifier)
                && after.starts_with(char::is_whitespace)
            {
                rest = after.trim_start();
                stripped = true;
            }
        }
        if !stripped {
            break;
        }
    }
    for kind in EDITOR_KINDS {
        let Some(after) = rest.strip_prefix(kind) else {
            continue;
        };
        if !after.starts_with(char::is_whitespace) {
            continue;
        }
        let mut after = after.trim_start();
        if *kind == "fun" && after.starts_with('<') {
            // Generic function: `fun <T> name(`.
            let close = after.find('>')?;
            after = after[close + 1..].trim_start();
        }
        let mut ident: String = after
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '.')
            .collect();
        if *kind == "fun" {
            // Extension function: `fun Receiver.name(` names `name`.
            if let Some((_, name)) = ident.rsplit_once('.') {
                ident = name.to_string();
            }
        } else if ident.contains('.') {
            return None;
        }
        let first = ident.chars().next()?;
        if first.is_ascii_digit() {
            return None;
        }
        return Some((kind, ident));
    }
    None
}

/// Production editor-plugin sources: Rust, Kotlin, Java, and TypeScript under
/// `editors/`, excluding test sources and build output.
fn editor_source(rel: &str) -> bool {
    let production = !rel.contains("/src/test/")
        && !rel.contains("/test/")
        && !rel.contains("/tests/")
        && !rel.contains("/__tests__/")
        && !rel.ends_with(".test.ts")
        && !rel.ends_with(".spec.ts")
        && !rel.ends_with(".d.ts");
    production
        && [".rs", ".kt", ".java", ".ts"]
            .iter()
            .any(|extension| rel.ends_with(extension))
}

/// Declaration kinds whose brace opens a type body (members, not locals).
const EDITOR_TYPE_KINDS: &[&str] = &["class", "object", "interface", "enum"];
/// Kinds that declare a local when they appear inside a function body.
const EDITOR_BINDING_KINDS: &[&str] = &["val", "var", "const", "let"];

fn scan_editor_file(rel: &str, source: &str, rows: &mut Vec<(String, String, String)>) {
    if rel.ends_with(".rs") {
        scan_file(rel, source, rows);
        return;
    }
    // One entry per open brace: whether it opened a type body. A binding is an
    // audited member only at top level or directly inside a type body; inside
    // a function or lambda it is a local, like a Rust `let`.
    let mut blocks: Vec<bool> = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with("/*") {
            continue;
        }
        let definition = editor_definition(line);
        let in_code = blocks.last().is_some_and(|is_type| !is_type);
        if let Some((kind, ident)) = &definition
            && !(in_code && EDITOR_BINDING_KINDS.contains(kind))
            && audited_identifier(ident)
        {
            rows.push((rel.to_string(), kind.to_string(), ident.clone()));
        }
        let opens_type = definition
            .as_ref()
            .is_some_and(|(kind, _)| EDITOR_TYPE_KINDS.contains(kind))
            || trimmed.starts_with("companion object");
        let delta = brace_delta(line);
        if delta > 0 {
            for _ in 0..delta {
                blocks.push(opens_type);
            }
        } else {
            for _ in 0..(-delta) {
                blocks.pop();
            }
        }
        for (marker, label) in EDITOR_CALL_MARKERS {
            if line.contains(marker) {
                rows.push((rel.to_string(), "call".to_string(), label.to_string()));
            }
        }
    }
}

const EDITOR_SKIP_DIRS: &[&str] = &[
    "target",
    "node_modules",
    "build",
    "out",
    "dist",
    "bin",
    "gradle",
    "test",
    "tests",
];

fn walk_editors(dir: &Path, root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.map(|entry| entry.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if path.is_dir() {
            if name.starts_with('.') || EDITOR_SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            walk_editors(&path, root, files);
        } else {
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if editor_source(&rel) {
                files.push(path);
            }
        }
    }
}

fn audited_callsites(root: &Path) -> Vec<Key> {
    let mut files = Vec::new();
    walk(root, &mut files);
    let mut editor_files = Vec::new();
    walk_editors(&root.join("editors"), root, &mut editor_files);
    let mut rows = Vec::new();
    for file in editor_files {
        let rel = file
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let source = String::from_utf8_lossy(&std::fs::read(&file).unwrap()).to_string();
        scan_editor_file(&rel, &source, &mut rows);
    }
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
    if path.starts_with("editors/") {
        return ("editor-plugin", "none (editor plugin, review)");
    }
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
        if status == "triage" {
            failures.push(format!(
                "{key:?} is unreviewed triage; classify it (see the plan's statuses)"
            ));
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
        "fn new() -> Self {\n",
        "    Self {\n",
        "        #[cfg(test)]\n",
        "        publisher_for_test: None,\n",
        "    }\n",
        "}\n",
        "fn arm_retry(\n",
        "    target: u8,\n",
        ") {\n",
        "}\n",
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
            "compare_exchange",
            "arm_retry"
        ]
    );
}

#[test]
fn audit_matches_editor_plugin_definitions_and_compare_and_set_calls() {
    let kotlin = concat!(
        "class PatchRetryScheduler {\n",
        "    private val retryBackoffMs = 250L\n",
        "    @Volatile private var leaseHolder: String? = null\n",
        "    private fun <T> withRetry(block: () -> T): T = block()\n",
        "    fun Editor.fenceStaleWrite() {}\n",
        "    // fun commentedRetry() {}\n",
        "    fun release() { if (flag.compareAndSet(false, true)) {} }\n",
        "    const val MAX_ATTEMPTS = 3\n",
        "}\n",
    );
    let mut rows = Vec::new();
    scan_editor_file("editors/jetbrains/src/main/kotlin/X.kt", kotlin, &mut rows);
    let found: Vec<(&str, &str)> = rows
        .iter()
        .map(|row| (row.1.as_str(), row.2.as_str()))
        .collect();
    assert_eq!(
        found,
        vec![
            ("class", "PatchRetryScheduler"),
            ("val", "retryBackoffMs"),
            ("var", "leaseHolder"),
            ("fun", "withRetry"),
            ("fun", "fenceStaleWrite"),
            ("call", "compareAndSet"),
            ("val", "MAX_ATTEMPTS"),
        ]
    );
    let typescript = concat!(
        "export async function sendWithRetry() {}\n",
        "export const LEASE_TTL_MS = 5000;\n",
        "export interface FenceToken {}\n",
        "function render() {}\n",
    );
    let mut rows = Vec::new();
    scan_editor_file("editors/vscode/src/x.ts", typescript, &mut rows);
    let symbols: Vec<&str> = rows.iter().map(|row| row.2.as_str()).collect();
    assert_eq!(symbols, vec!["sendWithRetry", "LEASE_TTL_MS", "FenceToken"]);
    assert!(editor_source("editors/vscode/src/x.ts"));
    assert!(!editor_source("editors/vscode/src/test/x.test.ts"));
    assert!(!editor_source("editors/jetbrains/src/test/kotlin/X.kt"));
    assert!(!editor_source("editors/vscode/src/x.d.ts"));
}
