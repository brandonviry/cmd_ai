//! Shared harness of the integration tests (constitution IX): a fake Ollama server speaking
//! the OpenAI-compatible protocol (SSE), a temporary cmd_ai folder and a runner for the
//! real `cmd_ai` binary. No network, no real API key: everything stays on 127.0.0.1 and
//! under `CARGO_TARGET_TMPDIR`.
//!
//! Each integration test file includes it with `mod common;`.
#![allow(
    dead_code,
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

/// Model name written in the test `config.json` (never a real model).
pub const MODEL: &str = "modele-test";
/// Final text answered by [`FakeOllama`] when its queue of replies is empty.
pub const DEFAULT_REPLY_TEXT: &str = "Terminé.";
/// Upper bound of one `cmd_ai` run in [`run_cmd_ai`].
pub const RUN_TIMEOUT: Duration = Duration::from_secs(60);
/// How long the runner waits for the pipes to close once the process has exited.
const PIPE_GRACE: Duration = Duration::from_secs(5);
/// Socket read timeout of the fake server (a stuck client never blocks it forever).
const SERVER_READ_TIMEOUT: Duration = Duration::from_secs(10);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---- scripted replies -------------------------------------------------------------------

/// One scripted answer of [`FakeOllama`], consumed by one HTTP request.
#[derive(Clone, Debug)]
pub enum Reply {
    /// `200 text/event-stream`, chunked: each string is the payload of one `data:` event
    /// (a payload with newlines becomes several `data:` lines); `data: [DONE]` is appended.
    Sse(Vec<String>),
    /// Status, body and optional `Retry-After` header (`application/json` when the body
    /// looks like JSON, `text/plain` otherwise).
    Status(u16, String, Option<String>),
    /// `200 application/json` with this body (non-streaming answer).
    Json(String),
    /// Bytes written as is, then the connection is closed (truncated or malformed answers).
    Raw(String),
    /// Waits, then sends the inner reply (slow provider, client timeouts).
    Delayed(Duration, Box<Reply>),
}

impl Reply {
    /// A complete streamed text answer: content, `finish_reason: "stop"`, usage.
    pub fn text(content: &str) -> Reply {
        Reply::Sse(vec![
            sse_content(content),
            sse_finish("stop"),
            sse_usage(12, 3),
        ])
    }

    /// A complete streamed tool call (arguments split in two chunks), then
    /// `finish_reason: "tool_calls"` and usage.
    pub fn tool_call(id: &str, name: &str, arguments: &str) -> Reply {
        let mut events = sse_tool_call(id, name, arguments);
        events.push(sse_finish("tool_calls"));
        events.push(sse_usage(20, 8));
        Reply::Sse(events)
    }

    /// A streamed `run_command` call with `command` and `purpose`.
    pub fn run_command(id: &str, command: &str, purpose: &str) -> Reply {
        Reply::tool_call(id, "run_command", &run_command_args(command, purpose))
    }
}

fn chunk(delta: Value, finish_reason: Option<&str>) -> String {
    json!({
        "id": "chatcmpl-fake",
        "object": "chat.completion.chunk",
        "created": 0,
        "model": MODEL,
        "choices": [{ "index": 0, "delta": delta, "finish_reason": finish_reason }],
    })
    .to_string()
}

/// SSE payload: a content delta.
pub fn sse_content(text: &str) -> String {
    chunk(json!({ "content": text }), None)
}

/// SSE payload: a reasoning delta (`reasoning`, the field Ollama uses).
pub fn sse_reasoning(text: &str) -> String {
    chunk(json!({ "reasoning": text }), None)
}

/// SSE payload: a reasoning delta under the alternative `reasoning_content` field.
pub fn sse_reasoning_content(text: &str) -> String {
    chunk(json!({ "reasoning_content": text }), None)
}

/// SSE payloads of one tool call at index 0: the first chunk carries `id`, `type`, `name`
/// and the first half of `arguments`, the second chunk the rest.
pub fn sse_tool_call(id: &str, name: &str, arguments: &str) -> Vec<String> {
    sse_tool_call_at(0, id, name, arguments)
}

