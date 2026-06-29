use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

pub fn serve(project_root: Option<&Path>) -> Result<()> {
    if let Some(root) = project_root {
        std::env::set_current_dir(root)
            .with_context(|| format!("failed to enter MCP project root {}", root.display()))?;
    }

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line.context("failed to read MCP stdin")?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_message(&line) {
            writeln!(stdout, "{}", response).context("failed to write MCP response")?;
            stdout.flush().context("failed to flush MCP response")?;
        }
    }
    Ok(())
}

pub(crate) fn handle_message(line: &str) -> Option<Value> {
    let value = match serde_json::from_str::<Value>(line) {
        Ok(value) => value,
        Err(err) => {
            return Some(error_response(
                Value::Null,
                -32700,
                format!("parse error: {err}"),
            ));
        }
    };

    let Some(object) = value.as_object() else {
        return Some(error_response(
            Value::Null,
            -32600,
            "invalid JSON-RPC request".to_string(),
        ));
    };

    let id = object.get("id").cloned();
    let response_id = id.clone().unwrap_or(Value::Null);
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return id
            .map(|_| error_response(response_id, -32600, "missing JSON-RPC method".to_string()));
    };

    id.as_ref()?;

    let result = match method {
        "initialize" => Ok(initialize_result(object.get("params"))),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools_list_result()),
        "tools/call" => handle_tools_call(object.get("params")),
        _ => Err(McpProtocolError::new(
            -32601,
            format!("method not found: {method}"),
        )),
    };

    Some(match result {
        Ok(result) => success_response(response_id, result),
        Err(err) => error_response(response_id, err.code, err.message),
    })
}

