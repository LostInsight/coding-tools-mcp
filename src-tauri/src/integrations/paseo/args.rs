use serde_json::Value;

use super::model::PaseoError;

pub(super) fn required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str, PaseoError> {
    string_arg(args, key)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| PaseoError::argument(format!("{key} is required")))
}

pub(super) fn string_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).map(str::trim)
}

pub(super) fn bounded_string_arg<'a>(
    args: &'a Value,
    key: &str,
    max_chars: usize,
) -> Result<Option<&'a str>, PaseoError> {
    let value = match args.get(key) {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(value)) => value.trim(),
        Some(_) => return Err(PaseoError::argument(format!("{key} must be a string"))),
    };
    if value.chars().count() > max_chars {
        return Err(PaseoError::argument(format!("{key} is too long")));
    }
    Ok(Some(value))
}

pub(super) fn bool_arg(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(default)
}

pub(super) fn usize_arg(
    args: &Value,
    key: &str,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize, PaseoError> {
    let value = match args.get(key) {
        None | Some(Value::Null) => default,
        Some(value) => value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| PaseoError::argument(format!("{key} must be an unsigned integer")))?,
    };
    (min..=max)
        .contains(&value)
        .then_some(value)
        .ok_or_else(|| PaseoError::argument(format!("{key} is out of range")))
}

pub(super) fn u32_arg(
    args: &Value,
    key: &str,
    default: u32,
    min: u32,
    max: u32,
) -> Result<u32, PaseoError> {
    let value = match args.get(key) {
        None | Some(Value::Null) => default,
        Some(value) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| PaseoError::argument(format!("{key} must be an unsigned integer")))?,
    };
    (min..=max)
        .contains(&value)
        .then_some(value)
        .ok_or_else(|| PaseoError::argument(format!("{key} is out of range")))
}

pub(super) fn string_array(
    args: &Value,
    key: &str,
    max_items: usize,
    max_chars: usize,
) -> Result<Vec<String>, PaseoError> {
    let values = match args.get(key) {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(values)) => values,
        Some(_) => return Err(PaseoError::argument(format!("{key} must be an array"))),
    };
    if values.len() > max_items {
        return Err(PaseoError::argument(format!(
            "{key} contains too many values"
        )));
    }
    values
        .iter()
        .map(|value| {
            let value = value
                .as_str()
                .ok_or_else(|| PaseoError::argument(format!("{key} must contain strings")))?;
            if value.chars().count() > max_chars {
                return Err(PaseoError::argument(format!("{key} value is too long")));
            }
            Ok(value.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn numeric_arguments_cannot_wrap_into_an_allowed_range() {
        let args = json!({"tail": u64::from(u32::MAX) + 31});
        assert_eq!(
            u32_arg(&args, "tail", 30, 1, 100).unwrap_err().code,
            "PASEO_ARGUMENT_INVALID"
        );
    }

    #[test]
    fn string_filters_are_bounded_in_policy_code() {
        let args = json!({"patterns": ["x".repeat(257)]});
        assert_eq!(
            string_array(&args, "patterns", 20, 256).unwrap_err().code,
            "PASEO_ARGUMENT_INVALID"
        );
        assert_eq!(
            bounded_string_arg(&json!({"path": "x".repeat(4097)}), "path", 4096)
                .unwrap_err()
                .code,
            "PASEO_ARGUMENT_INVALID"
        );
    }
}
