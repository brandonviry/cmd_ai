//! OpenAI-compatible client (ureq).
//!
//! One blocking `ureq::Agent` per client: `POST {base_url}/chat/completions` with
//! `stream: true`, the SSE body read line by line, content / reasoning / tool-call
//! deltas assembled into a [`ChatResponse`]. A non-streamed JSON body is accepted too.
//! The exchange runs on a worker thread so that the cancel flag is honoured within
//! [`CANCEL_POLL`] even while the server is silent. Errors are classified (FR-051) but
//! never retried here: the agent owns the retry policy. No error text ever contains the
//! API key (every text goes through a [`Redactor`] holding it).

use std::collections::HashMap;
use std::fmt;
use std::io::{self, BufRead, BufReader, Read};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use serde_json::Value;
use ureq::tls::{PemItem, RootCerts, TlsConfig};
use ureq::{Agent, Proxy};

use super::sse::{SseEvent, SseParser};
use super::{ChatBackend, ChatRequest, ChatResponse, LlmError, StreamEvent, ToolCall, Usage};
use crate::secret::{MASK, Redactor, Secret};

/// Longest silence accepted between two reads of the response (stream-idle watchdog:
/// a queued request on the server side must not freeze cmd_ai for minutes). Capped by
/// the request timeout.
pub const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
/// How often the cancel flag is checked while waiting for the server.
pub const CANCEL_POLL: Duration = Duration::from_millis(50);
/// Longest accepted line of the response body.
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
/// Longest accepted response body.
const MAX_BODY_BYTES: u64 = 64 * 1024 * 1024;
/// Part of an error body that is read.
const MAX_ERROR_BODY_BYTES: u64 = 64 * 1024;
/// Error messages are cut to this many characters.
const MAX_MESSAGE_CHARS: usize = 500;
/// Characters of an unparsable chunk quoted in the error.
const MAX_SNIPPET_CHARS: usize = 120;
const LINE_TOO_LONG: &str = "line too long";
const USER_AGENT: &str = concat!("cmd_ai/", env!("CARGO_PKG_VERSION"));

/// Connection settings of the provider.
#[derive(Clone)]
pub struct ClientOptions {
    /// `https://…/v1` (`http://` only to a loopback address), without the endpoint.
    pub base_url: String,
    /// Sent as `Authorization: Bearer …` when not empty.
    pub api_key: Secret,
    /// Explicit proxy URL; `None` = `ALL_PROXY`/`HTTPS_PROXY`/`HTTP_PROXY` (+ `NO_PROXY`),
    /// then the Windows system proxy.
    pub proxy: Option<String>,
    /// PEM file whose certificates replace the platform roots (see [`OpenAiClient::new`]).
    /// Already resolved by the caller (`config.json` gives it relative to the folder).
    pub ca_file: Option<PathBuf>,
    pub connect_timeout: Duration,
    /// Whole request, from connection to the last byte of the answer.
    pub request_timeout: Duration,
}

impl fmt::Debug for ClientOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientOptions")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key)
            .field("proxy", &self.proxy.as_deref().map(mask_url_credentials))
            .field("ca_file", &self.ca_file)
            .field("connect_timeout", &self.connect_timeout)
            .field("request_timeout", &self.request_timeout)
            .finish()
    }
}

/// Blocking OpenAI-compatible chat client.
pub struct OpenAiClient {
    endpoint: Arc<Endpoint>,
}

/// What a request needs, shared with the worker thread.
struct Endpoint {
    agent: Agent,
    url: String,
    api_key: Secret,
    redactor: Redactor,
}

/// Messages from the worker thread to the caller of `chat`.
enum WorkerMsg {
    Event(StreamEvent),
    Done(Result<ChatResponse, LlmError>),
}

impl fmt::Debug for OpenAiClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiClient")
            .field("url", &self.endpoint.url)
            .finish_non_exhaustive()
    }
}

impl OpenAiClient {
    /// Builds the HTTP agent; does no network I/O.
    ///
    /// - Timeouts: `connect_timeout` for the connection (proxy and TLS handshake
    ///   included), `request_timeout` for the whole request, and a per-read timeout of
    ///   [`STREAM_IDLE_TIMEOUT`] (at most `request_timeout`) suited to streaming.
    ///   A zero duration disables that timeout.
    /// - TLS: certificates are always verified. By default with the platform verifier
    ///   (OS trust store). With `ca_file`, the PEM certificates it contains are the ONLY
    ///   trusted roots: this is meant for corporate TLS inspection, where every HTTPS
    ///   connection is re-signed by the company CA and that CA is the only one that
    ///   matters. Put several certificates in the file to trust several roots.
    /// - Proxy: `Some(url)` is used as is; `None` relies on the environment variables,
    ///   then on the Windows system proxy (`win-system-proxy`). A loopback `base_url`
    ///   (local Ollama) never goes through an automatically detected proxy.
    /// - Redirects are never followed: the key and the conversation only go to
    ///   `base_url` (constitution VI); a 3xx answer is reported as an error.
    /// - The API key is refused over plain HTTP to anything but a loopback address.
    pub fn new(opts: ClientOptions) -> Result<Self, LlmError> {
        let base = opts.base_url.trim().trim_end_matches('/');
        let Some((scheme, rest)) = base.split_once("://") else {
            return Err(LlmError::Network(
                "base_url must start with https:// (or http:// for a loopback address)".into(),
            ));
        };
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "https" && scheme != "http" {
            return Err(LlmError::Network(format!(
                "unsupported base_url scheme {scheme:?}: use https://"
            )));
        }
        let host = url_host(rest);
        if host.is_empty() {
            return Err(LlmError::Network("base_url has no host".into()));
        }
        let loopback = is_loopback(&host);
        if scheme == "http" && !loopback && !opts.api_key.is_empty() {
            return Err(LlmError::Network(
                "refusing to send the API key over plain HTTP to a non-loopback host: use https://"
                    .into(),
            ));
        }
        let url = format!("{scheme}://{rest}/chat/completions");
        let redactor = build_redactor(&opts);

        let root_certs = match &opts.ca_file {
            Some(path) => load_ca_file(path)?,
            None => RootCerts::PlatformVerifier,
        };
        let explicit_proxy = opts
            .proxy
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty());
        let proxy = match explicit_proxy {
            Some(p) => Some(Proxy::new(p).map_err(|e| {
                LlmError::Network(redactor.redact(&format!(
                    "invalid proxy URL in network.proxy (expected http://[user:password@]host:port): {e}"
                )))
            })?),
            None if loopback => None,
            None => Proxy::try_from_env(),
        };
        let request_timeout = non_zero(opts.request_timeout);
        let per_read = request_timeout.map_or(STREAM_IDLE_TIMEOUT, |t| t.min(STREAM_IDLE_TIMEOUT));
        let config = Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_connect(non_zero(opts.connect_timeout))
            .timeout_global(request_timeout)
            .timeout_per_read(Some(per_read))
            .tls_config(TlsConfig::builder().root_certs(root_certs).build())
            .proxy(proxy)
            .user_agent(USER_AGENT)
            .build();
        Ok(OpenAiClient {
            endpoint: Arc::new(Endpoint {
                agent: Agent::new_with_config(config),
                url,
                api_key: opts.api_key,
                redactor,
            }),
        })
    }

    /// Runs the exchange on a worker thread and relays its events; returns
    /// `Cancelled` within [`CANCEL_POLL`] of the flag being raised. An abandoned worker
    /// stops at its next read, at the latest when a timeout fires.
    fn chat_inner(
        &self,
        req: &ChatRequest,
        on_event: &mut dyn FnMut(StreamEvent),
        cancel: &AtomicBool,
    ) -> Result<ChatResponse, LlmError> {
        if is_cancelled(cancel) {
            return Err(LlmError::Cancelled);
        }
        let body = req.to_json(true).to_string();
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let endpoint = Arc::clone(&self.endpoint);
        thread::Builder::new()
            .name("cmd_ai-llm".into())
            .spawn(move || {
                let events = tx.clone();
                let mut relay = |e: StreamEvent| {
                    // The caller may be gone (cancelled): nothing to do then.
                    let _ = events.send(WorkerMsg::Event(e));
                };
                let result = endpoint.exchange(&body, &mut relay, &worker_stop);
                let _ = tx.send(WorkerMsg::Done(result));
            })
            .map_err(|e| LlmError::Network(format!("cannot start the request thread: {e}")))?;
        loop {
            if is_cancelled(cancel) {
                stop.store(true, Ordering::SeqCst);
                return Err(LlmError::Cancelled);
            }
            match rx.recv_timeout(CANCEL_POLL) {
                Ok(WorkerMsg::Event(e)) => on_event(e),
                Ok(WorkerMsg::Done(result)) => {
                    return if is_cancelled(cancel) {
                        Err(LlmError::Cancelled)
                    } else {
                        result
                    };
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(LlmError::BadResponse(
                        "the request thread ended without a result".into(),
                    ));
                }
            }
        }
    }
}

