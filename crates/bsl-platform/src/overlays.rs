#[path = "overlay_schema.rs"]
mod overlay_schema;

use overlay_schema::{
    parse_overrides, validate_version_bounds, MethodParameterOverride, OverlayError,
};
use serde_json::Value;
use std::collections::HashSet;

pub(crate) fn apply_method_parameter_overlays(
    data: &mut Value,
    overlay_source: &str,
) -> Result<(), OverlayError> {
    let overrides = parse_overrides(overlay_source)?;
    let methods = data
        .get_mut("methods")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| OverlayError("base platform data has no methods array".to_owned()))?;
    let mut overridden_parameters = HashSet::new();

    for overlay in overrides {
        let ru_matches =
            method_matches(methods, &overlay.canonical_type, "name", &overlay.russian_name);
        let en_matches =
            method_matches(methods, &overlay.canonical_type, "english_name", &overlay.english_name);
        let method_index = resolve_method_index(&overlay, &ru_matches, &en_matches)?;
        let parameter_key = (overlay.canonical_type.clone(), method_index, overlay.parameter_index);
        if !overridden_parameters.insert(parameter_key) {
            return Err(OverlayError(format!(
                "duplicate override for {}.{} parameter {}",
                overlay.canonical_type, overlay.english_name, overlay.parameter_index
            )));
        }

        let method = &mut methods[method_index];
        validate_version_bounds(method, &overlay)?;
        let parameters =
            method.get_mut("parameters").and_then(Value::as_array_mut).ok_or_else(|| {
                OverlayError(format!(
                    "overlay target {}.{} has no parameters array",
                    overlay.canonical_type, overlay.english_name
                ))
            })?;
        let parameter = parameters.get_mut(overlay.parameter_index).ok_or_else(|| {
            OverlayError(format!(
                "overlay target {}.{} has no parameter at index {}",
                overlay.canonical_type, overlay.english_name, overlay.parameter_index
            ))
        })?;
        let parameter_object = parameter.as_object_mut().ok_or_else(|| {
            OverlayError(format!(
                "overlay target {}.{} parameter {} is not an object",
                overlay.canonical_type, overlay.english_name, overlay.parameter_index
            ))
        })?;
        parameter_object.insert(
            "param_type".to_owned(),
            Value::String(overlay.replacement_type_list.join(", ")),
        );
    }

    Ok(())
}

fn method_matches(
    methods: &[Value],
    canonical_type: &str,
    name_field: &str,
    name: &str,
) -> Vec<usize> {
    methods
        .iter()
        .enumerate()
        .filter_map(|(index, method)| {
            let object = method.as_object()?;
            let type_name = object.get("type_name")?.as_str()?;
            let method_name = object.get(name_field)?.as_str()?;
            (type_name == canonical_type && method_name.to_lowercase() == name.to_lowercase())
                .then_some(index)
        })
        .collect()
}

fn resolve_method_index(
    overlay: &MethodParameterOverride,
    ru_matches: &[usize],
    en_matches: &[usize],
) -> Result<usize, OverlayError> {
    match (ru_matches, en_matches) {
        ([ru_index], [en_index]) if ru_index == en_index => Ok(*ru_index),
        ([ru_index], [en_index]) => Err(OverlayError(format!(
            "overlay aliases for {}.{} resolve to different methods ({ru_index} and {en_index})",
            overlay.canonical_type, overlay.english_name
        ))),
        _ => Err(OverlayError(format!(
            "overlay target {}.{} is missing or ambiguous",
            overlay.canonical_type, overlay.english_name
        ))),
    }
}

#[cfg(test)]
#[path = "overlays_tests.rs"]
mod tests;
