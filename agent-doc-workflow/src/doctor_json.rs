//! Pure JSON fact projection for workflow doctor captures.

use serde_json::Value;

use crate::doctor::{PreflightDoctorFacts, SessionCheckDoctorFacts};

pub fn project_preflight_facts(value: &Value) -> PreflightDoctorFacts {
    PreflightDoctorFacts {
        json_provided: true,
        queue_active: lookup_bool(value, "queue_active"),
        queue_continuation_required: lookup_bool(value, "queue_continuation_required"),
        queue_drainable_head_count: lookup_usize(value, "queue_drainable_head_count"),
        queue_prompts: lookup_string_array(value, "queue_prompts"),
        warnings: lookup_string_array(value, "warnings"),
    }
}

pub fn project_session_check_facts(value: &Value) -> SessionCheckDoctorFacts {
    SessionCheckDoctorFacts {
        json_provided: true,
        ok: lookup_bool(value, "ok"),
        status: lookup_string(value, "status"),
        message: lookup_string(value, "message")
            .or_else(|| lookup_string(value, "reason"))
            .or_else(|| lookup_string(value, "detail")),
        warnings: lookup_string_array(value, "warnings"),
    }
}

pub fn lookup_value<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value
        .get(key)
        .or_else(|| value.get("report").and_then(|report| report.get(key)))
        .or_else(|| {
            value
                .get("session_check")
                .and_then(|report| report.get(key))
        })
}

pub fn lookup_bool(value: &Value, key: &str) -> Option<bool> {
    lookup_value(value, key).and_then(Value::as_bool)
}

pub fn lookup_usize(value: &Value, key: &str) -> Option<usize> {
    lookup_value(value, key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

pub fn lookup_string(value: &Value, key: &str) -> Option<String> {
    lookup_value(value, key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

pub fn lookup_string_array(value: &Value, key: &str) -> Vec<String> {
    lookup_value(value, key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lookup_value_prefers_top_level_before_nested_reports() {
        let value = json!({
            "status": "top",
            "report": { "status": "report" },
            "session_check": { "status": "session" }
        });

        assert_eq!(lookup_string(&value, "status").as_deref(), Some("top"));
    }

    #[test]
    fn lookup_value_reads_report_and_session_check_sections() {
        let value = json!({
            "report": { "ok": true },
            "session_check": { "detail": "interrupted" }
        });

        assert_eq!(lookup_bool(&value, "ok"), Some(true));
        assert_eq!(
            lookup_string(&value, "detail").as_deref(),
            Some("interrupted")
        );
    }

    #[test]
    fn lookup_string_array_skips_non_string_items() {
        let value = json!({
            "warnings": ["first", 7, false, "second"]
        });

        assert_eq!(
            lookup_string_array(&value, "warnings"),
            vec!["first".to_string(), "second".to_string()]
        );
    }

    #[test]
    fn projects_preflight_facts_from_nested_report_json() {
        let value = json!({
            "report": {
                "queue_active": true,
                "queue_continuation_required": false,
                "queue_drainable_head_count": 2,
                "queue_prompts": ["one", "two"],
                "warnings": ["careful"]
            }
        });

        let facts = project_preflight_facts(&value);

        assert!(facts.json_provided);
        assert_eq!(facts.queue_active, Some(true));
        assert_eq!(facts.queue_continuation_required, Some(false));
        assert_eq!(facts.queue_drainable_head_count, Some(2));
        assert_eq!(facts.queue_prompts, vec!["one", "two"]);
        assert_eq!(facts.warnings, vec!["careful"]);
    }

    #[test]
    fn projects_session_check_facts_with_message_fallbacks() {
        let reason = json!({
            "session_check": {
                "ok": false,
                "status": "interrupted",
                "reason": "needs closeout",
                "warnings": ["pending"]
            }
        });
        let detail = json!({
            "session_check": {
                "ok": false,
                "status": "interrupted",
                "detail": "detail fallback"
            }
        });

        let reason_facts = project_session_check_facts(&reason);
        let detail_facts = project_session_check_facts(&detail);

        assert!(reason_facts.json_provided);
        assert_eq!(reason_facts.ok, Some(false));
        assert_eq!(reason_facts.status.as_deref(), Some("interrupted"));
        assert_eq!(reason_facts.message.as_deref(), Some("needs closeout"));
        assert_eq!(reason_facts.warnings, vec!["pending"]);
        assert_eq!(detail_facts.message.as_deref(), Some("detail fallback"));
    }
}
