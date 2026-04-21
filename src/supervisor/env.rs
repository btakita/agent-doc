//! # Module: supervisor::env
//!
//! Resolves the complete environment for a supervised child process. Sits
//! **above** [`crate::supervisor::pty`] — `pty.rs` takes a pre-resolved
//! `HashMap<String, String>` and never reaches into the parent env, so every
//! restart under `state.rs` will reuse the exact same map that `start.rs`
//! computed at supervisor startup.
//!
//! ## Spec
//!
//! - [`EnvSpec`] carries two pieces of intent:
//!   1. Whether to start from the parent process env (`inherit_parent`).
//!   2. An ordered overlay of set/unset operations (`overrides`).
//! - [`EnvSpec::from_frontmatter`] reads the two frontmatter fields
//!   (`agent_doc_env_inherit`, default `true`; and `env`) and returns the spec.
//! - [`EnvSpec::resolve`] walks: capture base env → apply overrides in order
//!   → return the final `HashMap` for [`PtySpawnConfig::env`].
//! - Set values are shell-expanded via [`crate::env::expand_values`] so
//!   `$(passage ...)` / `$VAR` / `${HOME}/x` work the way a shell user expects.
//!   Expansion runs exactly once per `resolve()` call.
//!
//! ## Agentic Contracts
//!
//! - **Restart determinism.** Callers are expected to call `resolve()` once
//!   per supervisor lifetime (at startup) and reuse the returned map across
//!   every `state.rs` restart. A parent-env mutation mid-run **must not**
//!   silently reshape the child — that is the primary reason the resolver
//!   lives here and not inside `pty.rs`.
//! - **`pty.rs` invariant preserved.** `pty.rs` still calls `env_clear()` and
//!   sets everything from `cfg.env`; the `env_is_not_inherited_from_parent`
//!   test stays load-bearing. This module is where "inherit parent env" is
//!   deliberately re-introduced, under the control of a frontmatter flag.
//! - **Unset wins over set.** Document order: the last operation targeting a
//!   key is what the child sees. A later `KEY: null` removes an earlier
//!   `KEY: value`. (In practice YAML maps cannot have duplicate keys, so this
//!   only matters if a future compositor appends overrides.)
//!
//! ## Evals
//!
//! - from_frontmatter_default_inherits: no field → `inherit_parent = true`
//! - from_frontmatter_explicit_false: `agent_doc_env_inherit: false` → sealed
//! - from_frontmatter_copies_env_map: overrides match `fm.env` verbatim
//! - resolve_inherit_plus_overlay: parent `PATH` survives, override wins
//! - resolve_unset_removes_parent_key: `KEY: null` drops inherited parent value
//! - resolve_expansion_uses_parent: `${HOME}/x` expands against parent env
//! - resolve_sealed_drops_parent: `inherit_parent = false` → only overrides present
//! - resolve_is_deterministic_across_calls: same spec → identical map even if
//!   parent env mutates between calls (because the returned map is a value,
//!   not a live view — callers cache it)

use std::collections::HashMap;

use anyhow::Result;
use indexmap::IndexMap;

use crate::env::{expand_values, EnvMap};
use crate::frontmatter::Frontmatter;

/// Declarative description of how to build the child env.
///
/// Constructed once per supervisor lifetime — typically via
/// [`EnvSpec::from_frontmatter`] at startup — and then consumed by
/// [`EnvSpec::resolve`] to produce the `HashMap` fed to
/// [`crate::supervisor::pty::PtySpawnConfig::env`].
#[derive(Debug, Clone)]
pub struct EnvSpec {
    /// Whether to start from `std::env::vars()` (the parent shell) before
    /// applying overrides. Default `true`. Set `false` for a sealed env where
    /// only the explicit `overrides` keys reach the child.
    pub inherit_parent: bool,
    /// Frontmatter `env:` overlay, in document order. `Some(val)` sets the
    /// key (with shell expansion); `None` unsets it (drops it from the
    /// resolved map, even if the parent provided it).
    pub overrides: EnvMap,
}

