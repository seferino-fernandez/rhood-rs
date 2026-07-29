//! Schema post-processing: close every object schema so MCP tool inputs reject
//! unexpected properties, and a response-size guard helper.

use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{Map, Value};

/// Recursively sets `additionalProperties: false` on every object schema node
/// within a JSON Schema value: the root, nested `properties`, `items`, and any
/// `$defs`/`definitions`. Idempotent.
pub fn close_object_schemas(schema: &mut Value) {
    match schema {
        Value::Object(map) => {
            close_object_map(map);
        }
        Value::Array(items) => {
            for item in items {
                close_object_schemas(item);
            }
        }
        _ => {}
    }
}

/// Closes a single object schema map in place: for every object schema node it
/// sets `additionalProperties: false` and `required: []` when those keys are
/// absent, then recurses into nested values. Idempotent.
///
/// The empty `required: []` truthfully declares "no fields are required" for
/// parameterless/all-optional tools (schemars omits the key entirely when a
/// struct has no mandatory fields), satisfying scanners that require an explicit
/// `required` declaration. An existing `required` array is preserved verbatim.
pub fn close_object_map(map: &mut Map<String, Value>) {
    let is_object_schema =
        map.get("type").and_then(Value::as_str) == Some("object") || map.contains_key("properties");
    if is_object_schema {
        if !map.contains_key("additionalProperties") {
            map.insert("additionalProperties".to_string(), Value::Bool(false));
        }
        if !map.contains_key("required") {
            map.insert("required".to_string(), Value::Array(Vec::new()));
        }
    }
    for (_key, value) in map.iter_mut() {
        close_object_schemas(value);
    }
}

/// Replaces an oversized tool result with a valid, bounded JSON error.
///
/// Measures the serialized size of the result's content; if it exceeds
/// `max_bytes`, returns a small `CallToolResult` describing the overflow so the
/// model can narrow its request. Returns the original result otherwise.
pub fn enforce_response_budget(result: CallToolResult, max_bytes: usize) -> CallToolResult {
    // Size `result.content`: it's the only populated payload field for this
    // crate's tools (all return `Result<String, String>`, so the result is a
    // single text Content; `structured_content` is always `None`).
    let size = serde_json::to_string(&result.content)
        .map(|s| s.len())
        .unwrap_or(0);
    if size <= max_bytes {
        return result;
    }
    // Preserve the original error flag: a tool `Err(String)` arrives here as
    // `Ok(CallToolResult { is_error: Some(true), .. })`, so relabeling it as a
    // success would silently hide the failure.
    let was_error = result.is_error;
    let body = serde_json::json!({
        "error": "response_too_large",
        "bytes": size,
        "limit": max_bytes,
        "hint": "narrow the query (smaller span/interval, fewer symbols)"
    });
    let mut replacement = CallToolResult::success(vec![ContentBlock::text(body.to_string())]);
    replacement.is_error = was_error;
    replacement
}

#[cfg(test)]
mod tests {
    use super::close_object_schemas;
    use serde_json::json;

    #[test]
    fn closes_top_level_object() {
        let mut schema = json!({"type": "object", "properties": {"a": {"type": "string"}}});
        close_object_schemas(&mut schema);
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(schema["required"], json!([]));
    }

