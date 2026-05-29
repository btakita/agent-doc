//! # Module: model_tier
//!
//! ## Spec
//! - Defines `Tier`: a harness-agnostic complexity bucket (`Auto`, `Low`, `Med`, `High`)
//!   used to classify task complexity and gate model selection.
//! - `Tier` derives `PartialOrd` such that `Auto < Low < Med < High`. Gating is a simple
//!   `>` comparison: a task whose effective tier exceeds the running model's tier should
//!   prompt the user to switch.
//! - Defines `ModelConfig` (under `[model]` in the global TOML config) and `TierMap` (per-harness
//!   tier→model name resolution under `[model.tiers.<harness>]`).
//! - `detect_harness()` reads environment variables (`CLAUDE_CODE_SESSION`, `CLAUDE_CODE`,
//!   `CLAUDECODE`, `CODEX_SESSION`, `CODEX_THREAD_ID`, `CODEX_CLI`, `OPENCODE_CLIENT`, `OPENCODE`)
//!   to identify the active agent harness, falling back to `"default"`.
//! - `resolve_tier_to_model(tier, harness, config)` maps a `Tier` to the concrete model name
//!   configured for the given harness, falling back to built-in defaults for the
//!   `claude-code`, `codex`, `opencode`, and `default` harnesses.
//! - `tier_from_model_name(name, harness, config)` is the reverse lookup: given a concrete
//!   model name (e.g., `"opus"`), find the tier it belongs to in the harness mapping.
//! - `Tier::FromStr` accepts case-insensitive `auto | low | med | high`.
//!
//! ## Agentic Contracts
//! - **Total ordering**: `Tier` implements `PartialOrd` deterministically; gating logic
//!   is a single comparison and can be safely executed by any model tier.
//! - **Auto is the lowest**: `Tier::Auto` represents "no preference" and compares less than
//!   `Low`. The `effective_tier` composition treats `Auto` as "fall through to next source."
//! - **Built-in defaults**: when no `[model.tiers.<harness>]` section is present, the
//!   resolver falls back to compiled-in maps for known harnesses. This means a fresh
//!   install needs zero config for the common case.
//! - **Reverse lookup is partial**: `tier_from_model_name` returns `None` if the model
//!   name doesn't appear in any tier slot for the harness. Callers should treat `None`
//!   as "unknown — leave tier as Auto."
//!
//! ## Evals
//! - `tier_ordering`: `Auto < Low < Med < High` holds for `<`, `>`, `<=`, `>=`.
//! - `tier_from_str_case_insensitive`: `"LOW"`, `"low"`, `"Low"` all parse to `Tier::Low`.
//! - `tier_from_str_invalid`: unknown strings return `Err`.
//! - `harness_detection_default`: with no env vars set, `detect_harness()` returns `"default"`.
//! - `resolve_builtin_claude_code`: `resolve_tier_to_model(Tier::High, "claude-code", &Config::default())`
//!   returns `Some("claude-opus-4-8")`.
//! - `resolve_unknown_harness_uses_default`: an unknown harness falls through to the
//!   `"default"` built-in map.
//! - `tier_from_model_name_roundtrip`: `tier_from_model_name("opus", "claude-code", ...)`
//!   returns `Some(Tier::High)`.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::str::FromStr;

const CLAUDE_CODE_OPUS_MODEL: &str = "claude-opus-4-8";

/// Harness-agnostic model complexity tier.
///
/// Ordering: `Auto < Low < Med < High`. Gating logic uses a simple `>` comparison —
/// a task whose effective tier exceeds the running model's tier should prompt the
/// user to switch models.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// No preference; fall through to next source in the precedence chain.
    #[default]
    Auto,
    /// Cheap, fast model — small content additions, simple questions.
    Low,
    /// Default working model — multi-section edits, planning, moderate diffs.
    Med,
    /// Powerful model — complex debugging, architecture decisions, large code changes.
    High,
}

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::Low => write!(f, "low"),
            Self::Med => write!(f, "med"),
            Self::High => write!(f, "high"),
        }
    }
}

impl FromStr for Tier {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "low" => Ok(Self::Low),
            "med" | "medium" => Ok(Self::Med),
            "high" => Ok(Self::High),
            other => Err(anyhow!(
                "invalid tier `{}`: expected one of auto|low|med|high",
                other
            )),
        }
    }
}

/// Per-harness tier → concrete model name map.
///
/// Configured under `[model.tiers.<harness>]` in the global config.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TierMap {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub low: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub med: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high: Option<String>,
}

impl TierMap {
    pub fn get(&self, tier: Tier) -> Option<&str> {
        match tier {
            Tier::Auto => None,
            Tier::Low => self.low.as_deref(),
            Tier::Med => self.med.as_deref(),
            Tier::High => self.high.as_deref(),
        }
    }

