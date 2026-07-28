//! Node-keyed document patch C ABI.

use super::{FfiPatchResult, ffi_patch_err, ffi_patch_ok};
use std::ffi::{CStr, c_char};

#[derive(Debug, serde::Deserialize)]
struct IpcNodePatchJson {
    component: String,
    node_key: String,
    op: String,
    content: Option<String>,
    expected_content: Option<String>,
    before: Option<String>,
    after: Option<String>,
    #[serde(default)]
    order: Vec<String>,
}

fn parse_node_patch_op(
    op: &str,
) -> Result<agent_doc_markdown_ast::mutations::MutationNodePatchOp, String> {
    use agent_doc_markdown_ast::mutations::MutationNodePatchOp;
    match op {
        "insert" => Ok(MutationNodePatchOp::Insert),
        "remove" => Ok(MutationNodePatchOp::Remove),
        "replace" => Ok(MutationNodePatchOp::Replace),
        "move" => Ok(MutationNodePatchOp::Move),
        "strike" => Ok(MutationNodePatchOp::Strike),
        "unstrike" => Ok(MutationNodePatchOp::Unstrike),
        other => Err(format!("unsupported node patch op `{other}`")),
    }
}

fn parse_node_patches_json(
    raw: &str,
) -> Result<Vec<agent_doc_markdown_ast::mutations::MutationNodePatch>, String> {
    serde_json::from_str::<Vec<IpcNodePatchJson>>(raw)
        .map_err(|err| format!("invalid node_patches JSON: {err}"))?
        .into_iter()
        .map(|patch| {
            Ok(agent_doc_markdown_ast::mutations::MutationNodePatch {
                component: patch.component,
                node_key: patch.node_key,
                op: parse_node_patch_op(&patch.op)?,
                content: patch.content,
                expected_content: patch.expected_content,
                before: patch.before,
                after: patch.after,
                order: patch.order,
            })
        })
        .collect()
}

/// Apply node-keyed patches to a live editor document snapshot.
///
/// # Safety
///
/// Both pointers must reference valid, NUL-terminated UTF-8 strings. Returned
/// pointers must be freed with `agent_doc_free_string`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_apply_node_patches(
    doc: *const c_char,
    node_patches_json: *const c_char,
) -> FfiPatchResult {
    if doc.is_null() {
        return ffi_patch_err("doc pointer is null");
    }
    if node_patches_json.is_null() {
        return ffi_patch_err("node_patches_json pointer is null");
    }
    let doc = match unsafe { CStr::from_ptr(doc) }.to_str() {
        Ok(value) => value,
        Err(err) => return ffi_patch_err(&format!("doc is not valid UTF-8: {err}")),
    };
    let raw_patches = match unsafe { CStr::from_ptr(node_patches_json) }.to_str() {
        Ok(value) => value,
        Err(err) => {
            return ffi_patch_err(&format!("node_patches_json is not valid UTF-8: {err}"));
        }
    };
    let patches = match parse_node_patches_json(raw_patches) {
        Ok(value) => value,
        Err(err) => return ffi_patch_err(&err),
    };
    match agent_doc_markdown_ast::mutations::apply_node_patches(doc, &patches) {
        Ok(updated) => ffi_patch_ok(updated),
        Err(err) => ffi_patch_err(&err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_doc_free_string;
    use std::ffi::{CStr, CString};

    #[test]
    fn preserves_live_buffer_drift() {
        let doc = CString::new(
            "operator note\n<!-- agent:queue -->\n- do [#alpha]\n- do [#beta]\n- live buffer addition\n<!-- /agent:queue -->\n",
        )
        .unwrap();
        let patches = CString::new(
            r#"[
{"component":"queue","node_key":"queue:0:beta:0","op":"strike"},
{"component":"queue","node_key":"queue:0:gamma:0","op":"insert","content":"- do [#gamma]\n","after":"queue:0:beta:0"}
]"#,
        )
        .unwrap();

        let result = unsafe { agent_doc_apply_node_patches(doc.as_ptr(), patches.as_ptr()) };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(text.contains("operator note\n"));
        assert!(text.contains("- live buffer addition\n"));
        assert!(text.contains("- ~~do [#beta]~~\n- do [#gamma]\n"));
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn rejects_unknown_op() {
        let doc =
            CString::new("<!-- agent:queue -->\n- do [#alpha]\n<!-- /agent:queue -->\n").unwrap();
        let patches =
            CString::new(r#"[{"component":"queue","node_key":"queue:0:alpha:0","op":"unknown"}]"#)
                .unwrap();

        let result = unsafe { agent_doc_apply_node_patches(doc.as_ptr(), patches.as_ptr()) };
        assert!(result.text.is_null());
        assert!(!result.error.is_null());
        let error = unsafe { CStr::from_ptr(result.error) }.to_str().unwrap();
        assert!(error.contains("unsupported node patch op"));
        unsafe { agent_doc_free_string(result.error) };
    }
}
