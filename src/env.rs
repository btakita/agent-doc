//! # Module: env
//!
//! ## Spec
//! - `expand_values(env)` takes an order-preserving map of env var name → optional
//!   raw value (`$(command)` / `$VAR` expressions). Returns a
//!   `Vec<(String, Option<String>)>` of resolved entries in document order. A
//!   `None` input value stays `None` in the output and means "unset this key in
//!   the child env".
//! - Each set value is expanded through a single `sh -c` invocation that exports
//!   vars sequentially so later values can reference earlier keys. Unset entries
//!   are NOT exported into the expansion shell — cross-references only see keys
//!   that are being set.
//! - Returns an empty `Vec` when the input is empty (no shell spawn).
//! - `shell_export_prefix(env)` returns a string of the form
//!   `export K1="V1" && unset K2 && ...` using **unexpanded** values, suitable for
//!   prepending to a `tmux send-keys` command so the target pane's shell expands
//!   `$(passage ...)` internally — keeps secrets out of the tmux visual layer.
//!
//! ## Agentic Contracts
//! - `expand_values` performs synchronous shell I/O; failures propagate via
//!   `anyhow::Result`.
//! - The order of returned pairs matches the input `IndexMap` iteration order,
//!   including unset entries.
//! - `shell_export_prefix` is purely textual — it does NOT touch the shell or
//!   expand anything. It escapes double quotes in values with `\"`.
//!
//! ## Evals
//! - expand_values_plain: `{FOO: Some("bar"), BAZ: Some("qux")}` → ordered set pairs
//! - expand_values_shell_expansion: `{GREETING: Some("$(echo hello)")}` → `hello`
//! - expand_values_cross_reference: chained `$VAR` references resolve in order
//! - expand_values_unset_preserved: `{FOO: None}` → `[(FOO, None)]`, no shell spawn for unsets alone
//! - expand_values_mixed: set + unset preserved in document order
//! - expand_values_empty: empty map → empty Vec, no shell spawned
//! - shell_export_prefix_empty: empty map → empty string
//! - shell_export_prefix_basic: two sets → `export K1="v1" && export K2="v2" && `
//! - shell_export_prefix_unset: `{K: None}` → `unset K && `
//! - shell_export_prefix_quotes_in_value: value with `"` → escaped with `\"`

use anyhow::{Context, Result};
use indexmap::IndexMap;

/// Order-preserving env map. `None` values mean "unset this key".
pub type EnvMap = IndexMap<String, Option<String>>;

/// Expand an env var map through the shell, returning pairs in document order.
///
/// Values may contain `$(command)` and `$VAR` — the shell expands them. Later
/// values can reference earlier keys because all set vars are exported
/// sequentially within the same shell invocation before being printed.
/// `None` entries are passed through unchanged; they signal "unset" to callers.
pub fn expand_values(env: &EnvMap) -> Result<Vec<(String, Option<String>)>> {
    if env.is_empty() {
        return Ok(Vec::new());
    }
    // Partition: build a script that only exports & prints the Some entries,
    // but preserve document order so None entries can be re-interleaved.
    let set_keys: Vec<&String> = env
        .iter()
        .filter_map(|(k, v)| v.as_ref().map(|_| k))
        .collect();

    let expanded: Vec<String> = if set_keys.is_empty() {
        Vec::new()
    } else {
        let mut script = String::new();
        for (key, value) in env {
            if let Some(v) = value {
                script.push_str(&format!("export {}={}\n", key, v));
            }
        }
        for key in &set_keys {
            script.push_str(&format!("printf '%s\\0' \"${{{}}}\"\n", key));
        }
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .output()
            .context("failed to spawn shell for env expansion")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("env expansion failed: {}", stderr.trim());
        }
        let stdout = String::from_utf8(output.stdout)
            .context("env expansion output is not valid UTF-8")?;
        stdout
            .split('\0')
            .take(set_keys.len())
            .map(|s| s.to_string())
            .collect()
    };

    let mut set_iter = expanded.into_iter();
    let mut result = Vec::with_capacity(env.len());
    for (key, value) in env {
        match value {
            Some(_) => {
                let v = set_iter.next().unwrap_or_default();
                result.push((key.clone(), Some(v)));
            }
            None => {
                result.push((key.clone(), None));
            }
        }
    }
    Ok(result)
}