    #[test]
    fn closes_nested_object_and_defs() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "contracts": {
                    "type": "array",
                    "items": {"type": "object", "properties": {"x": {"type": "number"}}}
                }
            },
            "$defs": {"Inner": {"type": "object", "properties": {"y": {"type": "string"}}}}
        });
        close_object_schemas(&mut schema);
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(schema["required"], json!([]));
        assert_eq!(
            schema["properties"]["contracts"]["items"]["additionalProperties"],
            json!(false)
        );
        assert_eq!(
            schema["properties"]["contracts"]["items"]["required"],
            json!([])
        );
        assert_eq!(
            schema["$defs"]["Inner"]["additionalProperties"],
            json!(false)
        );
        assert_eq!(schema["$defs"]["Inner"]["required"], json!([]));
    }

    #[test]
    fn closes_empty_no_arg_object() {
        let mut schema = json!({"type": "object", "properties": {}});
        close_object_schemas(&mut schema);
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(schema["required"], json!([]));
    }

    #[test]
    fn inserts_empty_required_when_absent() {
        // An object schema with no `required` key gets an explicit `required: []`.
        let mut schema = json!({"type": "object", "properties": {"a": {"type": "string"}}});
        assert!(schema.get("required").is_none());
        close_object_schemas(&mut schema);
        assert_eq!(schema["required"], json!([]));
    }

    #[test]
    fn preserves_existing_non_empty_required() {
        // A populated `required` (e.g. from schemars for a mandatory field) must
        // be preserved verbatim, never overwritten or emptied.
        let mut schema = json!({
            "type": "object",
            "properties": {"symbol": {"type": "string"}},
            "required": ["symbol"]
        });
        close_object_schemas(&mut schema);
        assert_eq!(schema["required"], json!(["symbol"]));
        assert_eq!(schema["additionalProperties"], json!(false));
    }

    #[test]
    fn preserves_existing_additional_properties() {
        let mut schema = json!({"type": "object", "additionalProperties": true});
        close_object_schemas(&mut schema);
        assert_eq!(schema["additionalProperties"], json!(true));
    }
}

#[cfg(test)]
mod router_tests {
    use super::close_object_schemas;
    use crate::config::McpConfig;
    use crate::tools::WRITE_TOOLS;
    use crate::tools::handler::RhoodTools;
    use rhood_core::{RhoodConfig, RobinhoodClient};
    use serde_json::Value;
    use std::sync::Arc;

    /// Builds a `RhoodTools` with no network access.
    ///
    /// `with_config(RhoodConfig::default())` is a pure constructor: it builds the
    /// HTTP client but performs no I/O or authentication. Using the default
    /// config (rather than `RobinhoodClient::new()`, which loads env/TOML via
    /// `RhoodConfig::load`) keeps these tests deterministic.
    fn tools_with(read_only: bool) -> RhoodTools {
        let client = RobinhoodClient::with_config(RhoodConfig::default())
            .expect("default config builds a client");
        let hook = Arc::new(|_c: RobinhoodClient, _p: rmcp::Peer<rmcp::RoleServer>| {
            Box::pin(async { Ok(()) })
                as std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>
        });
        RhoodTools::new_lazy(client, hook, read_only, &McpConfig::default())
    }

    /// Pins the advertised protocol version so a future rmcp bump that moves
    /// `ProtocolVersion::LATEST` becomes a reviewed diff rather than a silent
    /// change to what this server tells clients.
    #[test]
    fn advertises_current_protocol_version() {
        use rmcp::ServerHandler;
        use rmcp::model::ProtocolVersion;

        assert_eq!(
            tools_with(false).get_info().protocol_version,
            ProtocolVersion::V_2026_07_28
        );
    }

    /// The server must identify itself, not the SDK. `Implementation::default()`
    /// and `from_build_env()` both expand `env!` inside rmcp, so they report
    /// `"rmcp"` at rmcp's version.
    #[test]
    fn identifies_itself_rather_than_the_sdk() {
        use rmcp::ServerHandler;

        let server_info = tools_with(false).get_info().server_info;
        assert_eq!(server_info.name, env!("CARGO_PKG_NAME"));
        assert_eq!(server_info.version, env!("CARGO_PKG_VERSION"));
        assert_ne!(
            server_info.name, "rmcp",
            "server_info must not report the SDK's identity"
        );
    }

    /// SEP-2577 deprecated MCP logging and rhood no longer implements it, so the
    /// capability must not be advertised. A client that saw it would be told to
    /// expect a `logging/setLevel` that rmcp now answers with method-not-found.
    #[test]
    fn does_not_advertise_deprecated_logging_capability() {
        use rmcp::ServerHandler;

        let capabilities = tools_with(false).get_info().capabilities;
        assert!(capabilities.tools.is_some(), "tools must be advertised");
        assert!(
            capabilities.logging.is_none(),
            "logging capability must not be set"
        );
    }

