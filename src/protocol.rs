use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PhpRequest<'a> {
    Discover,
    CallTool {
        name: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        arguments: Option<&'a serde_json::Map<String, serde_json::Value>>,
    },
    ReadResource {
        uri: &'a str,
    },
    GetPrompt {
        name: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        arguments: Option<&'a serde_json::Map<String, serde_json::Value>>,
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
        };
        let json: serde_json::Value = serde_json::to_value(&req).unwrap();
        assert_eq!(json["type"], "call_tool");
        assert_eq!(json["name"], "get_weather");
        assert_eq!(json["arguments"]["city"], "Moscow");
    }

    #[test]
    fn test_call_tool_no_arguments() {
        let req = PhpRequest::CallTool {
            name: "ping",
            arguments: None,
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
}
