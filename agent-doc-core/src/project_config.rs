//! # Module: project_config
//!
//! Project-level configuration loaded from `.agent-doc/config.toml`.
//! Shared between binary and library for consistent project config handling.
//!
//! ## Spec
//! - Defines `ProjectConfig`: per-project settings (tmux_session, components).
//! - Defines `ComponentConfig`: per-component patch configuration (mode, timestamps, hooks).
//! - `load_project()` reads and parses the project config file. On absence, I/O error, or parse
//!   error, returns `ProjectConfig::default()` and emits a warning to stderr (never panics).
//! - `project_tmux_session()` is a convenience wrapper returning the configured tmux session name.
//! - `save_project()` serialises `ProjectConfig` to TOML and writes it to
//!   `.agent-doc/config.toml`, creating the directory if needed.
//!
//! ## Agentic Contracts
//! - Never panics on missing config: `load_project()` returns defaults when the file is absent.
//! - Project config errors are non-fatal: errors are surfaced as stderr warnings, not propagated.
//! - Atomic-safe directory creation: `save_project()` calls `create_dir_all` before writing.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GuardConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_capture: Option<crate::frontmatter::PendingCaptureGuardMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_done: Option<crate::frontmatter::PendingCaptureGuardMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_done: Option<crate::frontmatter::PendingCaptureGuardMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_done: Option<bool>,
}

/// Workspace-level lint configuration (`[lint]` section in
/// `.agent-doc/config.toml`).
///
/// Currently exposes a single key, `dialect`, controlling the
/// `tagpath lint --dialect agent-doc` finalize gate. See
/// `crate::frontmatter::LintDialectMode` for the resolved semantics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LintConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dialect: Option<crate::frontmatter::LintDialectMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SshProfileConfig {
    /// Resolved SSH targets for a named profile.
    #[serde(default)]
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SshDocConfig {
    /// Optional profile name to resolve for this document path.
    #[serde(default)]
    pub profile: Option<String>,
    /// Optional direct targets for this document path.
    #[serde(default)]
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SshConfig {
    /// Named SSH profiles that expand to concrete targets.
    #[serde(default)]
    pub profiles: BTreeMap<String, SshProfileConfig>,
    /// Relative document paths that require SSH metadata resolution.
    #[serde(default)]
    pub docs: BTreeMap<String, SshDocConfig>,
}

/// Component patch configuration (mode, timestamps, max entries, hooks).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComponentConfig {
    /// Patch mode: "replace" (default), "append", "prepend".
    /// `patch` is the primary key; `mode` is a backward-compatible alias.
    #[serde(default = "default_patch_mode", alias = "mode")]
    pub patch: String,
    /// Merge strategy: "append-friendly" (default) or "strict".
    /// "append-friendly" auto-resolves conflicts where both sides only appended.
    /// "strict" preserves all conflict markers for manual resolution.
    /// Currently parsed for config validation; merge runs at document level.
    #[serde(default = "default_merge_strategy")]
    #[allow(dead_code)]
    pub merge_strategy: String,
    /// Auto-prefix entries with ISO timestamp (for append/prepend modes)
    #[serde(default)]
    pub timestamp: bool,
    /// Auto-trim old entries in append/prepend modes (0 = unlimited)
    #[serde(default)]
    pub max_entries: usize,
    /// Trim component content to the last N lines after patching (0 = unlimited).
    /// Currently used by template.rs post-patch processing.
    #[serde(default)]
    #[allow(dead_code)]
    pub max_lines: usize,
    /// Shell command to run before patching (stdin: content, stdout: transformed)
    #[serde(default)]
    pub pre_patch: Option<String>,
    /// Shell command to run after patching (fire-and-forget)
    #[serde(default)]
    pub post_patch: Option<String>,
}

fn default_patch_mode() -> String {
    "replace".to_string()
}

fn default_merge_strategy() -> String {
    "append-friendly".to_string()
}

