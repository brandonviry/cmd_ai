//! LLM provider types and the `ChatBackend` trait (see contracts/rust-api.md).
pub mod client;
pub mod sse;

use std::fmt;
use std::sync::atomic::AtomicBool;

use serde_json::{Map, Value, json};

/// Author of a chat message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    /// Role string on the OpenAI wire format.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// A tool call requested by the model (arguments are raw JSON text).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

impl ToolCall {
    /// OpenAI wire format: `{"id", "type": "function", "function": {"name", "arguments"}}`.
    /// `arguments` stays a JSON *string* (an object is rejected by Ollama's `/v1`).
    pub fn to_wire(&self) -> Value {
        json!({
            "id": self.id,
            "type": "function",
            "function": {"name": self.name, "arguments": self.arguments},
        })
    }
}

/// One message of the conversation history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_call_id: Option<String>,
}

/// Anything accepted as the content of an assistant message: `String`, `&str`,
/// `&String`, `Option<String>`, `Option<&str>`. An empty text means "no content".
pub trait MessageContent {
    fn into_content(self) -> Option<String>;
}

fn non_empty(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}

impl MessageContent for String {
    fn into_content(self) -> Option<String> {
        non_empty(self)
    }
}

impl MessageContent for &str {
    fn into_content(self) -> Option<String> {
        non_empty(self.to_string())
    }
}

impl MessageContent for &String {
    fn into_content(self) -> Option<String> {
        non_empty(self.clone())
    }
}

impl MessageContent for Option<String> {
    fn into_content(self) -> Option<String> {
        self.and_then(non_empty)
    }
}

impl MessageContent for Option<&str> {
    fn into_content(self) -> Option<String> {
        self.and_then(|s| non_empty(s.to_string()))
    }
}

impl Message {
    fn text(role: Role, content: String) -> Self {
        Message {
            role,
            content: Some(content),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    pub fn system(s: impl Into<String>) -> Self {
        Message::text(Role::System, s.into())
    }

    pub fn user(s: impl Into<String>) -> Self {
        Message::text(Role::User, s.into())
    }

    /// Assistant turn; `content` may be empty when the model only called tools.
    pub fn assistant(content: impl MessageContent, tool_calls: impl Into<Vec<ToolCall>>) -> Self {
        Message {
            role: Role::Assistant,
            content: content.into_content(),
            tool_calls: tool_calls.into(),
            tool_call_id: None,
        }
    }

    /// Result of the tool call `id`.
    pub fn tool(id: impl Into<String>, content: impl Into<String>) -> Self {
        Message {
            role: Role::Tool,
            content: Some(content.into()),
            tool_calls: Vec::new(),
            tool_call_id: Some(id.into()),
        }
    }

    /// OpenAI wire format of this message. An assistant message with tool calls has
    /// `"content": null` unless it also carries text; every other message has a
    /// string content (empty when `content` is `None`).
    pub fn to_wire(&self) -> Value {
        let mut m = Map::new();
        m.insert("role".into(), Value::from(self.role.as_str()));
        let text = self.content.clone().unwrap_or_default();
        if self.role == Role::Assistant && !self.tool_calls.is_empty() {
            let content = if text.is_empty() {
                Value::Null
            } else {
                Value::String(text)
            };
            m.insert("content".into(), content);
            let calls = self.tool_calls.iter().map(ToolCall::to_wire).collect();
            m.insert("tool_calls".into(), Value::Array(calls));
        } else {
            if self.role == Role::Tool {
                let id = self.tool_call_id.clone().unwrap_or_default();
                m.insert("tool_call_id".into(), Value::String(id));
            }
            m.insert("content".into(), Value::String(text));
        }
        Value::Object(m)
    }
}

impl serde::Serialize for Message {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_wire().serialize(serializer)
    }
}

/// One request to the model.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    /// OpenAI `tools` array (see `tool::schema()`); `Null` or `[]` means no tools.
    pub tools: Value,
    pub max_tokens: u32,
}

impl ChatRequest {
    /// JSON body for `POST {base_url}/chat/completions`. With `stream`, also asks for
    /// the final usage chunk (`stream_options.include_usage`).
    pub fn to_json(&self, stream: bool) -> Value {
        let mut m = Map::new();
        m.insert("model".into(), Value::String(self.model.clone()));
        let messages = self.messages.iter().map(Message::to_wire).collect();
        m.insert("messages".into(), Value::Array(messages));
        let no_tools = match &self.tools {
            Value::Null => true,
            Value::Array(a) => a.is_empty(),
            _ => false,
        };
        if !no_tools {
            m.insert("tools".into(), self.tools.clone());
        }
        if self.max_tokens > 0 {
            m.insert("max_tokens".into(), Value::from(self.max_tokens));
        }
        m.insert("stream".into(), Value::Bool(stream));
        if stream {
            m.insert("stream_options".into(), json!({"include_usage": true}));
        }
        Value::Object(m)
    }
}

/// Incremental output while the response streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    Content(String),
    Reasoning(String),
}

/// Token counts reported by the provider (`stream_options.include_usage`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

