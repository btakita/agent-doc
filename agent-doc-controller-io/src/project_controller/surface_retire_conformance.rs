//! `#surfaceretireformal`: bind `formal/tla/PassiveTmuxSyncRetireNet.tla` to the
//! code it models.
//!
//! The v0.35.502/503 tab-switch wedge was a transition the shipped code took
//! and the model did not contain (an editor retiring its own live endpoint),
//! so a passing model proved nothing about it. Three checks close that gap:
//!
//! * **Action-set conformance.** Every code site that mutates editor-surface
//!   endpoint state is DERIVED from the source: controller graph mutators and
//!   their callers and dispatch commands from `rpc.rs` parsed with `syn`, and
//!   the JetBrains plugin's surface RPCs, their call sites and its liveness
//!   transitions from the Kotlin sources. Each derived site must have a row in
//!   `formal/tla/PassiveTmuxSyncRetireNet.conformance.tsv` naming the model
//!   actions it implements; every row must still exist in the code; every named
//!   action must be a leaf action of the `.tla`; and every leaf action must be
//!   mapped. Adding an unmodeled transition fails `make check`.
//! * **Next-state transcription.** The controller half of the model
//!   (`ControllerRecvObs`, `ControllerRecvRetire`) is transcribed below, pinned
//!   to the `.tla` text, and driven against the production graph over every
//!   short operation sequence of a small domain.
//! * **Trace validation.** The two real wedges (00:45:43Z and 02:54:03Z on
//!   2026-10-15) are anonymized fixtures under
//!   `formal/traces/passive_tmux_sync_retire/`. Replayed through both the
//!   transcription and the production graph, every logged verdict reproduces
//!   and the shipped behaviour reaches the muted state; replayed under the
//!   re-registration rule, every observation lands within one round trip.

use super::{
    ControllerEditorSurfaceGraph, ControllerEditorSurfaceIntentRunner, SurfaceObservationRejection,
};
use agent_doc_editor_surface::{
    EditorSurface, EditorSurfaceObservation, RETIRED_ENDPOINT_MUTE_THRESHOLD, SurfaceColumn,
    reregistration_generation,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use syn::visit::Visit;

const TLA_MODEL: &str = "formal/tla/PassiveTmuxSyncRetireNet.tla";
const CONFORMANCE_TABLE: &str = "formal/tla/PassiveTmuxSyncRetireNet.conformance.tsv";
const RPC_SOURCE: &str = "agent-doc-controller-io/src/project_controller/rpc.rs";
const CP_ROUTE_CLIENT: &str =
    "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CpRouteClient.kt";
const TRACE_DIR: &str = "formal/traces/passive_tmux_sync_retire";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate lives in the workspace root")
        .to_path_buf()
}

fn repo_file(relative: &str) -> String {
    let path = repo_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

// ---------------------------------------------------------------------------
// The model's action names.

/// Strip `\*` line comments and `(* ... *)` block comments from TLA+ source.
fn tla_strip_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("(*") {
            rest = after.find("*)").map_or("", |end| &after[end + 2..]);
        } else if rest.starts_with("\\*") {
            rest = rest.find('\n').map_or("", |end| &rest[end..]);
        } else {
            let ch = rest.chars().next().expect("non-empty");
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    out
}

/// Column-0 operator definitions (`Name ==` / `Name(args) ==`) and their bodies.
fn tla_definitions(source: &str) -> BTreeMap<String, String> {
    let stripped = tla_strip_comments(source);
    let mut definitions = BTreeMap::new();
    let mut current: Option<(String, String)> = None;
    for line in stripped.lines() {
        let starts_definition = !line.starts_with(char::is_whitespace)
            && line.contains("==")
            && line
                .split("==")
                .next()
                .map(str::trim_end)
                .is_some_and(|head| {
                    let name_len = head
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .count();
                    name_len > 0 && (name_len == head.len() || head[name_len..].starts_with('('))
                });
        if starts_definition {
            if let Some((name, body)) = current.take() {
                definitions.insert(name, body);
            }
            let (head, body) = line.split_once("==").expect("checked above");
            let name = head
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect::<String>();
            current = Some((name, format!("{body}\n")));
        } else if line.starts_with(char::is_whitespace)
            && let Some((_, body)) = current.as_mut()
        {
            body.push_str(line);
            body.push('\n');
        } else if let Some((name, body)) = current.take() {
            definitions.insert(name, body);
        }
    }
    if let Some((name, body)) = current {
        definitions.insert(name, body);
    }
    definitions
}

/// Leaf actions: definitions whose body takes a step itself (a primed variable
/// or a `NetChannel` send/deliver/adversary step), as opposed to the
/// disjunctions (`Next`, `Recv`, `EditorRecvReply`) that only name them.
fn tla_leaf_actions(source: &str) -> BTreeSet<String> {
    tla_definitions(source)
        .into_iter()
        .filter(|(_, body)| {
            body.contains("' ")
                || body.contains("'\n")
                || body.contains("C!Send(")
                || body.contains("C!Deliver(")
                || body.contains("C!DeliverAndSend(")
                || body.contains("C!Adversary")
        })
        .map(|(name, _)| name)
        .collect()
}

// ---------------------------------------------------------------------------
// Controller sites, derived from `rpc.rs` with the Rust grammar.

const FENCE_FIELDS: [&str; 3] = ["generation", "sequence", "retired"];
const ROOT_MAP_MUTATORS: [&str; 6] = ["insert", "remove", "entry", "retain", "clear", "drain"];

fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<syn::Ident>()
                .is_ok_and(|ident| ident == "test")
    })
}

fn names_roots(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Path(path) => path.path.is_ident("roots"),
        syn::Expr::Field(field) => {
            matches!(&field.member, syn::Member::Named(name) if name == "roots")
        }
        syn::Expr::MethodCall(call) if call.method == "lock" => names_roots(&call.receiver),
        _ => false,
    }
}

fn is_field_named(expr: &syn::Expr, name: &str) -> bool {
    matches!(expr, syn::Expr::Field(field)
        if matches!(&field.member, syn::Member::Named(member) if member == name))
}