impl EnvSpec {
    /// Build a spec from parsed frontmatter.
    ///
    /// - `agent_doc_env_inherit: Option<bool>` → defaults to `true` when unset.
    /// - `env:` → copied verbatim (including ordering and `None` entries).
    pub fn from_frontmatter(fm: &Frontmatter) -> Self {
        Self {
            inherit_parent: fm.agent_doc_env_inherit.unwrap_or(true),
            overrides: fm.env.clone(),
        }
    }

    /// Build a sealed spec with an explicit overlay.
    #[allow(dead_code)] // API surface — used by tests and future callers
    pub fn sealed(overrides: EnvMap) -> Self {
        Self {
            inherit_parent: false,
            overrides,
        }
    }

    /// Resolve to the final `HashMap<String, String>` passed to
    /// [`crate::supervisor::pty::PtySpawnConfig::env`].
    ///
    /// This is the **one** call that reads `std::env::vars()`. Callers should
    /// invoke it once at supervisor startup and reuse the returned map across
    /// every `state.rs` restart so a parent-env mutation cannot silently
    /// reshape the child between restarts.
    pub fn resolve(&self) -> Result<HashMap<String, String>> {
        let mut base: HashMap<String, String> = if self.inherit_parent {
            std::env::vars().collect()
        } else {
            HashMap::new()
        };

        if self.overrides.is_empty() {
            return Ok(base);
        }

        let expanded = expand_values(&self.overrides)?;
        for (key, value) in expanded {
            match value {
                Some(v) => {
                    base.insert(key, v);
                }
                None => {
                    base.remove(&key);
                }
            }
        }
        Ok(base)
    }
}

