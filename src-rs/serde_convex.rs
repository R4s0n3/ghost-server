use serde::de::Deserializer;
use serde::{de, Deserialize};
use serde_json::Value;

pub fn de_i64_from_number<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    value_to_i64(&value).map_err(de::Error::custom)
}

pub fn de_opt_i64_from_number<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(Value::Null) => Ok(None),
        Some(value) => value_to_i64(&value).map(Some).map_err(de::Error::custom),
    }
}

fn value_to_i64(value: &Value) -> Result<i64, String> {
    match value {
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                return Ok(value);
            }
            if let Some(value) = number.as_u64() {
                return i64::try_from(value)
                    .map_err(|_| format!("u64 out of range for i64: {value}"));
            }
            if let Some(value) = number.as_f64() {
                return f64_to_i64(value);
            }
            Err(format!("unsupported JSON number: {number}"))
        }
        Value::String(text) => {
            if let Ok(value) = text.parse::<i64>() {
                return Ok(value);
            }
            if let Ok(value) = text.parse::<f64>() {
                return f64_to_i64(value);
            }
            Err(format!("invalid numeric string: {text}"))
        }
        other => Err(format!("expected number, got {other}")),
    }
}

fn f64_to_i64(value: f64) -> Result<i64, String> {
    if !value.is_finite() {
        return Err(format!("non-finite float: {value}"));
    }
    if value.fract() != 0.0 {
        return Err(format!(
            "non-integer float cannot be converted to i64: {value}"
        ));
    }
    if value < i64::MIN as f64 || value > i64::MAX as f64 {
        return Err(format!("float out of i64 range: {value}"));
    }
    Ok(value as i64)
}