/// What one function body does to editor-surface state.
#[derive(Default)]
struct BodyFacts {
    /// Assigns a root's fence field or inserts/removes a root.
    mutates_fence: bool,
    /// `self.<method>(` calls.
    self_calls: BTreeSet<String>,
    /// `<expr>.editor_surface_graph.<method>(` calls.
    graph_calls: BTreeSet<String>,
    /// `free_fn(` calls by bare name.
    free_calls: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for BodyFacts {
    fn visit_expr_assign(&mut self, expr: &'ast syn::ExprAssign) {
        if let syn::Expr::Field(field) = &*expr.left
            && let syn::Member::Named(name) = &field.member
            && FENCE_FIELDS.iter().any(|fence| name == fence)
        {
            self.mutates_fence = true;
        }
        syn::visit::visit_expr_assign(self, expr);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let method = call.method.to_string();
        if ROOT_MAP_MUTATORS.contains(&method.as_str()) && names_roots(&call.receiver) {
            self.mutates_fence = true;
        }
        if let syn::Expr::Path(path) = &*call.receiver
            && path.path.is_ident("self")
        {
            self.self_calls.insert(method.clone());
        }
        if is_field_named(&call.receiver, "editor_surface_graph") {
            self.graph_calls.insert(method);
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*call.func
            && let Some(ident) = path.path.get_ident()
        {
            self.free_calls.insert(ident.to_string());
        }
        syn::visit::visit_expr_call(self, call);
    }
}

fn body_facts(block: &syn::Block) -> BodyFacts {
    let mut facts = BodyFacts::default();
    facts.visit_block(block);
    facts
}

fn takes_surface_root(sig: &syn::Signature) -> bool {
    sig.inputs.iter().any(|input| {
        matches!(input, syn::FnArg::Typed(typed)
        if matches!(&*typed.ty, syn::Type::Reference(reference)
            if matches!(&*reference.elem, syn::Type::Path(path)
                if path.path.segments.last().is_some_and(|segment| {
                    segment.ident == "ControllerEditorSurfaceRoot"
                }))))
    })
}

fn impl_type_name(item: &syn::ItemImpl) -> Option<String> {
    match &*item.self_ty {
        syn::Type::Path(path) => path.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

/// One production (non-`#[cfg(test)]`) function of `rpc.rs`.
struct RustFn {
    /// `Type::method` or `free_fn`.
    site: String,
    owner: Option<String>,
    name: String,
    surface_root_arg: bool,
    facts: BodyFacts,
}

fn production_fns(file: &syn::File) -> Vec<RustFn> {
    let mut fns = Vec::new();
    collect_production_fns(&file.items, &mut fns);
    fns
}

fn collect_production_fns(items: &[syn::Item], fns: &mut Vec<RustFn>) {
    for item in items {
        match item {
            syn::Item::Mod(module) if !is_cfg_test(&module.attrs) => {
                if let Some((_, items)) = &module.content {
                    collect_production_fns(items, fns);
                }
            }
            syn::Item::Fn(item) if !is_cfg_test(&item.attrs) => fns.push(RustFn {
                site: item.sig.ident.to_string(),
                owner: None,
                name: item.sig.ident.to_string(),
                surface_root_arg: takes_surface_root(&item.sig),
                facts: body_facts(&item.block),
            }),
            syn::Item::Impl(item) if !is_cfg_test(&item.attrs) => {
                let owner = impl_type_name(item);
                for member in &item.items {
                    if let syn::ImplItem::Fn(method) = member
                        && !is_cfg_test(&method.attrs)
                    {
                        let name = method.sig.ident.to_string();
                        fns.push(RustFn {
                            site: format!("{}::{name}", owner.as_deref().unwrap_or("_")),
                            owner: owner.clone(),
                            name,
                            surface_root_arg: takes_surface_root(&method.sig),
                            facts: body_facts(&method.block),
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

/// String-literal match arms whose body calls `handler(`.
#[derive(Default)]
struct CommandArms {
    arms: Vec<(String, BTreeSet<String>)>,
}

impl<'ast> Visit<'ast> for CommandArms {
    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        if let syn::Pat::Lit(pat) = &arm.pat
            && let syn::Lit::Str(command) = &pat.lit
        {
            let mut calls = BodyFacts::default();
            calls.visit_expr(&arm.body);
            self.arms.push((command.value(), calls.free_calls));
        }
        syn::visit::visit_arm(self, arm);
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if !is_cfg_test(&module.attrs) {
            syn::visit::visit_item_mod(self, module);
        }
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if !is_cfg_test(&item.attrs) {
            syn::visit::visit_item_fn(self, item);
        }
    }
}

/// `(kind, site)` for every controller transition of editor-surface endpoint state.
fn derive_controller_sites(rpc_source: &str) -> BTreeSet<(String, String)> {
    let file = syn::parse_file(rpc_source).expect("rpc.rs parses");
    let fns = production_fns(&file);
    let graph = "ControllerEditorSurfaceGraph";

    // Free functions that mutate a root they are handed.
    let mutating_free: BTreeSet<String> = fns
        .iter()
        .filter(|f| f.owner.is_none() && f.surface_root_arg && f.facts.mutates_fence)
        .map(|f| f.name.clone())
        .collect();
    // Graph methods that mutate, directly, through such a function, or through
    // another mutating graph method (fixpoint).
    let mut mutators: BTreeSet<String> = fns
        .iter()
        .filter(|f| f.owner.as_deref() == Some(graph))
        .filter(|f| f.facts.mutates_fence || !f.facts.free_calls.is_disjoint(&mutating_free))
        .map(|f| f.name.clone())
        .collect();
    loop {
        let before = mutators.len();
        for f in fns.iter().filter(|f| f.owner.as_deref() == Some(graph)) {
            if !f.facts.self_calls.is_disjoint(&mutators) {
                mutators.insert(f.name.clone());
            }
        }
        if mutators.len() == before {
            break;
        }
    }
    let callers: BTreeSet<String> = fns
        .iter()
        .filter(|f| f.owner.as_deref() != Some(graph))
        .filter(|f| !f.facts.graph_calls.is_disjoint(&mutators))
        .map(|f| f.site.clone())
        .collect();
    let caller_names: BTreeSet<String> = fns
        .iter()
        .filter(|f| callers.contains(&f.site))
        .map(|f| f.name.clone())
        .collect();
    let mut arms = CommandArms::default();
    arms.visit_file(&file);

    let mut sites = BTreeSet::new();
    for name in &mutating_free {
        sites.insert(("graph_mutator".to_string(), name.clone()));
    }
    for name in &mutators {
        sites.insert(("graph_mutator".to_string(), format!("{graph}::{name}")));
    }
    for site in &callers {
        sites.insert(("graph_caller".to_string(), site.clone()));
    }
    for (command, calls) in arms.arms {
        if !calls.is_disjoint(&caller_names) {
            sites.insert(("controller_command".to_string(), command));
        }
    }
    sites
}

// ---------------------------------------------------------------------------
// Plugin sites, derived from the Kotlin sources.

#[derive(Debug, Clone, PartialEq, Eq)]
enum KtTok {
    Ident(String),
    Punct(char),
    /// `=` that is an assignment or an expression-body marker (not `==`, `!=`,
    /// `<=`, `>=`, `+=`, `->`).
    Assign,
    Other,
}

/// Skip a string literal starting at `i` (just past the opening quote(s)),
/// including `${ ... }` templates that may themselves contain strings.
fn kt_skip_string(chars: &[char], mut i: usize, raw: bool) -> usize {
    while i < chars.len() {
        if raw && chars[i..].starts_with(&['"', '"', '"']) {
            let mut end = i + 3;
            while end < chars.len() && chars[end] == '"' {
                end += 1;
            }
            return end;
        }
        match chars[i] {
            '\\' if !raw => i += 2,
            '"' if !raw => return i + 1,
            '$' if chars.get(i + 1) == Some(&'{') => i = kt_skip_template(chars, i + 2),
            _ => i += 1,
        }
    }
    i
}

/// Skip the code of a string template up to and including its closing `}`.
fn kt_skip_template(chars: &[char], mut i: usize) -> usize {
    let mut depth = 1usize;
    while i < chars.len() {
        match chars[i] {
            '"' if chars[i..].starts_with(&['"', '"', '"']) => {
                i = kt_skip_string(chars, i + 3, true)
            }
            '"' => i = kt_skip_string(chars, i + 1, false),
            '{' => {
                depth += 1;
                i += 1;
            }
            '}' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => i += 1,
        }
    }
    i
}

fn kt_tokens(source: &str) -> Vec<KtTok> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if chars[i..].starts_with(&['/', '/']) {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if chars[i..].starts_with(&['/', '*']) {
            let mut depth = 0usize;
            while i < chars.len() {
                if chars[i..].starts_with(&['/', '*']) {
                    depth += 1;
                    i += 2;
                } else if chars[i..].starts_with(&['*', '/']) {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if chars[i..].starts_with(&['"', '"', '"']) {
            i = kt_skip_string(&chars, i + 3, true);
            tokens.push(KtTok::Other);
        } else if c == '"' {
            i = kt_skip_string(&chars, i + 1, false);
            tokens.push(KtTok::Other);
        } else if c == '\'' {
            i += 1;
            while i < chars.len() && chars[i] != '\'' {
                i += if chars[i] == '\\' { 2 } else { 1 };
            }
            i += 1;
            tokens.push(KtTok::Other);
        } else if c == '`' {
            let start = i + 1;
            i = start;
            while i < chars.len() && chars[i] != '`' {
                i += 1;
            }
            tokens.push(KtTok::Ident(chars[start..i].iter().collect()));
            i += 1;
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            tokens.push(KtTok::Ident(chars[start..i].iter().collect()));
        } else if c.is_ascii_digit() {
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.')
            {
                i += 1;
            }
            tokens.push(KtTok::Other);
        } else if c == '=' {
            let prev = if i > 0 { chars[i - 1] } else { ' ' };
            let next = chars.get(i + 1).copied().unwrap_or(' ');
            if next == '=' {
                tokens.push(KtTok::Other);
                i += 2;
                if chars.get(i) == Some(&'=') {
                    i += 1;
                }
            } else if "!<>+-*/%".contains(prev) {
                tokens.push(KtTok::Other);
                i += 1;
            } else {
                tokens.push(KtTok::Assign);
                i += 1;
            }
        } else if c == '-' && chars.get(i + 1) == Some(&'>') {
            tokens.push(KtTok::Other);
            i += 2;
        } else {
            tokens.push(KtTok::Punct(c));
            i += 1;
        }
    }
    tokens
}

const KT_DECLARATION_STARTS: [&str; 22] = [
    "fun",
    "val",
    "var",
    "class",
    "object",
    "interface",
    "init",
    "companion",
    "private",
    "internal",
    "public",
    "protected",
    "override",
    "data",
    "enum",
    "sealed",
    "abstract",
    "open",
    "const",
    "lateinit",
    "inline",
    "suspend",
];

/// A Kotlin function: name and token span (the `fun` keyword to the end of its body).
#[derive(Debug, Clone)]
struct KtFn {
    name: String,
    start: usize,
    end: usize,
}

fn kt_matching(tokens: &[KtTok], open_index: usize, open: char, close: char) -> usize {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open_index) {
        if *token == KtTok::Punct(open) {
            depth += 1;
        } else if *token == KtTok::Punct(close) {
            depth -= 1;
            if depth == 0 {
                return index;
            }
        }
    }
    tokens.len().saturating_sub(1)
}

fn kt_functions(tokens: &[KtTok]) -> Vec<KtFn> {
    let mut functions = Vec::new();
    for start in 0..tokens.len() {
        if tokens[start] != KtTok::Ident("fun".to_string()) {
            continue;
        }
        // Name: the identifier right before the parameter list (skips
        // generics and an extension receiver).
        let mut index = start + 1;
        let mut angle = 0i32;
        let mut name = None;
        while index < tokens.len() {
            match &tokens[index] {
                KtTok::Punct('<') => angle += 1,
                KtTok::Punct('>') => angle -= 1,
                KtTok::Punct('(') if angle == 0 => break,
                KtTok::Ident(ident) if angle == 0 => name = Some(ident.clone()),
                _ => {}
            }
            index += 1;
        }
        let Some(name) = name else { continue };
        let params_end = kt_matching(tokens, index, '(', ')');
        // Body: the first `{` or `=` after the signature, outside parentheses.
        let mut cursor = params_end + 1;
        let mut paren = 0i32;
        let mut body = None;
        while cursor < tokens.len() {
            match &tokens[cursor] {
                KtTok::Punct('(') => paren += 1,
                KtTok::Punct(')') => paren -= 1,
                KtTok::Punct('{') if paren == 0 => {
                    body = Some((cursor, true));
                    break;
                }
                KtTok::Assign if paren == 0 => {
                    body = Some((cursor, false));
                    break;
                }
                KtTok::Punct('}') if paren == 0 => break,
                KtTok::Ident(ident)
                    if paren == 0 && KT_DECLARATION_STARTS.contains(&ident.as_str()) =>
                {
                    break;
                }
                _ => {}
            }
            cursor += 1;
        }
        let end = match body {
            None => params_end,
            Some((open, true)) => kt_matching(tokens, open, '{', '}'),
            Some((assign, false)) => {
                // An expression body runs to the next declaration at its own
                // nesting level, or to the brace that closes its scope.
                let mut cursor = assign + 1;
                let (mut brace, mut paren) = (0i32, 0i32);
                while cursor < tokens.len() {
                    match &tokens[cursor] {
                        KtTok::Punct('{') => brace += 1,
                        KtTok::Punct('(') => paren += 1,
                        KtTok::Punct(')') => paren -= 1,
                        KtTok::Punct('}') => {
                            if brace == 0 {
                                break;
                            }
                            brace -= 1;
                        }
                        KtTok::Punct('@') if brace == 0 && paren == 0 => break,
                        KtTok::Ident(ident)
                            if brace == 0
                                && paren == 0
                                && KT_DECLARATION_STARTS.contains(&ident.as_str()) =>
                        {
                            break;
                        }
                        _ => {}
                    }
                    cursor += 1;
                }
                cursor.saturating_sub(1)
            }
        };
        functions.push(KtFn { name, start, end });
    }
    functions
}

/// The outermost function enclosing token `index`: the attribution unit (a
/// lambda, local function, or anonymous object inside a member function
/// belongs to that member function).
fn kt_owner(functions: &[KtFn], index: usize) -> Option<&KtFn> {
    functions
        .iter()
        .filter(|f| f.start <= index && index <= f.end)
        .max_by_key(|f| f.end - f.start)
}

/// Unqualified calls (`name(`, not `Receiver.name(`) inside a function.
fn kt_calls(tokens: &[KtTok], function: &KtFn) -> BTreeSet<String> {
    let mut calls = BTreeSet::new();
    for index in (function.start + 2)..=function.end.min(tokens.len().saturating_sub(2)) {
        if let KtTok::Ident(name) = &tokens[index]
            && tokens[index + 1] == KtTok::Punct('(')
            && tokens[index - 1] != KtTok::Punct('.')
        {
            calls.insert(name.clone());
        }
    }
    calls
}

/// The JetBrains plugin's editor-surface RPCs: `CpRouteClient` functions that
/// send an `editor_surface_observe` / `editor_surface_forget` request, directly
/// or through another such function.
fn derive_plugin_rpcs(cp_route_client: &str) -> BTreeSet<String> {
    let tokens = kt_tokens(cp_route_client);
    let functions = kt_functions(&tokens);
    let outermost: Vec<&KtFn> = functions
        .iter()
        .filter(|f| kt_owner(&functions, f.start).is_some_and(|owner| owner.start == f.start))
        .collect();
    let builders = ["editorSurfaceObserveRequest", "editorSurfaceForgetRequest"];
    let mut rpcs: BTreeSet<String> = outermost
        .iter()
        .filter(|f| !builders.contains(&f.name.as_str()))
        .filter(|f| {
            let calls = kt_calls(&tokens, f);
            builders.iter().any(|builder| calls.contains(*builder))
        })
        .map(|f| f.name.clone())
        .collect();
    loop {
        let before = rpcs.len();
        for function in &outermost {
            if !kt_calls(&tokens, function).is_disjoint(&rpcs) {
                rpcs.insert(function.name.clone());
            }
        }
        if rpcs.len() == before {
            break;
        }
    }
    rpcs
}

fn kotlin_main_sources() -> Vec<(String, String)> {
    let mut files = Vec::new();
    let mut stack = vec![
        repo_root().join("editors/jetbrains/src/main"),
        repo_root().join("editors/jetbrains-262"),
    ];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                // Only production sources: skip tests and build output.
                if !matches!(name, "test" | "build" | ".gradle" | ".intellijPlatform") {
                    stack.push(path);
                }
            } else if path.extension().is_some_and(|ext| ext == "kt")
                && path.components().any(|c| c.as_os_str() == "main")
            {
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                files.push((
                    stem,
                    std::fs::read_to_string(&path).expect("read kotlin source"),
                ));
            }
        }
    }
    files.sort();
    files
}

/// Plugin call sites of the surface RPCs and the listener's liveness
/// transitions, attributed to their enclosing member function.
fn derive_plugin_sites(
    rpcs: &BTreeSet<String>,
    sources: &[(String, String)],
) -> BTreeSet<(String, String)> {
    let liveness_calls = ["retireControllerProjection", "resumeControllerProjection"];
    let mut sites = BTreeSet::new();
    for (stem, source) in sources {
        if stem == "CpRouteClient" {
            continue;
        }
        let tokens = kt_tokens(source);
        let functions = kt_functions(&tokens);
        let owner_name = |index: usize| {
            kt_owner(&functions, index).map_or("<top>".to_string(), |owner| owner.name.clone())
        };
        for index in 0..tokens.len().saturating_sub(3) {
            // `CpRouteClient.<rpc>(`
            if tokens[index] == KtTok::Ident("CpRouteClient".to_string())
                && tokens[index + 1] == KtTok::Punct('.')
                && let KtTok::Ident(callee) = &tokens[index + 2]
                && tokens[index + 3] == KtTok::Punct('(')
                && rpcs.contains(callee)
            {
                sites.insert((
                    "plugin_call".to_string(),
                    format!("{stem}.{}->{callee}", owner_name(index)),
                ));
            }
            // `<receiver>.retireControllerProjection(` / `.resumeControllerProjection(`
            if tokens[index] == KtTok::Punct('.')
                && let KtTok::Ident(callee) = &tokens[index + 1]
                && tokens[index + 2] == KtTok::Punct('(')
                && liveness_calls.contains(&callee.as_str())
            {
                sites.insert((
                    "plugin_liveness".to_string(),
                    format!("{stem}.{}->{callee}", owner_name(index)),
                ));
            }
            // `projectionRetired = ...`
            if tokens[index] == KtTok::Ident("projectionRetired".to_string())
                && tokens[index + 1] == KtTok::Assign
                && !matches!(index.checked_sub(1).map(|i| &tokens[i]), Some(KtTok::Ident(kw)) if kw == "var" || kw == "val")
            {
                sites.insert((
                    "plugin_liveness".to_string(),
                    format!("{stem}.{}:projectionRetired", owner_name(index)),
                ));
            }
        }
    }
    sites
}

fn derive_code_sites() -> BTreeSet<(String, String)> {
    let mut sites = derive_controller_sites(&repo_file(RPC_SOURCE));
    let rpcs = derive_plugin_rpcs(&repo_file(CP_ROUTE_CLIENT));
    for rpc in &rpcs {
        sites.insert(("plugin_rpc".to_string(), format!("CpRouteClient.{rpc}")));
    }
    sites.extend(derive_plugin_sites(&rpcs, &kotlin_main_sources()));
    sites
}

// ---------------------------------------------------------------------------
// The mapping table.

#[derive(Debug)]
struct Row {
    kind: String,
    site: String,
    actions: Vec<String>,
}

fn conformance_rows(table: &str) -> Vec<Row> {
    table
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| {
            let columns: Vec<&str> = line.split('\t').collect();
            assert!(
                columns.len() >= 3,
                "a conformance row is `kind<TAB>site<TAB>actions[<TAB>note]`: {line:?}"
            );
            Row {
                kind: columns[0].to_string(),
                site: columns[1].to_string(),
                actions: columns[2].split('|').map(str::to_string).collect(),
            }
        })
        .collect()
}

fn assert_conformance(
    derived: &BTreeSet<(String, String)>,
    rows: &[Row],
    leaf_actions: &BTreeSet<String>,
) -> Result<(), String> {
    let mut problems = Vec::new();
    let mapped: BTreeSet<(String, String)> = rows
        .iter()
        .filter(|row| row.kind != "model_only")
        .map(|row| (row.kind.clone(), row.site.clone()))
        .collect();
    for (kind, site) in derived.difference(&mapped) {
        problems.push(format!(
            "UNMODELED transition: {kind} `{site}` mutates editor-surface endpoint state but has \
             no row in {CONFORMANCE_TABLE}; model it as an action of {TLA_MODEL} and map it"
        ));
    }
    for (kind, site) in mapped.difference(derived) {
        problems.push(format!(
            "STALE row: {kind} `{site}` no longer exists in the code; update {CONFORMANCE_TABLE}"
        ));
    }
    let mut covered = BTreeSet::new();
    for row in rows {
        for action in &row.actions {
            if leaf_actions.contains(action) {
                covered.insert(action.clone());
            } else {
                problems.push(format!(
                    "row {} `{}` names `{action}`, which is not a leaf action of {TLA_MODEL}",
                    row.kind, row.site
                ));
            }
        }
        if row.kind == "model_only" && !row.actions.contains(&row.site) {
            problems.push(format!("model_only row `{}` must name itself", row.site));
        }
    }
    for action in leaf_actions.difference(&covered) {
        problems.push(format!(
            "model action `{action}` is mapped by no code site and no model_only row"
        ));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

#[test]
fn every_editor_surface_transition_in_the_code_is_a_named_action_of_the_model() {
    let derived = derive_code_sites();
    // Non-vacuity: the derivation must see the transitions this test exists
    // for, or a scanner that finds nothing would pass forever.
    for expected in [
        (
            "graph_mutator",
            "ControllerEditorSurfaceGraph::observe_monitored",
        ),
        (
            "graph_mutator",
            "ControllerEditorSurfaceGraph::forget_surface",
        ),
        ("graph_mutator", "retire_editor_surface_root"),
        ("controller_command", "editor_surface_observe"),
        ("controller_command", "editor_surface_forget"),
        ("plugin_rpc", "CpRouteClient.observeEditorSurface"),
        ("plugin_rpc", "CpRouteClient.forgetEditorSurface"),
        (
            "plugin_call",
            "EditorTabSyncListener.retireControllerProjection->forgetEditorSurface",
        ),
        (
            "plugin_liveness",
            "EditorTabSyncListener.resumeControllerProjection:projectionRetired",
        ),
    ] {
        assert!(
            derived.contains(&(expected.0.to_string(), expected.1.to_string())),
            "the site derivation missed {expected:?}; derived: {derived:#?}"
        );
    }
    let rows = conformance_rows(&repo_file(CONFORMANCE_TABLE));
    let leaf_actions = tla_leaf_actions(&repo_file(TLA_MODEL));
    if let Err(problems) = assert_conformance(&derived, &rows, &leaf_actions) {
        panic!("{problems}");
    }
}

#[test]
fn model_leaf_actions_are_the_steps_and_not_their_disjunctions() {
    let actions = tla_leaf_actions(&repo_file(TLA_MODEL));
    for step in [
        "ControllerRecvObs",
        "ControllerRecvRetire",
        "EditorReregister",
        "EditorRetireSuperseded",
        "EditorAck",
        "Handoff",
        "Fallback",
        "AdversaryStep",
    ] {
        assert!(actions.contains(step), "{step} missing from {actions:?}");
    }
    for composite in ["Next", "Recv", "EditorRecvReply", "Spec", "Init", "TypeOK"] {
        assert!(
            !actions.contains(composite),
            "{composite} is not a leaf action"
        );
    }
}

#[test]
fn conformance_check_rejects_an_unmodeled_site_a_stale_row_and_an_unknown_action() {
    let leaf_actions: BTreeSet<String> = ["A", "B"].map(str::to_string).into();
    let derived: BTreeSet<(String, String)> =
        [("graph_mutator".to_string(), "x".to_string())].into();
    let good = vec![
        Row {
            kind: "graph_mutator".into(),
            site: "x".into(),
            actions: vec!["A".into()],
        },
        Row {
            kind: "model_only".into(),
            site: "B".into(),
            actions: vec!["B".into()],
        },
    ];
    assert_eq!(assert_conformance(&derived, &good, &leaf_actions), Ok(()));

    let mut unmodeled = derived.clone();
    unmodeled.insert(("graph_mutator".into(), "resurrect".into()));
    let error = assert_conformance(&unmodeled, &good, &leaf_actions).unwrap_err();
    assert!(
        error.contains("UNMODELED transition: graph_mutator `resurrect`"),
        "{error}"
    );

    let error = assert_conformance(&BTreeSet::new(), &good, &leaf_actions).unwrap_err();
    assert!(error.contains("STALE row: graph_mutator `x`"), "{error}");

    let mut unknown = good;
    unknown[0].actions.push("Teleport".into());
    let error = assert_conformance(&derived, &unknown, &leaf_actions).unwrap_err();
    assert!(error.contains("names `Teleport`"), "{error}");
}

#[test]
fn kotlin_scanner_attributes_calls_to_their_member_function() {
    let source = r#"
        object Client {
            fun observe(a: String, ok: () -> Boolean = { false }): Result =
                send(a, "${a ?: "none"}")
            private fun send(a: String, b: String): Result {
                fun local() = editorSurfaceObserveRequest(a)
                return local()
            }
            fun unrelated(): Int = 1
            fun forget(): Boolean { return editorSurfaceForgetRequest() == x }
        }
        class Listener {
            private var projectionRetired = false
            fun handoff() {
                synchronized(lock) { projectionRetired = true }
                Thread({ CpRouteClient.forget() }).start()
            }
            internal fun live(): Boolean =
                synchronized(lock) { !projectionRetired }
            fun install() {
                bus.subscribe(object : Topic {
                    override fun fired() { listener.resumeControllerProjection() }
                })
                if (projectionRetired == true) CpRouteClient.observe("x")
            }
        }
    "#;
    let rpcs = derive_plugin_rpcs(source);
    assert_eq!(
        rpcs,
        ["forget", "observe", "send"].map(str::to_string).into(),
        "a local function is attributed to its member, and callers are transitive"
    );
    let sites = derive_plugin_sites(&rpcs, &[("Listener".to_string(), source.to_string())]);
    let expected: BTreeSet<(String, String)> = [
        ("plugin_call", "Listener.handoff->forget"),
        ("plugin_call", "Listener.install->observe"),
        ("plugin_liveness", "Listener.handoff:projectionRetired"),
        (
            "plugin_liveness",
            "Listener.install->resumeControllerProjection",
        ),
    ]
    .map(|(kind, site)| (kind.to_string(), site.to_string()))
    .into();
    assert_eq!(
        sites, expected,
        "the declaration and the `==` read are not transitions"
    );
}

// ---------------------------------------------------------------------------
// The controller half of the model's next-state relation, transcribed.

/// `ControllerEditorSurfaceRoot`'s fence as the model names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModelRoot {
    rgen: u64,
    rseq: u64,
    rretired: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelVerdict {
    Accepted,
    /// The model's "ok" receipt for a fence that means nothing was applied.
    Stale,
    /// The typed `retired_endpoint` receipt.
    Retired,
}

#[derive(Debug, Clone, Copy)]
struct ControllerKnobs {
    retired_fence: bool,
    retire_raises_fence: bool,
}

const FIXED_CONTROLLER: ControllerKnobs = ControllerKnobs {
    retired_fence: true,
    retire_raises_fence: true,
};

/// `ControllerRecvObs(g, s)`. An absent root is the first registration (the
/// model's `Init` starts registered; the code and the traces start empty).
fn model_recv_obs(
    root: &mut Option<ModelRoot>,
    knobs: ControllerKnobs,
    g: u64,
    s: u64,
) -> ModelVerdict {
    let Some(current) = root else {
        *root = Some(ModelRoot {
            rgen: g,
            rseq: s,
            rretired: false,
        });
        return ModelVerdict::Accepted;
    };
    if knobs.retired_fence && current.rretired && g <= current.rgen {
        ModelVerdict::Retired
    } else if g < current.rgen || (g == current.rgen && s <= current.rseq) {
        ModelVerdict::Stale
    } else {
        current.rretired = if g > current.rgen {
            false
        } else {
            current.rretired
        };
        current.rgen = g;
        current.rseq = s;
        ModelVerdict::Accepted
    }
}

/// `ControllerRecvRetire(g)`; returns whether the root's fence changed (the
/// code's `forgotten`).
fn model_recv_retire(root: &mut Option<ModelRoot>, knobs: ControllerKnobs, g: u64) -> bool {
    let Some(current) = root else {
        return false;
    };
    let before = *current;
    if knobs.retire_raises_fence {
        if current.rgen <= g {
            current.rgen = g;
            current.rretired = true;
        }
    } else if current.rgen == g {
        current.rretired = true;
    }
    *current != before
}

#[test]
fn transcription_is_pinned_to_the_model_text() {
    let model = repo_file(TLA_MODEL);
    for fragment in [
        "IF RetiredFence /\\ rretired /\\ g <= rgen",
        "ELSE IF g < rgen \\/ (g = rgen /\\ s <= rseq)",
        "/\\ rretired' = IF g > rgen THEN FALSE ELSE rretired",
        "THEN IF rgen <= g",
        "THEN rgen' = g /\\ rretired' = TRUE",
        "ELSE IF rgen = g",
        "ReregisterEnabled(g, v) == ReregisterOnRetired /\\ v = \"retired\" /\\ live /\\ g = cgen",
        "/\\ cgen' = cgen + 1",
    ] {
        assert!(
            model.contains(fragment),
            "{TLA_MODEL} no longer contains `{fragment}`; re-transcribe model_recv_obs / \
             model_recv_retire / the re-registration rule"
        );
    }
}

#[test]
fn transcribed_fence_equals_the_production_classification_on_every_small_input() {
    for retired in [false, true] {
        for rgen in 1..=4 {
            for rseq in 0..=4 {
                for g in 0..=5 {
                    for s in 0..=5 {
                        let mut root = Some(ModelRoot {
                            rgen,
                            rseq,
                            rretired: retired,
                        });
                        let model = model_recv_obs(&mut root, FIXED_CONTROLLER, g, s);
                        let code = match SurfaceObservationRejection::classify(
                            retired, rgen, rseq, g, s,
                        ) {
                            None => ModelVerdict::Accepted,
                            Some(SurfaceObservationRejection::RetiredEndpoint) => {
                                ModelVerdict::Retired
                            }
                            Some(_) => ModelVerdict::Stale,
                        };
                        assert_eq!(
                            model, code,
                            "retired={retired} rgen={rgen} rseq={rseq} g={g} s={s}"
                        );
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// One endpoint driven through the production graph and the transcription.

const TRACE_CLIENT: &str = "jetbrains@IDE.0-pid:1";
const TRACE_SURFACE: &str = "frame-1";

fn observation_surface() -> EditorSurface {
    EditorSurface {
        column_order: Default::default(),
        focused: "/project/a.md".to_string(),
        visible: vec!["/project/a.md".to_string()],
        open: vec!["/project/a.md".to_string()],
        columns: vec![SurfaceColumn {
            files: vec!["/project/a.md".to_string()],
        }],
        force_reconcile: false,
        focus_only: true,
        preserve_focus: false,
    }
}

/// The production graph and the transcription side by side, per client lane.
struct Paired {
    scope: agent_doc_state_scope::ProcessScope,
    graph: ControllerEditorSurfaceGraph,
    model: BTreeMap<String, Option<ModelRoot>>,
    knobs: ControllerKnobs,
    alerts: Vec<u64>,
}

impl Paired {
    fn new() -> Self {
        let runner: ControllerEditorSurfaceIntentRunner = Arc::new(|_, _| Ok("ok".to_string()));
        Self {
            scope: agent_doc_state_scope::ProcessScope::new(),
            graph: ControllerEditorSurfaceGraph::new(runner),
            model: BTreeMap::new(),
            knobs: FIXED_CONTROLLER,
            alerts: Vec::new(),
        }
    }

    /// Observe through both; they must agree. Returns the code's verdict and
    /// its re-registration directive.
    fn observe(&mut self, client: &str, g: u64, s: u64) -> (ModelVerdict, Option<u64>) {
        let (rejection, receipt, muted) = self.graph.observe_monitored(
            &self.scope,
            Path::new("/project"),
            EditorSurfaceObservation {
                client_id: client.to_string(),
                surface_id: TRACE_SURFACE.to_string(),
                generation: g,
                sequence: s,
                surface: observation_surface(),
            },
            None,
        );
        if muted.is_some() {
            self.alerts.push(s);
        }
        let code = match rejection {
            None => ModelVerdict::Accepted,
            Some(SurfaceObservationRejection::RetiredEndpoint) => ModelVerdict::Retired,
            Some(_) => ModelVerdict::Stale,
        };
        let model = model_recv_obs(
            self.model.entry(client.to_string()).or_default(),
            self.knobs,
            g,
            s,
        );
        assert_eq!(code, model, "observe {client} g={g} s={s}: code vs model");
        assert_eq!(
            receipt.reregister_above_generation.is_some(),
            code == ModelVerdict::Retired,
            "exactly the retired receipt carries the directive"
        );
        (code, receipt.reregister_above_generation)
    }

    fn forget(&mut self, client: &str, g: u64) -> bool {
        let (code, _) = self
            .graph
            .forget_surface(Path::new("/project"), client, TRACE_SURFACE, g);
        let model = model_recv_retire(
            self.model.entry(client.to_string()).or_default(),
            self.knobs,
            g,
        );
        assert_eq!(code, model, "forget {client} g={g}: code vs model");
        code
    }

    fn muted(&self, client: &str, plugin_generation: u64) -> bool {
        self.model
            .get(client)
            .copied()
            .flatten()
            .is_some_and(|root| root.rretired && plugin_generation <= root.rgen)
    }
}

#[derive(Debug, Clone, Copy)]
enum Op {
    Observe(u64, u64),
    Forget(u64),
}

#[test]
fn production_graph_follows_the_transcribed_next_state_relation_on_every_short_run() {
    let mut ops = Vec::new();
    for g in 1..=3 {
        for s in 1..=2 {
            ops.push(Op::Observe(g, s));
        }
        ops.push(Op::Forget(g));
    }
    let n = ops.len();
    let mut runs = 0;
    for code in 0..n.pow(4) {
        let mut paired = Paired::new();
        let mut rest = code;
        for _ in 0..4 {
            match ops[rest % n] {
                Op::Observe(g, s) => {
                    paired.observe(TRACE_CLIENT, g, s);
                }
                Op::Forget(g) => {
                    paired.forget(TRACE_CLIENT, g);
                }
            }
            rest /= n;
        }
        runs += 1;
    }
    assert_eq!(runs, 9usize.pow(4));
}

// ---------------------------------------------------------------------------
// Trace validation.

#[derive(Debug, Clone, PartialEq, Eq)]
enum TraceEvent {
    Observed {
        client: String,
        generation: u64,
        sequence: u64,
        accepted: bool,
        rejected: Option<String>,
    },
    Forgotten {
        client: String,
        surface: String,
        generation: u64,
        forgotten: bool,
    },
    ControllerRecycle,
}

fn trace_fields(line: &str) -> BTreeMap<&str, &str> {
    line.split_whitespace()
        .filter_map(|field| field.split_once('='))
        .collect()
}

fn parse_trace(text: &str) -> Vec<TraceEvent> {
    text.lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| {
            let fields = trace_fields(line);
            let number = |key: &str| -> u64 {
                fields
                    .get(key)
                    .unwrap_or_else(|| panic!("{key} missing: {line}"))
                    .parse()
                    .unwrap()
            };
            if line.contains("] controller_editor_surface_observed ") {
                TraceEvent::Observed {
                    client: fields["client"].to_string(),
                    generation: number("generation"),
                    sequence: number("sequence"),
                    accepted: fields["accepted"] == "true",
                    rejected: fields.get("rejected").map(|r| r.to_string()),
                }
            } else if line.contains("] controller_editor_surface_forgotten ") {
                TraceEvent::Forgotten {
                    client: fields["client"].to_string(),
                    surface: fields["surface"].to_string(),
                    generation: number("generation"),
                    forgotten: number("forgotten_clients") > 0,
                }
            } else if line.contains("] controller_self_recycle_started ") {
                TraceEvent::ControllerRecycle
            } else {
                panic!("unexpected trace line: {line}")
            }
        })
        .collect()
}

fn trace(name: &str) -> Vec<TraceEvent> {
    let events = parse_trace(&repo_file(&format!("{TRACE_DIR}/{name}")));
    assert!(!events.is_empty(), "{name} is empty");
    events
}

/// Replay the trace as logged (the shipped plugin: it ignored the receipt).
/// Every logged verdict must reproduce in both the code and the model.
/// Returns, per controller lifetime, the sequences rejected while muted.
fn replay_shipped(events: &[TraceEvent]) -> (Paired, Vec<Vec<u64>>, Vec<u64>) {
    let mut paired = Paired::new();
    let mut muted_runs = vec![Vec::new()];
    let mut alerts = Vec::new();
    let mut plugin_generation = BTreeMap::<String, u64>::new();
    for event in events {
        match event {
            TraceEvent::Observed {
                client,
                generation,
                sequence,
                accepted,
                rejected,
            } => {
                plugin_generation.insert(client.clone(), *generation);
                let (verdict, _) = paired.observe(client, *generation, *sequence);
                assert_eq!(
                    verdict == ModelVerdict::Accepted,
                    *accepted,
                    "logged accepted={accepted} for {client} g={generation} s={sequence}"
                );
                if let Some(rejected) = rejected {
                    assert_eq!(
                        rejected.as_str(),
                        match verdict {
                            ModelVerdict::Retired => "retired_endpoint",
                            _ => "",
                        }
                    );
                }
                if verdict == ModelVerdict::Retired {
                    assert!(paired.muted(client, *generation));
                    muted_runs.last_mut().unwrap().push(*sequence);
                }
            }
            TraceEvent::Forgotten {
                client,
                surface,
                generation,
                forgotten,
            } => {
                assert_eq!(surface, TRACE_SURFACE);
                assert_eq!(
                    paired.forget(client, *generation),
                    *forgotten,
                    "logged forgotten={forgotten} for {client} g={generation}"
                );
            }
            TraceEvent::ControllerRecycle => {
                alerts.append(&mut paired.alerts);
                paired = Paired::new();
                muted_runs.push(Vec::new());
            }
        }
    }
    alerts.append(&mut paired.alerts);
    (paired, muted_runs, alerts)
}

/// Replay the same publication stream under the re-registration rule
/// (`EditorReregister`): a typed retired receipt moves the plugin above the
/// retired generation and re-sends the observation once. The self-retirement
/// forgets stay in (the `SelfHeal` config): recovery must not depend on the
/// bug being absent.
fn replay_reregistering(events: &[TraceEvent]) -> (usize, Vec<u64>) {
    let mut paired = Paired::new();
    let mut alerts = Vec::new();
    // Logged generation -> the generation this plugin process publishes now.
    let mut publishing = BTreeMap::<(String, u64), u64>::new();
    let mut reregistrations = 0;
    for event in events {
        match event {
            TraceEvent::Observed {
                client,
                generation,
                sequence,
                ..
            } => {
                let current = *publishing
                    .entry((client.clone(), *generation))
                    .or_insert(*generation);
                let (verdict, above) = paired.observe(client, current, *sequence);
                if verdict == ModelVerdict::Retired {
                    let above = above.expect("a retired receipt carries the directive");
                    let next = reregistration_generation(current, above);
                    assert!(
                        next > above,
                        "re-registration lands strictly above the fence"
                    );
                    publishing.insert((client.clone(), *generation), next);
                    reregistrations += 1;
                    let (again, _) = paired.observe(client, next, *sequence);
                    assert_eq!(
                        again,
                        ModelVerdict::Accepted,
                        "one round trip: the re-sent observation lands (g={next} s={sequence})"
                    );
                } else {
                    assert_ne!(
                        verdict,
                        ModelVerdict::Stale,
                        "no sequence is replayed twice"
                    );
                }
            }
            TraceEvent::Forgotten {
                client, generation, ..
            } => {
                let current = publishing
                    .get(&(client.clone(), *generation))
                    .copied()
                    .unwrap_or(*generation);
                paired.forget(client, current);
            }
            TraceEvent::ControllerRecycle => {
                alerts.append(&mut paired.alerts);
                paired = Paired::new();
            }
        }
    }
    alerts.append(&mut paired.alerts);
    (reregistrations, alerts)
}

#[test]
fn trace_004543z_reproduces_the_wedge_and_recovers_under_reregistration() {
    let events = trace("wedge-2026-10-15T004543Z.log");
    let (_, muted_runs, alerts) = replay_shipped(&events);
    // Lifetimes: before the 00:39:31Z recycle, the wedged one, and after the
    // 01:23:14Z recycle that cleared it.
    assert_eq!(muted_runs.len(), 3, "{muted_runs:?}");
    assert_eq!(
        muted_runs[1],
        vec![7, 8, 9, 11, 15, 23, 39, 71],
        "every logged tab switch after the self-retirement was fenced"
    );
    assert!(muted_runs[2].is_empty(), "the recycle dropped the fence");
    assert_eq!(
        alerts,
        vec![9],
        "the runtime monitor fires once, on the {RETIRED_ENDPOINT_MUTE_THRESHOLD}rd logged rejection"
    );

    let (reregistrations, alerts) = replay_reregistering(&events);
    assert_eq!(reregistrations, 1, "one re-registration ends the mute");
    assert!(
        alerts.is_empty(),
        "a recovering endpoint never trips the monitor"
    );
}

#[test]
fn trace_025403z_reproduces_the_wedge_and_recovers_under_reregistration() {
    let events = trace("wedge-2026-10-15T025403Z.log");
    let (paired, muted_runs, alerts) = replay_shipped(&events);
    assert_eq!(
        muted_runs.last().unwrap(),
        &vec![28, 29, 30, 32, 36, 44, 60, 92],
        "{muted_runs:?}"
    );
    assert!(
        paired.muted(TRACE_CLIENT, 200),
        "the extract ends muted: retired at the plugin's live generation"
    );
    assert_eq!(alerts, vec![30]);

    let (reregistrations, alerts) = replay_reregistering(&events);
    assert_eq!(reregistrations, 1);
    assert!(alerts.is_empty());
}

#[test]
fn trace_fixtures_are_anonymized() {
    for name in [
        "wedge-2026-10-15T004543Z.log",
        "wedge-2026-10-15T025403Z.log",
    ] {
        let text = repo_file(&format!("{TRACE_DIR}/{name}"));
        for event in parse_trace(&text) {
            match event {
                TraceEvent::Observed {
                    client, generation, ..
                }
                | TraceEvent::Forgotten {
                    client, generation, ..
                } => {
                    assert!(
                        client.ends_with("@IDE.0-pid:1"),
                        "{name}: client id not anonymized: {client}"
                    );
                    assert!(
                        generation < 1000,
                        "{name}: raw nanoTime generation {generation}"
                    );
                }
                TraceEvent::ControllerRecycle => {}
            }
        }
        assert!(
            !text.contains("doc="),
            "{name}: document names must be stripped"
        );
        assert!(!text.contains("/home/"), "{name}: paths must be stripped");
    }
}
