//! Closed validation subset for the committed schemas, not a general schema engine.
//! Unknown keywords fail tests instead of silently weakening validation.
use serde_json::Value;

pub fn assert_output(value: &Value, execution: bool) {
    let name = if execution { "execution" } else { "status" };
    assert!(
        valid(value, &document(name), name),
        "upgrade output must match its reviewed schema: {value}"
    );
}

fn document(name: &str) -> Value {
    serde_json::from_str(match name {
        "status" => include_str!("../../schemas/upgrade-status-v1.schema.json"),
        "execution" => include_str!("../../schemas/upgrade-execution-v1.schema.json"),
        _ => panic!("only committed offline upgrade schemas may be resolved"),
    })
    .unwrap()
}

fn valid(value: &Value, schema: &Value, document_name: &str) -> bool {
    for keyword in schema.as_object().unwrap().keys() {
        assert!(
            [
                "$schema",
                "$defs",
                "$ref",
                "title",
                "type",
                "const",
                "enum",
                "anyOf",
                "allOf",
                "if",
                "then",
                "not",
                "properties",
                "required",
                "additionalProperties",
                "items",
                "pattern",
                "minimum"
            ]
            .contains(&keyword.as_str()),
            "review unsupported schema keyword: {keyword}"
        );
    }
    if let Some(reference) = schema.get("$ref") {
        let reference = reference.as_str().unwrap();
        if let Some(fragment) = reference.strip_prefix('#') {
            return valid(
                value,
                document(document_name).pointer(fragment).unwrap(),
                document_name,
            );
        }
        assert_eq!(reference, "upgrade-status-v1.schema.json");
        return valid(value, &document("status"), "status");
    }
    if let Some(alternatives) = schema.get("anyOf") {
        return alternatives
            .as_array()
            .unwrap()
            .iter()
            .any(|alternative| valid(value, alternative, document_name));
    }
    if let Some(all) = schema.get("allOf")
        && !all
            .as_array()
            .unwrap()
            .iter()
            .all(|item| valid(value, item, document_name))
    {
        return false;
    }
    if let Some(condition) = schema.get("if")
        && valid(value, condition, document_name)
        && !valid(value, &schema["then"], document_name)
    {
        return false;
    }
    if let Some(inverse) = schema.get("not")
        && valid(value, inverse, document_name)
    {
        return false;
    }
    if let Some(expected) = schema.get("const")
        && value != expected
    {
        return false;
    }
    if let Some(values) = schema.get("enum")
        && !values.as_array().unwrap().contains(value)
    {
        return false;
    }
    if let Some(kind) = schema.get("type") {
        let matches = match kind.as_str().unwrap() {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.is_u64() || value.is_i64(),
            "null" => value.is_null(),
            _ => panic!("review unknown schema type"),
        };
        if !matches {
            return false;
        }
    }
    if let Some(required) = schema.get("required")
        && required
            .as_array()
            .unwrap()
            .iter()
            .any(|key| value.get(key.as_str().unwrap()).is_none())
    {
        return false;
    }
    if let Some(properties) = schema.get("properties") {
        let value = value.as_object().unwrap();
        for (key, value) in value {
            let Some(property) = properties.get(key) else {
                if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                    return false;
                }
                continue;
            };
            if !valid(value, property, document_name) {
                return false;
            }
        }
    }
    if let Some(items) = schema.get("items")
        && !value
            .as_array()
            .unwrap()
            .iter()
            .all(|item| valid(item, items, document_name))
    {
        return false;
    }
    if let Some(pattern) = schema.get("pattern") {
        let text = match pattern.as_str().unwrap() {
            "^[0-9a-f]{64}$" => value.as_str().unwrap(),
            "^sha256:[0-9a-f]{64}$" => {
                let Some(text) = value.as_str().unwrap().strip_prefix("sha256:") else {
                    return false;
                };
                text
            }
            _ => panic!("review unsupported schema pattern"),
        };
        if text.len() != 64
            || !text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return false;
        }
    }
    if let Some(minimum) = schema.get("minimum")
        && value.as_i64().unwrap_or(-1) < minimum.as_i64().unwrap()
    {
        return false;
    }
    true
}

#[test]
fn schema_rejects_unknown_fields_versions_and_incomplete_receipts() {
    let fixture: Value =
        serde_json::from_str(include_str!("../snapshots/upgrade/ready.json")).unwrap();
    assert!(valid(&fixture, &document("status"), "status"));
    for (key, value) in [
        ("output_schema", serde_json::json!(2)),
        ("status", serde_json::json!("other")),
        ("secret", serde_json::json!("private")),
    ] {
        let mut invalid = fixture.clone();
        invalid[key] = value;
        assert!(!valid(&invalid, &document("status"), "status"));
    }
    assert!(!valid(
        &serde_json::json!({"changed_files": 3}),
        &document("execution")["$defs"]["receipt"],
        "execution"
    ));
    let applied: Value =
        serde_json::from_str(include_str!("../snapshots/upgrade/applied.json")).unwrap();
    assert!(valid(&applied, &document("execution"), "execution"));
    let mut missing = applied.clone();
    missing.as_object_mut().unwrap().remove("receipt");
    assert!(!valid(&missing, &document("execution"), "execution"));
    let mut invalid_digest = applied.clone();
    invalid_digest["plan"]["changes"][0]["result"]["sha256"] = serde_json::json!("secret");
    assert!(!valid(&invalid_digest, &document("execution"), "execution"));
    let mut failed = applied;
    failed["outcome"] = serde_json::json!("unavailable");
    assert!(!valid(&failed, &document("execution"), "execution"));
}
