//! Forma MCP's tool contract, shared with Electron Forma: `mcp/tool_manifest.json` (generated from `mcp/openapi.json`),
//! and a validator for the part of JSON Schema it uses, so the bridge and the MCP server check arguments the way
//! Electron's (Ajv) do before anything reaches the editor.

use serde_json::Value;
use std::sync::OnceLock;

const MANIFEST: &str = include_str!("../../mcp/tool_manifest.json");

/// A tool's failure, as Forma MCP reports it: a stable code and a message for the assistant.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolError {
    pub code: String,
    pub message: String,
}

pub fn fail(code: &str, message: impl Into<String>) -> ToolError {
    ToolError {
        code: code.into(),
        message: message.into(),
    }
}

pub fn manifest() -> &'static Value {
    static PARSED: OnceLock<Value> = OnceLock::new();
    PARSED.get_or_init(|| {
        serde_json::from_str(MANIFEST).expect("mcp/tool_manifest.json is valid JSON")
    })
}

pub fn tools() -> &'static [Value] {
    manifest()["tools"].as_array().map_or(&[], Vec::as_slice)
}

pub fn tool(name: &str) -> Option<&'static Value> {
    tools().iter().find(|tool| tool["name"] == name)
}

/// Whether a tool only reads (changes nothing in the scene).
pub fn read_only(name: &str) -> bool {
    tool(name).is_some_and(|tool| tool["annotations"]["readOnlyHint"] == true)
}

/// Checks a tool's arguments against its input schema.
pub fn validate(name: &str, args: &Value) -> Result<(), ToolError> {
    let tool =
        tool(name).ok_or_else(|| fail("UNKNOWN_TOOL", format!("Unknown Forma tool: {name}")))?;
    let mut problems = Vec::new();
    check(&tool["inputSchema"], args, "data", &mut problems);
    if problems.is_empty() {
        Ok(())
    } else {
        Err(fail("INVALID_ARGUMENTS", problems.join("; ")))
    }
}

fn check(schema: &Value, value: &Value, at: &str, problems: &mut Vec<String>) {
    if value.is_null() && schema["nullable"] == true {
        return;
    }
    if let Some(expected) = schema.get("const") {
        if value != expected {
            return problems.push(format!("{at} must be equal to constant {expected}"));
        }
    }
    if let Some(options) = schema.get("enum").and_then(Value::as_array) {
        if !options.contains(value) {
            let allowed: Vec<String> = options.iter().map(Value::to_string).collect();
            return problems.push(format!("{at} must be one of {}", allowed.join(", ")));
        }
    }
    let number = |key: &str| schema.get(key).and_then(Value::as_f64);
    let count = |key: &str| schema.get(key).and_then(Value::as_u64).map(|n| n as usize);
    match schema["type"].as_str() {
        Some("object") => {
            let Some(map) = value.as_object() else {
                return problems.push(format!("{at} must be object"));
            };
            for name in schema["required"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if !map.contains_key(name) {
                    problems.push(format!("{at} must have required property '{name}'"));
                }
            }
            if count("minProperties").is_some_and(|least| map.len() < least) {
                problems.push(format!(
                    "{at} must have at least {} properties",
                    count("minProperties").unwrap()
                ));
            }
            for (key, item) in map {
                match schema["properties"].get(key) {
                    Some(property) => check(property, item, &format!("{at}/{key}"), problems),
                    None if schema["additionalProperties"] == false => {
                        problems.push(format!("{at} must NOT have additional property '{key}'"))
                    }
                    None => {}
                }
            }
        }
        Some("array") => {
            let Some(items) = value.as_array() else {
                return problems.push(format!("{at} must be array"));
            };
            if count("minItems").is_some_and(|least| items.len() < least) {
                problems.push(format!(
                    "{at} must have at least {} items",
                    count("minItems").unwrap()
                ));
            }
            if count("maxItems").is_some_and(|most| items.len() > most) {
                problems.push(format!(
                    "{at} must have at most {} items",
                    count("maxItems").unwrap()
                ));
            }
            if let Some(each) = schema.get("items") {
                for (i, item) in items.iter().enumerate() {
                    check(each, item, &format!("{at}/{i}"), problems);
                }
            }
        }
        Some("string") => {
            let Some(text) = value.as_str() else {
                return problems.push(format!("{at} must be string"));
            };
            let length = text.chars().count();
            if count("minLength").is_some_and(|least| length < least) {
                problems.push(format!(
                    "{at} must have at least {} characters",
                    count("minLength").unwrap()
                ));
            }
            if count("maxLength").is_some_and(|most| length > most) {
                problems.push(format!(
                    "{at} must have at most {} characters",
                    count("maxLength").unwrap()
                ));
            }
            if let Some(pattern) = schema["pattern"].as_str() {
                if !matches_pattern(pattern, text) {
                    problems.push(format!("{at} must match pattern \"{pattern}\""));
                }
            }
        }
        Some(kind @ ("number" | "integer")) => {
            let Some(n) = value.as_f64() else {
                return problems.push(format!("{at} must be {kind}"));
            };
            if kind == "integer" && n.fract() != 0.0 {
                return problems.push(format!("{at} must be integer"));
            }
            if number("minimum").is_some_and(|least| n < least) {
                problems.push(format!("{at} must be >= {}", number("minimum").unwrap()));
            }
            if number("maximum").is_some_and(|most| n > most) {
                problems.push(format!("{at} must be <= {}", number("maximum").unwrap()));
            }
            if number("exclusiveMinimum").is_some_and(|least| n <= least) {
                problems.push(format!(
                    "{at} must be > {}",
                    number("exclusiveMinimum").unwrap()
                ));
            }
        }
        Some("boolean") if !value.is_boolean() => problems.push(format!("{at} must be boolean")),
        _ => {}
    }
}

