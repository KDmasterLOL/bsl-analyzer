use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct OverlayError(pub(crate) String);

impl fmt::Display for OverlayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for OverlayError {}

#[derive(Debug)]
pub(crate) struct MethodParameterOverride {
    pub(crate) canonical_type: String,
    pub(crate) russian_name: String,
    pub(crate) english_name: String,
    pub(crate) min_version: Option<Vec<u32>>,
    pub(crate) max_version: Option<Vec<u32>>,
    pub(crate) parameter_index: usize,
    pub(crate) replacement_type_list: Vec<String>,
}

/// A standard property the extracted help archive omits or files under a
/// misleading name (e.g. the managed-form `РежимОткрытияОкна`, whose help page
/// is stored as the enum type `FormWindowOpeningMode`). The addition attaches
/// the property to `canonical_type` so name resolution and the unused-local
/// check recognise it.
#[derive(Debug)]
pub(crate) struct TypePropertyAddition {
    pub(crate) canonical_type: String,
    pub(crate) russian_name: String,
    pub(crate) english_name: String,
    pub(crate) property_types: Vec<String>,
    pub(crate) is_readonly: bool,
    pub(crate) min_version: Option<String>,
}

const OVERLAY_ROOT_FIELDS: &[&str] =
    &["schema_version", "method_parameter_overrides", "type_property_additions"];

pub(crate) fn parse_overrides(
    overlay_source: &str,
) -> Result<Vec<MethodParameterOverride>, OverlayError> {
    let root: Value = serde_json::from_str(overlay_source)
        .map_err(|error| OverlayError(format!("malformed overlay JSON: {error}")))?;
    let object = root
        .as_object()
        .ok_or_else(|| OverlayError("overlay root must be an object".to_owned()))?;
    require_only_fields(object, OVERLAY_ROOT_FIELDS, "overlay root")?;
    validate_schema_version(object)?;
    let entries =
        object.get("method_parameter_overrides").and_then(Value::as_array).ok_or_else(|| {
            OverlayError("overlay method_parameter_overrides must be an array".to_owned())
        })?;

    entries.iter().enumerate().map(|(index, entry)| parse_override(index, entry)).collect()
}

/// Parses the optional `type_property_additions` section. An absent section
/// yields no additions; the root schema is validated the same way as
/// [`parse_overrides`].
pub(crate) fn parse_property_additions(
    overlay_source: &str,
) -> Result<Vec<TypePropertyAddition>, OverlayError> {
    let root: Value = serde_json::from_str(overlay_source)
        .map_err(|error| OverlayError(format!("malformed overlay JSON: {error}")))?;
    let object = root
        .as_object()
        .ok_or_else(|| OverlayError("overlay root must be an object".to_owned()))?;
    require_only_fields(object, OVERLAY_ROOT_FIELDS, "overlay root")?;
    validate_schema_version(object)?;
    let entries = match object.get("type_property_additions") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(value) => value.as_array().ok_or_else(|| {
            OverlayError("overlay type_property_additions must be an array".to_owned())
        })?,
    };

    entries.iter().enumerate().map(|(index, entry)| parse_property_addition(index, entry)).collect()
}

fn validate_schema_version(object: &Map<String, Value>) -> Result<(), OverlayError> {
    let schema_version = object
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| OverlayError("overlay schema_version must be the number 1".to_owned()))?;
    if schema_version != 1 {
        return Err(OverlayError(format!(
            "unsupported overlay schema_version {schema_version}; expected 1"
        )));
    }
    Ok(())
}

