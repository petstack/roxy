//! Minimal HTTP echo backend for load-testing `roxy`.
//!
//! Responds on `POST /mcp` with canned `UpstreamDiscoverResponse` /
//! `UpstreamContentResponse` JSON documents — enough for roxy to serve
//! `tools/list`, `tools/call`, `resources/read` and `prompts/get`
//! without any real business logic in the way.
//!
//! Usage:
//!
//!     cargo run --release --example echo_upstream
//!     ECHO_PORT=8001 cargo run --release --example echo_upstream
//!
//! The idea is to make `roxy` the only variable: benching against this
//! upstream measures roxy's overhead (parse/serialize/pool/scheduler),
//! not PHP-FPM or a downstream HTTP API.

use std::net::SocketAddr;

use axum::{Json, Router, routing::post};
use serde_json::{Value, json};

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("ECHO_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8001);
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();

    let app = Router::new().route("/mcp", post(handler));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind echo_upstream");
    eprintln!("echo_upstream listening on http://{addr}/mcp");

    axum::serve(listener, app)
        .await
        .expect("echo_upstream serve");
}

async fn handler(Json(req): Json<Value>) -> Json<Value> {
    match req.get("type").and_then(Value::as_str) {
        Some("discover") => Json(json!({
            "tools": [
                {
                    "name": "echo",
                    "description": "Echo back a canned reply",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "message": { "type": "string" }
                        }
                    }
                },
                {
                    "name": "ping",
                    "description": "Ping with no arguments",
                    "input_schema": { "type": "object" }
                }
            ],
            "resources": [
                { "uri": "echo://status", "name": "status" }
            ],
            "prompts": []
        })),
        Some("call_tool") => Json(json!({
            "content": [
                { "type": "text", "text": "pong" }
            ]
        })),
        Some("read_resource") => Json(json!({
            "content": [
                { "type": "text", "text": "echo status: ok" }
            ]
        })),
        Some("get_prompt") => Json(json!({
            "content": [
                { "type": "text", "text": "canned prompt body" }
            ]
        })),
        Some("elicitation_cancelled") => Json(json!({
            "content": [
                { "type": "text", "text": "cancelled" }
            ]
        })),
        _ => Json(json!({
            "error": { "code": 400, "message": "unknown request type" }
        })),
    }
}
