use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use jsonschema::Validator;
use serde_json::Value;

use crate::types::{ErrorCategory, Failure, Plane};

pub struct Schemas {
    control: Validator,
    scenario: Validator,
    result: Validator,
    distributed: Validator,
    snapshot: Validator,
    delta: Validator,
    reliable_sync: Validator,
}

impl Schemas {
    pub fn load(interop_root: &Path, spec_root: &Path) -> Result<Self> {
        let control = compile_schema(&read_json(
            &interop_root.join("schemas/control.schema.json"),
        )?)?;
        let scenario = compile_schema(&read_json(
            &interop_root.join("schemas/scenario.schema.json"),
        )?)?;
        let result = compile_schema(&read_json(
            &interop_root.join("schemas/result.schema.json"),
        )?)?;

        let mut distributed = read_json(&spec_root.join("schemas/distributed.json"))?;
        let defs = read_json(&spec_root.join("schemas/defs.json"))?;
        inline_shared_defs(&mut distributed, &defs)?;
        let distributed = compile_schema(&distributed)?;
        let mut snapshot = read_json(&spec_root.join("schemas/snapshot.json"))?;
        inline_shared_defs(&mut snapshot, &defs)?;
        let snapshot = compile_schema(&snapshot)?;
        let mut delta = read_json(&spec_root.join("schemas/delta.json"))?;
        inline_shared_defs(&mut delta, &defs)?;
        let delta = compile_schema(&delta)?;
        let reliable_sync =
            compile_schema(&read_json(&spec_root.join("schemas/reliable-sync.json"))?)?;

        Ok(Self {
            control,
            scenario,
            result,
            distributed,
            snapshot,
            delta,
            reliable_sync,
        })
    }

    pub fn validate_control(&self, value: &Value, culprit: &str) -> Result<(), Failure> {
        validate(
            &self.control,
            value,
            ErrorCategory::PeerProtocol,
            Plane::Harness,
            culprit,
            "control response",
        )
    }

    pub fn validate_scenario(&self, value: &Value, path: &Path) -> Result<(), Failure> {
        validate(
            &self.scenario,
            value,
            ErrorCategory::SchemaViolation,
            Plane::Harness,
            &path.display().to_string(),
            "scenario",
        )
    }

    pub fn validate_result(&self, value: &Value) -> Result<(), Failure> {
        validate(
            &self.result,
            value,
            ErrorCategory::SchemaViolation,
            Plane::Harness,
            "orchestrator",
            "result",
        )
    }

    pub fn validate_frame(&self, value: &Value, culprit: &str) -> Result<(), Failure> {
        self.validate_frame_on_plane(value, culprit, Plane::Semantic)
    }

    pub fn validate_transport_frame(&self, value: &Value, culprit: &str) -> Result<(), Failure> {
        self.validate_frame_on_plane(value, culprit, Plane::Transport)
    }

    fn validate_frame_on_plane(
        &self,
        value: &Value,
        culprit: &str,
        plane: Plane,
    ) -> Result<(), Failure> {
        let (validator, label) = if value.get("CrdtSync").is_some() {
            (&self.distributed, "CrdtSync frame")
        } else if value.get("Snapshot").is_some() {
            (&self.snapshot, "Snapshot frame")
        } else if value.get("Delta").is_some() {
            (&self.delta, "Delta frame")
        } else if value.get("ResyncRequest").is_some() || value.get("OutboxAck").is_some() {
            (&self.reliable_sync, "reliable-sync frame")
        } else {
            return Err(Failure::new(
                ErrorCategory::SchemaViolation,
                plane,
                culprit,
                "unknown externally tagged frame variant",
            ));
        };
        validate(
            validator,
            value,
            ErrorCategory::SchemaViolation,
            plane,
            culprit,
            label,
        )
    }
}

pub fn validate_schema_files(interop_root: &Path) -> Result<()> {
    for name in [
        "control.schema.json",
        "scenario.schema.json",
        "result.schema.json",
    ] {
        let path = interop_root.join("schemas").join(name);
        let schema = read_json(&path)?;
        if let Err(error) = jsonschema::draft202012::meta::validate(&schema) {
            bail!(
                "{} is not a valid Draft 2020-12 schema: {error}",
                path.display()
            );
        }
        compile_schema(&schema)
            .with_context(|| format!("compile JSON Schema {}", path.display()))?;
    }
    Ok(())
}

fn read_json(path: &Path) -> Result<Value> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse JSON {}", path.display()))
}

fn compile_schema(schema: &Value) -> Result<Validator> {
    jsonschema::draft202012::new(schema).map_err(|error| anyhow::anyhow!(error.to_string()))
}

fn validate(
    validator: &Validator,
    value: &Value,
    category: ErrorCategory,
    plane: Plane,
    culprit: &str,
    label: &str,
) -> Result<(), Failure> {
    if validator.is_valid(value) {
        return Ok(());
    }
    let details = validator
        .iter_errors(value)
        .take(3)
        .map(|error| error.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    Err(Failure::new(
        category,
        plane,
        culprit,
        format!("invalid {label}: {details}"),
    ))
}

fn inline_shared_defs(schema: &mut Value, defs: &Value) -> Result<()> {
    let shared_defs = defs
        .get("$defs")
        .and_then(Value::as_object)
        .context("defs.json has no object $defs")?;
    let schema_defs = schema
        .get_mut("$defs")
        .and_then(Value::as_object_mut)
        .context("frame schema has no object $defs")?;

    for (name, definition) in shared_defs {
        schema_defs.insert(name.clone(), definition.clone());
    }
    rewrite_shared_refs(schema);
    Ok(())
}

fn rewrite_shared_refs(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if let Some(Value::String(reference)) = object.get_mut("$ref")
                && let Some(fragment) =
                    reference.strip_prefix("https://lazily.dev/schemas/defs.json")
            {
                *reference = fragment.to_owned();
            }
            for child in object.values_mut() {
                rewrite_shared_refs(child);
            }
        }
        Value::Array(array) => {
            for child in array {
                rewrite_shared_refs(child);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inlines_shared_distributed_definitions() {
        let mut distributed = serde_json::json!({
            "$defs": {
                "NodeId": {"$ref": "https://lazily.dev/schemas/defs.json#/$defs/NodeId"},
                "PeerId": {},
                "NodeKey": {},
                "WireStamp": {},
                "IpcValue": {}
            }
        });
        let defs = serde_json::json!({
            "$defs": {
                "NodeId": {"type": "integer"},
                "PeerId": {"type": "integer"},
                "NodeKey": {"type": "string"},
                "WireStamp": {"type": "object"},
                "IpcValue": {"type": "object"}
            }
        });
        inline_shared_defs(&mut distributed, &defs).unwrap();
        assert_eq!(distributed["$defs"]["NodeId"]["type"], "integer");
    }
}
