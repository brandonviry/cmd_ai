//! Self-test of the integration harness (`tests/common`): the fake Ollama server is driven
//! with a raw `TcpStream` client, the temporary folder and the runner are checked. The
//! behaviour of cmd_ai itself is tested elsewhere (`oneshot.rs`, `portability.rs`, …).
// Helpers outside #[test] functions are test code too (clippy's allow-*-in-tests only
// covers #[test] functions and #[cfg(test)] modules).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io::{BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant};

use common::*;
use serde_json::Value;

struct RawResponse {
    status: u16,
    headers: Vec<(String, String)>,
    /// Body, de-chunked when `Transfer-Encoding: chunked`.
    body: String,
    raw: Vec<u8>,
}

impl RawResponse {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Payloads of the SSE `data:` lines.
    fn sse_data(&self) -> Vec<String> {
        self.body
            .lines()
            .filter_map(|l| l.strip_prefix("data: "))
            .map(str::to_string)
            .collect()
    }
}

fn connect(addr: SocketAddr) -> TcpStream {
    let stream = TcpStream::connect(addr).expect("connect to the fake server");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream
}

fn read_response(mut stream: TcpStream) -> RawResponse {
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read the answer");
    parse_response(raw)
}

fn parse_response(raw: Vec<u8>) -> RawResponse {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("end of the headers");
    let head = String::from_utf8(raw[..split].to_vec()).unwrap();
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap();
    let status = status_line
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    let rest = &raw[split + 4..];
    let chunked = headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("transfer-encoding") && v.contains("chunked"));
    let body = if chunked {
        read_chunked_body(&mut BufReader::new(rest)).expect("valid chunked body")
    } else {
        rest.to_vec()
    };
    RawResponse {
        status,
        headers,
        body: String::from_utf8(body).expect("UTF-8 body"),
        raw,
    }
}