/// Same as [`sse_tool_call`] for the tool call number `index` of the response.
pub fn sse_tool_call_at(index: usize, id: &str, name: &str, arguments: &str) -> Vec<String> {
    let mut mid = arguments.len() / 2;
    while !arguments.is_char_boundary(mid) {
        mid -= 1;
    }
    let (head, tail) = arguments.split_at(mid);
    vec![
        chunk(
            json!({ "tool_calls": [{
                "index": index, "id": id, "type": "function",
                "function": { "name": name, "arguments": head },
            }]}),
            None,
        ),
        chunk(
            json!({ "tool_calls": [{ "index": index, "function": { "arguments": tail } }] }),
            None,
        ),
    ]
}

/// SSE payload: an empty delta with a `finish_reason` (`stop`, `tool_calls`, `length`).
pub fn sse_finish(reason: &str) -> String {
    chunk(json!({}), Some(reason))
}

/// SSE payload: the final usage chunk (`stream_options.include_usage`), without choices.
pub fn sse_usage(prompt_tokens: u64, completion_tokens: u64) -> String {
    json!({
        "id": "chatcmpl-fake",
        "object": "chat.completion.chunk",
        "created": 0,
        "model": MODEL,
        "choices": [],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": prompt_tokens + completion_tokens,
        },
    })
    .to_string()
}

/// JSON arguments of a `run_command` call.
pub fn run_command_args(command: &str, purpose: &str) -> String {
    json!({ "command": command, "purpose": purpose }).to_string()
}

/// Non-streaming `chat.completion` body with a final text message.
pub fn json_completion(content: &str) -> String {
    json!({
        "id": "chatcmpl-fake",
        "object": "chat.completion",
        "created": 0,
        "model": MODEL,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": content },
            "finish_reason": "stop",
        }],
        "usage": { "prompt_tokens": 12, "completion_tokens": 3, "total_tokens": 15 },
    })
    .to_string()
}

// ---- fake Ollama server -------------------------------------------------------------------

/// One HTTP request received by [`FakeOllama`].
#[derive(Clone, Debug)]
pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl RecordedRequest {
    /// Header value, name compared case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        header_value(&self.headers, name)
    }

    /// The body parsed as JSON (`Value::Null` when it is not JSON).
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or(Value::Null)
    }
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

struct Shared {
    replies: Mutex<VecDeque<Reply>>,
    requests: Mutex<Vec<RecordedRequest>>,
}

/// Local HTTP/1.1 server on `127.0.0.1:0` that plays scripted [`Reply`]s in order, one per
/// request (sequential, `Connection: close`), and records every request. With an empty
/// queue it answers a final text message ([`DEFAULT_REPLY_TEXT`]): SSE when the request
/// asks for `"stream": true`, JSON otherwise. Stopped when dropped.
pub struct FakeOllama {
    addr: SocketAddr,
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl FakeOllama {
    /// A server with an empty queue.
    pub fn start() -> FakeOllama {
        FakeOllama::with_replies(Vec::new())
    }

    /// A server that will play `replies` in order.
    pub fn with_replies(replies: Vec<Reply>) -> FakeOllama {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1:0");
        let addr = listener
            .local_addr()
            .expect("local address of the fake server");
        let shared = Arc::new(Shared {
            replies: Mutex::new(replies.into()),
            requests: Mutex::new(Vec::new()),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (shared, stop) = (Arc::clone(&shared), Arc::clone(&stop));
            thread::Builder::new()
                .name("fake-ollama".into())
                .spawn(move || serve(&listener, &shared, &stop))
                .expect("spawn the fake server thread")
        };
        FakeOllama {
            addr,
            shared,
            stop,
            thread: Some(thread),
        }
    }

    /// Queues one more reply.
    pub fn push(&self, reply: Reply) {
        lock(&self.shared.replies).push_back(reply);
    }

    /// `http://127.0.0.1:PORT/v1`, the `provider.base_url` to configure.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.addr.port())
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Every request received so far, in order.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        lock(&self.shared.requests).clone()
    }

    pub fn request_count(&self) -> usize {
        lock(&self.shared.requests).len()
    }

    /// Scripted replies not consumed yet.
    pub fn pending_replies(&self) -> usize {
        lock(&self.shared.replies).len()
    }
}

impl Drop for FakeOllama {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocking accept() so the thread sees the stop flag.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_secs(1));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(listener: &TcpListener, shared: &Shared, stop: &AtomicBool) {
    for conn in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        if let Ok(stream) = conn {
            // A broken client connection must not take the server down.
            let _ = handle(stream, shared);
        }
    }
}