/// Opt-in document gating (`[documents]` section in `.agent-doc/config.toml`).
///
/// Controls which `.md` files agent-doc may auto-convert into a session. By
/// default a plain `.md` is **not** an agent-doc document — it must opt in via
/// frontmatter (an `agent_doc_*` field), a matching `include` glob here, or the
/// `auto_session_for_all_md` escape hatch. See
/// [`is_agent_doc_document`].
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DocumentsConfig {
    /// Project-relative globs whose matching `.md` files are treated as
    /// agent-doc session documents (e.g. `["tasks/**/*.md", "plan.md"]`).
    #[serde(default)]
    pub include: Vec<String>,
    /// Legacy escape hatch restoring the old "every `.md` is a session"
    /// behavior. Default `false`.
    #[serde(default)]
    pub auto_session_for_all_md: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VisionConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub agents: BTreeMap<String, AgentVisionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentVisionConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl VisionConfig {
    pub fn agent_config(&self, agent_name: &str) -> Option<&AgentVisionConfig> {
        self.agents.get(agent_name)
    }

    pub fn effective_provider(&self, agent_name: Option<&str>) -> Option<&str> {
        if let Some(name) = agent_name
            && let Some(ac) = self.agents.get(name)
            && let Some(ref p) = ac.provider
        {
            return Some(p.as_str());
        }
        self.provider.as_deref()
    }

    pub fn effective_model(&self, agent_name: Option<&str>) -> Option<&str> {
        if let Some(name) = agent_name
            && let Some(ac) = self.agents.get(name)
            && let Some(ref m) = ac.model
        {
            return Some(m.as_str());
        }
        self.model.as_deref()
    }

    pub fn effective_api_key(&self, agent_name: Option<&str>) -> Option<&str> {
        if let Some(name) = agent_name
            && let Some(ac) = self.agents.get(name)
            && let Some(ref k) = ac.api_key
        {
            return Some(k.as_str());
        }
        self.api_key.as_deref()
    }

    pub fn agent_mode(&self, agent_name: &str) -> Option<&str> {
        self.agents
            .get(agent_name)
            .and_then(|ac| ac.mode.as_deref())
    }
}

/// Project-level configuration, read from `.agent-doc/config.toml` relative to CWD.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Target tmux session name for this project.
    #[serde(default)]
    pub tmux_session: Option<String>,
    /// Explicit opt-in for automatic compaction/reload policies.
    /// Session-accretion heuristics never compact by themselves; omit to disable.
    #[serde(default, alias = "auto_compact")]
    pub agent_doc_auto_compact: Option<usize>,
    /// Explicit opt-in for the supervisor idle-queue watch to pre-emptively
    /// interleave a context-clear (`/clear`) before an active queue head when
    /// session accretion is unhealthy (`#nm1x-no-preempt-clear`). Off by default
    /// so a manual `Run Agent Doc` / auto-loop drain never fires a pre-emptive
    /// `/clear` that churns the session or is rejected mid-turn.
    #[serde(default, alias = "queue_context_reset")]
    pub agent_doc_queue_context_reset: Option<bool>,
    /// Guard behavior overrides (for example pending-capture enforcement).
    #[serde(default)]
    pub guards: GuardConfig,
    /// Lint behavior overrides for the agent-doc finalize lint gate.
    #[serde(default)]
    pub lint: LintConfig,
    /// Project-local SSH requirement mappings for known ops documents.
    #[serde(default)]
    pub ssh: SshConfig,
    /// Component-specific configuration (patch modes, timestamps, max_entries, hooks).
    #[serde(default)]
    pub components: BTreeMap<String, ComponentConfig>,
    /// Opt-in gating for which `.md` files become agent-doc session documents.
    #[serde(default)]
    pub documents: DocumentsConfig,
    /// Vision (describe-image) provider configuration.
    #[serde(default)]
    pub vision: VisionConfig,
    /// `#stash-session-ttl-prune`: idle TTL in seconds for stash-window agent-doc
    /// panes. When set (> 0), resync logs which panes *would* be reaped. Set
    /// `stash_session_ttl_prune_enabled = true` to enable destructive kill.
    /// Default 0 (disabled).
    #[serde(default)]
    pub stash_session_ttl_secs: u64,
    /// `#stash-session-ttl-prune`: enable destructive kill of idle stash panes
    /// that exceed `stash_session_ttl_secs`. Report-only when false.
    /// Default false.
    #[serde(default)]
    pub stash_session_ttl_prune_enabled: bool,
}

/// Parse a TOML string into a [`ProjectConfig`]. Pure — no fs I/O.
///
/// File-based loading (with legacy `components.toml` migration) lives in
/// `crate::project_config_io` in the main `agent-doc` crate.
pub fn parse_project_toml(content: &str) -> Result<ProjectConfig> {
    toml::from_str(content).map_err(anyhow::Error::from)
}

/// Parse a legacy `components.toml` body (flat `[name]` tables of
/// [`ComponentConfig`] fields) into a map. Used by the file-based migrator
/// in `crate::project_config_io`.
pub fn parse_legacy_components_toml(content: &str) -> Result<BTreeMap<String, ComponentConfig>> {
    toml::from_str(content).map_err(anyhow::Error::from)
}

/// Opt-in predicate: is this `.md` an agent-doc session document?
///
/// Pure — takes the project-relative path, the document content (for
/// frontmatter inspection), and the resolved project config. Returns `true`
/// when ANY of the following hold (in cheapest-first order):
///
/// 1. `documents.auto_session_for_all_md = true` (legacy escape hatch).
/// 2. Frontmatter already carries any agent-doc-managed field (see
///    [`crate::frontmatter::Frontmatter::has_agent_doc_marker`]) — existing
///    sessions and harness/model/preset config stay sessions.
/// 3. The project-relative path matches a `documents.include` glob.
///
/// A plain `.md` with no agent-doc frontmatter, not matching any configured
/// glob, with the escape hatch off, returns `false` — callers must fail closed
/// instead of silently converting it.
pub fn is_agent_doc_document(rel_path: &str, content: &str, config: &ProjectConfig) -> bool {
    if config.documents.auto_session_for_all_md {
        return true;
    }
    if let Ok((fm, _)) = crate::frontmatter::parse(content)
        && fm.has_agent_doc_marker()
    {
        return true;
    }
    let norm = rel_path.trim_start_matches("./");
    config
        .documents
        .include
        .iter()
        .any(|pattern| glob_match(pattern.trim_start_matches("./"), norm))
}