/// Build a shell `export K="V" && unset K2 && ...` prefix suitable for
/// prepending to a `tmux send-keys` command. Values are passed **unexpanded**
/// so secrets like `$(passage ...)` are resolved by the target pane's shell,
/// never appearing in the send-keys argument list or tmux scrollback.
pub fn shell_export_prefix(env: &EnvMap) -> String {
    if env.is_empty() {
        return String::new();
    }
    let mut prefix = String::new();
    for (key, value) in env {
        match value {
            Some(v) => {
                let escaped = v.replace('"', "\\\"");
                prefix.push_str(&format!("export {}=\"{}\" && ", key, escaped));
            }
            None => {
                prefix.push_str(&format!("unset {} && ", key));
            }
        }
    }
    prefix
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(k: &str, v: &str) -> (String, Option<String>) {
        (k.to_string(), Some(v.to_string()))
    }
    fn unset(k: &str) -> (String, Option<String>) {
        (k.to_string(), None)
    }

    #[test]
    fn expand_values_plain() {
        let env: EnvMap = [set("FOO", "bar"), set("BAZ", "qux")].into_iter().collect();
        let result = expand_values(&env).unwrap();
        assert_eq!(
            result,
            vec![
                ("FOO".to_string(), Some("bar".to_string())),
                ("BAZ".to_string(), Some("qux".to_string())),
            ]
        );
    }

    #[test]
    fn expand_values_shell_expansion() {
        let env: EnvMap = [set("GREETING", "$(echo hello)")].into_iter().collect();
        let result = expand_values(&env).unwrap();
        assert_eq!(result[0].1, Some("hello".to_string()));
    }

    #[test]
    fn expand_values_cross_reference() {
        let env: EnvMap = [set("BASE", "world"), set("DERIVED", "$BASE")]
            .into_iter()
            .collect();
        let result = expand_values(&env).unwrap();
        assert_eq!(result[0], ("BASE".to_string(), Some("world".to_string())));
        assert_eq!(result[1], ("DERIVED".to_string(), Some("world".to_string())));
    }

    #[test]
    fn expand_values_unset_preserved() {
        let env: EnvMap = [unset("FOO")].into_iter().collect();
        let result = expand_values(&env).unwrap();
        assert_eq!(result, vec![("FOO".to_string(), None)]);
    }

    #[test]
    fn expand_values_mixed_set_and_unset_document_order() {
        let env: EnvMap = [
            set("FIRST", "1"),
            unset("MIDDLE"),
            set("LAST", "$FIRST-tail"),
        ]
        .into_iter()
        .collect();
        let result = expand_values(&env).unwrap();
        assert_eq!(result[0], ("FIRST".to_string(), Some("1".to_string())));
        assert_eq!(result[1], ("MIDDLE".to_string(), None));
        assert_eq!(
            result[2],
            ("LAST".to_string(), Some("1-tail".to_string()))
        );
    }

    #[test]
    fn expand_values_empty() {
        let env: EnvMap = IndexMap::new();
        let result = expand_values(&env).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn shell_export_prefix_empty() {
        let env: EnvMap = IndexMap::new();
        assert_eq!(shell_export_prefix(&env), "");
    }

    #[test]
    fn shell_export_prefix_basic() {
        let env: EnvMap = [set("K1", "v1"), set("K2", "v2")].into_iter().collect();
        assert_eq!(
            shell_export_prefix(&env),
            "export K1=\"v1\" && export K2=\"v2\" && "
        );
    }

    #[test]
    fn shell_export_prefix_unset() {
        let env: EnvMap = [unset("K1")].into_iter().collect();
        assert_eq!(shell_export_prefix(&env), "unset K1 && ");
    }

    #[test]
    fn shell_export_prefix_mixed() {
        let env: EnvMap = [set("K1", "v1"), unset("K2"), set("K3", "v3")]
            .into_iter()
            .collect();
        assert_eq!(
            shell_export_prefix(&env),
            "export K1=\"v1\" && unset K2 && export K3=\"v3\" && "
        );
    }

    #[test]
    fn shell_export_prefix_quotes_in_value() {
        let env: EnvMap = [set("K", "a\"b")].into_iter().collect();
        assert_eq!(shell_export_prefix(&env), "export K=\"a\\\"b\" && ");
    }

    #[test]
    fn shell_export_prefix_unexpanded_passage() {
        let env: EnvMap = [set("TOKEN", "$(passage btak/secret)")]
            .into_iter()
            .collect();
        let prefix = shell_export_prefix(&env);
        assert!(prefix.contains("$(passage btak/secret)"));
        assert!(!prefix.contains("export TOKEN=\"\""));
    }
}
