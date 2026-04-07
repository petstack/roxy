use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct PhpEnvelope<'a> {
    pub session_id: Option<&'a str>,
    pub request_id: &'a str,
    #[serde(flatten)]
    pub request: PhpRequest<'a>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PhpRequest<'a> {
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
pub struct PhpDiscoverResponse {
    #[serde(default)]
    pub tools: Vec<PhpToolDef>,
    #[serde(default)]
    pub resources: Vec<PhpResourceDef>,
    #[serde(default)]
    pub prompts: Vec<PhpPromptDef>,
}

#[derive(Debug, Deserialize)]
pub struct PhpToolDef {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub input_schema: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
pub struct PhpResourceDef {
    pub uri: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PhpPromptDef {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub arguments: Vec<PhpPromptArgument>,
}

#[derive(Debug, Deserialize)]
pub struct PhpPromptArgument {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Deserialize)]
pub struct PhpContentResponse {
    pub content: Vec<PhpContent>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PhpContent {
    Text { text: String },
}

#[derive(Debug, Deserialize)]
pub struct PhpErrorResponse {
    pub error: PhpError,
}

#[derive(Debug, Deserialize)]
pub struct PhpError {
    pub code: i32,
    pub message: String,
}

/// Result of a tool/resource/prompt call (not discover).
/// Parsed manually: if "error" key present -> Error, else -> Content.
#[derive(Debug)]
pub enum PhpCallResult {
    Content(PhpContentResponse),
    Error(PhpErrorResponse),
}

impl PhpCallResult {
    pub fn parse(bytes: &[u8]) -> anyhow::Result<Self> {
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        if value.get("error").is_some() {
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
        let req = PhpRequest::Discover;
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"type":"discover"}"#);
    }

    #[test]
    fn test_call_tool_request_serialization() {
        let mut args = serde_json::Map::new();
        args.insert("city".into(), serde_json::Value::String("Moscow".into()));
        let req = PhpRequest::CallTool {
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
        let req = PhpRequest::CallTool {
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
        let resp: PhpDiscoverResponse = serde_json::from_str(json).unwrap();
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
        let resp: PhpContentResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.content.len(), 1);
        match &resp.content[0] {
            PhpContent::Text { text } => assert_eq!(text, "Hello"),
        }
    }

    #[test]
    fn test_error_response_deserialization() {
        let json = r#"{"error": {"code": 404, "message": "Tool not found"}}"#;
        let resp: PhpErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error.code, 404);
        assert_eq!(resp.error.message, "Tool not found");
    }

    #[test]
    fn test_php_call_result_parses_error() {
        let json = br#"{"error": {"code": 400, "message": "Bad request"}}"#;
        let result = PhpCallResult::parse(json).unwrap();
        assert!(matches!(result, PhpCallResult::Error(_)));
    }

    #[test]
    fn test_php_call_result_parses_content() {
        let json = br#"{"content": [{"type": "text", "text": "OK"}]}"#;
        let result = PhpCallResult::parse(json).unwrap();
        assert!(matches!(result, PhpCallResult::Content(_)));
    }

    #[test]
    fn test_envelope_serialization() {
        let envelope = PhpEnvelope {
            session_id: Some("sess-1"),
            request_id: "req-1",
            request: PhpRequest::Discover,
        };
        let json: serde_json::Value = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["session_id"], "sess-1");
        assert_eq!(json["request_id"], "req-1");
        assert_eq!(json["type"], "discover");
    }

    #[test]
    fn test_envelope_null_session() {
        let envelope = PhpEnvelope {
            session_id: None,
            request_id: "req-2",
            request: PhpRequest::Discover,
        };
        let json: serde_json::Value = serde_json::to_value(&envelope).unwrap();
        assert!(json["session_id"].is_null());
        assert_eq!(json["request_id"], "req-2");
    }

    #[test]
    fn test_call_tool_with_elicitation_context() {
        let results = vec![serde_json::json!({"class": "business"})];
        let context = serde_json::json!({"step": 1});
        let envelope = PhpEnvelope {
            session_id: Some("s1"),
            request_id: "r1",
            request: PhpRequest::CallTool {
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
    fn test_elicitation_cancelled_serialization() {
        let ctx = serde_json::json!({"step": 1});
        let envelope = PhpEnvelope {
            session_id: Some("s1"),
            request_id: "r1",
            request: PhpRequest::ElicitationCancelled {
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
