use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct UpstreamEnvelope<'a> {
    pub session_id: Option<&'a str>,
    pub request_id: &'a str,
    #[serde(flatten)]
    pub request: UpstreamRequest<'a>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UpstreamRequest<'a> {
    Discover,
    CallTool {
        name: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        arguments: Option<&'a serde_json::Map<String, serde_json::Value>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        elicitation_results: Option<&'a [serde_json::Value]>,
        #[serde(skip_serializing_if = "Option::is_none")]
        context: Option<&'a serde_json::Value>,
    },
    ReadResource {
        uri: &'a str,
    },
    GetPrompt {
        name: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        arguments: Option<&'a serde_json::Map<String, serde_json::Value>>,
    },
    ElicitationCancelled {
        name: &'a str,
        action: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        context: Option<&'a serde_json::Value>,
    },
}

#[derive(Debug, Deserialize)]
pub struct UpstreamDiscoverResponse {
    #[serde(default)]
    pub tools: Vec<UpstreamToolDef>,
    #[serde(default)]
    pub resources: Vec<UpstreamResourceDef>,
    #[serde(default)]
    pub prompts: Vec<UpstreamPromptDef>,
}

#[derive(Debug, Deserialize)]
pub struct UpstreamToolDef {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub input_schema: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default)]
    pub output_schema: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
pub struct UpstreamResourceDef {
    pub uri: String,
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpstreamPromptDef {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub arguments: Vec<UpstreamPromptArgument>,
}

#[derive(Debug, Deserialize)]
pub struct UpstreamPromptArgument {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpstreamContentResponse {
    pub content: Vec<UpstreamContent>,
    #[serde(default)]
    pub structured_content: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UpstreamContent {
    Text { text: String },
    ResourceLink {
        uri: String,
        name: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        mime_type: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
pub struct UpstreamErrorResponse {
    pub error: UpstreamError,
}

#[derive(Debug, Deserialize)]
pub struct UpstreamError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct UpstreamElicitResponse {
    pub message: String,
    pub schema: serde_json::Value,
    #[serde(default)]
    pub context: Option<serde_json::Value>,
}

/// Result of a tool/resource/prompt call (not discover).
/// Parsed manually: if "elicit" key present -> Elicit, if "error" key present -> Error, else -> Content.
#[derive(Debug)]
pub enum UpstreamCallResult {
    Content(UpstreamContentResponse),
    Error(UpstreamErrorResponse),
    Elicit(UpstreamElicitResponse),
}

impl UpstreamCallResult {
    pub fn parse(bytes: &[u8]) -> anyhow::Result<Self> {
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        if value.get("elicit").is_some() {
            let inner = value["elicit"].clone();
            Ok(Self::Elicit(serde_json::from_value(inner)?))
        } else if value.get("error").is_some() {
            Ok(Self::Error(serde_json::from_value(value)?))
        } else {
            Ok(Self::Content(serde_json::from_value(value)?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_request_serialization() {
        let req = UpstreamRequest::Discover;
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"type":"discover"}"#);
    }

    #[test]
    fn test_call_tool_request_serialization() {
        let mut args = serde_json::Map::new();
        args.insert("city".into(), serde_json::Value::String("Moscow".into()));
        let req = UpstreamRequest::CallTool {
            name: "get_weather",
            arguments: Some(&args),
            elicitation_results: None,
            context: None,
        };
        let json: serde_json::Value = serde_json::to_value(&req).unwrap();
        assert_eq!(json["type"], "call_tool");
        assert_eq!(json["name"], "get_weather");
        assert_eq!(json["arguments"]["city"], "Moscow");
        assert!(json.get("elicitation_results").is_none());
        assert!(json.get("context").is_none());
    }

    #[test]
    fn test_call_tool_no_arguments() {
        let req = UpstreamRequest::CallTool {
            name: "ping",
            arguments: None,
            elicitation_results: None,
            context: None,
        };
        let json: serde_json::Value = serde_json::to_value(&req).unwrap();
        assert!(json.get("arguments").is_none());
    }

    #[test]
    fn test_discover_response_deserialization() {
        let json = r#"{
            "tools": [{"name": "get_weather", "description": "Get weather", "input_schema": {"type": "object"}}],
            "resources": [{"uri": "file:///config.yaml", "name": "config"}],
            "prompts": [{"name": "review", "description": "Code review", "arguments": [{"name": "lang", "required": true}]}]
        }"#;
        let resp: UpstreamDiscoverResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.tools.len(), 1);
        assert_eq!(resp.tools[0].name, "get_weather");
        assert_eq!(resp.resources.len(), 1);
        assert_eq!(resp.prompts.len(), 1);
        assert_eq!(resp.prompts[0].arguments[0].name, "lang");
        assert!(resp.prompts[0].arguments[0].required);
    }

    #[test]
    fn test_content_response_deserialization() {
        let json = r#"{"content": [{"type": "text", "text": "Hello"}]}"#;
        let resp: UpstreamContentResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            UpstreamContent::Text { text } => assert_eq!(text, "Hello"),
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn test_error_response_deserialization() {
        let json = r#"{"error": {"code": 404, "message": "Tool not found"}}"#;
        let resp: UpstreamErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error.code, 404);
        assert_eq!(resp.error.message, "Tool not found");
    }

    #[test]
    fn test_upstream_call_result_parses_error() {
        let json = br#"{"error": {"code": 400, "message": "Bad request"}}"#;
        let result = UpstreamCallResult::parse(json).unwrap();
        assert!(matches!(result, UpstreamCallResult::Error(_)));
    }

    #[test]
    fn test_upstream_call_result_parses_content() {
        let json = br#"{"content": [{"type": "text", "text": "OK"}]}"#;
        let result = UpstreamCallResult::parse(json).unwrap();
        assert!(matches!(result, UpstreamCallResult::Content(_)));
    }

    #[test]
    fn test_envelope_serialization() {
        let envelope = UpstreamEnvelope {
            session_id: Some("sess-1"),
            request_id: "req-1",
            request: UpstreamRequest::Discover,
        };
        let json: serde_json::Value = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["session_id"], "sess-1");
        assert_eq!(json["request_id"], "req-1");
        assert_eq!(json["type"], "discover");
    }

    #[test]
    fn test_envelope_null_session() {
        let envelope = UpstreamEnvelope {
            session_id: None,
            request_id: "req-2",
            request: UpstreamRequest::Discover,
        };
        let json: serde_json::Value = serde_json::to_value(&envelope).unwrap();
        assert!(json["session_id"].is_null());
        assert_eq!(json["request_id"], "req-2");
    }

    #[test]
    fn test_call_tool_with_elicitation_context() {
        let results = vec![serde_json::json!({"class": "business"})];
        let context = serde_json::json!({"step": 1});
        let envelope = UpstreamEnvelope {
            session_id: Some("s1"),
            request_id: "r1",
            request: UpstreamRequest::CallTool {
                name: "book",
                arguments: None,
                elicitation_results: Some(&results),
                context: Some(&context),
            },
        };
        let json: serde_json::Value = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["type"], "call_tool");
        assert_eq!(json["elicitation_results"][0]["class"], "business");
        assert_eq!(json["context"]["step"], 1);
    }

    #[test]
    fn test_discover_response_with_titles_and_output_schema() {
        let json = r#"{
            "tools": [{
                "name": "book",
                "title": "Book Flight",
                "description": "Books a flight",
                "input_schema": {"type": "object"},
                "output_schema": {"type": "object", "properties": {"id": {"type": "integer"}}}
            }],
            "resources": [{
                "uri": "roxy://status",
                "name": "status",
                "title": "Server Status"
            }],
            "prompts": [{
                "name": "greet",
                "title": "Greeting",
                "arguments": [{
                    "name": "name",
                    "title": "Person Name",
                    "required": true
                }]
            }]
        }"#;
        let resp: UpstreamDiscoverResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.tools[0].title.as_deref(), Some("Book Flight"));
        assert!(resp.tools[0].output_schema.is_some());
        assert_eq!(resp.resources[0].title.as_deref(), Some("Server Status"));
        assert_eq!(resp.prompts[0].title.as_deref(), Some("Greeting"));
        assert_eq!(resp.prompts[0].arguments[0].title.as_deref(), Some("Person Name"));
    }