/// Minimal path-glob matcher supporting `*` (any run of non-`/` chars), `?`
/// (one non-`/` char), and `**` (spans `/` boundaries, including zero
/// segments). Both inputs are `/`-separated, project-relative paths. Pure, no
/// I/O — avoids pulling in a glob crate for the small set of patterns
/// `[documents] include` needs.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pat: Vec<&str> = pattern.split('/').collect();
    let seg: Vec<&str> = path.split('/').collect();
    glob_match_segments(&pat, &seg)
}

fn glob_match_segments(pat: &[&str], seg: &[&str]) -> bool {
    match pat.first() {
        None => seg.is_empty(),
        Some(&"**") => {
            // `**` matches zero or more whole path segments.
            if pat.len() == 1 {
                return true;
            }
            // Try consuming 0..=seg.len() leading segments.
            (0..=seg.len()).any(|skip| glob_match_segments(&pat[1..], &seg[skip..]))
        }
        Some(first) => match seg.first() {
            Some(s) if glob_match_segment(first, s) => glob_match_segments(&pat[1..], &seg[1..]),
            _ => false,
        },
    }
}

/// Match a single path segment against a pattern segment containing `*`/`?`
/// (neither of which may match a `/`, which never appears inside a segment).
fn glob_match_segment(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_pi, mut star_ti): (Option<usize>, usize) = (None, 0);
    while ti < txt.len() {
        if pi < pat.len() && (pat[pi] == '?' || pat[pi] == txt[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == '*' {
            star_pi = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < pat.len() && pat[pi] == '*' {
        pi += 1;
    }
    pi == pat.len()
}

#[cfg(test)]
mod documents_gate_tests {
    use super::*;

    fn cfg(include: &[&str], all_md: bool) -> ProjectConfig {
        ProjectConfig {
            documents: DocumentsConfig {
                include: include.iter().map(|s| s.to_string()).collect(),
                auto_session_for_all_md: all_md,
            },
            ..Default::default()
        }
    }

    #[test]
    fn glob_match_literal_and_star() {
        assert!(glob_match("plan.md", "plan.md"));
        assert!(!glob_match("plan.md", "other.md"));
        assert!(glob_match("*.md", "notes.md"));
        assert!(!glob_match("*.md", "notes.txt"));
        // `*` does not cross a path separator.
        assert!(!glob_match("*.md", "tasks/notes.md"));
    }

    #[test]
    fn glob_match_doublestar_spans_dirs() {
        assert!(glob_match("tasks/**/*.md", "tasks/agent-doc/bugs.md"));
        assert!(glob_match("tasks/**/*.md", "tasks/a/b/c/deep.md"));
        // `**` matches zero segments too.
        assert!(glob_match("tasks/**/*.md", "tasks/flat.md"));
        assert!(!glob_match("tasks/**/*.md", "other/flat.md"));
        assert!(glob_match("**/*.md", "anything/here.md"));
        assert!(glob_match("**", "anything/at/all.md"));
    }

    #[test]
    fn glob_match_question_mark() {
        assert!(glob_match("plan?.md", "plan1.md"));
        assert!(!glob_match("plan?.md", "plan.md"));
        assert!(!glob_match("plan?.md", "plan12.md"));
    }

    #[test]
    fn plain_md_is_not_a_session_by_default() {
        let c = cfg(&[], false);
        assert!(!is_agent_doc_document("notes.md", "# Just notes\n", &c));
        assert!(!is_agent_doc_document(
            "tasks/foo.md",
            "---\ntitle: foo\n---\nbody\n",
            &c
        ));
    }

    #[test]
    fn frontmatter_agent_doc_field_opts_in() {
        let c = cfg(&[], false);
        assert!(is_agent_doc_document(
            "notes.md",
            "---\nagent_doc_format: template\n---\nbody\n",
            &c
        ));
        assert!(is_agent_doc_document(
            "notes.md",
            "---\nagent_doc_session: 123\n---\nbody\n",
            &c
        ));
        assert!(is_agent_doc_document(
            "notes.md",
            "---\nsession: legacy-id\n---\nbody\n",
            &c
        ));
        assert!(is_agent_doc_document(
            "notes.md",
            "---\nagent_doc_write: crdt\n---\nbody\n",
            &c
        ));
    }

    #[test]
    fn include_glob_opts_in() {
        let c = cfg(&["tasks/**/*.md", "plan.md"], false);
        assert!(is_agent_doc_document("tasks/agent-doc/x.md", "plain\n", &c));
        assert!(is_agent_doc_document("plan.md", "plain\n", &c));
        assert!(is_agent_doc_document("./plan.md", "plain\n", &c));
        assert!(!is_agent_doc_document("README.md", "plain\n", &c));
    }

    #[test]
    fn escape_hatch_opts_in_everything() {
        let c = cfg(&[], true);
        assert!(is_agent_doc_document("README.md", "plain notes\n", &c));
        assert!(is_agent_doc_document("anywhere/file.md", "plain\n", &c));
    }
}