fn parse_property_addition(
    index: usize,
    entry: &Value,
) -> Result<TypePropertyAddition, OverlayError> {
    let object = entry
        .as_object()
        .ok_or_else(|| OverlayError(format!("type property addition {index} must be an object")))?;
    require_only_fields(
        object,
        &[
            "canonical_type",
            "russian_name",
            "english_name",
            "property_types",
            "is_readonly",
            "min_version",
            "evidence_source",
            "rationale",
        ],
        &format!("type property addition {index}"),
    )?;
    let canonical_type = required_string(object, "canonical_type", index)?;
    let russian_name = required_string(object, "russian_name", index)?;
    let english_name = required_string(object, "english_name", index)?;
    let property_types = object
        .get("property_types")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            OverlayError(format!("type property addition {index} property_types must be an array"))
        })?
        .iter()
        .map(|value| {
            value.as_str().filter(|name| !name.trim().is_empty()).map(str::to_owned).ok_or_else(
                || {
                    OverlayError(format!(
                        "type property addition {index} property_types must contain non-empty strings"
                    ))
                },
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    if property_types.is_empty() {
        return Err(OverlayError(format!(
            "type property addition {index} property_types must not be empty"
        )));
    }
    let unique_types: HashSet<&str> = property_types.iter().map(String::as_str).collect();
    if unique_types.len() != property_types.len() {
        return Err(OverlayError(format!(
            "type property addition {index} property_types contains duplicate types"
        )));
    }
    let is_readonly = match object.get("is_readonly") {
        None | Some(Value::Null) => false,
        Some(value) => value.as_bool().ok_or_else(|| {
            OverlayError(format!("type property addition {index} is_readonly must be a boolean"))
        })?,
    };
    let min_version = match object.get("min_version") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => {
            if parse_version(value).is_none() {
                return Err(OverlayError(format!(
                    "type property addition {index} min_version must be a dotted numeric version"
                )));
            }
            Some(value.clone())
        }
        Some(_) => {
            return Err(OverlayError(format!(
            "type property addition {index} min_version must be a dotted numeric version or null"
        )))
        }
    };
    required_string(object, "evidence_source", index)?;
    required_string(object, "rationale", index)?;

    Ok(TypePropertyAddition {
        canonical_type,
        russian_name,
        english_name,
        property_types,
        is_readonly,
        min_version,
    })
}

pub(crate) fn validate_version_bounds(
    method: &Value,
    overlay: &MethodParameterOverride,
) -> Result<(), OverlayError> {
    if overlay.min_version.is_none() && overlay.max_version.is_none() {
        return Ok(());
    }
    let target_version =
        method.get("min_version").and_then(Value::as_str).and_then(parse_version).ok_or_else(
            || {
                OverlayError(format!(
                    "overlay target {}.{} has no valid minimum version",
                    overlay.canonical_type, overlay.english_name
                ))
            },
        )?;
    let below_minimum =
        overlay.min_version.as_ref().is_some_and(|min_version| target_version < *min_version);
    let above_maximum =
        overlay.max_version.as_ref().is_some_and(|max_version| target_version > *max_version);
    if below_minimum || above_maximum {
        return Err(OverlayError(format!(
            "overlay target {}.{} version is outside its declared bounds",
            overlay.canonical_type, overlay.english_name
        )));
    }
    Ok(())
}

fn parse_override(index: usize, entry: &Value) -> Result<MethodParameterOverride, OverlayError> {
    let object = entry
        .as_object()
        .ok_or_else(|| OverlayError(format!("overlay entry {index} must be an object")))?;
    require_only_fields(
        object,
        &[
            "canonical_type",
            "russian_name",
            "english_name",
            "min_version",
            "max_version",
            "parameter_index",
            "replacement_type_list",
            "evidence_source",
            "rationale",
        ],
        &format!("overlay entry {index}"),
    )?;
    let canonical_type = required_string(object, "canonical_type", index)?;
    let russian_name = required_string(object, "russian_name", index)?;
    let english_name = required_string(object, "english_name", index)?;
    let min_version = optional_version(object, "min_version", index)?;
    let max_version = optional_version(object, "max_version", index)?;
    if min_version.as_ref().zip(max_version.as_ref()).is_some_and(|(min, max)| min > max) {
        return Err(OverlayError(format!(
            "overlay entry {index} has min_version after max_version"
        )));
    }
    let parameter_index = object
        .get("parameter_index")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| {
            OverlayError(format!("overlay entry {index} parameter_index must be an integer"))
        })?;
    let replacement_type_list = object
        .get("replacement_type_list")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            OverlayError(format!("overlay entry {index} replacement_type_list must be an array"))
        })?
        .iter()
        .map(|value| {
            value.as_str().filter(|name| !name.trim().is_empty()).map(str::to_owned).ok_or_else(
                || {
                    OverlayError(format!(
                    "overlay entry {index} replacement_type_list must contain non-empty strings"
                ))
                },
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    if replacement_type_list.is_empty() {
        return Err(OverlayError(format!(
            "overlay entry {index} replacement_type_list must not be empty"
        )));
    }
    let unique_types: HashSet<&str> = replacement_type_list.iter().map(String::as_str).collect();
    if unique_types.len() != replacement_type_list.len() {
        return Err(OverlayError(format!(
            "overlay entry {index} replacement_type_list contains duplicate types"
        )));
    }
    required_string(object, "evidence_source", index)?;
    required_string(object, "rationale", index)?;

    Ok(MethodParameterOverride {
        canonical_type,
        russian_name,
        english_name,
        min_version,
        max_version,
        parameter_index,
        replacement_type_list,
    })
}

fn require_only_fields(
    object: &Map<String, Value>,
    allowed_fields: &[&str],
    context: &str,
) -> Result<(), OverlayError> {
    if let Some(field) = object.keys().find(|field| !allowed_fields.contains(&field.as_str())) {
        return Err(OverlayError(format!("{context} has unknown field {field}")));
    }
    Ok(())
}

fn required_string(
    object: &Map<String, Value>,
    field: &str,
    entry_index: usize,
) -> Result<String, OverlayError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            OverlayError(format!("overlay entry {entry_index} {field} must be a non-empty string"))
        })
}

fn optional_version(
    object: &Map<String, Value>,
    field: &str,
    entry_index: usize,
) -> Result<Option<Vec<u32>>, OverlayError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => parse_version(value).map(Some).ok_or_else(|| {
            OverlayError(format!(
                "overlay entry {entry_index} {field} must be a dotted numeric version"
            ))
        }),
        Some(_) => Err(OverlayError(format!(
            "overlay entry {entry_index} {field} must be a dotted numeric version or null"
        ))),
    }
}

fn parse_version(value: &str) -> Option<Vec<u32>> {
    let components = value.split('.').map(str::parse).collect::<Result<Vec<u32>, _>>().ok()?;
    (!components.is_empty()).then_some(components)
}