fn initialize_result(params: Option<&Value>) -> Value {
    let protocol_version = params
        .and_then(|params| params.get("protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or(MCP_PROTOCOL_VERSION);
    let negotiated = if protocol_version == MCP_PROTOCOL_VERSION {
        protocol_version
    } else {
        MCP_PROTOCOL_VERSION
    };

    json!({
        "protocolVersion": negotiated,
        "capabilities": {
            "tools": {
                "listChanged": false
            }
        },
        "serverInfo": {
            "name": "agent-doc",
            "title": "agent-doc MCP Server",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": "Use agent_doc_admit before answering a live session prompt, then agent_doc_finalize for binary-owned closeout; do not patch session documents directly."
    })
}

fn string_property(description: Option<&str>) -> Value {
    let mut property = Map::new();
    property.insert("type".to_string(), Value::String("string".to_string()));
    if let Some(description) = description {
        property.insert(
            "description".to_string(),
            Value::String(description.to_string()),
        );
    }
    Value::Object(property)
}

fn bool_property(description: Option<&str>) -> Value {
    let mut property = Map::new();
    property.insert("type".to_string(), Value::String("boolean".to_string()));
    if let Some(description) = description {
        property.insert(
            "description".to_string(),
            Value::String(description.to_string()),
        );
    }
    Value::Object(property)
}

fn string_array_property(description: Option<&str>) -> Value {
    let mut property = Map::new();
    property.insert("type".to_string(), Value::String("array".to_string()));
    property.insert("items".to_string(), json!({ "type": "string" }));
    if let Some(description) = description {
        property.insert(
            "description".to_string(),
            Value::String(description.to_string()),
        );
    }
    Value::Object(property)
}

fn finalize_input_schema() -> Value {
    let mut properties = Map::new();
    properties.insert(
        "file".to_string(),
        string_property(Some("Path to the session document.")),
    );
    properties.insert(
        "response".to_string(),
        string_property(Some("Assistant response or template patchback body.")),
    );
    properties.insert(
        "baseline_file".to_string(),
        string_property(Some(
            "Optional baseline file from agent_doc_preflight for legacy flows.",
        )),
    );
    properties.insert(
        "template".to_string(),
        bool_property(Some("Treat the response as a template patchback body.")),
    );
    properties.insert(
        "stream".to_string(),
        bool_property(Some("Use stream-mode write semantics.")),
    );
    properties.insert(
        "ipc".to_string(),
        bool_property(Some("Use IPC-mode write semantics.")),
    );
    properties.insert(
        "done".to_string(),
        string_array_property(Some(
            "Optional backlog ids to mark done, equivalent to repeated --done.",
        )),
    );
    properties.insert(
        "pending_add".to_string(),
        string_array_property(Some(
            "Legacy name for backlog_add. Optional backlog items to add.",
        )),
    );
    properties.insert(
        "backlog_add".to_string(),
        string_array_property(Some(
            "Optional backlog items to add, equivalent to repeated --backlog-add.",
        )),
    );
    properties.insert(
        "pending_add_back".to_string(),
        string_array_property(Some(
            "Legacy name for backlog_add_back. Optional backlog items to append.",
        )),
    );
    properties.insert(
        "backlog_add_back".to_string(),
        string_array_property(Some(
            "Optional backlog items to append, equivalent to repeated --backlog-add-back.",
        )),
    );
    for key in [
        "backlog_add_to",
        "backlog_add_gated",
        "backlog_add_after",
        "backlog_add_before",
        "pending_add_to",
        "pending_add_gated",
        "pending_add_after",
        "pending_add_before",
        "icebox_add",
        "icebox_add_after",
        "icebox_add_before",
        "icebox_add_back",
        "icebox_edit",
        "backlog_edit",
        "backlog_gate",
        "backlog_ungate",
        "backlog_resolve_gate",
        "backlog_set_gate_type",
        "backlog_set_verify",
        "pending_edit",
        "pending_gate",
        "pending_ungate",
        "pending_resolve_gate",
        "pending_set_gate_type",
        "pending_set_verify",
        "review_add",
        "review_edit",
        "review_remove",
        "review_resolve",
        "commit_sibling",
        "commit_sibling_message",
    ] {
        properties.insert(key.to_string(), string_array_property(None));
    }
    properties.insert("pending_clear".to_string(), bool_property(None));
    properties.insert("backlog_clear".to_string(), bool_property(None));
    properties.insert("icebox_clear".to_string(), bool_property(None));
    properties.insert("backlog_reorder".to_string(), string_property(None));
    properties.insert("icebox_reorder".to_string(), string_property(None));
    properties.insert("pending_reorder".to_string(), string_property(None));
    properties.insert("allow_replace_pending".to_string(), bool_property(None));
    properties.insert("force_disk".to_string(), bool_property(None));
    properties.insert(
        "force_disk_operator_override".to_string(),
        string_property(Some(
            "Required when force_disk=true. Summarize the operator's explicit decision to bypass live-editor convergence.",
        )),
    );
    properties.insert("origin".to_string(), string_property(None));
    properties.insert("status".to_string(), string_property(None));

    json!({
        "type": "object",
        "properties": Value::Object(properties),
        "required": ["file", "response"]
    })
}

fn tools_list_result() -> Value {
    json!({
        "tools": [
            {
                "name": "agent_doc_read",
                "title": "Read agent-doc document",
                "description": "Read a full agent-doc markdown document or one named component body.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file": { "type": "string", "description": "Path to the session document." },
                        "component": { "type": "string", "description": "Optional component name such as exchange, queue, backlog, or status." }
                    },
                    "required": ["file"]
                }
            },
            {
                "name": "agent_doc_admit",
                "title": "Admit agent-doc response turn",
                "description": "Open a lightweight response-cycle checkpoint for a live session prompt without running legacy preflight maintenance.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file": { "type": "string", "description": "Path to the session document." }
                    },
                    "required": ["file"]
                }
            },
            {
                "name": "agent_doc_preflight",
                "title": "Run agent-doc preflight",
                "description": "Run preflight for a session document and return its JSON report plus captured diagnostics.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file": { "type": "string", "description": "Path to the session document." },
                        "probe": { "type": "boolean", "description": "When true, run preflight as a side-effect-free probe." }
                    },
                    "required": ["file"]
                }
            },
            {
                "name": "agent_doc_plan",
                "title": "Build agent-doc plan",
                "description": "Derive the structured post-preflight planning record for a session document.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file": { "type": "string", "description": "Path to the session document." }
                    },
                    "required": ["file"]
                }
            },
            {
                "name": "agent_doc_session_check",
                "title": "Check agent-doc session state",
                "description": "Inspect closeout state and active queue continuation for a session document.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file": { "type": "string", "description": "Path to the session document." }
                    },
                    "required": ["file"]
                }
            },
            {
                "name": "agent_doc_finalize",
                "title": "Finalize agent-doc response",
                "description": "Write an assistant response through the strict agent-doc finalize path and require a committed closeout.",
                "inputSchema": finalize_input_schema()
            }
        ]
    })
}