    #[test]
    fn test_resource_link_content_deserialization() {
        let json = r#"{"content": [
            {"type": "text", "text": "See details:"},
            {"type": "resource_link", "uri": "roxy://b/1", "name": "booking-1", "title": "Booking #1"}
        ]}"#;
        let resp: UpstreamContentResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.content.len(), 2);
        assert!(matches!(&resp.content[0], UpstreamContent::Text { .. }));
        assert!(matches!(&resp.content[1], UpstreamContent::ResourceLink { uri, .. } if uri == "roxy://b/1"));
    }

    #[test]
    fn test_structured_content_deserialization() {
        let json = r#"{
            "content": [{"type": "text", "text": "Done"}],
            "structured_content": {"id": 42, "status": "ok"}
        }"#;
        let resp: UpstreamContentResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.structured_content.as_ref().unwrap()["id"], 42);
    }

    #[test]
    fn test_upstream_call_result_parses_elicit() {
        let json = br#"{"elicit": {"message": "Choose", "schema": {"type": "object"}, "context": {"s": 1}}}"#;
        let result = UpstreamCallResult::parse(json).unwrap();
        match result {
            UpstreamCallResult::Elicit(e) => {
                assert_eq!(e.message, "Choose");
                assert_eq!(e.context.as_ref().unwrap()["s"], 1);
            }
            _ => panic!("expected Elicit variant"),
        }
    }

    #[test]
    fn test_upstream_call_result_parses_elicit_without_context() {
        let json = br#"{"elicit": {"message": "Choose", "schema": {"type": "object"}}}"#;
        let result = UpstreamCallResult::parse(json).unwrap();
        match result {
            UpstreamCallResult::Elicit(e) => {
                assert!(e.context.is_none());
            }
            _ => panic!("expected Elicit variant"),
        }
    }

    #[test]
    fn test_elicitation_cancelled_serialization() {
        let ctx = serde_json::json!({"step": 1});
        let envelope = UpstreamEnvelope {
            session_id: Some("s1"),
            request_id: "r1",
            request: UpstreamRequest::ElicitationCancelled {
                name: "book",
                action: "decline",
                context: Some(&ctx),
            },
        };
        let json: serde_json::Value = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["type"], "elicitation_cancelled");
        assert_eq!(json["name"], "book");
        assert_eq!(json["action"], "decline");
        assert_eq!(json["context"]["step"], 1);
    }
}