    /// Reverse lookup: find which tier a concrete model name belongs to.
    pub fn tier_of(&self, model_name: &str) -> Option<Tier> {
        if self.low.as_deref() == Some(model_name) {
            Some(Tier::Low)
        } else if self.med.as_deref() == Some(model_name) {
            Some(Tier::Med)
        } else if self.high.as_deref() == Some(model_name) {
            Some(Tier::High)
        } else {
            None
        }
    }
}

/// Global `[model]` config section.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Whether automatic tier-based recommendations are enabled (default: true).
    #[serde(default = "default_auto")]
    pub auto: bool,
    /// Per-harness tier → model name maps. Key is the harness name
    /// (e.g., `claude-code`, `codex`, `default`).
    #[serde(default)]
    pub tiers: BTreeMap<String, TierMap>,
}

fn default_auto() -> bool {
    true
}

/// Built-in tier map for the `claude-code` harness.
fn builtin_claude_code() -> TierMap {
    TierMap {
        low: Some("haiku".to_string()),
        med: Some("sonnet".to_string()),
        high: Some(CLAUDE_CODE_OPUS_MODEL.to_string()),
    }
}

/// Built-in tier map for the `codex` harness.
fn builtin_codex() -> TierMap {
    TierMap {
        low: Some("gpt-4o-mini".to_string()),
        med: Some("gpt-4o".to_string()),
        high: Some("o3".to_string()),
    }
}

/// Built-in fallback tier map.
fn builtin_default() -> TierMap {
    TierMap {
        low: Some("haiku".to_string()),
        med: Some("sonnet".to_string()),
        high: Some("opus".to_string()),
    }
}

/// Return the built-in tier map for a known harness, or `default` for unknowns.
fn builtin_for(harness: &str) -> TierMap {
    match harness {
        "claude-code" => builtin_claude_code(),
        "codex" => builtin_codex(),
        _ => builtin_default(),
    }
}

pub fn detect_harness() -> String {
    if ["CLAUDE_CODE_SESSION", "CLAUDE_CODE", "CLAUDECODE"]
        .iter()
        .any(|key| std::env::var_os(key).is_some())
    {
        "claude-code".to_string()
    } else if ["CODEX_SESSION", "CODEX_THREAD_ID", "CODEX_CLI", "CODEX"]
        .iter()
        .any(|key| std::env::var_os(key).is_some())
    {
        "codex".to_string()
    } else if ["OPENCODE_CLIENT", "OPENCODE"]
        .iter()
        .any(|key| std::env::var_os(key).is_some())
    {
        "opencode".to_string()
    } else {
        "default".to_string()
    }
}

pub fn harness_key_for_agent_name(agent_name: &str) -> String {
    let normalized = agent_name
        .trim()
        .to_ascii_lowercase()
        .replace(['_', ' '], "-");
    match normalized.as_str() {
        "claude" | "claude-code" | "claudecode" | "claude-code-cli" => "claude-code".to_string(),
        "codex" | "codex-cli" | "openai-codex" => "codex".to_string(),
        "opencode" | "open-code" | "opencode-ai" => "opencode".to_string(),
        "" => "default".to_string(),
        other => other.to_string(),
    }
}

/// Resolve a `Tier` to a concrete model name for the given harness.
///
/// Tries the user's `[model.tiers.<harness>]` config first, then falls back to the
/// built-in map for the harness. Returns `None` for `Tier::Auto`.
pub fn resolve_tier_to_model(
    tier: Tier,
    harness: &str,
    model_config: &ModelConfig,
) -> Option<String> {
    if matches!(tier, Tier::Auto) {
        return None;
    }
    if let Some(map) = model_config.tiers.get(harness)
        && let Some(name) = map.get(tier)
    {
        return Some(name.to_string());
    }
    builtin_for(harness).get(tier).map(|s| s.to_string())
}

fn claude_code_model_alias(model_name: &str) -> Option<&'static str> {
    match model_name.trim() {
        "opus" => Some(CLAUDE_CODE_OPUS_MODEL),
        _ => None,
    }
}

/// Returns `true` when `model_name` lacks a provider prefix required by the
/// opencode harness. OpenCode expects `provider/model` syntax (e.g.
/// `zai-coding-plan/glm-5.1`); bare names like `glm-5.1` are ambiguous.
pub fn is_bare_model_name(model_name: &str) -> bool {
    let trimmed = model_name.trim();
    !trimmed.contains('/') || trimmed.starts_with('/')
}