impl Endpoint {
    /// One HTTP exchange, on the worker thread. `cancel` is checked between lines.
    fn exchange(
        &self,
        body: &str,
        on_event: &mut dyn FnMut(StreamEvent),
        cancel: &AtomicBool,
    ) -> Result<ChatResponse, LlmError> {
        let mut request = self
            .agent
            .post(self.url.as_str())
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream, application/json");
        if !self.api_key.is_empty() {
            request = request.header("Authorization", format!("Bearer {}", self.api_key.expose()));
        }
        let response = request
            .send(body.as_bytes())
            .map_err(|e| transport_error(&e))?;
        let status = response.status().as_u16();
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let retry_after = header("retry-after");
        let location = header("location");
        let content_type = header("content-type")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let reader = response.into_body().into_reader();
        if (300..400).contains(&status) {
            let location = location.unwrap_or_default();
            return Err(LlmError::Server(
                status,
                clean_message(&self.redactor.redact(&format!(
                    "redirect not followed (Location: {location}); check provider.base_url"
                ))),
            ));
        }
        if !(200..300).contains(&status) {
            let text = read_text_limited(reader, MAX_ERROR_BODY_BYTES);
            let text = self.redactor.redact(&text);
            return Err(classify_http_error(status, &text, retry_after.as_deref()));
        }
        let json_body = content_type.contains("json") && !content_type.contains("event-stream");
        let mut assembler = Assembler::default();
        read_body(reader, json_body, &mut assembler, on_event, cancel)?;
        if is_cancelled(cancel) {
            return Err(LlmError::Cancelled);
        }
        assembler.finish()
    }
}

impl ChatBackend for OpenAiClient {
    fn chat(
        &self,
        req: &ChatRequest,
        on_event: &mut dyn FnMut(StreamEvent),
        cancel: &AtomicBool,
    ) -> Result<ChatResponse, LlmError> {
        self.chat_inner(req, on_event, cancel)
            .map_err(|e| redact_error(&self.endpoint.redactor, e))
    }
}

fn is_cancelled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::SeqCst)
}

fn non_zero(d: Duration) -> Option<Duration> {
    if d.is_zero() { None } else { Some(d) }
}

/// Lower-case host of `authority/path` (brackets of an IPv6 literal removed).
fn url_host(rest: &str) -> String {
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = match host_port.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => host_port.split(':').next().unwrap_or_default(),
    };
    host.to_ascii_lowercase()
}

fn is_loopback(host: &str) -> bool {
    host == "localhost"
        || host == "::1"
        || host.parse::<Ipv4Addr>().is_ok_and(|ip| ip.is_loopback())
}

/// `user:password` part of a URL, if any.
fn url_userinfo(url: &str) -> Option<&str> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    authority.rsplit_once('@').map(|(userinfo, _)| userinfo)
}

/// The URL with its `user:password@` part replaced by the mask.
fn mask_url_credentials(url: &str) -> String {
    match url_userinfo(url) {
        Some(userinfo) => url.replacen(&format!("{userinfo}@"), &format!("{MASK}@"), 1),
        None => url.to_string(),
    }
}

/// Masks the API key and the proxy credentials.
fn build_redactor(opts: &ClientOptions) -> Redactor {
    let mut secrets = vec![opts.api_key.expose()];
    if let Some(userinfo) = opts.proxy.as_deref().and_then(url_userinfo) {
        secrets.push(userinfo);
        if let Some((_, password)) = userinfo.split_once(':') {
            secrets.push(password);
        }
    }
    Redactor::new(&secrets)
}

/// Reads the PEM certificates of `network.ca_file` (see [`OpenAiClient::new`]: they
/// replace the platform roots, verification stays on).
fn load_ca_file(path: &Path) -> Result<RootCerts, LlmError> {
    let shown = path.display();
    let bytes = std::fs::read(path)
        .map_err(|e| LlmError::Tls(format!("cannot read CA file {shown}: {e}")))?;
    let mut certs = Vec::new();
    for item in ureq::tls::parse_pem(&bytes) {
        match item {
            Ok(PemItem::Certificate(cert)) => certs.push(cert),
            Ok(_) => {}
            Err(e) => {
                return Err(LlmError::Tls(format!(
                    "invalid PEM in CA file {shown}: {e}"
                )));
            }
        }
    }
    if certs.is_empty() {
        return Err(LlmError::Tls(format!(
            "no certificate found in CA file {shown}"
        )));
    }
    Ok(RootCerts::new_with_certs(&certs))
}

fn read_text_limited(reader: impl Read, limit: u64) -> String {
    let mut buf = Vec::new();
    // A broken error body still gives what was read.
    let _ = reader.take(limit).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

/// Reads one line (terminator included) into `buf`; fails once the line exceeds `cap`
/// bytes. Returns the number of bytes read, 0 at end of input.
fn read_line_capped(reader: &mut impl BufRead, buf: &mut Vec<u8>, cap: usize) -> io::Result<usize> {
    let mut total = 0;
    loop {
        let available = match reader.fill_buf() {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if available.is_empty() {
            return Ok(total);
        }
        let (used, complete) = match available.iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true),
            None => (available.len(), false),
        };
        buf.extend_from_slice(&available[..used]);
        reader.consume(used);
        total += used;
        if buf.len() > cap {
            return Err(io::Error::new(io::ErrorKind::InvalidData, LINE_TOO_LONG));
        }
        if complete {
            return Ok(total);
        }
    }
}

/// Reads the body line by line: SSE, or JSON (a whole completion, or one chunk per
/// line) when the content type says so or the first line starts with `{`.
fn read_body(
    reader: impl Read,
    json_body: bool,
    assembler: &mut Assembler,
    on_event: &mut dyn FnMut(StreamEvent),
    cancel: &AtomicBool,
) -> Result<(), LlmError> {
    let mut reader = BufReader::new(reader);
    let mut parser = SseParser::new();
    let mut raw_json: Option<String> = json_body.then(String::new);
    let mut sse_started = false;
    let mut line = Vec::new();
    let mut total: u64 = 0;
    loop {
        if is_cancelled(cancel) {
            return Err(LlmError::Cancelled);
        }
        line.clear();
        let n = read_line_capped(&mut reader, &mut line, MAX_LINE_BYTES).map_err(|e| {
            if e.kind() == io::ErrorKind::InvalidData && e.to_string() == LINE_TOO_LONG {
                LlmError::BadResponse(format!("response line longer than {MAX_LINE_BYTES} bytes"))
            } else {
                io_error(&e)
            }
        })?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_BODY_BYTES {
            return Err(LlmError::BadResponse(format!(
                "response body larger than {MAX_BODY_BYTES} bytes"
            )));
        }
        let text = String::from_utf8_lossy(&line);
        if let Some(raw) = raw_json.as_mut() {
            raw.push_str(&text);
            continue;
        }
        if !sse_started {
            let start = text.trim_start_matches('\u{feff}').trim_start();
            if start.is_empty() {
                continue;
            }
            if start.starts_with('{') {
                raw_json = Some(text.into_owned());
                continue;
            }
            sse_started = true;
        }
        match parser.push_line(&text) {
            Some(SseEvent::Done) => return Ok(()),
            Some(SseEvent::Data(data)) => handle_data(&data, assembler, on_event)?,
            None => {}
        }
    }
    if let Some(raw) = raw_json {
        return handle_data(&raw, assembler, on_event);
    }
    if let Some(SseEvent::Data(data)) = parser.finish() {
        handle_data(&data, assembler, on_event)?;
    }
    Ok(())
}

