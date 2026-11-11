//! Model Context Protocol (MCP) message handling, transport-free.
//!
//! The stdio server (`aos mcp`) and the HTTP endpoint (`POST /mcp`) both feed JSON-RPC messages
//! to [`route`]. Everything but `tools/call` is answered right away. A `tools/call` comes back as
//! [`McpAction::Call`] so the caller can run it where it likes (the server runs it on a blocking
//! thread) and then wrap the outcome with [`call_response`].
//!
//! Hand-written instead of using an SDK: the protocol surface we need is five methods, and
//! keeping it here avoids pulling a second HTTP stack next to Dioxus's pinned versions.

use serde_json::{Value, json};

use crate::registry::{Registry, Risk, ToolError};

/// Newest protocol revision we speak. Older clients get their own version echoed back.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

pub const SERVER_NAME: &str = "agentic-os";

/// What the transport should do with an incoming message.
#[derive(Debug, PartialEq)]
pub enum McpAction {
    /// Send this response.
    Respond(Value),
    /// Run this tool, then send `call_response(id, result)`.
    Call {
        id: Value,
        name: String,
        args: Value,
    },
    /// A notification or a response from the client: send nothing.
    Nothing,
}

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

pub fn error_response(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// Parse one line of text (stdio) into an action.
pub fn route_text(registry: &Registry, text: &str) -> McpAction {
    match serde_json::from_str::<Value>(text) {
        Ok(msg) => route(registry, msg),
        Err(e) => McpAction::Respond(error_response(
            Value::Null,
            PARSE_ERROR,
            format!("parse error: {e}"),
        )),
    }
}

/// Decide what to do with one JSON-RPC message.
pub fn route(registry: &Registry, msg: Value) -> McpAction {
    let Some(obj) = msg.as_object() else {
        return McpAction::Respond(error_response(
            Value::Null,
            INVALID_REQUEST,
            "expected a JSON object",
        ));
    };
    let id = obj.get("id").cloned();
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        // A response to something we sent (we never send requests), or junk.
        return match id {
            Some(id) if !obj.contains_key("result") && !obj.contains_key("error") => {
                McpAction::Respond(error_response(id, INVALID_REQUEST, "missing method"))
            }
            _ => McpAction::Nothing,
        };
    };
    // Notifications have no id and get no reply.
    let Some(id) = id else {
        return McpAction::Nothing;
    };
    let params = obj.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => {
            let asked = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(PROTOCOL_VERSION);
            let version = if SUPPORTED_VERSIONS.contains(&asked) {
                asked
            } else {
                PROTOCOL_VERSION
            };
            McpAction::Respond(ok(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
                    "instructions": "A personal Agentic OS: tasks, habits, mood, journal, alarms, XP. \
                        Call `today` first for context. Tool risk is in each description: free = just do it, \
                        tell = do it and say so, ask = confirm first. Journal text is stored verbatim."
                }),
            ))
        }
        "ping" => McpAction::Respond(ok(id, json!({}))),
        "tools/list" => McpAction::Respond(ok(id, json!({ "tools": tool_list(registry) }))),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return McpAction::Respond(error_response(
                    id,
                    INVALID_PARAMS,
                    "tools/call needs a name",
                ));
            };
            if registry.get(name).is_none() {
                return McpAction::Respond(error_response(
                    id,
                    INVALID_PARAMS,
                    format!("unknown tool: {name}"),
                ));
            }
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            McpAction::Call {
                id,
                name: name.to_string(),
                args,
            }
        }
        "resources/list" => McpAction::Respond(ok(id, json!({ "resources": [] }))),
        "prompts/list" => McpAction::Respond(ok(id, json!({ "prompts": [] }))),
        other => McpAction::Respond(error_response(
            id,
            METHOD_NOT_FOUND,
            format!("method not found: {other}"),
        )),
    }
}

/// The `tools/list` entries.
pub fn tool_list(registry: &Registry) -> Vec<Value> {
    registry
        .tools()
        .iter()
        .map(|t| {
            let risk_note = match t.risk {
                Risk::Free => "Risk: free.",
                Risk::Tell => "Risk: tell (do it, then tell him).",
                Risk::Ask => "Risk: ask (confirm with him first).",
            };
            json!({
                "name": t.name,
                "description": format!("{} {}", t.description, risk_note),
                "inputSchema": t.input_schema,
                "annotations": {
                    "readOnlyHint": t.read_only,
                    "destructiveHint": t.risk == Risk::Ask,
                    "idempotentHint": t.read_only,
                    "openWorldHint": false
                }
            })
        })
        .collect()
}

/// Wrap a tool's outcome as a `tools/call` response. Tool failures are results with
/// `isError: true` (so the model sees them), not protocol errors.
pub fn call_response(id: Value, result: Result<Value, ToolError>) -> Value {
    match result {
        Ok(value) => {
            let text = serde_json::to_string(&value).unwrap_or_default();
            let structured = if value.is_object() {
                value
            } else {
                json!({ "result": value })
            };
            ok(
                id,
                json!({
                    "content": [{ "type": "text", "text": text }],
                    "structuredContent": structured,
                    "isError": false
                }),
            )
        }
        Err(e) => ok(
            id,
            json!({
                "content": [{ "type": "text", "text": format!("{} ({})", e, e.code()) }],
                "isError": true
            }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolCtx;
    use aos_core::Store;

    #[test]
    fn handshake_and_list() {
        let reg = crate::registry();
        let init = route_text(
            &reg,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
        );
        let McpAction::Respond(r) = init else {
            panic!()
        };
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(
            route_text(
                &reg,
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
            ),
            McpAction::Nothing
        );
        let McpAction::Respond(r) =
            route_text(&reg, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
        else {
            panic!()
        };
        let tools = r["result"]["tools"].as_array().unwrap();
        assert!(tools.iter().any(|t| t["name"] == "task_add"));
        assert!(tools.iter().all(|t| t["inputSchema"]["type"] == "object"));
    }

    #[test]
    fn call_round_trip() {
        let reg = crate::registry();
        let store = Store::open_temporary().unwrap();
        let msg = r#"{"jsonrpc":"2.0","id":"a","method":"tools/call","params":{"name":"task_add","arguments":{"title":"From MCP"}}}"#;
        let McpAction::Call { id, name, args } = route_text(&reg, msg) else {
            panic!()
        };
        let ctx = ToolCtx::new(&store, "mcp");
        let resp = call_response(id, reg.call(&ctx, &name, args));
        assert_eq!(resp["id"], "a");
        assert_eq!(resp["result"]["isError"], false);
        assert_eq!(resp["result"]["structuredContent"]["title"], "From MCP");

        let bad = call_response(
            json!(3),
            reg.call(&ctx, "task_done", json!({"id": "missing"})),
        );
        assert_eq!(bad["result"]["isError"], true);
    }

    #[test]
    fn errors() {
        let reg = crate::registry();
        let McpAction::Respond(r) = route_text(&reg, "{not json") else {
            panic!()
        };
        assert_eq!(r["error"]["code"], PARSE_ERROR);
        let McpAction::Respond(r) = route_text(&reg, r#"{"jsonrpc":"2.0","id":1,"method":"nope"}"#)
        else {
            panic!()
        };
        assert_eq!(r["error"]["code"], METHOD_NOT_FOUND);
        let McpAction::Respond(r) = route_text(
            &reg,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"zzz"}}"#,
        ) else {
            panic!()
        };
        assert_eq!(r["error"]["code"], INVALID_PARAMS);
    }
}