fn handle(mut stream: TcpStream, shared: &Shared) -> io::Result<()> {
    stream.set_read_timeout(Some(SERVER_READ_TIMEOUT))?;
    let _ = stream.set_nodelay(true);
    let mut reader = BufReader::new(stream.try_clone()?);
    let Some(request) = read_request(&mut reader, &mut stream)? else {
        return Ok(());
    };
    let streaming = serde_json::from_str::<Value>(&request.body)
        .ok()
        .and_then(|v| v.get("stream").and_then(Value::as_bool))
        .unwrap_or(false);
    lock(&shared.requests).push(request);
    let reply = lock(&shared.replies).pop_front();
    let reply = reply.unwrap_or_else(|| default_reply(streaming));
    write_reply(&mut stream, reply)?;
    stream.flush()?;
    stream.shutdown(Shutdown::Write)?;
    // Drain until the client closes: closing a socket with unread bytes sends a RST that
    // can make the client lose the end of the answer (notably on Windows).
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut sink = [0u8; 1024];
    while matches!(reader.read(&mut sink), Ok(n) if n > 0) {}
    Ok(())
}

fn default_reply(streaming: bool) -> Reply {
    if streaming {
        Reply::text(DEFAULT_REPLY_TEXT)
    } else {
        Reply::Json(json_completion(DEFAULT_REPLY_TEXT))
    }
}

fn read_request(
    reader: &mut BufReader<TcpStream>,
    stream: &mut TcpStream,
) -> io::Result<Option<RecordedRequest>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None); // connection closed without a request (e.g. the stop wake-up)
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    if method.is_empty() {
        return Ok(None);
    }
    let mut headers = Vec::new();
    loop {
        let mut raw = String::new();
        if reader.read_line(&mut raw)? == 0 {
            break;
        }
        let raw = raw.trim_end_matches(['\r', '\n']);
        if raw.is_empty() {
            break;
        }
        if let Some((name, value)) = raw.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    if header_value(&headers, "Expect").is_some_and(|v| v.eq_ignore_ascii_case("100-continue")) {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
        stream.flush()?;
    }
    let chunked = header_value(&headers, "Transfer-Encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    let body = if chunked {
        read_chunked_body(reader)?
    } else {
        let len = header_value(&headers, "Content-Length")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; len];
        reader.read_exact(&mut body)?;
        body
    };
    Ok(Some(RecordedRequest {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    }))
}

/// Decodes an HTTP/1.1 chunked body (chunk extensions and trailers ignored).
pub fn read_chunked_body(reader: &mut impl BufRead) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let mut size_line = String::new();
        if reader.read_line(&mut size_line)? == 0 {
            return Ok(body);
        }
        let size_field = size_line.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_field, 16)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if size == 0 {
            loop {
                let mut trailer = String::new();
                if reader.read_line(&mut trailer)? == 0 || trailer.trim().is_empty() {
                    return Ok(body);
                }
            }
        }
        let start = body.len();
        body.resize(start + size, 0);
        reader.read_exact(&mut body[start..])?;
        let mut crlf = String::new();
        reader.read_line(&mut crlf)?;
    }
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        408 => "Request Timeout",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Status",
    }
}

fn write_fixed(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
    retry_after: Option<&str>,
) -> io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n",
        reason_phrase(status),
        body.len()
    );
    if let Some(seconds) = retry_after {
        head.push_str("Retry-After: ");
        head.push_str(seconds);
        head.push_str("\r\n");
    }
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())
}

/// One SSE event: every line of the payload prefixed by `data: `, then a blank line.
fn sse_event(payload: &str) -> String {
    let mut event = String::with_capacity(payload.len() + 8);
    for line in payload.split('\n') {
        event.push_str("data: ");
        event.push_str(line);
        event.push('\n');
    }
    event.push('\n');
    event
}