/// One JSON document; or, when that fails, one JSON document per line (servers that
/// omit the blank line between SSE events, newline-delimited JSON).
fn handle_data(
    data: &str,
    assembler: &mut Assembler,
    on_event: &mut dyn FnMut(StreamEvent),
) -> Result<(), LlmError> {
    let data = data.trim();
    if data.is_empty() {
        return Ok(());
    }
    let error = match serde_json::from_str::<Value>(data) {
        Ok(value) => return assembler.chunk(&value, on_event),
        Err(e) => e,
    };
    if data.contains('\n') {
        let values: Option<Vec<Value>> = data
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str(l.strip_prefix("data:").unwrap_or(l).trim()).ok())
            .collect();
        if let Some(values) = values {
            for value in &values {
                assembler.chunk(value, on_event)?;
            }
            return Ok(());
        }
    }
    let snippet: String = data.chars().take(MAX_SNIPPET_CHARS).collect();
    Err(LlmError::BadResponse(format!(
        "invalid JSON in response ({error}): {snippet}"
    )))
}

/// A tool call being assembled from deltas.
#[derive(Default)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
}

/// Accumulates the chunks of one response.
#[derive(Default)]
struct Assembler {
    content: String,
    reasoning: String,
    calls: Vec<PartialCall>,
    /// Delta `index` → position in `calls` (the latest call with that index).
    by_index: HashMap<u64, usize>,
    usage: Option<Usage>,
    finish_reason: Option<String>,
}

impl Assembler {
    /// One streamed chunk (`choices[0].delta`) or a whole completion (`choices[0].message`).
    fn chunk(&mut self, v: &Value, on_event: &mut dyn FnMut(StreamEvent)) -> Result<(), LlmError> {
        if let Some(error) = v.get("error").filter(|e| !e.is_null()) {
            return Err(stream_error(error));
        }
        if let Some(usage) = v.get("usage").filter(|u| u.is_object()) {
            let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
            self.usage = Some(Usage {
                prompt_tokens: count("prompt_tokens"),
                completion_tokens: count("completion_tokens"),
            });
        }
        let Some(choice) = v
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
        else {
            return Ok(());
        };
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish_reason = Some(reason.to_string());
        }
        let Some(delta) = choice.get("delta").or_else(|| choice.get("message")) else {
            return Ok(());
        };
        // Ollama sends `reasoning`, others `reasoning_content`.
        let reasoning = text_of(delta.get("reasoning"))
            .filter(|s| !s.is_empty())
            .or_else(|| text_of(delta.get("reasoning_content")));
        if let Some(r) = reasoning.filter(|s| !s.is_empty()) {
            self.reasoning.push_str(&r);
            on_event(StreamEvent::Reasoning(r));
        }
        if let Some(c) = text_of(delta.get("content")).filter(|s| !s.is_empty()) {
            self.content.push_str(&c);
            on_event(StreamEvent::Content(c));
        }
        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for (position, call) in calls.iter().enumerate() {
                self.tool_call_delta(position, call);
            }
        }
        Ok(())
    }

    /// Merges one `tool_calls[]` delta: by `index` (array position when absent); the
    /// same index with a different id starts a new call (seen with some models).
    fn tool_call_delta(&mut self, position: usize, call: &Value) {
        let index = call
            .get("index")
            .and_then(Value::as_u64)
            .unwrap_or(position as u64);
        let id = call.get("id").and_then(Value::as_str).unwrap_or_default();
        let function = call.get("function");
        let name = function
            .and_then(|f| f.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let arguments = match function.and_then(|f| f.get("arguments")) {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            // Some servers send the arguments as a JSON object.
            Some(other) => Some(other.to_string()),
        };
        let calls = &self.calls;
        let existing = self.by_index.get(&index).copied().filter(|&slot| {
            calls
                .get(slot)
                .is_some_and(|c| id.is_empty() || c.id.is_empty() || c.id == id)
        });
        let slot = match existing {
            Some(slot) => slot,
            None => {
                self.calls.push(PartialCall::default());
                let slot = self.calls.len() - 1;
                self.by_index.insert(index, slot);
                slot
            }
        };
        let Some(entry) = self.calls.get_mut(slot) else {
            return;
        };
        if entry.id.is_empty() {
            entry.id = id.to_string();
        }
        // A repeated full name is ignored; name pieces are concatenated.
        if !name.is_empty() && entry.name != name {
            entry.name.push_str(name);
        }
        if let Some(a) = arguments {
            entry.arguments.push_str(&a);
        }
    }

    fn finish(self) -> Result<ChatResponse, LlmError> {
        let tool_calls: Vec<ToolCall> = self
            .calls
            .into_iter()
            .filter(|c| !(c.name.trim().is_empty() && c.arguments.trim().is_empty()))
            .enumerate()
            .map(|(i, c)| ToolCall {
                id: if c.id.is_empty() {
                    format!("call_{i}")
                } else {
                    c.id
                },
                name: c.name,
                arguments: c.arguments,
            })
            .collect();
        if self.content.trim().is_empty() && tool_calls.is_empty() {
            return Err(LlmError::Empty);
        }
        Ok(ChatResponse {
            content: self.content,
            reasoning: self.reasoning,
            tool_calls,
            usage: self.usage,
            finish_reason: self.finish_reason,
        })
    }
}

/// A string, or an array of `{"type": "text", "text": …}` parts.
fn text_of(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Array(parts) => Some(
            parts
                .iter()
                .filter_map(|p| p.as_str().or_else(|| p.get("text").and_then(Value::as_str)))
                .collect(),
        ),
        _ => None,
    }
}

/// An `{"error": …}` object received with a 200 status (inside the stream).
fn stream_error(error: &Value) -> LlmError {
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .map_or_else(|| error.to_string(), str::to_string);
    let code = ["code", "status"]
        .iter()
        .find_map(|k| {
            let v = error.get(*k)?;
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        })
        .and_then(|c| u16::try_from(c).ok())
        .filter(|c| (400..600).contains(c));
    match code {
        Some(status) => classify_http_error(status, &message, None),
        None => LlmError::Server(500, clean_message(&message)),
    }
}

fn transport_error(e: &ureq::Error) -> LlmError {
    match e {
        ureq::Error::Io(io) => io_error(io),
        ureq::Error::Timeout(_) => LlmError::Network(timeout_text(&e.to_string())),
        ureq::Error::Tls(_) | ureq::Error::Pem(_) | ureq::Error::Rustls(_) => {
            LlmError::Tls(e.to_string())
        }
        _ => classify_transport_text(&e.to_string()),
    }
}

fn io_error(e: &io::Error) -> LlmError {
    if let Some(inner) = e.get_ref().and_then(|i| i.downcast_ref::<ureq::Error>()) {
        return transport_error(inner);
    }
    if e.kind() == io::ErrorKind::TimedOut {
        return LlmError::Network(timeout_text(&e.to_string()));
    }
    classify_transport_text(&e.to_string())
}

fn timeout_text(text: &str) -> String {
    if text.to_lowercase().contains("timeout") {
        text.to_string()
    } else {
        format!("timeout: {text}")
    }
}

/// Words of TLS failures (rustls texts included: they reach us as plain I/O errors).
const TLS_MARKERS: [&str; 10] = [
    "certificate",
    "unknownissuer",
    "handshake",
    "tls",
    "ssl",
    "corrupt message",
    "fatal alert",
    "peer is incompatible",
    "peer misbehaved",
    "decrypt",
];