/// The assembled answer of the model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatResponse {
    pub content: String,
    pub reasoning: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
    pub finish_reason: Option<String>,
}

/// Provider failures, classified for the retry policy of the agent (FR-051).
/// Texts are short technical English; the French UI messages come from `text.rs`.
/// The client never puts the API key in these texts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmError {
    /// 401/403: key missing, invalid or revoked.
    Auth(String),
    /// 402: credits exhausted.
    Credits(String),
    /// 429 worth retrying: concurrency slot, or `Retry-After` given (seconds when numeric).
    RateLimited {
        retry_after: Option<u64>,
        message: String,
    },
    /// 429 usage limit: retrying does not help.
    Quota(String),
    /// 404 about the model.
    ModelNotFound(String),
    /// Any other HTTP status (5xx and unexpected 4xx), or an error reported inside the stream.
    Server(u16, String),
    /// Connection, DNS, proxy or timeout failure.
    Network(String),
    /// Certificate or TLS handshake failure (TLS inspection proxies, `network.ca_file`).
    Tls(String),
    /// The body could not be understood.
    BadResponse(String),
    /// No content and no tool call.
    Empty,
    /// The cancel flag was raised.
    Cancelled,
}

fn write_with_detail(f: &mut fmt::Formatter<'_>, label: &str, detail: &str) -> fmt::Result {
    if detail.is_empty() {
        f.write_str(label)
    } else {
        write!(f, "{label}: {detail}")
    }
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LlmError::Auth(m) => write_with_detail(f, "authentication rejected", m),
            LlmError::Credits(m) => write_with_detail(f, "credits exhausted", m),
            LlmError::RateLimited {
                retry_after: Some(s),
                message,
            } => write_with_detail(f, &format!("rate limited (retry after {s} s)"), message),
            LlmError::RateLimited {
                retry_after: None,
                message,
            } => write_with_detail(f, "rate limited", message),
            LlmError::Quota(m) => write_with_detail(f, "usage quota reached", m),
            LlmError::ModelNotFound(m) => write_with_detail(f, "model not found", m),
            LlmError::Server(code, m) => write_with_detail(f, &format!("server error {code}"), m),
            LlmError::Network(m) => write_with_detail(f, "network error", m),
            LlmError::Tls(m) => write_with_detail(f, "TLS error", m),
            LlmError::BadResponse(m) => write_with_detail(f, "invalid response", m),
            LlmError::Empty => f.write_str("empty response (no content and no tool call)"),
            LlmError::Cancelled => f.write_str("request cancelled"),
        }
    }
}

impl std::error::Error for LlmError {}