fn write_reply(stream: &mut TcpStream, reply: Reply) -> io::Result<()> {
    match reply {
        Reply::Sse(payloads) => {
            stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n\
                  Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
            )?;
            for payload in payloads.iter().map(String::as_str).chain(["[DONE]"]) {
                let event = sse_event(payload);
                write!(stream, "{:x}\r\n{event}\r\n", event.len())?;
                stream.flush()?;
            }
            stream.write_all(b"0\r\n\r\n")
        }
        Reply::Status(status, body, retry_after) => {
            let content_type = if body.trim_start().starts_with(['{', '[']) {
                "application/json"
            } else {
                "text/plain; charset=utf-8"
            };
            write_fixed(stream, status, content_type, &body, retry_after.as_deref())
        }
        Reply::Json(body) => write_fixed(stream, 200, "application/json", &body, None),
        Reply::Raw(raw) => stream.write_all(raw.as_bytes()),
        Reply::Delayed(delay, inner) => {
            thread::sleep(delay);
            write_reply(stream, *inner)
        }
    }
}

// ---- temporary cmd_ai folder ----------------------------------------------------------------

static HOME_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A throw-away cmd_ai folder under `CARGO_TARGET_TMPDIR` with `log/`, `memory/` and
/// `skills/`. Removed when dropped, unless `CMD_AI_KEEP_TEST_HOMES` is set (debugging).
pub struct TempHome {
    root: PathBuf,
}

impl TempHome {
    /// A folder whose `config.json` points at `base_url` with `api_key` and
    /// `permissions` (`"plan"` or `"bypass"`).
    pub fn new(base_url: &str, api_key: &str, permissions: &str) -> TempHome {
        let home = TempHome::empty();
        home.write_config(&config_json(base_url, api_key, permissions));
        home
    }

    /// A folder without `config.json` (first run, missing configuration).
    pub fn empty() -> TempHome {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let name = format!(
            "home-{}-{}-{nanos}",
            std::process::id(),
            HOME_COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
        for sub in ["log", "memory", "skills"] {
            fs::create_dir_all(root.join(sub)).expect("create the temporary cmd_ai folder");
        }
        TempHome { root }
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    pub fn log_dir(&self) -> PathBuf {
        self.root.join("log")
    }

    pub fn memory_dir(&self) -> PathBuf {
        self.root.join("memory")
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.root.join("skills")
    }

    /// Replaces `config.json`.
    pub fn write_config(&self, json: &str) {
        fs::write(self.config_path(), json).expect("write config.json");
    }

    /// Writes `skills/<name>/SKILL.md` and returns its path.
    pub fn write_skill(&self, name: &str, skill_md: &str) -> PathBuf {
        let dir = self.skills_dir().join(name);
        fs::create_dir_all(&dir).expect("create the skill folder");
        let path = dir.join("SKILL.md");
        fs::write(&path, skill_md).expect("write SKILL.md");
        path
    }

    /// Session logs (`log/*.log`), sorted by name (hence by date).
    pub fn log_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = fs::read_dir(self.log_dir())
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().is_some_and(|x| x == "log"))
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        files
    }

    /// Every session log concatenated, oldest first (lossy UTF-8).
    pub fn read_logs(&self) -> String {
        self.log_files()
            .iter()
            .map(|p| String::from_utf8_lossy(&fs::read(p).unwrap_or_default()).into_owned())
            .collect()
    }

    /// `memory/MEMORY.md`, or an empty string.
    pub fn read_memory(&self) -> String {
        fs::read_to_string(self.memory_dir().join("MEMORY.md")).unwrap_or_default()
    }