/// Resolve harness-owned model aliases to the concrete model id agent-doc should
/// launch and stamp in attribution. The Claude Code `opus` alias is intentionally
/// versioned here so `claude_model: opus`, `/model opus`, and high-tier fallback
/// all follow the same current Claude Code definition.
///
/// For the opencode harness, bare model names (without a `provider/` prefix)
/// are rejected with a warning to stderr and returned unchanged so the caller
/// can still proceed — but probe builders and callers should check
/// `is_bare_model_name` before relying on the result for dispatch.
pub fn canonical_model_name(
    model_name: &str,
    harness: &str,
    _model_config: &ModelConfig,
) -> String {
    if harness == "claude-code"
        && let Some(canonical) = claude_code_model_alias(model_name)
    {
        return canonical.to_string();
    }
    if harness == "opencode" && is_bare_model_name(model_name) {
        eprintln!(
            "[model_tier] WARNING: opencode model name {:?} lacks a provider prefix \
             (expected \"provider/model\", e.g. \"zai-coding-plan/glm-5.1\"). \
             Dispatch may fail or use a wrong provider.",
            model_name.trim()
        );
    }
    model_name.to_string()
}

/// Reverse lookup: given a concrete model name, find its tier in the harness's mapping.
///
/// Tries the user's config first, then falls back to the built-in map. Returns `None`
/// if the model name doesn't appear in any tier slot for the harness.
pub fn tier_from_model_name(
    model_name: &str,
    harness: &str,
    model_config: &ModelConfig,
) -> Option<Tier> {
    if let Some(map) = model_config.tiers.get(harness)
        && let Some(t) = map.tier_of(model_name)
    {
        return Some(t);
    }
    builtin_for(harness).tier_of(model_name).or_else(|| {
        if harness == "claude-code" && claude_code_model_alias(model_name).is_some() {
            Some(Tier::High)
        } else {
            None
        }
    })
}

