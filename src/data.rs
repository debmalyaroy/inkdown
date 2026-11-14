//! Structured data: parse JSON / YAML / TOML into one tree model and convert between them.

use serde_json::{json, Map, Value};

pub struct ParseError {
    pub message: String,
    /// 1-based line of the problem, when the parser reports one.
    pub line: Option<usize>,
}

fn toml_to_json(v: toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => json!(i),
        toml::Value::Float(f) => serde_json::Number::from_f64(f).map(Value::Number).unwrap_or_else(|| Value::String(f.to_string())),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(d) => Value::String(d.to_string()),
        toml::Value::Array(a) => Value::Array(a.into_iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => Value::Object(t.into_iter().map(|(k, v)| (k, toml_to_json(v))).collect::<Map<_, _>>()),
    }
}

pub fn parse(format: &str, text: &str) -> Result<Value, ParseError> {
    match format {
        "json" => serde_json::from_str(text).map_err(|e| ParseError { message: e.to_string(), line: Some(e.line()) }),
        "yaml" => {
            use serde::Deserialize;
            let mut docs = Vec::new();
            for de in serde_norway::Deserializer::from_str(text) {
                let v = Value::deserialize(de).map_err(|e| ParseError {
                    line: e.location().map(|l| l.line()),
                    message: e.to_string(),
                })?;
                docs.push(v);
            }
            Ok(match docs.len() {
                0 => Value::Null,
                1 => docs.remove(0),
                _ => Value::Array(docs),
            })
        }
        "toml" => match text.parse::<toml::Table>() {
            Ok(t) => Ok(toml_to_json(toml::Value::Table(t))),
            Err(e) => {
                let line = e.span().map(|s| text[..s.start.min(text.len())].matches('\n').count() + 1);
                Err(ParseError { message: e.message().to_string(), line })
            }
        },
        other => Err(ParseError { message: format!("unsupported data format: {other}"), line: None }),
    }
}

pub fn emit(format: &str, value: &Value, compact: bool) -> Result<String, String> {
    match format {
        "json" if compact => serde_json::to_string(value).map_err(|e| e.to_string()),
        "json" => serde_json::to_string_pretty(value).map(|s| s + "\n").map_err(|e| e.to_string()),
        "yaml" => serde_norway::to_string(value).map_err(|e| e.to_string()),
        "toml" => {
            if !value.is_object() {
                return Err("TOML needs a table (object) at the top level.".into());
            }
            toml::to_string_pretty(value).map_err(|e| format!("Cannot be written as TOML: {e}"))
        }
        other => Err(format!("unsupported data format: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_between_formats() {
        let v = parse("toml", "title = \"x\"\n[owner]\nname = \"a\"\nports = [1, 2]\nwhen = 2026-01-02T03:04:05Z\n").ok().unwrap();
        assert_eq!(v["owner"]["ports"][1], 2);
        assert_eq!(v["owner"]["when"], "2026-01-02T03:04:05Z");
        let yaml = emit("yaml", &v, false).unwrap();
        let back = parse("yaml", &yaml).ok().unwrap();
        assert_eq!(back, v);
        let json = emit("json", &v, false).unwrap();
        assert!(json.find("title").unwrap() < json.find("owner").unwrap(), "key order kept");
        assert!(emit("toml", &json!([1, 2]), false).is_err());
        assert_eq!(parse("yaml", "a: 1\n---\nb: 2\n").ok().unwrap().as_array().unwrap().len(), 2);
        let err = parse("json", "{\n  \"a\": 1,\n  oops\n}").err().unwrap();
        assert_eq!(err.line, Some(3));
    }
}