fn handle_tools_call(params: Option<&Value>) -> std::result::Result<Value, McpProtocolError> {
    let params = params
        .and_then(Value::as_object)
        .ok_or_else(|| McpProtocolError::invalid_params("tools/call requires params object"))?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| McpProtocolError::invalid_params("tools/call requires string name"))?;
    let args = match params.get("arguments") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(args)) => args.clone(),
        Some(_) => {
            return Err(McpProtocolError::invalid_params(
                "tools/call arguments must be an object",
            ));
        }
    };

    let result = match name {
        "agent_doc_read" => tool_read(&args),
        "agent_doc_admit" => tool_admit(&args),
        "agent_doc_preflight" => tool_preflight(&args),
        "agent_doc_plan" => tool_plan(&args),
        "agent_doc_session_check" => tool_session_check(&args),
        "agent_doc_finalize" => tool_finalize(&args),
        _ => {
            return Err(McpProtocolError::invalid_params(format!(
                "unknown tool: {name}"
            )));
        }
    };

    Ok(match result {
        Ok(result) => result,
        Err(err) => tool_error_result(err.to_string()),
    })
}

fn tool_read(args: &Map<String, Value>) -> Result<Value> {
    let file = required_path_arg(args, "file")?;
    let component = optional_string_arg(args, "component")?;
    let content = read_document(&file, component.as_deref())?;
    let structured = json!({
        "ok": true,
        "file": file.display().to_string(),
        "component": component,
        "content": content,
    });
    Ok(tool_success_result(content, structured))
}

fn tool_admit(args: &Map<String, Value>) -> Result<Value> {
    ensure_mcp_binary_fresh_for_mutation()?;
    let file = required_path_arg(args, "file")?;
    let admit = agent_doc_orchestration::admit::admit(&file)?;
    let structured = json!({
        "ok": true,
        "file": file.display().to_string(),
        "admission": admit,
    });
    Ok(tool_success_result(
        serde_json::to_string_pretty(&structured)?,
        structured,
    ))
}

fn tool_preflight(args: &Map<String, Value>) -> Result<Value> {
    ensure_mcp_binary_fresh_for_mutation()?;
    let file = required_path_arg(args, "file")?;
    let probe = bool_arg(args, "probe", false)?;
    let mut command =
        Command::new(std::env::current_exe().context("failed to resolve agent-doc binary")?);
    command.arg("preflight");
    if probe {
        command.arg("--probe");
    }
    command.arg(&file);
    let output = command
        .output()
        .with_context(|| format!("failed to run preflight for {}", file.display()))?;
    let stdout = String::from_utf8(output.stdout).context("preflight stdout was not UTF-8")?;
    let stderr = String::from_utf8(output.stderr).context("preflight stderr was not UTF-8")?;
    if !output.status.success() {
        bail!(
            "preflight failed for {} with status {}{}\n{}",
            file.display(),
            output.status,
            if stdout.trim().is_empty() {
                String::new()
            } else {
                format!("\nstdout:\n{}", stdout.trim_end())
            },
            stderr.trim_end()
        );
    }

    let report: Value = serde_json::from_str(stdout.trim())
        .with_context(|| format!("preflight for {} did not return JSON", file.display()))?;
    let structured = json!({
        "ok": true,
        "file": file.display().to_string(),
        "probe": probe,
        "report": report,
        "stderr": stderr,
    });
    let text = if stderr.trim().is_empty() {
        serde_json::to_string_pretty(&structured)?
    } else {
        format!(
            "{}\n{}",
            stderr.trim_end(),
            serde_json::to_string_pretty(&structured)?
        )
    };
    Ok(tool_success_result(text, structured))
}