/// A chat completion provider. The real one is `client::OpenAiClient`; tests use fakes.
/// One request at a time; retries are decided by the caller (the agent).
pub trait ChatBackend {
    fn chat(
        &self,
        req: &ChatRequest,
        on_event: &mut dyn FnMut(StreamEvent),
        cancel: &AtomicBool,
    ) -> Result<ChatResponse, LlmError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(id: &str, args: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: "run_command".to_string(),
            arguments: args.to_string(),
        }
    }

    #[test]
    fn role_strings_match_openai() {
        assert_eq!(Role::System.as_str(), "system");
        assert_eq!(Role::User.as_str(), "user");
        assert_eq!(Role::Assistant.as_str(), "assistant");
        assert_eq!(Role::Tool.as_str(), "tool");
    }

    #[test]
    fn constructors_set_role_and_content() {
        let s = Message::system("règles");
        assert_eq!(s.role, Role::System);
        assert_eq!(s.content.as_deref(), Some("règles"));
        assert!(s.tool_calls.is_empty());
        assert_eq!(s.tool_call_id, None);

        let u = Message::user(String::from("affiche la date"));
        assert_eq!(u.role, Role::User);
        assert_eq!(u.content.as_deref(), Some("affiche la date"));

        let t = Message::tool("call_1", "code de sortie : 0");
        assert_eq!(t.role, Role::Tool);
        assert_eq!(t.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(t.content.as_deref(), Some("code de sortie : 0"));
    }

    #[test]
    fn assistant_accepts_string_str_and_options() {
        let a = Message::assistant(String::from("ok"), Vec::new());
        assert_eq!(a.role, Role::Assistant);
        assert_eq!(a.content.as_deref(), Some("ok"));
        let b = Message::assistant("ok", Vec::new());
        assert_eq!(b.content.as_deref(), Some("ok"));
        let c = Message::assistant(Some("ok".to_string()), Vec::new());
        assert_eq!(c.content.as_deref(), Some("ok"));
        let d = Message::assistant(None::<String>, vec![call("c1", "{}")]);
        assert_eq!(d.content, None);
        assert_eq!(d.tool_calls.len(), 1);
        let owned = String::new();
        let e = Message::assistant(&owned, Vec::new());
        assert_eq!(e.content, None, "empty text means no content");
        let calls = [call("c1", "{}")];
        let f = Message::assistant(Some(""), &calls[..]);
        assert_eq!(f.content, None);
        assert_eq!(f.tool_calls, calls.to_vec());
    }

    #[test]
    fn simple_messages_serialize_to_openai_shape() {
        assert_eq!(
            Message::system("sys").to_wire(),
            json!({"role": "system", "content": "sys"})
        );
        assert_eq!(
            Message::user("bonjour é").to_wire(),
            json!({"role": "user", "content": "bonjour é"})
        );
        assert_eq!(
            Message::assistant("réponse", Vec::new()).to_wire(),
            json!({"role": "assistant", "content": "réponse"})
        );
    }

    #[test]
    fn assistant_with_tool_calls_has_null_content() {
        let m = Message::assistant("", vec![call("call_9", r#"{"command":"ls"}"#)]);
        assert_eq!(
            m.to_wire(),
            json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_9",
                    "type": "function",
                    "function": {"name": "run_command", "arguments": "{\"command\":\"ls\"}"}
                }]
            })
        );
    }

    #[test]
    fn assistant_with_tool_calls_keeps_non_empty_text() {
        let m = Message::assistant("Je regarde.", vec![call("a", "{}"), call("b", "{}")]);
        let w = m.to_wire();
        assert_eq!(w["content"], json!("Je regarde."));
        assert_eq!(w["tool_calls"].as_array().map(Vec::len), Some(2));
        assert_eq!(w["tool_calls"][1]["id"], json!("b"));
    }

    #[test]
    fn tool_message_carries_tool_call_id() {
        assert_eq!(
            Message::tool("call_1", "sortie").to_wire(),
            json!({"role": "tool", "tool_call_id": "call_1", "content": "sortie"})
        );
    }

    #[test]
    fn missing_content_serializes_as_empty_string_without_tool_calls() {
        let m = Message {
            role: Role::User,
            content: None,
            tool_calls: Vec::new(),
            tool_call_id: None,
        };
        assert_eq!(m.to_wire(), json!({"role": "user", "content": ""}));
    }

    #[test]
    fn serde_serialize_matches_wire() {
        let m = Message::assistant("", vec![call("x", "{}")]);
        assert_eq!(serde_json::to_value(&m).unwrap(), m.to_wire());
    }

    #[test]
    fn tool_call_wire_shape() {
        assert_eq!(
            call("id1", "{\"a\":1}").to_wire(),
            json!({"id": "id1", "type": "function", "function": {"name": "run_command", "arguments": "{\"a\":1}"}})
        );
    }

    fn request(tools: Value) -> ChatRequest {
        ChatRequest {
            model: "modele-test".to_string(),
            messages: vec![Message::system("s"), Message::user("u")],
            tools,
            max_tokens: 4096,
        }
    }

    #[test]
    fn streaming_body_has_all_fields() {
        let tools = json!([{"type": "function", "function": {"name": "run_command"}}]);
        let body = request(tools.clone()).to_json(true);
        assert_eq!(body["model"], json!("modele-test"));
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["stream_options"], json!({"include_usage": true}));
        assert_eq!(body["max_tokens"], json!(4096));
        assert_eq!(body["tools"], tools);
        assert_eq!(
            body["messages"],
            json!([{"role": "system", "content": "s"}, {"role": "user", "content": "u"}])
        );
    }

    #[test]
    fn non_streaming_body_has_no_stream_options() {
        let body = request(Value::Null).to_json(false);
        assert_eq!(body["stream"], json!(false));
        assert!(body.get("stream_options").is_none());
    }

    #[test]
    fn null_or_empty_tools_are_omitted() {
        assert!(request(Value::Null).to_json(true).get("tools").is_none());
        assert!(request(json!([])).to_json(true).get("tools").is_none());
    }

    #[test]
    fn zero_max_tokens_is_omitted() {
        let mut r = request(Value::Null);
        r.max_tokens = 0;
        assert!(r.to_json(true).get("max_tokens").is_none());
    }

    #[test]
    fn errors_display_short_technical_text() {
        let cases = [
            (LlmError::Auth("bad key".into()), "authentication"),
            (LlmError::Credits("none left".into()), "credits"),
            (
                LlmError::RateLimited {
                    retry_after: Some(7),
                    message: "slow down".into(),
                },
                "retry after 7 s",
            ),
            (LlmError::Quota("limit".into()), "quota"),
            (LlmError::ModelNotFound("m".into()), "model not found"),
            (LlmError::Server(503, "down".into()), "503"),
            (LlmError::Network("refused".into()), "network"),
            (LlmError::Tls("UnknownIssuer".into()), "TLS"),
            (LlmError::BadResponse("garbage".into()), "invalid response"),
            (LlmError::Empty, "empty response"),
            (LlmError::Cancelled, "cancelled"),
        ];
        for (err, needle) in cases {
            let text = err.to_string();
            assert!(text.contains(needle), "{text:?} should contain {needle:?}");
        }
        assert!(
            LlmError::Server(500, "boom".into())
                .to_string()
                .contains("boom")
        );
        assert_eq!(
            LlmError::Auth(String::new()).to_string(),
            "authentication rejected"
        );
        let no_delay = LlmError::RateLimited {
            retry_after: None,
            message: "busy".into(),
        };
        assert_eq!(no_delay.to_string(), "rate limited: busy");
    }
}
