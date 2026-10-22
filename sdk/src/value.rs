use serde_json::Value;

static NULL: Value = Value::Null;

/// Short reads of loosely typed JSON from the server: a missing or mistyped
/// field reads as empty instead of an error.
pub trait ValueExt {
    /// Field `key`, or `null`.
    fn at(&self, key: &str) -> &Value;
    /// Text field, or `""`.
    fn str(&self, key: &str) -> &str;
    fn opt_str(&self, key: &str) -> Option<&str>;
    fn i64(&self, key: &str) -> Option<i64>;
    fn f64(&self, key: &str) -> Option<f64>;
    fn bool(&self, key: &str) -> bool;
    /// Array field, or an empty slice.
    fn list(&self, key: &str) -> &[Value];
    /// Truthiness as in Python: `null`, `false`, `0`, `""`, `[]` and `{}` are false.
    fn truthy(&self) -> bool;
}

impl ValueExt for Value {
    fn at(&self, key: &str) -> &Value {
        self.get(key).unwrap_or(&NULL)
    }

    fn str(&self, key: &str) -> &str {
        self.opt_str(key).unwrap_or("")
    }

    fn opt_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    fn i64(&self, key: &str) -> Option<i64> {
        let value = self.get(key)?;
        value
            .as_i64()
            .or_else(|| value.as_f64().map(|number| number as i64))
    }

    fn f64(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Value::as_f64)
    }

    fn bool(&self, key: &str) -> bool {
        self.get(key).and_then(Value::as_bool).unwrap_or(false)
    }

    fn list(&self, key: &str) -> &[Value] {
        self.get(key)
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(value) => *value,
            Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
            Value::String(text) => !text.is_empty(),
            Value::Array(items) => !items.is_empty(),
            Value::Object(fields) => !fields.is_empty(),
        }
    }
}