/// Extract the value inside a `<!-- agent:model -->...<!-- /agent:model -->` component.
///
/// Returns the trimmed inner content if the component is present, `None` otherwise.
/// This uses the existing component parser, so guards against fenced code blocks
/// and inline code apply automatically.
pub fn extract_model_component(content: &str) -> Option<String> {
    let comps = crate::component::parse(content).ok()?;
    let comp = comps.into_iter().find(|c| c.name == "model")?;
    let inner = &content[comp.open_end..comp.close_start];
    let trimmed = inner.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Resolve a `<!-- agent:model -->` component value to a `Tier`.
///
/// Accepts tier names (`auto|low|med|high`) or concrete model names (resolved
/// via the harness's tier map). Returns `None` if the value is unrecognized.
pub fn component_value_to_tier(
    value: &str,
    harness: &str,
    model_config: &ModelConfig,
) -> Option<Tier> {
    if let Ok(tier) = Tier::from_str(value) {
        return Some(tier);
    }
    tier_from_model_name(value, harness, model_config)
}

/// Compute a `suggested_tier` from structural diff signals.
///
/// This is the deterministic, harness-agnostic heuristic used when no explicit
/// tier source (inline command, component, frontmatter) is present.
///
/// Inputs:
/// - `diff_type`: classification string from `diff::classify_diff` (e.g., `"simple_question"`)
/// - `lines_added`: count of `+` lines in the unified diff (excluding `+++`)
/// - `doc_path`: relative document path; certain prefixes bump the tier
///
/// Mapping (primary):
/// - `simple_question`, `approval`, `boundary_artifact`, `annotation` → `Low`
/// - `content_addition` < 10 lines → `Low`; ≥ 10 lines → `Med`
/// - `multi_topic`, `structural_change` → `Med`
/// - unknown / missing → `Med` (safe default)
///
/// Path boost: `tasks/software/` and `src/**/specs/` paths bump one tier (cap `High`).
pub fn suggested_tier(
    diff_type: Option<&str>,
    lines_added: usize,
    doc_path: &std::path::Path,
) -> Tier {
    let base = match diff_type {
        Some("simple_question")
        | Some("approval")
        | Some("boundary_artifact")
        | Some("annotation") => Tier::Low,
        Some("content_addition") => {
            if lines_added < 10 {
                Tier::Low
            } else {
                Tier::Med
            }
        }
        Some("multi_topic") | Some("structural_change") => Tier::Med,
        _ => Tier::Med,
    };

    // Path boost: tasks/software/ → bump one tier (cap at High).
    let path_str = doc_path.to_string_lossy();
    let boost = path_str.contains("tasks/software/")
        || path_str.contains("/specs/")
        || path_str.contains("agent-doc-bugs")
        || path_str.contains("plan-")
        || path_str.contains("/plan.md");
    if boost {
        match base {
            Tier::Auto | Tier::Low => Tier::Med,
            Tier::Med => Tier::High,
            Tier::High => Tier::High,
        }
    } else {
        base
    }
}

/// Result of scanning a unified diff for an inline `/model <x>` command.
#[derive(Debug, Clone)]
pub struct ModelSwitchScan {
    /// The concrete model name from `/model <name>` (e.g., `"opus"`).
    pub model_switch: Option<String>,
    /// The resolved tier for the model switch (e.g., `Tier::High` for `opus`).
    pub model_switch_tier: Option<Tier>,
    /// The diff text with the `/model <x>` command line(s) stripped.
    pub stripped_diff: String,
}

/// Scan a unified diff for an inline `/model <x>` command in user-added lines.
///
/// Behavior:
/// - Only matches `+` lines (user additions), excluding `+++` headers.
/// - Skips lines inside fenced code blocks (``` or ~~~).
/// - Skips blockquote lines (`+>`).
/// - Pattern: line content matches `/model <arg>` (whitespace allowed).
/// - On match, the line is removed from the returned diff so it does not
///   propagate to classification or response generation.
/// - Only the first match is captured; subsequent `/model` lines are still stripped.
///
/// The `arg` is parsed via `parse_model_arg`, which accepts both tier names
/// (`low|med|high`) and concrete model names (`opus|sonnet|...`).
pub fn scan_model_switch(diff: &str, harness: &str, model_config: &ModelConfig) -> ModelSwitchScan {
    let mut model_switch: Option<String> = None;
    let mut model_switch_tier: Option<Tier> = None;
    let mut kept_lines: Vec<&str> = Vec::with_capacity(diff.lines().count());

    let mut in_fence = false;
    let mut fence_char = '`';
    let mut fence_len = 0usize;

    for line in diff.lines() {
        // Skip unified diff meta-lines unchanged.
        if line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@") {
            kept_lines.push(line);
            continue;
        }

        // Strip leading diff marker to inspect content.
        let content = if line.starts_with('+') || line.starts_with('-') || line.starts_with(' ') {
            &line[1..]
        } else {
            line
        };

        // Track code-fence state across all lines.
        let trimmed = content.trim_start();
        if !in_fence {
            let fc = trimmed.chars().next().unwrap_or('\0');
            if (fc == '`' || fc == '~')
                && let fl = trimmed.chars().take_while(|&c| c == fc).count()
                && fl >= 3
            {
                in_fence = true;
                fence_char = fc;
                fence_len = fl;
                kept_lines.push(line);
                continue;
            }
        } else {
            let fc = trimmed.chars().next().unwrap_or('\0');
            if fc == fence_char {
                let fl = trimmed.chars().take_while(|&c| c == fc).count();
                if fl >= fence_len && trimmed[fl..].trim().is_empty() {
                    in_fence = false;
                    kept_lines.push(line);
                    continue;
                }
            }
        }

        // Only consider `+` lines (excluding `+++`) for stripping.
        let is_added = line.starts_with('+') && !line.starts_with("+++");
        if !is_added {
            kept_lines.push(line);
            continue;
        }

        // In a fence — keep as-is (no stripping inside fences).
        if in_fence {
            kept_lines.push(line);
            continue;
        }

        // Skip blockquotes.
        if content.starts_with('>') {
            kept_lines.push(line);
            continue;
        }

        // Match `/model <arg>` pattern.
        let stripped = content.trim_end();
        if let Some(rest) = stripped.strip_prefix("/model")
            && let Some(arg) = rest.split_whitespace().next()
            && !arg.is_empty()
        {
            // Parse the arg into (tier, concrete_name).
            if let Some((tier, name)) = parse_model_arg(arg, harness, model_config) {
                if model_switch.is_none() {
                    model_switch = Some(name);
                    model_switch_tier = Some(tier);
                }
                // Drop the line from the diff regardless (always strip /model).
                continue;
            }
            // Unknown arg — still strip the line to avoid /model leaking through.
            continue;
        }

        kept_lines.push(line);
    }

    ModelSwitchScan {
        model_switch,
        model_switch_tier,
        stripped_diff: kept_lines.join("\n"),
    }
}

/// Compose the final `effective_tier` from all available sources.
///
/// Precedence (highest wins): inline `/model` command, then `<!-- agent:model -->`
/// component, then `agent_doc_model_tier` frontmatter, then diff heuristic.
/// `Tier::Auto` is a no-preference sentinel and falls through to the next source.
pub fn compose_effective_tier(
    model_switch_tier: Option<Tier>,
    component_tier: Option<Tier>,
    frontmatter_tier: Option<Tier>,
    suggested: Tier,
) -> Tier {
    for candidate in [model_switch_tier, component_tier, frontmatter_tier] {
        if let Some(t) = candidate
            && !matches!(t, Tier::Auto)
        {
            return t;
        }
    }
    suggested
}

