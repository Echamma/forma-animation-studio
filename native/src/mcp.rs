//! `Forma.exe --mcp-server`: Forma MCP's stdio server (Electron Forma's `mcp/server.cjs`) without Node. An MCP client
//! starts it; it lists the contract's tools and passes each call to the open editor through its local bridge, so
//! Codex, Claude Code and Claude Desktop can drive native Forma the way they drive Electron's.

use crate::contract::{ToolError, fail, manifest, tools, validate};
use serde_json::{Value, json};
use std::io::{BufRead, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

const INSTRUCTIONS: &str = "Control the open Forma desktop editor. Start with forma_get_scene. Coordinates are layer centers in pixels. Back up important scenes using forma_export(format=project) before replacement. Preview frames to assess results. Exports go to the desktop app export folder. Control must be enabled in Forma. Tool errors do not mean an edit is safe to retry without inspecting the current scene.";

/// The protocol versions this server speaks; it answers with the client's when it's one of them.
const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// Serves MCP on stdin and stdout until stdin closes. Returns the process's exit code.
pub fn serve() -> i32 {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => answer(&message, &crate::bridge::discovery_file()),
            Err(_) => Some(
                json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "Parse error"}}),
            ),
        };
        if let Some(reply) = reply {
            if writeln!(stdout, "{reply}")
                .and_then(|()| stdout.flush())
                .is_err()
            {
                break;
            }
        }
    }
    0
}

/// The reply to one JSON-RPC message (none for notifications).
pub fn answer(message: &Value, discovery: &Path) -> Option<Value> {
    let id = message.get("id").cloned();
    let params = &message["params"];
    let result = match message["method"].as_str().unwrap_or("") {
        "initialize" => {
            let asked = params["protocolVersion"]
                .as_str()
                .filter(|v| VERSIONS.contains(v))
                .unwrap_or(VERSIONS[0]);
            Ok(json!({
                "protocolVersion": asked,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "forma", "version": manifest()["version"]},
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools().iter().map(|t| json!({
            "name": t["name"], "description": t["description"], "inputSchema": t["inputSchema"], "annotations": t["annotations"],
        })).collect::<Vec<_>>()})),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("");
            let args = params
                .get("arguments")
                .cloned()
                .filter(|a| !a.is_null())
                .unwrap_or_else(|| json!({}));
            Ok(match call(discovery, name, &args) {
                Ok(result) if name == "forma_preview" => {
                    let mut metadata = result.clone();
                    let data = metadata
                        .as_object_mut()
                        .and_then(|m| m.remove("data"))
                        .unwrap_or_default();
                    json!({"content": [{"type": "text", "text": metadata.to_string()}, {"type": "image", "data": data, "mimeType": "image/png"}], "structuredContent": metadata})
                }
                Ok(result) => {
                    json!({"content": [{"type": "text", "text": result.to_string()}], "structuredContent": result})
                }
                Err(error) => {
                    json!({"isError": true, "content": [{"type": "text", "text": json!({"code": error.code, "message": error.message}).to_string()}]})
                }
            })
        }
        // Notifications (initialized, cancelled) need no reply.
        _ if id.is_none() => return None,
        _ => Err(json!({"code": -32601, "message": "Method not found"})),
    };
    let id = id?;
    Some(match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
    })
}

/// One tool call through the bridge the discovery file names.
pub fn call(discovery: &Path, name: &str, args: &Value) -> Result<Value, ToolError> {
    validate(name, args)?;
    let unavailable = || {
        fail(
            "APP_UNAVAILABLE",
            "Open the Forma desktop app and click MCP off to enable control, or launch it with --mcp.",
        )
    };
    let found: Value = std::fs::read(discovery)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or_else(unavailable)?;
    let port = found["port"]
        .as_u64()
        .filter(|p| (1..=65535).contains(p))
        .map(|p| p as u16);
    let token = found["token"]
        .as_str()
        .filter(|t| t.len() == 64 && t.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
    let (Some(port), Some(token), true) = (port, token, found["version"] == 1) else {
        return Err(fail(
            "INVALID_DISCOVERY",
            "The Forma bridge file is invalid; toggle MCP off and on in Forma.",
        ));
    };
    let gone = || {
        fail(
            "APP_UNAVAILABLE",
            "Forma is unavailable or the tool timed out. Check the desktop app and inspect the scene before retrying an edit.",
        )
    };
    let mut stream =
        TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(5))
            .map_err(|_| gone())?;
    let _ = stream.set_read_timeout(Some(crate::bridge::CALL_TIMEOUT + Duration::from_secs(30)));
    let body = args.to_string();
    write!(stream, "POST /tools/{name} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
        .map_err(|_| gone())?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).map_err(|_| gone())?;
    let text = String::from_utf8_lossy(&response);
    let (head, body) = text.split_once("\r\n\r\n").ok_or_else(gone)?;
    let ok = head.split_whitespace().nth(1) == Some("200");
    let reply: Value = serde_json::from_str(body).map_err(|_| gone())?;
    match (ok, reply.get("error")) {
        (true, None) => Ok(reply["result"].clone()),
        (_, error) => Err(fail(
            error
                .and_then(|e| e["code"].as_str())
                .unwrap_or("BRIDGE_ERROR"),
            error
                .and_then(|e| e["message"].as_str())
                .unwrap_or("Forma rejected the tool."),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_speaks_mcp_and_explains_a_closed_editor() {
        let nowhere = std::env::temp_dir()
            .join(format!("forma-mcp-{}", crate::app::new_id()))
            .join("bridge.json");
        let reply = |message: Value| answer(&message, &nowhere);
        let hello = reply(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26", "capabilities": {}}})).unwrap();
        assert_eq!(
            (
                hello["result"]["protocolVersion"].as_str(),
                hello["result"]["serverInfo"]["name"].as_str()
            ),
            (Some("2025-03-26"), Some("forma"))
        );
        assert!(reply(json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());
        let listed = reply(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).unwrap();
        assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 13);
        let closed = reply(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "forma_get_scene", "arguments": {}}})).unwrap();
        assert_eq!(closed["result"]["isError"], true);
        assert!(
            closed["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("APP_UNAVAILABLE")
        );
        let bad = reply(json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "forma_add_shape", "arguments": {"type": "blob"}}})).unwrap();
        assert!(
            bad["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("INVALID_ARGUMENTS")
        );
        assert_eq!(
            reply(json!({"jsonrpc": "2.0", "id": 5, "method": "resources/list"})).unwrap()["error"]
                ["code"],
            -32601
        );
    }
}