/// Transport failure: TLS when the text mentions a certificate, the handshake or
/// another TLS failure, network otherwise.
pub fn classify_transport_text(text: &str) -> LlmError {
    let lower = text.to_lowercase();
    if TLS_MARKERS.iter().any(|w| lower.contains(w)) {
        LlmError::Tls(text.to_string())
    } else {
        LlmError::Network(text.to_string())
    }
}

/// Classifies an HTTP error status (FR-051). `body` must already be redacted.
/// 401/403 → `Auth`; 402 → `Credits`; 404 mentioning a model → `ModelNotFound`;
/// 429 → `RateLimited` when the body mentions "concurrent" or `Retry-After` is present,
/// else `Quota`; anything else → `Server(status, …)`.
pub fn classify_http_error(status: u16, body: &str, retry_after: Option<&str>) -> LlmError {
    let message = error_message(body);
    let haystack = body.to_lowercase();
    match status {
        401 | 403 => LlmError::Auth(message),
        402 => LlmError::Credits(message),
        404 if haystack.contains("model") => LlmError::ModelNotFound(message),
        429 => {
            let retry_after = retry_after.map(str::trim).filter(|v| !v.is_empty());
            if haystack.contains("concurrent") || retry_after.is_some() {
                LlmError::RateLimited {
                    retry_after: retry_after.and_then(parse_retry_after),
                    message,
                }
            } else {
                LlmError::Quota(message)
            }
        }
        _ => LlmError::Server(status, message),
    }
}

/// `Retry-After` in seconds (fractions rounded up); an HTTP date gives `None`.
pub fn parse_retry_after(value: &str) -> Option<u64> {
    let v = value.trim();
    v.parse::<u64>().ok().or_else(|| {
        v.parse::<f64>()
            .ok()
            .filter(|f| f.is_finite() && *f >= 0.0)
            .map(|f| f.ceil() as u64)
    })
}

/// The human part of an error body: `error.message`, `error` (string), `message` or
/// `detail` of a JSON body, else the body itself; one line, cut to 500 characters.
fn error_message(body: &str) -> String {
    let trimmed = body.trim();
    let extracted = serde_json::from_str::<Value>(trimmed)
        .ok()
        .and_then(|v| json_message(&v));
    clean_message(extracted.as_deref().unwrap_or(trimmed))
}

fn json_message(v: &Value) -> Option<String> {
    if let Some(error) = v.get("error") {
        if let Some(m) = error.get("message").and_then(Value::as_str) {
            return Some(m.to_string());
        }
        if let Some(m) = error.as_str() {
            return Some(m.to_string());
        }
    }
    ["message", "detail"]
        .iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str))
        .map(str::to_string)
}

fn clean_message(s: &str) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= MAX_MESSAGE_CHARS {
        return one_line;
    }
    let mut out: String = one_line.chars().take(MAX_MESSAGE_CHARS).collect();
    out.push('…');
    out
}

