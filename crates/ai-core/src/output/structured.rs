//! Structured (JSON) output parsing and contract validation.
//!
//! # Scope
//!
//! [`validate_json`] implements a deliberately small, documented subset of JSON
//! Schema — enough to guarantee the shape a host declared without pulling a full
//! validator (and its dependency tree) into the core:
//!
//! * `type` (string or array of strings)
//! * `enum`
//! * `required`
//! * `properties`
//! * `items`
//! * `additionalProperties: false`
//!
//! Unknown keywords are ignored, and a non-object schema is treated as
//! "anything goes": the validator never rejects a document for a feature it does
//! not understand.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{CoreError, CoreResult};
use crate::provider::types::ResponseFormat;

/// Parsed (and optionally validated) structured output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuredOutput {
    /// Text exactly as the model produced it.
    pub raw: String,
    /// Parsed JSON value.
    pub value: Value,
    /// Schema name declared on the request, when any.
    pub schema_name: Option<String>,
    /// Whether the value was checked against a declared schema.
    pub validated: bool,
}

/// Parses JSON output and validates it against the request's schema.
///
/// # Errors
///
/// * [`CoreError::Serialization`] when the text is not valid JSON (source kept).
/// * [`CoreError::InvalidArgument`] when the value violates the declared schema,
///   or when the request did not ask for JSON at all.
pub fn parse_structured(raw: &str, format: &ResponseFormat) -> CoreResult<StructuredOutput> {
    let value: Value = serde_json::from_str(raw).map_err(|error| {
        CoreError::serialization_with_source("the model output is not valid JSON", error)
    })?;

    match format {
        ResponseFormat::Text => Err(CoreError::invalid_argument(
            "output.format",
            "the request did not ask for JSON output",
        )),
        ResponseFormat::JsonObject => Ok(StructuredOutput {
            raw: raw.to_owned(),
            value,
            schema_name: None,
            validated: false,
        }),
        ResponseFormat::JsonSchema { name, schema } => {
            validate_json(&value, schema)?;
            Ok(StructuredOutput {
                raw: raw.to_owned(),
                value,
                schema_name: Some(name.clone()),
                validated: true,
            })
        }
    }
}

/// Validates `value` against the supported JSON Schema subset.
///
/// # Errors
///
/// Returns [`CoreError::InvalidArgument`] naming the offending JSON path.
pub fn validate_json(value: &Value, schema: &Value) -> CoreResult<()> {
    validate_at(value, schema, "$")
}

fn validate_at(value: &Value, schema: &Value, path: &str) -> CoreResult<()> {
    let Some(object) = schema.as_object() else {
        return Ok(());
    };

    if let Some(expected) = object.get("type") {
        if !type_matches(value, expected) {
            return Err(contract_error(path, format!("must be of type {expected}")));
        }
    }

    if let Some(options) = object.get("enum").and_then(Value::as_array) {
        if !options.iter().any(|candidate| candidate == value) {
            return Err(contract_error(path, "is not one of the allowed values"));
        }
    }

    let properties = object.get("properties").and_then(Value::as_object);
    if let Some(map) = value.as_object() {
        if let Some(required) = object.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !map.contains_key(key) {
                    return Err(contract_error(
                        &format!("{path}.{key}"),
                        "is required but missing",
                    ));
                }
            }
        }
        if let Some(properties) = properties {
            for (key, subschema) in properties {
                if let Some(child) = map.get(key) {
                    validate_at(child, subschema, &format!("{path}.{key}"))?;
                }
            }
        }
        if object
            .get("additionalProperties")
            .and_then(Value::as_bool)
            .is_some_and(|allowed| !allowed)
        {
            for key in map.keys() {
                let declared = properties.is_some_and(|properties| properties.contains_key(key));
                if !declared {
                    return Err(contract_error(
                        &format!("{path}.{key}"),
                        "is not declared by the schema",
                    ));
                }
            }
        }
    }

    if let (Some(items), Some(array)) = (object.get("items"), value.as_array()) {
        for (index, child) in array.iter().enumerate() {
            validate_at(child, items, &format!("{path}[{index}]"))?;
        }
    }

    Ok(())
}

fn type_matches(value: &Value, expected: &Value) -> bool {
    match expected {
        Value::String(name) => matches_type(value, name),
        Value::Array(names) => names
            .iter()
            .filter_map(Value::as_str)
            .any(|name| matches_type(value, name)),
        _ => true,
    }
}

fn matches_type(value: &Value, name: &str) -> bool {
    match name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        // Unknown type names do not reject the document.
        _ => true,
    }
}

fn contract_error(path: &str, message: impl AsRef<str>) -> CoreError {
    CoreError::invalid_argument(
        format!("output.schema[{path}]"),
        message.as_ref().to_owned(),
    )
}