fn tool_plan(args: &Map<String, Value>) -> Result<Value> {
    let file = required_path_arg(args, "file")?;
    let plan = crate::plan::build(&file)?;
    let structured = json!({
        "ok": true,
        "file": file.display().to_string(),
        "plan": plan,
    });
    Ok(tool_success_result(
        serde_json::to_string_pretty(&structured)?,
        structured,
    ))
}

fn tool_session_check(args: &Map<String, Value>) -> Result<Value> {
    let file = required_path_arg(args, "file")?;
    let report = agent_doc_orchestration::session_check::inspect_with_warnings(&file)?;
    let (ok, status, message) = match report.status {
        agent_doc_orchestration::session_check::SessionCheckStatus::Ok(message) => {
            (true, "ok", message)
        }
        agent_doc_orchestration::session_check::SessionCheckStatus::Interrupted(message) => {
            (false, "interrupted", message)
        }
    };
    let continuation = agent_doc_orchestration::queue_continuation::detect(&file)?;
    let structured = json!({
        "ok": ok,
        "status": status,
        "message": message,
        "warnings": report.warnings,
        "queue_continuation_required": continuation.is_some(),
        "next_queue_prompt": continuation.as_ref().map(|item| item.head_prompt.clone()),
        "next_queue_head_id": continuation.as_ref().and_then(|item| item.head_id.clone()),
        "queue_continuation_reason": continuation.as_ref().map(|item| item.reason.clone()),
    });
    Ok(tool_success_result(
        serde_json::to_string_pretty(&structured)?,
        structured,
    ))
}