/// The contract's patterns. One this validator doesn't know fails every value, so a new pattern can't slip through
/// unchecked (a test lists the manifest's patterns).
const PATTERNS: [&str; 2] = ["^#[0-9a-fA-F]{6}$", "^[A-Za-z0-9][A-Za-z0-9._ -]*$"];

fn matches_pattern(pattern: &str, text: &str) -> bool {
    if !PATTERNS.contains(&pattern) {
        return false;
    }
    match pattern {
        "^#[0-9a-fA-F]{6}$" => {
            text.len() == 7
                && text.starts_with('#')
                && text[1..].chars().all(|c| c.is_ascii_hexdigit())
        }
        "^[A-Za-z0-9][A-Za-z0-9._ -]*$" => {
            text.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric())
                && text
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._ -".contains(c))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_validator_knows_every_pattern_in_the_contract() {
        fn walk(value: &Value, found: &mut Vec<String>) {
            match value {
                Value::Object(map) => {
                    if let Some(Value::String(pattern)) = map.get("pattern") {
                        found.push(pattern.clone());
                    }
                    map.values().for_each(|v| walk(v, found));
                }
                Value::Array(items) => items.iter().for_each(|v| walk(v, found)),
                _ => {}
            }
        }
        let mut found = Vec::new();
        walk(manifest(), &mut found);
        assert!(
            !found.is_empty() && found.iter().all(|p| PATTERNS.contains(&p.as_str())),
            "{found:?}"
        );
        assert_eq!(tools().len(), 13);
    }

    #[test]
    fn arguments_are_checked_like_electron_forma_checks_them() {
        assert!(
            validate(
                "forma_add_shape",
                &json!({"type": "circle", "color": "#ed795e", "x": 100})
            )
            .is_ok()
        );
        let problem = |name: &str, args: Value| validate(name, &args).unwrap_err().message;
        assert!(
            problem("forma_add_shape", json!({"color": "#ed795e"}))
                .contains("required property 'type'")
        );
        assert!(
            problem("forma_add_shape", json!({"type": "circle", "color": "red"}))
                .contains("pattern")
        );
        assert!(problem("forma_add_shape", json!({"type": "blob"})).contains("must be one of"));
        assert!(problem("forma_add_shape", json!({"type": "circle", "w": 0})).contains("> 0"));
        assert!(
            problem("forma_get_scene", json!({"everything": true})).contains("additional property")
        );
        assert!(problem("forma_create_scene", json!({"replace": false})).contains("constant"));
        assert!(
            problem(
                "forma_export",
                json!({"format": "png", "filename": "../x.png"})
            )
            .contains("pattern")
        );
        assert!(problem("forma_update_scene", json!({})).contains("at least 1"));
        assert!(validate("forma_update_shape", &json!({"id": "a", "outTime": null})).is_ok());
        assert!(
            problem(
                "forma_set_keyframes",
                json!({"id": "a", "keyframes": [{"t": 0}]})
            )
            .contains("required property 'x'")
        );
        assert_eq!(
            validate("forma_fly", &json!({})).unwrap_err().code,
            "UNKNOWN_TOOL"
        );
        assert!(read_only("forma_preview") && !read_only("forma_add_shape"));
    }
}