/// Last line of defense: the key (and proxy credentials) never leave in an error.
fn redact_error(r: &Redactor, e: LlmError) -> LlmError {
    match e {
        LlmError::Auth(m) => LlmError::Auth(r.redact(&m)),
        LlmError::Credits(m) => LlmError::Credits(r.redact(&m)),
        LlmError::RateLimited {
            retry_after,
            message,
        } => LlmError::RateLimited {
            retry_after,
            message: r.redact(&message),
        },
        LlmError::Quota(m) => LlmError::Quota(r.redact(&m)),
        LlmError::ModelNotFound(m) => LlmError::ModelNotFound(r.redact(&m)),
        LlmError::Server(code, m) => LlmError::Server(code, r.redact(&m)),
        LlmError::Network(m) => LlmError::Network(r.redact(&m)),
        LlmError::Tls(m) => LlmError::Tls(r.redact(&m)),
        LlmError::BadResponse(m) => LlmError::BadResponse(r.redact(&m)),
        LlmError::Empty => LlmError::Empty,
        LlmError::Cancelled => LlmError::Cancelled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{Message, ToolCall, Usage};
    use crate::secret::MASK;
    use serde_json::{Value, json};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, mpsc};
    use std::thread;
    use std::time::Instant;

    const KEY: &str = "sk-canary-0123456789abcdef";

    // ---- tiny scripted HTTP/1.1 server ---------------------------------------

    struct Reply {
        status: u16,
        headers: Vec<(String, String)>,
        /// Body pieces, each sent as one HTTP chunk after `delay` milliseconds.
        parts: Vec<(String, u64)>,
    }

    impl Reply {
        fn new(status: u16, content_type: Option<&str>, parts: Vec<(String, u64)>) -> Reply {
            let mut headers = Vec::new();
            if let Some(ct) = content_type {
                headers.push(("Content-Type".to_string(), ct.to_string()));
            }
            Reply {
                status,
                headers,
                parts,
            }
        }

        fn sse(parts: Vec<String>) -> Reply {
            Reply::new(
                200,
                Some("text/event-stream"),
                parts.into_iter().map(|p| (p, 0)).collect(),
            )
        }

        fn body(status: u16, content_type: &str, body: &str) -> Reply {
            Reply::new(status, Some(content_type), vec![(body.to_string(), 0)])
        }

        fn header(mut self, name: &str, value: &str) -> Reply {
            self.headers.push((name.to_string(), value.to_string()));
            self
        }
    }

    struct Seen {
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: String,
    }

    impl Seen {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        }

        fn json(&self) -> Value {
            serde_json::from_str(&self.body).unwrap()
        }
    }

    fn serve(reply: Reply) -> (String, mpsc::Receiver<Seen>) {
        serve_many(vec![reply])
    }

    /// One reply per accepted connection, in order; each connection has its thread.
    fn serve_many(replies: Vec<Reply>) -> (String, mpsc::Receiver<Seen>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for reply in replies {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                let tx = tx.clone();
                thread::spawn(move || handle(stream, reply, &tx));
            }
        });
        (format!("http://{addr}/v1"), rx)
    }

    fn reason(status: u16) -> &'static str {
        match status {
            200 => "OK",
            401 => "Unauthorized",
            404 => "Not Found",
            429 => "Too Many Requests",
            _ => "Status",
        }
    }

    fn handle(stream: TcpStream, reply: Reply, tx: &mpsc::Sender<Seen>) {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            return;
        }
        let mut words = request_line.split_whitespace();
        let method = words.next().unwrap_or_default().to_string();
        let path = words.next().unwrap_or_default().to_string();
        let mut headers = Vec::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                headers.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
        let len = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, v)| v.parse::<usize>().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; len];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        let _ = tx.send(Seen {
            method,
            path,
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
        });
        let mut out = stream;
        let mut head = format!(
            "HTTP/1.1 {} {}\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n",
            reply.status,
            reason(reply.status)
        );
        for (k, v) in &reply.headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("\r\n");
        if out.write_all(head.as_bytes()).is_err() || out.flush().is_err() {
            return;
        }
        for (part, delay) in &reply.parts {
            if *delay > 0 {
                thread::sleep(Duration::from_millis(*delay));
            }
            if part.is_empty() {
                continue;
            }
            let chunk = format!("{:x}\r\n{part}\r\n", part.len());
            if out.write_all(chunk.as_bytes()).is_err() || out.flush().is_err() {
                return;
            }
        }
        let _ = out.write_all(b"0\r\n\r\n");
        let _ = out.flush();
    }

    // ---- helpers ------------------------------------------------------------

    fn options(base: &str, key: &str) -> ClientOptions {
        ClientOptions {
            base_url: base.to_string(),
            api_key: Secret::new(key),
            proxy: None,
            ca_file: None,
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(20),
        }
    }

    fn client(base: &str, key: &str) -> OpenAiClient {
        OpenAiClient::new(options(base, key)).unwrap()
    }

    fn tools() -> Value {
        json!([{"type": "function", "function": {"name": "run_command", "parameters": {"type": "object"}}}])
    }

    fn request() -> ChatRequest {
        ChatRequest {
            model: "modele-test".to_string(),
            messages: vec![Message::system("système"), Message::user("affiche la date")],
            tools: tools(),
            max_tokens: 2048,
        }
    }

    fn run(c: &OpenAiClient) -> (Result<ChatResponse, LlmError>, Vec<StreamEvent>) {
        let mut events = Vec::new();
        let cancel = AtomicBool::new(false);
        let r = c.chat(&request(), &mut |e| events.push(e), &cancel);
        (r, events)
    }

    fn chat_once(reply: Reply) -> (Result<ChatResponse, LlmError>, Vec<StreamEvent>, Seen) {
        let (base, rx) = serve(reply);
        let (r, events) = run(&client(&base, KEY));
        let seen = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        (r, events, seen)
    }

    fn data(v: Value) -> String {
        format!("data: {v}\n\n")
    }

    fn delta(d: Value) -> String {
        data(
            json!({"id": "x", "object": "chat.completion.chunk", "choices": [{"index": 0, "delta": d, "finish_reason": null}]}),
        )
    }

    fn finish(reason: &str) -> String {
        data(json!({"choices": [{"index": 0, "delta": {}, "finish_reason": reason}]}))
    }

    fn done() -> String {
        "data: [DONE]\n\n".to_string()
    }

    fn content_stream(text: &str) -> Reply {
        Reply::sse(vec![
            delta(json!({"content": text})),
            finish("stop"),
            done(),
        ])
    }

    // ---- request ------------------------------------------------------------

    #[test]
    fn posts_openai_request_with_bearer_key() {
        let (r, _, seen) = chat_once(content_stream("ok"));
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(seen.method, "POST");
        assert_eq!(seen.path, "/v1/chat/completions");
        assert_eq!(
            seen.header("authorization"),
            Some(format!("Bearer {KEY}").as_str())
        );
        assert!(
            seen.header("content-type")
                .unwrap()
                .starts_with("application/json")
        );
        let body = seen.json();
        assert_eq!(body["model"], json!("modele-test"));
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["stream_options"], json!({"include_usage": true}));
        assert_eq!(body["max_tokens"], json!(2048));
        assert_eq!(body["tools"], tools());
        assert_eq!(
            body["messages"][0],
            json!({"role": "system", "content": "système"})
        );
        assert_eq!(
            body["messages"][1],
            json!({"role": "user", "content": "affiche la date"})
        );
    }

    #[test]
    fn no_authorization_header_without_key() {
        let (base, rx) = serve(content_stream("ok"));
        let (r, _) = run(&client(&base, ""));
        assert!(r.is_ok(), "{r:?}");
        let seen = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(seen.header("authorization"), None);
    }

    #[test]
    fn trailing_slash_in_base_url_is_ignored() {
        let (base, rx) = serve(content_stream("ok"));
        let (r, _) = run(&client(&format!("{base}/"), KEY));
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap().path,
            "/v1/chat/completions"
        );
    }

    // ---- streaming ----------------------------------------------------------

    #[test]
    fn streams_content_reasoning_tool_call_and_usage() {
        let split = delta(
            json!({"tool_calls": [{"index": 0, "function": {"arguments": "\"ls\",\"purpose\":\"Liste\"}"}}]}),
        );
        let (head, tail) = split.split_at(40);
        let reply = Reply::sse(vec![
            ": OPENROUTER PROCESSING\n\n".to_string(),
            delta(json!({"role": "assistant", "reasoning": "Je réfléchis"})),
            delta(json!({"content": "Bon"})).replace('\n', "\r\n"),
            delta(json!({"content": "jour é"})),
            delta(
                json!({"tool_calls": [{"index": 0, "id": "call_abc", "type": "function", "function": {"name": "run_command", "arguments": ""}}]}),
            ),
            delta(
                json!({"tool_calls": [{"index": 0, "function": {"arguments": "{\"command\":"}}]}),
            ),
            head.to_string(),
            tail.to_string(),
            finish("tool_calls"),
            data(
                json!({"choices": [], "usage": {"prompt_tokens": 12, "completion_tokens": 34, "total_tokens": 46}}),
            ),
            done(),
        ]);
        let (r, events, _) = chat_once(reply);
        let resp = r.unwrap();
        assert_eq!(resp.content, "Bonjour é");
        assert_eq!(resp.reasoning, "Je réfléchis");
        assert_eq!(
            resp.tool_calls,
            vec![ToolCall {
                id: "call_abc".into(),
                name: "run_command".into(),
                arguments: "{\"command\":\"ls\",\"purpose\":\"Liste\"}".into(),
            }]
        );
        assert_eq!(
            resp.usage,
            Some(Usage {
                prompt_tokens: 12,
                completion_tokens: 34
            })
        );
        assert_eq!(resp.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(
            events,
            vec![
                StreamEvent::Reasoning("Je réfléchis".into()),
                StreamEvent::Content("Bon".into()),
                StreamEvent::Content("jour é".into()),
            ]
        );
    }

    #[test]
    fn reasoning_content_alias_is_accepted() {
        let reply = Reply::sse(vec![
            delta(json!({"reasoning_content": "pense"})),
            delta(json!({"content": "fini"})),
            done(),
        ]);
        let (r, events, _) = chat_once(reply);
        let resp = r.unwrap();
        assert_eq!(resp.reasoning, "pense");
        assert_eq!(events[0], StreamEvent::Reasoning("pense".into()));
    }

    #[test]
    fn content_parts_array_is_concatenated() {
        let reply = Reply::sse(vec![
            delta(
                json!({"content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]}),
            ),
            done(),
        ]);
        let (r, _, _) = chat_once(reply);
        assert_eq!(r.unwrap().content, "ab");
    }

    #[test]
    fn parallel_tool_calls_are_assembled_by_index() {
        let reply = Reply::sse(vec![
            delta(json!({"tool_calls": [
                {"index": 0, "id": "c0", "function": {"name": "run_command", "arguments": "{\"a\""}},
                {"index": 1, "id": "c1", "function": {"name": "run_command", "arguments": "{\"b\""}}
            ]})),
            delta(json!({"tool_calls": [{"index": 1, "function": {"arguments": ":2}"}}]})),
            delta(json!({"tool_calls": [{"index": 0, "function": {"arguments": ":1}"}}]})),
            done(),
        ]);
        let (r, _, _) = chat_once(reply);
        let calls = r.unwrap().tool_calls;
        assert_eq!(calls.len(), 2);
        assert_eq!(
            (calls[0].id.as_str(), calls[0].arguments.as_str()),
            ("c0", "{\"a\":1}")
        );
        assert_eq!(
            (calls[1].id.as_str(), calls[1].arguments.as_str()),
            ("c1", "{\"b\":2}")
        );
    }

    #[test]
    fn object_arguments_are_serialized() {
        let reply = Reply::sse(vec![
            delta(
                json!({"tool_calls": [{"index": 0, "id": "c", "function": {"name": "run_command", "arguments": {"command": "ls", "purpose": "p"}}}]}),
            ),
            done(),
        ]);
        let (r, _, _) = chat_once(reply);
        let call = &r.unwrap().tool_calls[0];
        let v: Value = serde_json::from_str(&call.arguments).unwrap();
        assert_eq!(v, json!({"command": "ls", "purpose": "p"}));
    }

    #[test]
    fn same_index_with_new_id_starts_a_new_call() {
        let reply = Reply::sse(vec![
            delta(
                json!({"tool_calls": [{"index": 0, "id": "first", "function": {"name": "run_command", "arguments": "{}"}}]}),
            ),
            delta(
                json!({"tool_calls": [{"index": 0, "id": "second", "function": {"name": "run_command", "arguments": "{\"x\":1}"}}]}),
            ),
            done(),
        ]);
        let (r, _, _) = chat_once(reply);
        let calls = r.unwrap().tool_calls;
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "first");
        assert_eq!(calls[0].arguments, "{}");
        assert_eq!(calls[1].id, "second");
        assert_eq!(calls[1].arguments, "{\"x\":1}");
    }

    #[test]
    fn repeated_name_is_not_duplicated_and_missing_id_is_synthesized() {
        let reply = Reply::sse(vec![
            delta(
                json!({"tool_calls": [{"index": 0, "function": {"name": "run_command", "arguments": "{\"a\""}}]}),
            ),
            delta(
                json!({"tool_calls": [{"index": 0, "function": {"name": "run_command", "arguments": ":1}"}}]}),
            ),
            done(),
        ]);
        let (r, _, _) = chat_once(reply);
        let calls = r.unwrap().tool_calls;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "run_command");
        assert_eq!(calls[0].arguments, "{\"a\":1}");
        assert_eq!(calls[0].id, "call_0");
    }

    #[test]
    fn incremental_name_pieces_are_concatenated() {
        let reply = Reply::sse(vec![
            delta(json!({"tool_calls": [{"index": 0, "id": "c", "function": {"name": "run_"}}]})),
            delta(
                json!({"tool_calls": [{"index": 0, "function": {"name": "command", "arguments": "{}"}}]}),
            ),
            done(),
        ]);
        let (r, _, _) = chat_once(reply);
        assert_eq!(r.unwrap().tool_calls[0].name, "run_command");
    }

    #[test]
    fn empty_stream_is_empty_error() {
        let reply = Reply::sse(vec![
            delta(json!({"role": "assistant", "content": ""})),
            finish("stop"),
            data(json!({"choices": [], "usage": {"prompt_tokens": 5, "completion_tokens": 4}})),
            done(),
        ]);
        let (r, events, _) = chat_once(reply);
        assert_eq!(r, Err(LlmError::Empty));
        assert!(events.is_empty());
    }

    #[test]
    fn whitespace_only_content_or_reasoning_only_is_empty() {
        let (r, _, _) = chat_once(content_stream("\n  \n"));
        assert_eq!(r, Err(LlmError::Empty));
        let reply = Reply::sse(vec![
            delta(json!({"reasoning": "beaucoup"})),
            finish("length"),
            done(),
        ]);
        let (r, _, _) = chat_once(reply);
        assert_eq!(r, Err(LlmError::Empty));
    }

    #[test]
    fn stream_ending_without_done_or_blank_line_is_accepted() {
        let last = delta(json!({"content": "fin"}));
        let reply = Reply::sse(vec![
            delta(json!({"content": "dé"})),
            last.trim_end().to_string(),
        ]);
        let (r, _, _) = chat_once(reply);
        assert_eq!(r.unwrap().content, "défin");
    }

    #[test]
    fn data_lines_without_blank_separators_are_tolerated() {
        let a = delta(json!({"content": "a"}));
        let b = delta(json!({"content": "b"}));
        let reply = Reply::sse(vec![
            format!("{}\n{}\n", a.trim_end(), b.trim_end()),
            "\n".to_string(),
            done(),
        ]);
        let (r, _, _) = chat_once(reply);
        assert_eq!(r.unwrap().content, "ab");
    }

    #[test]
    fn invalid_json_chunk_is_bad_response() {
        let reply = Reply::sse(vec!["data: {not json\n\n".to_string(), done()]);
        let (r, _, _) = chat_once(reply);
        assert!(matches!(r, Err(LlmError::BadResponse(_))), "{r:?}");
    }

    #[test]
    fn mid_stream_error_object_is_reported() {
        let reply = Reply::sse(vec![
            delta(json!({"content": "a"})),
            data(json!({"error": {"message": "model crashed", "type": "api_error"}})),
            done(),
        ]);
        let (r, _, _) = chat_once(reply);
        assert_eq!(r, Err(LlmError::Server(500, "model crashed".into())));
        let reply = Reply::sse(vec![data(
            json!({"error": {"message": "slow down: concurrent request limit", "code": 429}}),
        )]);
        let (r, _, _) = chat_once(reply);
        assert!(matches!(r, Err(LlmError::RateLimited { .. })), "{r:?}");
    }

    // ---- non-streamed bodies -------------------------------------------------

    fn completion() -> Value {
        json!({
            "id": "chatcmpl-1",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Voici",
                    "reasoning": "court",
                    "tool_calls": [{"id": "t1", "type": "function", "function": {"name": "run_command", "arguments": "{\"command\":\"date\"}"}}]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 3, "completion_tokens": 4, "total_tokens": 7}
        })
    }

    #[test]
    fn non_streamed_json_response_is_accepted() {
        let reply = Reply::body(
            200,
            "application/json; charset=utf-8",
            &completion().to_string(),
        );
        let (r, events, _) = chat_once(reply);
        let resp = r.unwrap();
        assert_eq!(resp.content, "Voici");
        assert_eq!(resp.reasoning, "court");
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].id, "t1");
        assert_eq!(resp.tool_calls[0].arguments, "{\"command\":\"date\"}");
        assert_eq!(resp.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(
            resp.usage,
            Some(Usage {
                prompt_tokens: 3,
                completion_tokens: 4
            })
        );
        assert_eq!(
            events,
            vec![
                StreamEvent::Reasoning("court".into()),
                StreamEvent::Content("Voici".into())
            ]
        );
    }

    #[test]
    fn json_body_without_json_content_type_is_detected() {
        let text = serde_json::to_string_pretty(&completion()).unwrap();
        let reply = Reply::new(200, None, vec![(text, 0)]);
        let (r, _, _) = chat_once(reply);
        assert_eq!(r.unwrap().content, "Voici");
    }

    #[test]
    fn newline_delimited_chunks_without_sse_framing_are_accepted() {
        let a = json!({"choices": [{"index": 0, "delta": {"content": "x"}}]});
        let b = json!({"choices": [{"index": 0, "delta": {"content": "y"}}]});
        let reply = Reply::new(
            200,
            Some("application/x-ndjson"),
            vec![(format!("{a}\n{b}\n"), 0)],
        );
        let (r, _, _) = chat_once(reply);
        assert_eq!(r.unwrap().content, "xy");
    }

    #[test]
    fn json_error_body_with_status_200_is_reported() {
        let reply = Reply::body(
            200,
            "application/json",
            r#"{"error":{"message":"overloaded"}}"#,
        );
        let (r, _, _) = chat_once(reply);
        assert_eq!(r, Err(LlmError::Server(500, "overloaded".into())));
    }

    // ---- HTTP errors -----------------------------------------------------------

    fn status_error(reply: Reply) -> LlmError {
        let (r, events, _) = chat_once(reply);
        assert!(events.is_empty());
        r.unwrap_err()
    }

    #[test]
    fn http_statuses_are_classified() {
        let e = status_error(Reply::body(
            401,
            "application/json",
            r#"{"error":"unauthorized"}"#,
        ));
        assert_eq!(e, LlmError::Auth("unauthorized".into()));
        let e = status_error(Reply::body(403, "text/plain", "forbidden"));
        assert_eq!(e, LlmError::Auth("forbidden".into()));
        let e = status_error(Reply::body(
            402,
            "application/json",
            r#"{"error":{"message":"no credits left"}}"#,
        ));
        assert_eq!(e, LlmError::Credits("no credits left".into()));
        let e = status_error(Reply::body(
            404,
            "application/json",
            r#"{"error":{"message":"model \"x\" not found","type":"not_found_error"}}"#,
        ));
        assert_eq!(e, LlmError::ModelNotFound("model \"x\" not found".into()));
        let e = status_error(Reply::body(
            429,
            "application/json",
            r#"{"error":"timed out waiting for a concurrent request slot"}"#,
        ));
        assert_eq!(
            e,
            LlmError::RateLimited {
                retry_after: None,
                message: "timed out waiting for a concurrent request slot".into()
            }
        );
        let e =
            status_error(Reply::body(429, "text/plain", "slow down").header("Retry-After", "7"));
        assert_eq!(
            e,
            LlmError::RateLimited {
                retry_after: Some(7),
                message: "slow down".into()
            }
        );
        let e = status_error(Reply::body(
            429,
            "application/json",
            r#"{"error":"you have reached your session usage limit, upgrade for higher limits"}"#,
        ));
        assert!(
            matches!(e, LlmError::Quota(ref m) if m.contains("usage limit")),
            "{e:?}"
        );
        let e = status_error(Reply::body(
            503,
            "text/html",
            "<h1>Service Unavailable</h1>",
        ));
        assert_eq!(
            e,
            LlmError::Server(503, "<h1>Service Unavailable</h1>".into())
        );
        let e = status_error(Reply::body(
            400,
            "application/json",
            r#"{"error":{"message":"invalid tool call arguments"}}"#,
        ));
        assert_eq!(
            e,
            LlmError::Server(400, "invalid tool call arguments".into())
        );
    }

    #[test]
    fn api_key_is_masked_in_error_texts() {
        let body = format!(r#"{{"error":"invalid key {KEY}"}}"#);
        let e = status_error(Reply::body(401, "application/json", &body));
        let LlmError::Auth(ref msg) = e else {
            panic!("expected Auth, got {e:?}");
        };
        assert!(!msg.contains(KEY), "{msg}");
        assert!(msg.contains(MASK), "{msg}");
        assert!(!e.to_string().contains(KEY));
        let reply = Reply::sse(vec![data(
            json!({"error": {"message": format!("bad {KEY}")}}),
        )]);
        let (r, _, _) = chat_once(reply);
        assert!(!format!("{r:?}").contains(KEY));
    }

    // ---- transport, cancel, timeouts ---------------------------------------

    fn closed_port_base() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        format!("http://{addr}/v1")
    }

    #[test]
    fn connection_refused_is_network_error_without_key() {
        let (r, _) = run(&client(&closed_port_base(), KEY));
        let e = r.unwrap_err();
        assert!(matches!(e, LlmError::Network(_)), "{e:?}");
        assert!(!format!("{e:?}").contains(KEY));
    }

    #[test]
    fn cancel_before_request_does_not_connect() {
        let c = client(&closed_port_base(), KEY);
        let cancel = AtomicBool::new(true);
        let r = c.chat(&request(), &mut |_| {}, &cancel);
        assert_eq!(r, Err(LlmError::Cancelled));
    }

    #[test]
    fn cancel_is_checked_between_lines() {
        let mut parts = vec![(delta(json!({"content": "a"})), 0)];
        for _ in 0..300 {
            parts.push((": ping\n\n".to_string(), 20));
        }
        parts.push((delta(json!({"content": "late"})), 0));
        let (base, _rx) = serve(Reply::new(200, Some("text/event-stream"), parts));
        let c = client(&base, KEY);
        let cancel = AtomicBool::new(false);
        let started = Instant::now();
        let mut events = Vec::new();
        let r = c.chat(
            &request(),
            &mut |e| {
                events.push(e);
                cancel.store(true, Ordering::SeqCst);
            },
            &cancel,
        );
        assert_eq!(r, Err(LlmError::Cancelled));
        assert_eq!(events, vec![StreamEvent::Content("a".into())]);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn stalled_stream_hits_request_timeout() {
        let parts = vec![
            (delta(json!({"content": "a"})), 0),
            (delta(json!({"content": "b"})), 4000),
        ];
        let (base, _rx) = serve(Reply::new(200, Some("text/event-stream"), parts));
        let mut opts = options(&base, KEY);
        opts.request_timeout = Duration::from_millis(500);
        let c = OpenAiClient::new(opts).unwrap();
        let started = Instant::now();
        let (r, _) = run(&c);
        let e = r.unwrap_err();
        assert!(
            matches!(e, LlmError::Network(ref m) if m.contains("timeout")),
            "{e:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
    }

    /// Raises the returned flag after `ms` milliseconds, from another thread.
    fn cancel_after(ms: u64) -> Arc<AtomicBool> {
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(ms));
            flag.store(true, Ordering::SeqCst);
        });
        cancel
    }

    #[test]
    fn cancel_interrupts_a_silent_stream_quickly() {
        let parts = vec![
            (delta(json!({"content": "a"})), 0),
            (delta(json!({"content": "b"})), 6000),
        ];
        let (base, _rx) = serve(Reply::new(200, Some("text/event-stream"), parts));
        let c = client(&base, KEY);
        let cancel = cancel_after(300);
        let started = Instant::now();
        let mut events = Vec::new();
        let r = c.chat(&request(), &mut |e| events.push(e), &cancel);
        assert_eq!(r, Err(LlmError::Cancelled));
        assert_eq!(events, vec![StreamEvent::Content("a".into())]);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn cancel_interrupts_the_wait_for_response_headers() {
        // Accepts the connection, then answers nothing for 6 s.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                thread::sleep(Duration::from_secs(6));
                drop(stream);
            }
        });
        let c = client(&format!("http://{addr}/v1"), KEY);
        let cancel = cancel_after(300);
        let started = Instant::now();
        let r = c.chat(&request(), &mut |_| {}, &cancel);
        assert_eq!(r, Err(LlmError::Cancelled));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn client_is_reusable_after_a_cancel() {
        let parts = vec![
            (delta(json!({"content": "a"})), 0),
            (delta(json!({"content": "b"})), 3000),
        ];
        let (base, rx) = serve_many(vec![
            Reply::new(200, Some("text/event-stream"), parts),
            content_stream("encore"),
        ]);
        let c = client(&base, KEY);
        let cancel = cancel_after(200);
        assert_eq!(
            c.chat(&request(), &mut |_| {}, &cancel),
            Err(LlmError::Cancelled)
        );
        // Same client: the abandoned request does not block the next one.
        let started = Instant::now();
        assert_eq!(run(&c).0.unwrap().content, "encore");
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(rx.iter().take(2).count(), 2);
    }

    // ---- redirects --------------------------------------------------------------

    #[test]
    fn redirects_are_not_followed() {
        let (target, target_rx) = serve(content_stream("détourné"));
        let location = format!("{target}/chat/completions");
        for status in [301, 302, 307, 308] {
            let reply = Reply::body(status, "text/plain", "moved").header("Location", &location);
            let (r, events, seen) = chat_once(reply);
            assert_eq!(seen.path, "/v1/chat/completions");
            assert!(events.is_empty());
            let e = r.unwrap_err();
            assert!(
                matches!(e, LlmError::Server(s, ref m) if s == status && m.contains("redirect") && m.contains(&location)),
                "{e:?}"
            );
        }
        assert!(
            target_rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "the redirect target must never be contacted"
        );
    }

    #[test]
    fn redirect_location_never_shows_the_key() {
        let reply = Reply::body(302, "text/plain", "")
            .header("Location", &format!("https://evil.example/?k={KEY}"));
        let (r, _, _) = chat_once(reply);
        let text = format!("{r:?}");
        assert!(!text.contains(KEY), "{text}");
        assert!(text.contains("evil.example"), "{text}");
    }

    // ---- construction ---------------------------------------------------------

    #[test]
    fn base_url_scheme_is_validated() {
        let e = OpenAiClient::new(options("ftp://example.com/v1", KEY))
            .err()
            .unwrap();
        assert!(matches!(e, LlmError::Network(_)), "{e:?}");
        let e = OpenAiClient::new(options("example.com/v1", ""))
            .err()
            .unwrap();
        assert!(matches!(e, LlmError::Network(_)), "{e:?}");
    }

    #[test]
    fn key_is_never_sent_over_plain_http_to_a_remote_host() {
        let e = OpenAiClient::new(options("http://example.com/v1", KEY))
            .err()
            .unwrap();
        assert!(
            matches!(e, LlmError::Network(ref m) if m.contains("HTTP")),
            "{e:?}"
        );
        assert!(OpenAiClient::new(options("http://example.com/v1", "")).is_ok());
        assert!(OpenAiClient::new(options("https://example.com/v1", KEY)).is_ok());
        assert!(OpenAiClient::new(options("http://localhost:11434/v1", KEY)).is_ok());
        assert!(OpenAiClient::new(options("http://[::1]:11434/v1", KEY)).is_ok());
        assert!(OpenAiClient::new(options("HTTP://127.0.0.1:11434/v1", KEY)).is_ok());
    }

    #[test]
    fn explicit_proxy_is_validated_and_never_echoed() {
        let mut opts = options("https://example.com/v1", KEY);
        opts.proxy = Some("http://127.0.0.1:3128".into());
        assert!(OpenAiClient::new(opts.clone()).is_ok());
        opts.proxy = Some("http://user:hunter2pass@[::bad".into());
        let e = OpenAiClient::new(opts.clone()).err().unwrap();
        assert!(
            matches!(e, LlmError::Network(ref m) if m.contains("proxy")),
            "{e:?}"
        );
        assert!(!format!("{e:?}").contains("hunter2pass"));
        opts.proxy = Some("   ".into());
        assert!(
            OpenAiClient::new(opts).is_ok(),
            "blank proxy means automatic detection"
        );
    }

    #[test]
    fn options_debug_masks_key_and_proxy_credentials() {
        let mut opts = options("https://example.com/v1", KEY);
        opts.proxy = Some("http://user:hunter2pass@proxy.local:8080".into());
        let text = format!("{opts:?}");
        assert!(!text.contains(KEY), "{text}");
        assert!(!text.contains("hunter2pass"), "{text}");
        assert!(text.contains("proxy.local:8080"), "{text}");
        assert!(text.contains("https://example.com/v1"), "{text}");
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("llm-client-tests")
            .join(format!("{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // Base64 content is not validated before the first TLS handshake.
    const PEM: &str = "-----BEGIN CERTIFICATE-----\nMIIBszCCAVmgAwIBAgIUQ2FyZ29UZXN0Q2VydGlmaWNhdGUwCgYIKoZIzj0EAwIw\nFDESMBAGA1UEAwwJY21kX2FpLWNhMB4XDTI2MTAyOTAwMDAwMFoXDTM2MTAyOTAw\n-----END CERTIFICATE-----\n";

    #[test]
    fn ca_file_errors_are_tls_errors() {
        let dir = scratch_dir("ca");
        let missing = dir.join("absent.pem");
        let mut opts = options("https://example.com/v1", KEY);
        opts.ca_file = Some(missing.clone());
        let e = OpenAiClient::new(opts.clone()).err().unwrap();
        assert!(
            matches!(e, LlmError::Tls(ref m) if m.contains("absent.pem")),
            "{e:?}"
        );

        let empty = dir.join("empty.pem");
        std::fs::write(&empty, "no certificate here\n").unwrap();
        opts.ca_file = Some(empty);
        let e = OpenAiClient::new(opts.clone()).err().unwrap();
        assert!(
            matches!(e, LlmError::Tls(ref m) if m.contains("certificate")),
            "{e:?}"
        );

        let good = dir.join("corp.pem");
        std::fs::write(&good, PEM).unwrap();
        opts.ca_file = Some(good);
        assert!(OpenAiClient::new(opts).is_ok());
    }

    #[test]
    fn tls_handshake_failure_is_tls_error() {
        // A plain-HTTP server answering a TLS ClientHello.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 512];
                let _ = s.read(&mut buf);
                let _ = s.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n");
                let _ = s.flush();
                thread::sleep(Duration::from_millis(200));
            }
        });
        let c = client(&format!("https://localhost:{}/v1", addr.port()), KEY);
        let (r, _) = run(&c);
        let e = r.unwrap_err();
        assert!(matches!(e, LlmError::Tls(_)), "{e:?}");
    }

    // ---- pure functions -------------------------------------------------------

    #[test]
    fn classify_extracts_error_messages() {
        assert_eq!(
            classify_http_error(401, r#"{"error":{"message":"bad","type":"auth"}}"#, None),
            LlmError::Auth("bad".into())
        );
        assert_eq!(
            classify_http_error(401, r#"{"error":"bad"}"#, None),
            LlmError::Auth("bad".into())
        );
        assert_eq!(
            classify_http_error(401, r#"{"message":"m"}"#, None),
            LlmError::Auth("m".into())
        );
        assert_eq!(
            classify_http_error(401, "  plain\n text  ", None),
            LlmError::Auth("plain text".into())
        );
        assert_eq!(
            classify_http_error(401, "", None),
            LlmError::Auth(String::new())
        );
        let long = "é".repeat(2000);
        let LlmError::Server(500, m) = classify_http_error(500, &long, None) else {
            panic!("expected Server");
        };
        assert!(m.chars().count() <= 501, "{}", m.chars().count());
    }

    #[test]
    fn classify_404_and_429_variants() {
        assert_eq!(
            classify_http_error(404, "404 page not found", None),
            LlmError::Server(404, "404 page not found".into())
        );
        assert!(matches!(
            classify_http_error(404, "Model gone", None),
            LlmError::ModelNotFound(_)
        ));
        assert_eq!(
            classify_http_error(429, "busy", Some(" 12 ")),
            LlmError::RateLimited {
                retry_after: Some(12),
                message: "busy".into()
            }
        );
        assert_eq!(
            classify_http_error(429, "busy", Some("Wed, 21 Oct 2026 07:28:00 GMT")),
            LlmError::RateLimited {
                retry_after: None,
                message: "busy".into()
            }
        );
        assert_eq!(
            classify_http_error(429, "busy", Some("  ")),
            LlmError::Quota("busy".into())
        );
        assert!(matches!(
            classify_http_error(429, "Too many CONCURRENT requests", None),
            LlmError::RateLimited { .. }
        ));
        assert_eq!(
            classify_http_error(502, "bad gateway", None),
            LlmError::Server(502, "bad gateway".into())
        );
    }

    #[test]
    fn retry_after_parsing() {
        assert_eq!(parse_retry_after("7"), Some(7));
        assert_eq!(parse_retry_after(" 0 "), Some(0));
        assert_eq!(parse_retry_after("1.2"), Some(2));
        assert_eq!(parse_retry_after("-3"), None);
        assert_eq!(parse_retry_after("soon"), None);
        assert_eq!(parse_retry_after(""), None);
    }

    #[test]
    fn transport_texts_are_classified() {
        for t in [
            "invalid peer certificate: UnknownIssuer",
            "rustls: unknownissuer",
            "TLS handshake failed",
            "Certificate expired",
            "received corrupt message of type InvalidContentType",
            "received fatal alert: HandshakeFailure",
            "peer misbehaved: x",
        ] {
            assert!(
                matches!(classify_transport_text(t), LlmError::Tls(_)),
                "{t}"
            );
        }
        for t in [
            "Connection refused (os error 10061)",
            "host not found",
            "timeout: global",
            "connection reset by peer",
            "An existing connection was forcibly closed by the remote host. (os error 10054)",
        ] {
            assert!(
                matches!(classify_transport_text(t), LlmError::Network(_)),
                "{t}"
            );
        }
        assert_eq!(classify_transport_text("x"), LlmError::Network("x".into()));
    }

    #[test]
    fn capped_line_reader() {
        let mut r = BufReader::with_capacity(4, &b"ab\r\ncdef\nlast"[..]);
        let mut buf = Vec::new();
        assert_eq!(read_line_capped(&mut r, &mut buf, 100).unwrap(), 4);
        assert_eq!(buf, b"ab\r\n");
        buf.clear();
        assert_eq!(read_line_capped(&mut r, &mut buf, 100).unwrap(), 5);
        assert_eq!(buf, b"cdef\n");
        buf.clear();
        assert_eq!(read_line_capped(&mut r, &mut buf, 100).unwrap(), 4);
        assert_eq!(buf, b"last");
        buf.clear();
        assert_eq!(read_line_capped(&mut r, &mut buf, 100).unwrap(), 0);
        let mut r = BufReader::with_capacity(4, &b"0123456789\n"[..]);
        let mut buf = Vec::new();
        assert!(read_line_capped(&mut r, &mut buf, 5).is_err());
    }
}