fn tool_finalize(args: &Map<String, Value>) -> Result<Value> {
    ensure_mcp_binary_fresh_for_mutation()?;
    let file = required_path_arg(args, "file")?;
    let response = required_string_arg(args, "response")?;
    let origin = optional_string_arg(args, "origin")?.unwrap_or_else(|| "mcp".to_string());
    let mut pending_add = string_vec_arg(args, "backlog_add")?;
    pending_add.extend(string_vec_arg(args, "pending_add")?);
    let mut pending_add_to = string_vec_arg(args, "backlog_add_to")?;
    pending_add_to.extend(string_vec_arg(args, "pending_add_to")?);
    let mut pending_add_gated = string_vec_arg(args, "backlog_add_gated")?;
    pending_add_gated.extend(string_vec_arg(args, "pending_add_gated")?);
    let mut pending_add_after = string_vec_arg(args, "backlog_add_after")?;
    pending_add_after.extend(string_vec_arg(args, "pending_add_after")?);
    let mut pending_add_before = string_vec_arg(args, "backlog_add_before")?;
    pending_add_before.extend(string_vec_arg(args, "pending_add_before")?);
    let mut pending_add_back = string_vec_arg(args, "backlog_add_back")?;
    pending_add_back.extend(string_vec_arg(args, "pending_add_back")?);
    let mut pending_edit = string_vec_arg(args, "backlog_edit")?;
    pending_edit.extend(string_vec_arg(args, "pending_edit")?);
    let pending_clear =
        bool_arg(args, "backlog_clear", false)? || bool_arg(args, "pending_clear", false)?;
    let pending_reorder = optional_string_arg(args, "backlog_reorder")?
        .or(optional_string_arg(args, "pending_reorder")?);
    let mut pending_gate = string_vec_arg(args, "backlog_gate")?;
    pending_gate.extend(string_vec_arg(args, "pending_gate")?);
    let mut pending_ungate = string_vec_arg(args, "backlog_ungate")?;
    pending_ungate.extend(string_vec_arg(args, "pending_ungate")?);
    let mut pending_resolve_gate = string_vec_arg(args, "backlog_resolve_gate")?;
    pending_resolve_gate.extend(string_vec_arg(args, "pending_resolve_gate")?);
    let mut pending_set_gate_type = string_vec_arg(args, "backlog_set_gate_type")?;
    pending_set_gate_type.extend(string_vec_arg(args, "pending_set_gate_type")?);
    let mut pending_set_verify = string_vec_arg(args, "backlog_set_verify")?;
    pending_set_verify.extend(string_vec_arg(args, "pending_set_verify")?);
    let force_disk = bool_arg(args, "force_disk", false)?;
    if force_disk {
        let override_note =
            optional_string_arg(args, "force_disk_operator_override")?.unwrap_or_default();
        if override_note.trim().is_empty() {
            bail!(
                "force_disk_operator_override is required when force_disk=true; use force-disk only after the operator explicitly chose to bypass live-editor convergence"
            );
        }
    }
    let options = agent_doc_orchestration::write::CommandOptions {
        file: file.clone(),
        baseline_file: optional_path_arg(args, "baseline_file")?,
        is_template: bool_arg(args, "template", false)?,
        is_stream: bool_arg(args, "stream", false)?,
        is_ipc: bool_arg(args, "ipc", false)?,
        force_disk,
        origin: Some(origin),
        pending_add,
        pending_add_to,
        pending_add_gated,
        pending_add_after,
        pending_add_before,
        pending_add_back,
        icebox_add: string_vec_arg(args, "icebox_add")?,
        icebox_add_after: string_vec_arg(args, "icebox_add_after")?,
        icebox_add_before: string_vec_arg(args, "icebox_add_before")?,
        icebox_add_back: string_vec_arg(args, "icebox_add_back")?,
        icebox_edit: string_vec_arg(args, "icebox_edit")?,
        icebox_clear: bool_arg(args, "icebox_clear", false)?,
        icebox_reorder: optional_string_arg(args, "icebox_reorder")?,
        pending_done: string_vec_arg(args, "done")?,
        pending_edit,
        pending_clear,
        pending_reorder,
        pending_gate,
        pending_ungate,
        pending_resolve_gate,
        pending_set_gate_type,
        pending_set_verify,
        review_add: string_vec_arg(args, "review_add")?,
        review_edit: string_vec_arg(args, "review_edit")?,
        review_remove: string_vec_arg(args, "review_remove")?,
        review_resolve: string_vec_arg(args, "review_resolve")?,
        queue_completion_ids: Vec::new(),
        allow_replace_pending: bool_arg(args, "allow_replace_pending", false)?,
        pending_only: false,
        status: optional_string_arg(args, "status")?,
        lint_override: None,
        commit_sibling: path_vec_arg(args, "commit_sibling")?,
        commit_sibling_message: string_vec_arg(args, "commit_sibling_message")?,
    };

    agent_doc_orchestration::write::run_command_with_response(
        options,
        agent_doc_orchestration::write::CommitMode::Required,
        response,
    )?;
    let report = agent_doc_orchestration::session_check::inspect_with_warnings(&file)?;
    let (ok, status, message) = match report.status {
        agent_doc_orchestration::session_check::SessionCheckStatus::Ok(message) => {
            (true, "ok", message)
        }
        agent_doc_orchestration::session_check::SessionCheckStatus::Interrupted(message) => {
            (false, "interrupted", message)
        }
    };
    let structured = json!({
        "ok": ok,
        "status": status,
        "message": message,
        "warnings": report.warnings,
    });
    Ok(tool_success_result(
        serde_json::to_string_pretty(&structured)?,
        structured,
    ))
}

fn ensure_mcp_binary_fresh_for_mutation() -> Result<()> {
    if mcp_binary_stale_for_test() {
        bail!(stale_mcp_binary_message());
    }

    let exe = std::env::current_exe().context("failed to resolve running agent-doc MCP binary")?;
    let launchable = std::fs::metadata(&exe)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false);
    if launchable {
        return Ok(());
    }

    bail!(
        "{} running_exe={}",
        stale_mcp_binary_message(),
        exe.display()
    );
}

fn stale_mcp_binary_message() -> &'static str {
    "stale agent-doc MCP server: the running MCP process maps an agent-doc binary \
     that is no longer launchable, usually because `make install` or `cargo install` \
     replaced it. Restart/recycle the MCP server, then retry the mutation; refusing \
     to run stale admit/preflight/finalize logic against a live document."
}

#[cfg(test)]
fn mcp_binary_stale_for_test() -> bool {
    MCP_BINARY_STALE_FOR_TEST.with(|stale| stale.get())
}

#[cfg(not(test))]
fn mcp_binary_stale_for_test() -> bool {
    false
}