fn post(server: &FakeOllama, body: &str, extra_headers: &[(&str, &str)]) -> RawResponse {
    let mut stream = connect(server.addr());
    let mut request = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        server.addr(),
        body.len()
    );
    for (k, v) in extra_headers {
        request.push_str(&format!("{k}: {v}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes()).unwrap();
    read_response(stream)
}

const STREAM_BODY: &str = r#"{"model":"modele-test","stream":true,"messages":[]}"#;

fn parse(payload: &str) -> Value {
    serde_json::from_str(payload).unwrap_or_else(|e| panic!("{payload}: {e}"))
}

// ---- fake server ------------------------------------------------------------------------

#[test]
fn base_url_points_at_the_loopback_v1_api() {
    let server = FakeOllama::start();
    let url = server.base_url();
    let port: u16 = url
        .strip_prefix("http://127.0.0.1:")
        .and_then(|rest| rest.strip_suffix("/v1"))
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("unexpected base URL {url}"));
    assert_ne!(port, 0);
    assert_eq!(port, server.addr().port());
    assert!(server.addr().ip().is_loopback());
}

#[test]
fn sse_reply_is_streamed_chunked_with_done_and_the_request_is_recorded() {
    let server = FakeOllama::with_replies(vec![Reply::Sse(vec![sse_content("Bonjour é")])]);
    let resp = post(
        &server,
        STREAM_BODY,
        &[("Authorization", "Bearer cle-factice-1234")],
    );
    assert_eq!(resp.status, 200);
    assert_eq!(resp.header("content-type"), Some("text/event-stream"));
    assert_eq!(resp.header("transfer-encoding"), Some("chunked"));
    assert_eq!(resp.header("connection"), Some("close"));
    let data = resp.sse_data();
    assert_eq!(data.len(), 2, "{data:?}");
    assert_eq!(
        parse(&data[0])["choices"][0]["delta"]["content"],
        "Bonjour é"
    );
    assert_eq!(data[1], "[DONE]");
    assert!(resp.body.ends_with("data: [DONE]\n\n"));

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let req = &requests[0];
    assert_eq!(req.method, "POST");
    assert_eq!(req.path, "/v1/chat/completions");
    assert_eq!(req.header("authorization"), Some("Bearer cle-factice-1234"));
    assert_eq!(req.header("CONTENT-TYPE"), Some("application/json"));
    assert_eq!(req.body, STREAM_BODY);
    assert_eq!(req.json()["model"], "modele-test");
    assert_eq!(server.pending_replies(), 0);
}

#[test]
fn multi_line_sse_payload_becomes_several_data_lines() {
    let server = FakeOllama::with_replies(vec![Reply::Sse(vec!["ligne 1\nligne 2".into()])]);
    let resp = post(&server, STREAM_BODY, &[]);
    assert!(
        resp.body
            .starts_with("data: ligne 1\ndata: ligne 2\n\ndata: [DONE]\n\n"),
        "{:?}",
        resp.body
    );
}

#[test]
fn status_reply_carries_body_and_retry_after() {
    let server = FakeOllama::with_replies(vec![
        Reply::Status(
            429,
            r#"{"error":"too many concurrent requests"}"#.into(),
            Some("7".into()),
        ),
        Reply::Status(503, "surcharge".into(), None),
    ]);
    let resp = post(&server, STREAM_BODY, &[]);
    assert_eq!(resp.status, 429);
    assert_eq!(resp.header("retry-after"), Some("7"));
    assert_eq!(resp.header("content-type"), Some("application/json"));
    assert_eq!(resp.body, r#"{"error":"too many concurrent requests"}"#);
    assert!(String::from_utf8_lossy(&resp.raw).starts_with("HTTP/1.1 429 Too Many Requests\r\n"));

    let resp = post(&server, STREAM_BODY, &[]);
    assert_eq!(resp.status, 503);
    assert_eq!(resp.header("retry-after"), None);
    assert_eq!(
        resp.header("content-type"),
        Some("text/plain; charset=utf-8")
    );
    assert_eq!(resp.body, "surcharge");
}

#[test]
fn json_reply_has_a_content_length() {
    let body = json_completion("Réponse");
    let server = FakeOllama::with_replies(vec![Reply::Json(body.clone())]);
    let resp = post(&server, r#"{"stream":false}"#, &[]);
    assert_eq!(resp.status, 200);
    assert_eq!(resp.header("content-type"), Some("application/json"));
    assert_eq!(
        resp.header("content-length"),
        Some(body.len().to_string().as_str())
    );
    assert_eq!(
        parse(&resp.body)["choices"][0]["message"]["content"],
        "Réponse"
    );
}

#[test]
fn empty_queue_answers_a_final_text_message() {
    let server = FakeOllama::start();
    let resp = post(&server, STREAM_BODY, &[]);
    assert_eq!(resp.header("content-type"), Some("text/event-stream"));
    let data = resp.sse_data();
    assert_eq!(data.last().map(String::as_str), Some("[DONE]"));
    let chunks: Vec<Value> = data[..data.len() - 1].iter().map(|d| parse(d)).collect();
    assert_eq!(
        chunks[0]["choices"][0]["delta"]["content"],
        DEFAULT_REPLY_TEXT
    );
    assert!(
        chunks
            .iter()
            .any(|c| c["choices"][0]["finish_reason"] == "stop")
    );
    assert!(chunks.iter().any(|c| c["usage"]["total_tokens"].is_u64()));

    // Without "stream": true, the default answer is a plain JSON completion.
    let resp = post(&server, r#"{"model":"modele-test"}"#, &[]);
    assert_eq!(resp.header("content-type"), Some("application/json"));
    assert_eq!(
        parse(&resp.body)["choices"][0]["message"]["content"],
        DEFAULT_REPLY_TEXT
    );
    assert_eq!(server.request_count(), 2);
}

#[test]
fn replies_are_played_in_order_one_per_request() {
    let server = FakeOllama::with_replies(vec![Reply::Json("{\"n\":1}".into())]);
    server.push(Reply::Json("{\"n\":2}".into()));
    server.push(Reply::Status(500, "{\"n\":3}".into(), None));
    assert_eq!(server.pending_replies(), 3);
    let bodies: Vec<(u16, String)> = (0..3)
        .map(|i| {
            let r = post(&server, &format!("{{\"i\":{i}}}"), &[]);
            (r.status, r.body)
        })
        .collect();
    assert_eq!(
        bodies,
        vec![
            (200, "{\"n\":1}".to_string()),
            (200, "{\"n\":2}".to_string()),
            (500, "{\"n\":3}".to_string())
        ]
    );
    let sent: Vec<i64> = server
        .requests()
        .iter()
        .map(|r| r.json()["i"].as_i64().unwrap())
        .collect();
    assert_eq!(sent, vec![0, 1, 2]);
    assert_eq!(server.pending_replies(), 0);
}

#[test]
fn chunked_request_body_is_decoded() {
    let server = FakeOllama::with_replies(vec![Reply::Json("{}".into())]);
    let mut stream = connect(server.addr());
    stream
        .write_all(
            b"POST /v1/chat/completions HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n\
              5\r\n{\"a\":\r\n4;ext=1\r\n\"b\"}\r\n0\r\n\r\n",
        )
        .unwrap();
    let resp = read_response(stream);
    assert_eq!(resp.status, 200);
    assert_eq!(server.requests()[0].body, r#"{"a":"b"}"#);
}

#[test]
fn expect_continue_is_honoured() {
    let server = FakeOllama::with_replies(vec![Reply::Json("{}".into())]);
    let mut stream = connect(server.addr());
    stream
        .write_all(
            b"POST /v1/chat/completions HTTP/1.1\r\nHost: x\r\nContent-Length: 2\r\nExpect: 100-continue\r\n\r\n",
        )
        .unwrap();
    let mut interim = [0u8; 25];
    stream.read_exact(&mut interim).unwrap();
    assert_eq!(&interim, b"HTTP/1.1 100 Continue\r\n\r\n");
    stream.write_all(b"{}").unwrap();
    let mut rest = Vec::new();
    stream.read_to_end(&mut rest).unwrap();
    assert_eq!(parse_response(rest).status, 200);
    assert_eq!(server.requests()[0].body, "{}");
}

#[test]
fn raw_reply_is_written_as_is() {
    let raw = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\ndata: {\"tronqu";
    let server = FakeOllama::with_replies(vec![Reply::Raw(raw.into())]);
    let mut stream = connect(server.addr());
    stream
        .write_all(b"POST /v1/chat/completions HTTP/1.1\r\nContent-Length: 0\r\n\r\n")
        .unwrap();
    let mut got = Vec::new();
    stream.read_to_end(&mut got).unwrap();
    assert_eq!(String::from_utf8(got).unwrap(), raw);
}

#[test]
fn delayed_reply_waits_before_answering() {
    let server = FakeOllama::with_replies(vec![Reply::Delayed(
        Duration::from_millis(300),
        Box::new(Reply::Json("{}".into())),
    )]);
    let start = Instant::now();
    let resp = post(&server, "{}", &[]);
    assert!(start.elapsed() >= Duration::from_millis(300));
    assert_eq!(resp.status, 200);
}

#[test]
fn dropping_the_server_stops_it_promptly() {
    let server = FakeOllama::start();
    let _ = post(&server, STREAM_BODY, &[]);
    let start = Instant::now();
    drop(server);
    assert!(start.elapsed() < Duration::from_secs(5));
}

// ---- SSE payload helpers ----------------------------------------------------------------

#[test]
fn tool_call_helper_splits_the_arguments_in_two_chunks() {
    let args = run_command_args("echo «éàçù»", "Afficher des accents");
    let chunks: Vec<Value> = sse_tool_call("call_1", "run_command", &args)
        .iter()
        .map(|c| parse(c))
        .collect();
    assert_eq!(chunks.len(), 2);
    let first = &chunks[0]["choices"][0]["delta"]["tool_calls"][0];
    let second = &chunks[1]["choices"][0]["delta"]["tool_calls"][0];
    assert_eq!(first["index"], 0);
    assert_eq!(first["id"], "call_1");
    assert_eq!(first["type"], "function");
    assert_eq!(first["function"]["name"], "run_command");
    assert_eq!(second["index"], 0);
    assert!(second.get("id").is_none() && second["function"].get("name").is_none());
    let (a, b) = (
        first["function"]["arguments"].as_str().unwrap(),
        second["function"]["arguments"].as_str().unwrap(),
    );
    assert!(!a.is_empty() && !b.is_empty());
    assert_eq!(format!("{a}{b}"), args);
    let parsed = parse(&args);
    assert_eq!(parsed["command"], "echo «éàçù»");
    assert_eq!(parsed["purpose"], "Afficher des accents");

    let at = parse(&sse_tool_call_at(2, "call_3", "run_command", "{}")[0]);
    assert_eq!(at["choices"][0]["delta"]["tool_calls"][0]["index"], 2);
}

#[test]
fn delta_finish_and_usage_helpers() {
    let c = parse(&sse_content("salut"));
    assert_eq!(c["object"], "chat.completion.chunk");
    assert_eq!(c["choices"][0]["delta"]["content"], "salut");
    assert!(c["choices"][0]["finish_reason"].is_null());
    let r = parse(&sse_reasoning("je réfléchis"));
    assert_eq!(r["choices"][0]["delta"]["reasoning"], "je réfléchis");
    let rc = parse(&sse_reasoning_content("aussi"));
    assert_eq!(rc["choices"][0]["delta"]["reasoning_content"], "aussi");
    let f = parse(&sse_finish("tool_calls"));
    assert_eq!(f["choices"][0]["finish_reason"], "tool_calls");
    let u = parse(&sse_usage(100, 25));
    assert_eq!(u["choices"].as_array().map(Vec::len), Some(0));
    assert_eq!(u["usage"]["prompt_tokens"], 100);
    assert_eq!(u["usage"]["completion_tokens"], 25);
    assert_eq!(u["usage"]["total_tokens"], 125);
}

#[test]
fn reply_constructors_compose_complete_streams() {
    let Reply::Sse(text) = Reply::text("fini") else {
        panic!("Reply::text must be SSE");
    };
    assert_eq!(parse(&text[0])["choices"][0]["delta"]["content"], "fini");
    assert!(
        text.iter()
            .any(|e| parse(e)["choices"][0]["finish_reason"] == "stop")
    );

    let Reply::Sse(call) = Reply::run_command("call_9", "ls -la", "Lister") else {
        panic!("Reply::run_command must be SSE");
    };
    let values: Vec<Value> = call.iter().map(|e| parse(e)).collect();
    assert_eq!(
        values[0]["choices"][0]["delta"]["tool_calls"][0]["function"]["name"],
        "run_command"
    );
    assert!(
        values
            .iter()
            .any(|v| v["choices"][0]["finish_reason"] == "tool_calls")
    );
    assert!(values.last().unwrap()["usage"].is_object());
    let args: String = values
        .iter()
        .filter_map(|v| v["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str())
        .collect();
    assert_eq!(parse(&args)["command"], "ls -la");
}

// ---- temporary folder ---------------------------------------------------------------------

#[test]
fn temp_home_has_the_portable_layout_and_a_valid_config() {
    let home = TempHome::new("http://127.0.0.1:9/v1", "cle-canari-0123", "bypass");
    assert!(home.path().starts_with(env!("CARGO_TARGET_TMPDIR")));
    for dir in [home.log_dir(), home.memory_dir(), home.skills_dir()] {
        assert!(dir.is_dir(), "{}", dir.display());
    }
    let config: Value =
        serde_json::from_str(&std::fs::read_to_string(home.config_path()).unwrap()).unwrap();
    assert_eq!(config["version"], 1);
    assert_eq!(config["provider"]["base_url"], "http://127.0.0.1:9/v1");
    assert_eq!(config["provider"]["api_key"], "cle-canari-0123");
    assert_eq!(config["provider"]["model"], MODEL);
    assert_eq!(config["permissions"], "bypass");

    let other = TempHome::new("http://127.0.0.1:9/v1", "", "plan");
    assert_ne!(home.path(), other.path(), "each folder is unique");
}

#[test]
fn temp_home_is_removed_on_drop() {
    let home = TempHome::empty();
    let root = home.path().to_path_buf();
    assert!(root.is_dir());
    assert!(
        !home.config_path().exists(),
        "empty() writes no config.json"
    );
    drop(home);
    if std::env::var_os("CMD_AI_KEEP_TEST_HOMES").is_none() {
        assert!(!root.exists());
    }
}

#[test]
fn temp_home_reads_logs_memory_and_finds_canaries() {
    let home = TempHome::new("http://127.0.0.1:9/v1", "cle-canari-0123", "plan");
    std::fs::write(home.log_dir().join("2026-10-29_120000.log"), "premier\n").unwrap();
    std::fs::write(home.log_dir().join("2026-10-29_110000.log"), "zéro\n").unwrap();
    std::fs::write(home.log_dir().join("notes.txt"), "ignoré").unwrap();
    assert_eq!(home.log_files().len(), 2);
    assert_eq!(home.read_logs(), "zéro\npremier\n");
    assert_eq!(home.read_memory(), "");
    std::fs::write(
        home.memory_dir().join("MEMORY.md"),
        "fuite cle-canari-0123\n",
    )
    .unwrap();
    assert!(home.read_memory().starts_with("fuite"));
    let skill = home.write_skill("projet-npm", "---\nname: projet-npm\n---\n");
    assert_eq!(skill, home.skills_dir().join("projet-npm").join("SKILL.md"));
    assert!(skill.is_file());

    let leaks = home.files_containing("cle-canari-0123");
    assert_eq!(
        leaks,
        vec![home.memory_dir().join("MEMORY.md")],
        "config.json is excluded"
    );
    assert!(home.all_files().contains(&home.config_path()));
}

#[test]
fn config_json_escapes_its_values() {
    let json = config_json("http://127.0.0.1:1/v1", "a\"b\\c", "plan");
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["provider"]["api_key"], "a\"b\\c");
}

// ---- runner -----------------------------------------------------------------------------------

#[test]
fn cmd_ai_binary_is_built() {
    let bin = cmd_ai_bin();
    assert!(bin.is_file(), "{}", bin.display());
    let name = bin.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    assert_eq!(name, "cmd_ai");
}

#[test]
fn run_cmd_ai_completes_with_and_without_stdin() {
    // Only the runner is checked here, not what cmd_ai prints.
    let server = FakeOllama::start();
    let home = TempHome::new(&server.base_url(), "", "plan");
    let out = run_cmd_ai(&home, &["--version"], None, &[]);
    assert!(
        out.status.code().is_some(),
        "exited normally: {:?}",
        out.status
    );
    let out = run_cmd_ai(&home, &["--version"], Some("o\n"), &[("CMD_AI_TEST", "1")]);
    assert!(out.status.code().is_some());
    let _ = (stdout_of(&out), stderr_of(&out));
}

fn echo_stdin_command() -> std::process::Command {
    if cfg!(windows) {
        command("sort")
    } else {
        command("cat")
    }
}

#[test]
fn run_with_timeout_feeds_stdin_and_captures_stdout() {
    let out = run_with_timeout(
        echo_stdin_command(),
        Some("bonjour\n"),
        Duration::from_secs(30),
    );
    assert!(out.status.success());
    assert!(stdout_of(&out).contains("bonjour"), "{:?}", stdout_of(&out));
}

#[test]
#[should_panic(expected = "did not finish within")]
fn run_with_timeout_kills_a_process_that_hangs() {
    let mut cmd = if cfg!(windows) {
        let mut c = command("ping");
        c.args(["-n", "30", "127.0.0.1"]);
        c
    } else {
        let mut c = command("sleep");
        c.arg("30");
        c
    };
    cmd.current_dir(Path::new(env!("CARGO_TARGET_TMPDIR")));
    let _ = run_with_timeout(cmd, None, Duration::from_millis(500));
}