impl Default for EnvSpec {
    /// Default: inherit parent env, no overrides.
    fn default() -> Self {
        Self {
            inherit_parent: true,
            overrides: IndexMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guard to set and restore a parent env var within a single test.
    /// Tests share process state, so callers must pick unique variable names.
    struct EnvGuard {
        key: &'static str,
        prior: Option<String>,
    }
    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let prior = std::env::var(key).ok();
            // SAFETY: tests set unique keys; process env mutation is single-threaded
            // within a single test body. Cargo may run tests in parallel but each
            // guard uses a distinct key so there is no cross-test interference.
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, prior }
        }
        fn unset(key: &'static str) -> Self {
            let prior = std::env::var(key).ok();
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, prior }
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.prior {
                    Some(v) => std::env::set_var(self.key, v),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    fn env_map(entries: &[(&str, Option<&str>)]) -> EnvMap {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.map(|s| s.to_string())))
            .collect()
    }

    #[test]
    fn from_frontmatter_default_inherits() {
        let fm = Frontmatter::default();
        let spec = EnvSpec::from_frontmatter(&fm);
        assert!(spec.inherit_parent);
        assert!(spec.overrides.is_empty());
    }

    #[test]
    fn from_frontmatter_explicit_false_seals() {
        let fm = Frontmatter {
            agent_doc_env_inherit: Some(false),
            ..Default::default()
        };
        let spec = EnvSpec::from_frontmatter(&fm);
        assert!(!spec.inherit_parent);
    }

    #[test]
    fn from_frontmatter_copies_env_map_verbatim() {
        let fm = Frontmatter {
            env: env_map(&[("FOO", Some("bar")), ("BAZ", None)]),
            ..Default::default()
        };
        let spec = EnvSpec::from_frontmatter(&fm);
        assert_eq!(spec.overrides.len(), 2);
        assert_eq!(spec.overrides["FOO"], Some("bar".to_string()));
        assert_eq!(spec.overrides["BAZ"], None);
    }

    #[test]
    fn resolve_inherit_plus_overlay() {
        let _g_keep = EnvGuard::set("AGENT_DOC_ENV_TEST_KEEP", "parent_value");
        let _g_over = EnvGuard::set("AGENT_DOC_ENV_TEST_OVERRIDE", "parent");

        let spec = EnvSpec {
            inherit_parent: true,
            overrides: env_map(&[("AGENT_DOC_ENV_TEST_OVERRIDE", Some("child"))]),
        };
        let resolved = spec.resolve().unwrap();
        assert_eq!(
            resolved.get("AGENT_DOC_ENV_TEST_KEEP"),
            Some(&"parent_value".to_string()),
            "inherited parent key survives overlay"
        );
        assert_eq!(
            resolved.get("AGENT_DOC_ENV_TEST_OVERRIDE"),
            Some(&"child".to_string()),
            "overlay wins over parent"
        );
    }

    #[test]
    fn resolve_unset_removes_parent_key() {
        let _g = EnvGuard::set("AGENT_DOC_ENV_TEST_DROP", "parent");
        let spec = EnvSpec {
            inherit_parent: true,
            overrides: env_map(&[("AGENT_DOC_ENV_TEST_DROP", None)]),
        };
        let resolved = spec.resolve().unwrap();
        assert!(
            !resolved.contains_key("AGENT_DOC_ENV_TEST_DROP"),
            "unset drops inherited parent key"
        );
    }

    #[test]
    fn resolve_expansion_uses_parent_env() {
        let _g = EnvGuard::set("AGENT_DOC_ENV_TEST_BASE", "/parent/base");
        let spec = EnvSpec {
            inherit_parent: true,
            overrides: env_map(&[(
                "DERIVED",
                Some("$AGENT_DOC_ENV_TEST_BASE/child"),
            )]),
        };
        let resolved = spec.resolve().unwrap();
        assert_eq!(
            resolved.get("DERIVED"),
            Some(&"/parent/base/child".to_string()),
            "parent env is visible inside shell expansion"
        );
    }

    #[test]
    fn resolve_sealed_drops_parent() {
        // Pick a key that is effectively always present in any test env.
        let _g = EnvGuard::set("AGENT_DOC_ENV_TEST_SEAL_WITNESS", "present");
        let spec = EnvSpec {
            inherit_parent: false,
            overrides: env_map(&[("ONLY", Some("this"))]),
        };
        let resolved = spec.resolve().unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved.get("ONLY"), Some(&"this".to_string()));
        assert!(
            !resolved.contains_key("AGENT_DOC_ENV_TEST_SEAL_WITNESS"),
            "sealed env ignores parent"
        );
    }

    #[test]
    fn resolve_is_deterministic_across_parent_mutation() {
        // Demonstrate that once resolve() has returned, the caller's map is
        // frozen — later parent-env mutations do not leak into it. This is
        // why state.rs should cache the resolved map across restarts.
        let spec = EnvSpec {
            inherit_parent: true,
            overrides: env_map(&[("FIRST_RUN_KEY", Some("fixed"))]),
        };
        let resolved_once = spec.resolve().unwrap();

        // Mutate parent env after resolve().
        unsafe {
            std::env::set_var("AGENT_DOC_ENV_TEST_POSTRESOLVE", "leaked");
        }
        let leaked_present = resolved_once.contains_key("AGENT_DOC_ENV_TEST_POSTRESOLVE");
        unsafe {
            std::env::remove_var("AGENT_DOC_ENV_TEST_POSTRESOLVE");
        }

        assert!(
            !leaked_present,
            "cached resolved map must not see post-resolve parent mutations"
        );
        assert_eq!(
            resolved_once.get("FIRST_RUN_KEY"),
            Some(&"fixed".to_string())
        );
    }

    #[test]
    fn resolve_empty_sealed_is_empty() {
        let spec = EnvSpec::sealed(IndexMap::new());
        let resolved = spec.resolve().unwrap();
        assert!(resolved.is_empty());
    }
}