#[cfg(test)]
thread_local! {
    static MCP_BINARY_STALE_FOR_TEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn set_mcp_binary_stale_for_test(stale: bool) {
    MCP_BINARY_STALE_FOR_TEST.with(|value| value.set(stale));
}

fn read_document(file: &Path, component: Option<&str>) -> Result<String> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let Some(component) = component else {
        return Ok(content);
    };
    let components = agent_doc_element::element::parse(&content)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;
    let component = components
        .iter()
        .find(|item| item.name == component)
        .with_context(|| format!("component '{}' not found in {}", component, file.display()))?;
    Ok(component.content(&content).to_string())
}

fn tool_success_result(text: String, structured: Value) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": structured,
        "isError": false
    })
}

fn tool_error_result(message: String) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "structuredContent": {
            "ok": false,
            "error": message
        },
        "isError": true
    })
}

fn required_path_arg(args: &Map<String, Value>, key: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(required_string_arg(args, key)?))
}

fn optional_path_arg(args: &Map<String, Value>, key: &str) -> Result<Option<PathBuf>> {
    Ok(optional_string_arg(args, key)?.map(PathBuf::from))
}

fn required_string_arg(args: &Map<String, Value>, key: &str) -> Result<String> {
    optional_string_arg(args, key)?.with_context(|| format!("missing required argument `{key}`"))
}

fn optional_string_arg(args: &Map<String, Value>, key: &str) -> Result<Option<String>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => bail!("argument `{key}` must be a string"),
    }
}

fn bool_arg(args: &Map<String, Value>, key: &str, default: bool) -> Result<bool> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => bail!("argument `{key}` must be a boolean"),
    }
}

fn string_vec_arg(args: &Map<String, Value>, key: &str) -> Result<Vec<String>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(value)) => Ok(vec![value.clone()]),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .with_context(|| format!("argument `{key}` entries must be strings"))
            })
            .collect(),
        Some(_) => bail!("argument `{key}` must be a string or array of strings"),
    }
}

fn path_vec_arg(args: &Map<String, Value>, key: &str) -> Result<Vec<PathBuf>> {
    Ok(string_vec_arg(args, key)?
        .into_iter()
        .map(PathBuf::from)
        .collect())
}

fn success_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn error_response(id: Value, code: i64, message: String) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}

#[derive(Debug)]
struct McpProtocolError {
    code: i64,
    message: String,
}

impl McpProtocolError {
    fn new(code: i64, message: String) -> Self {
        Self { code, message }
    }

    fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(-32602, message.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response_for(request: Value) -> Value {
        handle_message(&request.to_string()).expect("request should produce response")
    }

    #[test]
    fn initialize_advertises_tools() {
        let response = response_for(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" }
            }
        }));
        assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
        assert!(response["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn tools_list_includes_agent_doc_finalize() {
        let response = response_for(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }));
        let names: Vec<&str> = response["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert!(names.contains(&"agent_doc_read"));
        assert!(names.contains(&"agent_doc_admit"));
        assert!(names.contains(&"agent_doc_preflight"));
        assert!(names.contains(&"agent_doc_plan"));
        assert!(names.contains(&"agent_doc_session_check"));
        assert!(names.contains(&"agent_doc_finalize"));
    }

    #[test]
    fn finalize_schema_advertises_supported_closeout_options() {
        let response = response_for(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }));
        let tools = response["result"]["tools"].as_array().unwrap();
        let finalize = tools
            .iter()
            .find(|tool| tool["name"].as_str() == Some("agent_doc_finalize"))
            .unwrap();
        let props = finalize["inputSchema"]["properties"].as_object().unwrap();
        for key in [
            "baseline_file",
            "done",
            "backlog_add",
            "backlog_add_to",
            "backlog_add_gated",
            "backlog_add_after",
            "backlog_add_before",
            "backlog_add_back",
            "pending_add",
            "pending_add_to",
            "pending_add_gated",
            "pending_add_after",
            "pending_add_before",
            "pending_add_back",
            "icebox_add",
            "icebox_add_after",
            "icebox_add_before",
            "icebox_add_back",
            "icebox_edit",
            "icebox_clear",
            "icebox_reorder",
            "backlog_edit",
            "backlog_clear",
            "backlog_reorder",
            "backlog_gate",
            "backlog_ungate",
            "backlog_resolve_gate",
            "backlog_set_gate_type",
            "backlog_set_verify",
            "pending_edit",
            "pending_clear",
            "pending_reorder",
            "pending_gate",
            "pending_ungate",
            "pending_resolve_gate",
            "pending_set_gate_type",
            "pending_set_verify",
            "review_add",
            "review_edit",
            "review_remove",
            "review_resolve",
            "commit_sibling",
            "commit_sibling_message",
            "force_disk_operator_override",
        ] {
            assert!(props.contains_key(key), "missing finalize schema key {key}");
        }
        assert!(
            props["force_disk_operator_override"]["description"]
                .as_str()
                .unwrap()
                .contains("operator's explicit decision"),
            "force-disk override schema must describe the operator authority boundary"
        );
    }

    #[test]
    fn finalize_rejects_force_disk_without_operator_override() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let response = response_for(json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "agent_doc_finalize",
                "arguments": {
                    "file": file.path(),
                    "response": "<!-- patch:exchange -->\n### Re: blocked — gpt-5\nbody\n<!-- /patch:exchange -->\n",
                    "force_disk": true
                }
            }
        }));
        assert_eq!(response["result"]["isError"], true);
        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("force_disk_operator_override")
                && text.contains("operator explicitly chose"),
            "MCP force-disk must require an explicit operator override note, got: {text}"
        );
    }

    #[test]
    fn read_tool_returns_named_component() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            file.path(),
            "<!-- agent:exchange -->\nbody text\n<!-- /agent:exchange -->\n",
        )
        .unwrap();
        let response = response_for(json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "agent_doc_read",
                "arguments": {
                    "file": file.path(),
                    "component": "exchange"
                }
            }
        }));
        assert_eq!(response["result"]["isError"], false);
        assert_eq!(
            response["result"]["structuredContent"]["content"],
            "body text\n"
        );
    }

    #[test]
    fn admit_tool_opens_lightweight_cycle() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let file = root.join("session.md");
        let content = "---\nagent_doc_session: sid-1\n---\n\n# Session\n\n❯ Please inspect\n";
        std::fs::write(&file, content).unwrap();

        let response = response_for(json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "agent_doc_admit",
                "arguments": { "file": file.display().to_string() }
            }
        }));

        assert_eq!(response["result"]["isError"], false);
        let admission = &response["result"]["structuredContent"]["admission"];
        assert_eq!(admission["admitted"], true);
        assert_eq!(admission["source"], "admit");
        assert_eq!(admission["maintenance_required"], false);
        assert_eq!(admission["preflight_required"], false);
        assert_eq!(admission["cycle_phase"], "preflight_started");

        let state = agent_doc_orchestration::cycle_state::load(&file)
            .unwrap()
            .unwrap();
        assert_eq!(
            state.phase,
            agent_doc_orchestration::cycle_state::CyclePhase::PreflightStarted
        );
    }

    #[test]
    fn stale_mcp_binary_blocks_mutating_tools_before_cycle_start() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let file = root.join("session.md");
        std::fs::write(
            &file,
            "---\nagent_doc_session: sid-1\n---\n\n# Session\n\n❯ Please inspect\n",
        )
        .unwrap();

        set_mcp_binary_stale_for_test(true);
        let response = response_for(json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": {
                "name": "agent_doc_admit",
                "arguments": { "file": file.display().to_string() }
            }
        }));
        set_mcp_binary_stale_for_test(false);

        assert_eq!(response["result"]["isError"], true);
        let error = response["result"]["structuredContent"]["error"]
            .as_str()
            .unwrap();
        assert!(error.contains("stale agent-doc MCP server"));
        assert!(error.to_ascii_lowercase().contains("restart"));
        assert!(
            agent_doc_orchestration::cycle_state::load(&file)
                .unwrap()
                .is_none(),
            "stale MCP mutation guard must fire before opening a response cycle"
        );
    }

    #[test]
    fn tool_errors_are_tool_results() {
        let response = response_for(json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "agent_doc_read",
                "arguments": { "file": "/tmp/agent-doc-missing-mcp-test.md" }
            }
        }));
        assert_eq!(response["result"]["isError"], true);
        assert!(
            response["result"]["structuredContent"]["error"]
                .as_str()
                .unwrap()
                .contains("failed to read")
        );
    }
}