    /// The third read-only enforcement point. `list_tools` filtering and the
    /// `call_tool` rejection are covered end-to-end in `http::wire_tests`; this
    /// covers the lookup rmcp uses to resolve `Mcp-Param-*` headers, which would
    /// otherwise be a way to observe a hidden tool's schema.
    #[test]
    fn read_only_get_tool_hides_write_tools() {
        use rmcp::ServerHandler;

        let read_only = tools_with(true);
        let read_write = tools_with(false);
        for write_tool in WRITE_TOOLS {
            assert!(
                read_only.get_tool(write_tool).is_none(),
                "read-only mode must not resolve `{write_tool}`"
            );
            assert!(
                read_write.get_tool(write_tool).is_some(),
                "read-write mode must resolve `{write_tool}`"
            );
        }
    }

    /// `get_tool` and `list_tools` must return the same document because they share
    /// `close_tool_schemas` precisely so the schema a client is shown is the one
    /// rmcp validates promoted headers against.
    #[test]
    fn get_tool_returns_a_closed_schema() {
        use rmcp::ServerHandler;

        let tools = tools_with(false);
        let name = tools
            .tool_router
            .list_all()
            .first()
            .map(|tool| tool.name.to_string())
            .expect("the router advertises at least one tool");
        let tool = tools.get_tool(&name).expect("router tool resolves");
        let schema = Value::Object((*tool.input_schema).clone());
        assert_eq!(
            schema["additionalProperties"],
            Value::Bool(false),
            "get_tool must return the same closed schema list_tools advertises"
        );
    }

    #[test]
    fn every_listed_tool_schema_is_closed() {
        let tools = tools_with(false);
        for tool in tools.tool_router.list_all() {
            let mut v = Value::Object((*tool.input_schema).clone());
            close_object_schemas(&mut v); // idempotent; asserts no panic
            assert_eq!(
                v["additionalProperties"],
                Value::Bool(false),
                "tool {} root schema must be closed",
                tool.name
            );
            // Every advertised tool's root schema must declare `required`
            // explicitly (either `[]` for all-optional/no-arg tools or a
            // populated array), so deterministic scanners don't flag a missing
            // `required` key.
            let required = v.get("required").unwrap_or_else(|| {
                panic!("tool {} root schema must declare `required`", tool.name)
            });
            assert!(
                required.is_array(),
                "tool {} root `required` must be an array, got {required}",
                tool.name
            );
        }
    }
}

#[cfg(test)]
mod budget_tests {
    use super::enforce_response_budget;
    use rmcp::model::{CallToolResult, ContentBlock};

    #[test]
    fn passes_small_results_through() {
        let original =
            CallToolResult::success(vec![ContentBlock::text("{\"ok\":true}".to_string())]);
        let guarded = enforce_response_budget(original, 1024);
        let text = serde_json::to_string(&guarded.content).unwrap();
        assert!(text.contains("ok"));
    }

    #[test]
    fn replaces_oversized_results() {
        let big = "x".repeat(2048);
        let original = CallToolResult::success(vec![ContentBlock::text(big)]);
        let guarded = enforce_response_budget(original, 256);
        let text = serde_json::to_string(&guarded.content).unwrap();
        assert!(text.contains("response_too_large"), "got: {text}");
        assert!(
            text.len() < 512,
            "guard payload must be small: {}",
            text.len()
        );
    }

    #[test]
    fn preserves_error_flag_on_replacement() {
        let mut original = CallToolResult::success(vec![ContentBlock::text("x".repeat(2048))]);
        original.is_error = Some(true);
        let guarded = enforce_response_budget(original, 256);
        assert_eq!(
            guarded.is_error,
            Some(true),
            "a failed call must stay flagged as an error after guarding"
        );
        let text = serde_json::to_string(&guarded.content).unwrap();
        assert!(text.contains("response_too_large"), "got: {text}");
    }
}