/// Parse a `/model <arg>` argument: either a tier name (`low|med|high`) or a concrete
/// model name (`opus|sonnet|...`).
///
/// Returns the resolved `Tier` and the concrete model name. Tier names resolve
/// through config/built-ins, and harness-owned aliases such as Claude Code
/// `opus` resolve to their current concrete model id.
pub fn parse_model_arg(
    arg: &str,
    harness: &str,
    model_config: &ModelConfig,
) -> Option<(Tier, String)> {
    let trimmed = arg.trim();
    // Try parsing as a tier name first.
    if let Ok(tier) = Tier::from_str(trimmed) {
        if matches!(tier, Tier::Auto) {
            return None;
        }
        let name = resolve_tier_to_model(tier, harness, model_config)
            .unwrap_or_else(|| trimmed.to_string());
        return Some((tier, name));
    }
    // Otherwise treat as a concrete model name and reverse-lookup the tier.
    if let Some(tier) = tier_from_model_name(trimmed, harness, model_config) {
        return Some((tier, canonical_model_name(trimmed, harness, model_config)));
    }
    // For opencode, reject bare model names (no provider prefix).
    if harness == "opencode" && !is_bare_model_name(trimmed) {
        return Some((Tier::Auto, canonical_model_name(trimmed, harness, model_config)));
    }
    // Unknown — accept the name but leave tier as Auto so it doesn't gate.
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvRestore {
        values: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl EnvRestore {
        fn clear(keys: &[&'static str]) -> Self {
            let values = keys
                .iter()
                .map(|key| (*key, std::env::var_os(key)))
                .collect::<Vec<_>>();
            for key in keys {
                // SAFETY: test holds the shared env lock before constructing this guard.
                unsafe { std::env::remove_var(key) };
            }
            Self { values }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            for (key, value) in &self.values {
                match value {
                    Some(value) => {
                        // SAFETY: test holds the shared env lock while the guard is dropped.
                        unsafe { std::env::set_var(key, value) };
                    }
                    None => {
                        // SAFETY: test holds the shared env lock while the guard is dropped.
                        unsafe { std::env::remove_var(key) };
                    }
                }
            }
        }
    }

    const HARNESS_ENV_KEYS: &[&str] = &[
        "CLAUDE_CODE_SESSION",
        "CLAUDE_CODE",
        "CLAUDECODE",
        "CODEX_SESSION",
        "CODEX_THREAD_ID",
        "CODEX_CLI",
        "CODEX",
        "OPENCODE_CLIENT",
        "OPENCODE",
    ];

    #[test]
    fn tier_ordering() {
        assert!(Tier::Auto < Tier::Low);
        assert!(Tier::Low < Tier::Med);
        assert!(Tier::Med < Tier::High);
        assert!(Tier::High > Tier::Low);
        assert!(Tier::Med >= Tier::Med);
    }

    #[test]
    fn tier_from_str_case_insensitive() {
        assert_eq!("LOW".parse::<Tier>().unwrap(), Tier::Low);
        assert_eq!("low".parse::<Tier>().unwrap(), Tier::Low);
        assert_eq!("Low".parse::<Tier>().unwrap(), Tier::Low);
        assert_eq!("AUTO".parse::<Tier>().unwrap(), Tier::Auto);
        assert_eq!("med".parse::<Tier>().unwrap(), Tier::Med);
        assert_eq!("medium".parse::<Tier>().unwrap(), Tier::Med);
        assert_eq!("HIGH".parse::<Tier>().unwrap(), Tier::High);
    }

    #[test]
    fn tier_from_str_invalid() {
        assert!("ultra".parse::<Tier>().is_err());
        assert!("".parse::<Tier>().is_err());
        assert!("opus".parse::<Tier>().is_err());
    }

    #[test]
    fn tier_display() {
        assert_eq!(Tier::Low.to_string(), "low");
        assert_eq!(Tier::Med.to_string(), "med");
        assert_eq!(Tier::High.to_string(), "high");
        assert_eq!(Tier::Auto.to_string(), "auto");
    }

    #[test]
    fn harness_detection_returns_known_value() {
        // Don't mutate env (Rust 2024 marks env mutators unsafe + tests may run
        // in parallel). Just assert the function returns one of the known values.
        let h = detect_harness();
        assert!(
            matches!(h.as_str(), "claude-code" | "codex" | "opencode" | "default"),
            "unexpected harness: {h}"
        );
    }

    #[test]
    fn harness_key_for_agent_name_maps_cli_aliases() {
        assert_eq!(harness_key_for_agent_name("claude"), "claude-code");
        assert_eq!(harness_key_for_agent_name("claude_code"), "claude-code");
        assert_eq!(harness_key_for_agent_name("codex"), "codex");
        assert_eq!(harness_key_for_agent_name("opencode"), "opencode");
    }

    #[test]
    fn harness_detection_recognizes_cli_environment_aliases() {
        let _env_lock = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _restore = EnvRestore::clear(HARNESS_ENV_KEYS);

        // SAFETY: this test holds the shared env lock.
        unsafe { std::env::set_var("CLAUDE_CODE", "1") };
        assert_eq!(detect_harness(), "claude-code");
        // SAFETY: this test holds the shared env lock.
        unsafe { std::env::remove_var("CLAUDE_CODE") };

        // SAFETY: this test holds the shared env lock.
        unsafe { std::env::set_var("CODEX_THREAD_ID", "thread-123") };
        assert_eq!(detect_harness(), "codex");
        // SAFETY: this test holds the shared env lock.
        unsafe { std::env::remove_var("CODEX_THREAD_ID") };

        // SAFETY: this test holds the shared env lock.
        unsafe { std::env::set_var("OPENCODE", "1") };
        assert_eq!(detect_harness(), "opencode");
    }

    #[test]
    fn resolve_builtin_claude_code() {
        let cfg = ModelConfig::default();
        assert_eq!(
            resolve_tier_to_model(Tier::High, "claude-code", &cfg).as_deref(),
            Some("claude-opus-4-8")
        );
        assert_eq!(
            resolve_tier_to_model(Tier::Med, "claude-code", &cfg).as_deref(),
            Some("sonnet")
        );
        assert_eq!(
            resolve_tier_to_model(Tier::Low, "claude-code", &cfg).as_deref(),
            Some("haiku")
        );
        assert_eq!(resolve_tier_to_model(Tier::Auto, "claude-code", &cfg), None);
    }

    #[test]
    fn resolve_builtin_codex() {
        let cfg = ModelConfig::default();
        assert_eq!(
            resolve_tier_to_model(Tier::High, "codex", &cfg).as_deref(),
            Some("o3")
        );
        assert_eq!(
            resolve_tier_to_model(Tier::Low, "codex", &cfg).as_deref(),
            Some("gpt-4o-mini")
        );
    }

    #[test]
    fn resolve_unknown_harness_uses_default() {
        let cfg = ModelConfig::default();
        // Unknown harness falls through to the `default` built-in map.
        assert_eq!(
            resolve_tier_to_model(Tier::High, "junie", &cfg).as_deref(),
            Some("opus")
        );
    }

    #[test]
    fn user_config_overrides_builtin() {
        let mut cfg = ModelConfig::default();
        let mut tiers = BTreeMap::new();
        tiers.insert(
            "claude-code".to_string(),
            TierMap {
                low: Some("haiku-3".to_string()),
                med: Some("sonnet-4".to_string()),
                high: Some("opus-4-1".to_string()),
            },
        );
        cfg.tiers = tiers;
        assert_eq!(
            resolve_tier_to_model(Tier::High, "claude-code", &cfg).as_deref(),
            Some("opus-4-1")
        );
    }

    #[test]
    fn tier_from_model_name_builtin() {
        let cfg = ModelConfig::default();
        assert_eq!(
            tier_from_model_name("opus", "claude-code", &cfg),
            Some(Tier::High)
        );
        assert_eq!(
            tier_from_model_name("claude-opus-4-8", "claude-code", &cfg),
            Some(Tier::High)
        );
        assert_eq!(
            tier_from_model_name("sonnet", "claude-code", &cfg),
            Some(Tier::Med)
        );
        assert_eq!(
            tier_from_model_name("haiku", "claude-code", &cfg),
            Some(Tier::Low)
        );
        assert_eq!(tier_from_model_name("unknown", "claude-code", &cfg), None);
    }

    #[test]
    fn parse_model_arg_tier_name() {
        let cfg = ModelConfig::default();
        let (tier, name) = parse_model_arg("high", "claude-code", &cfg).unwrap();
        assert_eq!(tier, Tier::High);
        assert_eq!(name, "claude-opus-4-8");
    }

    #[test]
    fn parse_model_arg_concrete_name() {
        let cfg = ModelConfig::default();
        let (tier, name) = parse_model_arg("opus", "claude-code", &cfg).unwrap();
        assert_eq!(tier, Tier::High);
        assert_eq!(name, "claude-opus-4-8");
    }

    #[test]
    fn canonical_model_name_expands_claude_code_opus_alias() {
        let cfg = ModelConfig::default();
        assert_eq!(
            canonical_model_name("opus", "claude-code", &cfg),
            "claude-opus-4-8"
        );
        assert_eq!(canonical_model_name("opus", "codex", &cfg), "opus");
    }

    #[test]
    fn is_bare_model_name_detects_missing_provider_prefix() {
        assert!(is_bare_model_name("glm-5.1"));
        assert!(is_bare_model_name("opus"));
        assert!(is_bare_model_name("haiku"));
        assert!(!is_bare_model_name("zai-coding-plan/glm-5.1"));
        assert!(!is_bare_model_name("anthropic/claude-opus-4-7"));
        assert!(is_bare_model_name("/leading-slash-only"));
    }

    #[test]
    fn canonical_model_name_warns_on_bare_opencode_name() {
        let cfg = ModelConfig::default();
        assert_eq!(
            canonical_model_name("glm-5.1", "opencode", &cfg),
            "glm-5.1"
        );
        assert_eq!(
            canonical_model_name("zai-coding-plan/glm-5.1", "opencode", &cfg),
            "zai-coding-plan/glm-5.1"
        );
    }

    #[test]
    fn parse_model_arg_opencode_rejects_bare_name() {
        let cfg = ModelConfig::default();
        assert!(parse_model_arg("glm-5.1", "opencode", &cfg).is_none());
    }

    #[test]
    fn parse_model_arg_opencode_accepts_provider_prefixed_name() {
        let cfg = ModelConfig::default();
        let (tier, name) = parse_model_arg("zai-coding-plan/glm-5.1", "opencode", &cfg).unwrap();
        assert_eq!(tier, Tier::Auto);
        assert_eq!(name, "zai-coding-plan/glm-5.1");
    }

    #[test]
    fn parse_model_arg_unknown() {
        let cfg = ModelConfig::default();
        assert!(parse_model_arg("xyz-3000", "claude-code", &cfg).is_none());
    }

    #[test]
    fn parse_model_arg_auto_rejected() {
        let cfg = ModelConfig::default();
        assert!(parse_model_arg("auto", "claude-code", &cfg).is_none());
    }

    #[test]
    fn extract_model_component_present() {
        let doc = "# Title\n\n<!-- agent:model -->\nhigh\n<!-- /agent:model -->\n\nbody\n";
        assert_eq!(extract_model_component(doc).as_deref(), Some("high"));
    }

    #[test]
    fn extract_model_component_absent() {
        let doc = "# Title\n\nbody only\n";
        assert_eq!(extract_model_component(doc), None);
    }

    #[test]
    fn extract_model_component_empty_inner() {
        let doc = "<!-- agent:model -->\n<!-- /agent:model -->\n";
        assert_eq!(extract_model_component(doc), None);
    }

    #[test]
    fn extract_model_component_concrete_name() {
        let doc = "<!-- agent:model -->\nopus\n<!-- /agent:model -->\n";
        assert_eq!(extract_model_component(doc).as_deref(), Some("opus"));
    }

    #[test]
    fn component_value_to_tier_tier_name() {
        let cfg = ModelConfig::default();
        assert_eq!(
            component_value_to_tier("high", "claude-code", &cfg),
            Some(Tier::High)
        );
    }

    #[test]
    fn component_value_to_tier_concrete_name() {
        let cfg = ModelConfig::default();
        assert_eq!(
            component_value_to_tier("opus", "claude-code", &cfg),
            Some(Tier::High)
        );
    }

    #[test]
    fn component_value_to_tier_unknown() {
        let cfg = ModelConfig::default();
        assert_eq!(component_value_to_tier("xyz", "claude-code", &cfg), None);
    }

    #[test]
    fn suggested_tier_simple_question() {
        let path = std::path::Path::new("tasks/research/x.md");
        assert_eq!(suggested_tier(Some("simple_question"), 1, path), Tier::Low);
    }

    #[test]
    fn suggested_tier_small_addition() {
        let path = std::path::Path::new("tasks/research/x.md");
        assert_eq!(suggested_tier(Some("content_addition"), 5, path), Tier::Low);
    }

    #[test]
    fn suggested_tier_large_addition() {
        let path = std::path::Path::new("tasks/research/x.md");
        assert_eq!(
            suggested_tier(Some("content_addition"), 50, path),
            Tier::Med
        );
    }

    #[test]
    fn suggested_tier_default_for_unknown() {
        let path = std::path::Path::new("tasks/research/x.md");
        assert_eq!(suggested_tier(None, 0, path), Tier::Med);
    }

    #[test]
    fn suggested_tier_path_boost_software() {
        let path = std::path::Path::new("tasks/software/foo.md");
        // Low gets boosted to Med
        assert_eq!(suggested_tier(Some("simple_question"), 1, path), Tier::Med);
        // Med gets boosted to High
        assert_eq!(
            suggested_tier(Some("content_addition"), 50, path),
            Tier::High
        );
    }

    #[test]
    fn suggested_tier_path_boost_caps_at_high() {
        let path = std::path::Path::new("tasks/software/foo.md");
        // Already High stays High
        let t = suggested_tier(Some("content_addition"), 50, path);
        assert_eq!(t, Tier::High);
    }

    #[test]
    fn compose_effective_tier_model_switch_wins() {
        let t = compose_effective_tier(
            Some(Tier::High),
            Some(Tier::Low),
            Some(Tier::Med),
            Tier::Low,
        );
        assert_eq!(t, Tier::High);
    }

    #[test]
    fn compose_effective_tier_component_beats_frontmatter() {
        let t = compose_effective_tier(None, Some(Tier::High), Some(Tier::Low), Tier::Med);
        assert_eq!(t, Tier::High);
    }

    #[test]
    fn compose_effective_tier_frontmatter_beats_heuristic() {
        let t = compose_effective_tier(None, None, Some(Tier::High), Tier::Low);
        assert_eq!(t, Tier::High);
    }

    #[test]
    fn compose_effective_tier_falls_through_to_heuristic() {
        let t = compose_effective_tier(None, None, None, Tier::Med);
        assert_eq!(t, Tier::Med);
    }

    #[test]
    fn scan_model_switch_concrete_name() {
        let cfg = ModelConfig::default();
        let diff = "@@ -1,3 +1,4 @@\n context\n+/model opus\n+real edit\n";
        let result = scan_model_switch(diff, "claude-code", &cfg);
        assert_eq!(result.model_switch.as_deref(), Some("claude-opus-4-8"));
        assert_eq!(result.model_switch_tier, Some(Tier::High));
        assert!(!result.stripped_diff.contains("/model opus"));
        assert!(result.stripped_diff.contains("real edit"));
    }

    #[test]
    fn scan_model_switch_tier_name() {
        let cfg = ModelConfig::default();
        let diff = "+/model high\n+other line\n";
        let result = scan_model_switch(diff, "claude-code", &cfg);
        assert_eq!(result.model_switch_tier, Some(Tier::High));
        assert_eq!(result.model_switch.as_deref(), Some("claude-opus-4-8"));
        assert!(!result.stripped_diff.contains("/model high"));
    }

    #[test]
    fn scan_model_switch_haiku() {
        let cfg = ModelConfig::default();
        let diff = "+/model haiku\n";
        let result = scan_model_switch(diff, "claude-code", &cfg);
        assert_eq!(result.model_switch_tier, Some(Tier::Low));
    }

    #[test]
    fn scan_model_switch_inside_fenced_code_ignored() {
        let cfg = ModelConfig::default();
        let diff = "+```\n+/model opus\n+```\n+real line\n";
        let result = scan_model_switch(diff, "claude-code", &cfg);
        assert_eq!(result.model_switch, None);
        assert!(result.stripped_diff.contains("/model opus"));
    }

    #[test]
    fn scan_model_switch_inside_blockquote_ignored() {
        let cfg = ModelConfig::default();
        let diff = "+> /model opus\n+real line\n";
        let result = scan_model_switch(diff, "claude-code", &cfg);
        assert_eq!(result.model_switch, None);
        assert!(result.stripped_diff.contains("/model opus"));
    }

    #[test]
    fn scan_model_switch_only_added_lines() {
        let cfg = ModelConfig::default();
        // Context line with /model is NOT a user addition.
        let diff = " /model opus\n+real line\n";
        let result = scan_model_switch(diff, "claude-code", &cfg);
        assert_eq!(result.model_switch, None);
    }

    #[test]
    fn scan_model_switch_no_match() {
        let cfg = ModelConfig::default();
        let diff = "+just a normal line\n+another\n";
        let result = scan_model_switch(diff, "claude-code", &cfg);
        assert_eq!(result.model_switch, None);
        // Diff is unchanged (modulo trailing newline normalization).
        assert!(result.stripped_diff.contains("just a normal line"));
        assert!(result.stripped_diff.contains("another"));
    }

    #[test]
    fn scan_model_switch_unknown_arg_still_stripped() {
        let cfg = ModelConfig::default();
        // Unknown arg → no tier captured but line still stripped.
        let diff = "+/model xyz-3000\n+real line\n";
        let result = scan_model_switch(diff, "claude-code", &cfg);
        assert_eq!(result.model_switch, None);
        assert!(!result.stripped_diff.contains("/model xyz-3000"));
        assert!(result.stripped_diff.contains("real line"));
    }

    #[test]
    fn scan_model_switch_first_match_wins() {
        let cfg = ModelConfig::default();
        let diff = "+/model opus\n+/model haiku\n";
        let result = scan_model_switch(diff, "claude-code", &cfg);
        assert_eq!(result.model_switch.as_deref(), Some("claude-opus-4-8"));
        // Both lines stripped.
        assert!(!result.stripped_diff.contains("/model"));
    }

    #[test]
    fn compose_effective_tier_auto_falls_through() {
        // Auto values should fall through to next source.
        let t = compose_effective_tier(
            Some(Tier::Auto),
            Some(Tier::Auto),
            Some(Tier::High),
            Tier::Low,
        );
        assert_eq!(t, Tier::High);
    }
}
