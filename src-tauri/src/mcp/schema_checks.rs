//! Checks of every tool schema ReMa itself sends to models: the built-in
//! tools (career search, Network Connect, Business) and ReMa MCP's tools,
//! as other clients receive them. The connector tools (mail, calendar,
//! applications) are built inside `services::connector_tools` from
//! private functions and are not covered here.
//!
//! What is checked is what the providers' dialects have in common: one
//! `type` per value (never an array), only JSON Schema's own `format`s,
//! every object schema with `properties`, and only keywords of JSON Schema
//! draft 2020-12 (a schema made of other words is not a schema a client
//! can validate). Nothing is checked for OpenAI's strict mode
//! (`additionalProperties: false` everywhere, every property required):
//! ReMa sends its tools with `strict: false` (`llm::openai_responses`), so
//! that closing is not needed.

use serde_json::Value;

use crate::llm::ToolSpec;

/// Draft 2020-12's keywords (core, applicator, validation, meta-data,
/// format), plus `definitions`, its older spelling of `$defs`.
const KEYWORDS: [&str; 58] = [
    "$anchor",
    "$comment",
    "$defs",
    "$dynamicAnchor",
    "$dynamicRef",
    "$id",
    "$ref",
    "$schema",
    "$vocabulary",
    "additionalProperties",
    "allOf",
    "anyOf",
    "const",
    "contains",
    "contentEncoding",
    "contentMediaType",
    "contentSchema",
    "default",
    "definitions",
    "dependentRequired",
    "dependentSchemas",
    "deprecated",
    "description",
    "else",
    "enum",
    "examples",
    "exclusiveMaximum",
    "exclusiveMinimum",
    "format",
    "if",
    "items",
    "maxContains",
    "maxItems",
    "maxLength",
    "maxProperties",
    "maximum",
    "minContains",
    "minItems",
    "minLength",
    "minProperties",
    "minimum",
    "multipleOf",
    "not",
    "oneOf",
    "pattern",
    "patternProperties",
    "prefixItems",
    "properties",
    "propertyNames",
    "readOnly",
    "required",
    "then",
    "title",
    "type",
    "unevaluatedItems",
    "unevaluatedProperties",
    "uniqueItems",
    "writeOnly",
];

/// The formats draft 2020-12 defines (its format vocabulary).
const FORMATS: [&str; 19] = [
    "date-time",
    "date",
    "time",
    "duration",
    "email",
    "idn-email",
    "hostname",
    "idn-hostname",
    "ipv4",
    "ipv6",
    "uri",
    "uri-reference",
    "iri",
    "iri-reference",
    "uuid",
    "uri-template",
    "json-pointer",
    "relative-json-pointer",
    "regex",
];

/// The keywords whose value is one schema.
const ONE_SCHEMA: [&str; 10] = [
    "additionalProperties",
    "contains",
    "else",
    "if",
    "items",
    "not",
    "propertyNames",
    "then",
    "unevaluatedItems",
    "unevaluatedProperties",
];
/// The keywords whose value is a list of schemas.
const SCHEMA_LIST: [&str; 4] = ["allOf", "anyOf", "oneOf", "prefixItems"];
/// The keywords whose value maps names to schemas.
const SCHEMA_MAP: [&str; 5] = [
    "$defs",
    "definitions",
    "dependentSchemas",
    "patternProperties",
    "properties",
];

/// Walks one schema and its subschemas, noting what a client could refuse.
fn check(schema: &Value, path: &str, problems: &mut Vec<String>) {
    let map = match schema {
        Value::Object(map) => map,
        // `true`/`false` are schemas too.
        Value::Bool(_) => return,
        other => {
            problems.push(format!("{path}: not a schema ({other})"));
            return;
        }
    };
    for key in map.keys() {
        if !KEYWORDS.contains(&key.as_str()) {
            problems.push(format!("{path}: unknown keyword {key}"));
        }
    }
    match map.get("type") {
        Some(Value::Array(_)) => problems.push(format!("{path}: type is an array")),
        Some(Value::String(kind)) if kind == "object" && !map.contains_key("properties") => {
            problems.push(format!("{path}: object without properties"))
        }
        _ => {}
    }
    if let Some(format) = map.get("format") {
        match format.as_str() {
            Some(format) if FORMATS.contains(&format) => {}
            _ => problems.push(format!("{path}: non-standard format {format}")),
        }
    }
    for key in ONE_SCHEMA {
        if let Some(child) = map.get(key) {
            check(child, &format!("{path}.{key}"), problems);
        }
    }
    for key in SCHEMA_LIST {
        if let Some(Value::Array(items)) = map.get(key) {
            for (i, child) in items.iter().enumerate() {
                check(child, &format!("{path}.{key}[{i}]"), problems);
            }
        }
    }
    for key in SCHEMA_MAP {
        if let Some(Value::Object(children)) = map.get(key) {
            for (name, child) in children {
                check(child, &format!("{path}.{key}.{name}"), problems);
            }
        }
    }
}

/// The built-in tools' specifications, as a request carries them.
fn built_in_specs() -> Vec<ToolSpec> {
    let mut specs = crate::retrieval::tools::specs();
    specs.extend(crate::network::tools::specs());
    specs.extend(crate::business::tools::specs());
    specs
}

#[test]
fn built_in_tool_schemas_are_portable() {
    let specs = built_in_specs();
    assert!(specs.len() >= 6, "{}", specs.len());
    let mut problems = Vec::new();
    for spec in &specs {
        assert_eq!(spec.input_schema["type"], "object", "{}", spec.name);
        check(&spec.input_schema, &spec.name, &mut problems);
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn rema_mcp_tool_schemas_are_portable() {
    let tools = crate::rema_mcp::server::tools();
    assert_eq!(tools.len(), 5);
    let mut problems = Vec::new();
    for tool in &tools {
        check(
            &Value::Object((*tool.input_schema).clone()),
            &format!("{}.input", tool.name),
            &mut problems,
        );
        let output = tool.output_schema.as_ref().expect("an output schema");
        check(
            &Value::Object((**output).clone()),
            &format!("{}.output", tool.name),
            &mut problems,
        );
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

/// The checker itself sees what it is meant to see.
#[test]
fn the_checks_notice_what_clients_refuse() {
    let mut problems = Vec::new();
    check(
        &serde_json::json!({
            "type": "object",
            "properties": {
                "count": { "type": ["integer", "null"], "format": "uint32", "minimum": 1 },
                "when": { "type": "string", "format": "date-time" },
                "bag": { "type": "object" },
                "list": { "type": "array", "items": { "type": "string", "nullable": true } },
                "either": { "anyOf": [{ "type": "number", "format": "double" }, { "type": "null" }] }
            },
            "$defs": { "Thing": { "type": "object", "properties": {}, "x-made-up": 1 } },
            "additionalProperties": { "type": "string", "format": "int" }
        }),
        "t",
        &mut problems,
    );
    problems.sort();
    assert_eq!(
        problems,
        [
            "t.$defs.Thing: unknown keyword x-made-up",
            "t.additionalProperties: non-standard format \"int\"",
            "t.properties.bag: object without properties",
            "t.properties.count: non-standard format \"uint32\"",
            "t.properties.count: type is an array",
            "t.properties.either.anyOf[0]: non-standard format \"double\"",
            // OpenAPI's word, not JSON Schema's.
            "t.properties.list.items: unknown keyword nullable",
        ]
    );
}