    /// Every file of the folder, recursively, sorted.
    pub fn all_files(&self) -> Vec<PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.push(path);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &mut out);
        out.sort();
        out
    }

    /// Files other than `config.json` whose bytes contain `needle` (canary checks).
    pub fn files_containing(&self, needle: &str) -> Vec<PathBuf> {
        let config = self.config_path();
        self.all_files()
            .into_iter()
            .filter(|p| *p != config)
            .filter(|p| {
                fs::read(p).is_ok_and(|bytes| {
                    bytes
                        .windows(needle.len().max(1))
                        .any(|w| w == needle.as_bytes())
                })
            })
            .collect()
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        if std::env::var_os("CMD_AI_KEEP_TEST_HOMES").is_none() {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

/// Minimal valid `config.json` (schema version 1) for the tests.
pub fn config_json(base_url: &str, api_key: &str, permissions: &str) -> String {
    let config = json!({
        "version": 1,
        "provider": { "base_url": base_url, "api_key": api_key, "model": MODEL },
        "permissions": permissions,
    });
    serde_json::to_string_pretty(&config).unwrap_or_else(|_| config.to_string())
}

// ---- running the binary -------------------------------------------------------------------

/// Path of the `cmd_ai` binary built by cargo for the integration tests.
pub fn cmd_ai_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cmd_ai"))
}

/// A `Command` for tests that need another program (the harness is the only place of the
/// test suite allowed to create one).
pub fn command(program: impl AsRef<OsStr>) -> Command {
    Command::new(program)
}

/// Runs `cmd_ai --home <home> <args…>` from the folder itself, with `NO_COLOR=1`, without
/// any inherited `OLLAMA_API_KEY` or `CMD_AI_*` variable (unless given in `env`), `stdin`
/// piped when given (closed otherwise), within [`RUN_TIMEOUT`].
pub fn run_cmd_ai(
    home: &TempHome,
    args: &[&str],
    stdin: Option<&str>,
    env: &[(&str, &str)],
) -> Output {
    run_cmd_ai_in(home, home.path(), args, stdin, env)
}

/// Same as [`run_cmd_ai`] with another working directory.
pub fn run_cmd_ai_in(
    home: &TempHome,
    cwd: &Path,
    args: &[&str],
    stdin: Option<&str>,
    env: &[(&str, &str)],
) -> Output {
    let mut cmd = Command::new(cmd_ai_bin());
    cmd.arg("--home")
        .arg(home.path())
        .args(args)
        .current_dir(cwd);
    cmd.env_remove("OLLAMA_API_KEY");
    for (key, _) in std::env::vars_os() {
        if key
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("CMD_AI_")
        {
            cmd.env_remove(&key);
        }
    }
    cmd.env("NO_COLOR", "1");
    for (key, value) in env {
        cmd.env(key, value);
    }
    run_with_timeout(cmd, stdin, RUN_TIMEOUT)
}

/// Collects a pipe on a thread; `finish` waits at most [`PIPE_GRACE`] for its end, so a
/// grandchild still holding the pipe cannot hang the test.
struct Collector {
    buf: Arc<Mutex<Vec<u8>>>,
    done: mpsc::Receiver<()>,
}

impl Collector {
    fn start<R: Read + Send + 'static>(pipe: Option<R>) -> Collector {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let (tx, done) = mpsc::channel();
        match pipe {
            Some(mut pipe) => {
                let buf = Arc::clone(&buf);
                thread::spawn(move || {
                    let mut chunk = [0u8; 8192];
                    loop {
                        match pipe.read(&mut chunk) {
                            Ok(0) => break,
                            Ok(n) => lock(&buf).extend_from_slice(&chunk[..n]),
                            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                            Err(_) => break,
                        }
                    }
                    let _ = tx.send(());
                });
            }
            None => {
                let _ = tx.send(());
            }
        }
        Collector { buf, done }
    }

    fn finish(self) -> Vec<u8> {
        let _ = self.done.recv_timeout(PIPE_GRACE);
        std::mem::take(&mut *lock(&self.buf))
    }
}

/// Runs `cmd` (stdout and stderr captured, `stdin` written then closed) and panics with the
/// captured output if it does not exit within `timeout` (the process is killed first).
pub fn run_with_timeout(mut cmd: Command, stdin: Option<&str>, timeout: Duration) -> Output {
    cmd.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let program = format!("{cmd:?}");
    let mut child = cmd
        .spawn()
        .unwrap_or_else(|e| panic!("cannot start {program}: {e}"));
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let input = input.to_owned();
        // Detached: a child that never reads its input must not block the runner.
        thread::spawn(move || {
            let _ = pipe.write_all(input.as_bytes());
        });
    }
    let stdout = Collector::start(child.stdout.take());
    let stderr = Collector::start(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let (out, err) = (stdout.finish(), stderr.finish());
                panic!(
                    "{program} did not finish within {timeout:?} (killed)\n--- stdout ---\n{}\n--- stderr ---\n{}",
                    String::from_utf8_lossy(&out),
                    String::from_utf8_lossy(&err)
                );
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(e) => panic!("waiting for {program}: {e}"),
        }
    };
    Output {
        status,
        stdout: stdout.finish(),
        stderr: stderr.finish(),
    }
}

/// Lossy UTF-8 stdout of a run.
pub fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Lossy UTF-8 stderr of a run.
pub fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
