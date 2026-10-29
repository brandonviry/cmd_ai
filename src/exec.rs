#![allow(clippy::disallowed_methods)]
//! The ONLY process spawn gate of cmd_ai (constitution, principe II): every child process (model
//! commands, hooks, `sudo` probes) is created here, with the minimal-trace environment of
//! [`shell::child_env`].
//!
//! - `Captured`: stdin closed, stdout/stderr piped and read by one thread per stream (raw 8 KiB
//!   chunks, decoded by [`platform::decode_output`] without splitting a UTF-8 sequence), streamed
//!   to the caller, capped at `capture_limit` bytes per stream. The process tree lives in a Job
//!   Object (Windows, hidden console) or its own process group (Unix) and is killed as a whole on
//!   timeout, on cancel, when background processes keep the pipes open after the shell exited,
//!   and when the call unwinds (drop guard).
//! - `Detached`: GUI launchers; stdio null, outside any job, waits at most 10 s for the launcher.
//! - `Inherited`: interactive steps; the terminal is shared, no capture.
//!
//! See specs/001-cmd-ai-v1/contracts/rust-api.md (exec).

use std::ffi::OsStr;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt as _;
#[cfg(windows)]
use std::os::windows::process::CommandExt as _;

#[cfg(unix)]
use process_wrap::std::ProcessGroup;
use process_wrap::std::{ChildWrapper, CommandWrap};
#[cfg(windows)]
use process_wrap::std::{CreationFlags, JobObject};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{
    CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, DETACHED_PROCESS,
};

use crate::platform;
use crate::shell::{self, ShellInfo};

/// Polling period of the caller loop (cancel flag, timeout, exit status).
const POLL: Duration = Duration::from_millis(50);
/// Size of one raw read on a pipe.
const READ_CHUNK: usize = 8 * 1024;
/// Decoded chunks in flight between a reader thread and the caller (back-pressure).
const CHANNEL_DEPTH: usize = 64;
/// Once the shell has exited, how long background processes may keep the pipes open before the
/// tree is killed so that the call can return.
const EXIT_GRACE: Duration = Duration::from_millis(500);
/// Once the tree has been killed, how long to wait for the pipes to close.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
/// Once the tree has been killed, how long to wait for the exit status.
const REAP_TIMEOUT: Duration = Duration::from_secs(5);
/// Longest wait for the launcher process of a detached command.
const DETACHED_WAIT: Duration = Duration::from_secs(10);

/// Output stream of a child.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

/// How a command is run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecMode {
    /// Output captured and streamed, stdin closed (the default).
    Captured,
    /// GUI launchers (`start`, `Start-Process`, `open`, `xdg-open`…): detached, stdio null.
    Detached,
    /// Interactive step: the terminal is handed over (stdio inherited), nothing captured.
    Inherited,
}

/// Everything needed to run one command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecSpec {
    pub shell: ShellInfo,
    pub command: String,
    pub cwd: PathBuf,
    pub timeout: Duration,
    pub mode: ExecMode,
    /// Added on top of [`shell::child_env`] (e.g. the `CMD_AI_*` variables of hooks).
    pub extra_env: Vec<(String, String)>,
    /// Unix only: run as `sudo -n <shell> <args>`. Ignored on Windows.
    pub sudo: bool,
    /// Bytes of decoded text kept per stream; the rest is counted and flagged.
    pub capture_limit: usize,
}

/// Captured text of one stream.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Captured {
    /// At most `capture_limit` bytes, cut on a character boundary.
    pub text: String,
    /// Raw bytes received on the pipe, kept or not.
    pub total_bytes: u64,
    /// `text` does not hold everything that was received.
    pub truncated: bool,
}

/// Outcome of one command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecResult {
    /// Exit code; `None` when killed (timeout, cancel, signal) or still running (detached).
    pub code: Option<i32>,
    pub stdout: Captured,
    pub stderr: Captured,
    pub duration: Duration,
    pub timed_out: bool,
    pub cancelled: bool,
}

/// The execution port used by the agent (faked in tests of other modules).
pub trait Executor {
    fn run(
        &mut self,
        spec: &ExecSpec,
        on_chunk: &mut dyn FnMut(Stream, &str),
        cancel: &AtomicBool,
    ) -> io::Result<ExecResult>;
}

/// The real executor: spawns processes on this host.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemExecutor;

impl Executor for SystemExecutor {
    fn run(
        &mut self,
        spec: &ExecSpec,
        on_chunk: &mut dyn FnMut(Stream, &str),
        cancel: &AtomicBool,
    ) -> io::Result<ExecResult> {
        match spec.mode {
            ExecMode::Captured => run_captured(spec, on_chunk, cancel),
            ExecMode::Detached => run_detached(spec, cancel, DETACHED_WAIT.min(spec.timeout)),
            ExecMode::Inherited => run_inherited(spec, cancel),
        }
    }
}

/// Runs `program` with the terminal handed over (stdio inherited) and waits for it, without
/// timeout: used for `sudo -v` (Unix), where the user types the password directly into `sudo`.
/// The environment is the minimal-trace one of [`shell::child_env`].
pub fn run_inherited_program(program: &str, args: &[&str]) -> io::Result<Option<i32>> {
    let mut command = base_command(program, args.iter().copied(), &[]);
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let status = command.spawn()?.wait()?;
    Ok(status.code())
}

/// Runs `program` with stdin closed and output discarded, and returns its exit code; `None` when it
/// cannot be started or does not finish within `timeout` (its tree is then killed). Used for
/// `sudo -n true` (Unix).
pub fn probe_program(program: &str, args: &[&str], timeout: Duration) -> Option<i32> {
    let mut command = base_command(program, args.iter().copied(), &[]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut guard = TreeGuard::spawn(command, true).ok()?;
    let started = Instant::now();
    loop {
        match guard.child.try_wait() {
            Ok(Some(status)) => {
                guard.disarm();
                return status.code();
            }
            Ok(None) if started.elapsed() < timeout => thread::sleep(POLL.min(timeout)),
            // Timeout or wait error: the guard kills the tree.
            _ => return None,
        }
    }
}

/// Program, arguments and (cmd.exe) raw tail for a spec: the shell and [`shell::build_args`], or
/// `sudo -n <shell path> <shell args>` when `sudo` is requested on Unix (never reading a password
/// from stdin: the user authenticates beforehand with `sudo -v` on the real terminal).
fn command_parts(spec: &ExecSpec) -> (PathBuf, Vec<String>, Option<String>) {
    let shell_args = shell::build_args(spec.shell.shell, &spec.command);
    if cfg!(unix) && spec.sudo {
        let mut args = vec![
            "-n".to_string(),
            spec.shell.path.to_string_lossy().into_owned(),
        ];
        args.extend(shell_args.args);
        (PathBuf::from("sudo"), args, shell_args.raw_tail)
    } else {
        (
            spec.shell.path.clone(),
            shell_args.args,
            shell_args.raw_tail,
        )
    }
}

/// A `Command` with the minimal-trace environment ([`shell::child_env`] then `extra_env`).
fn base_command<'a>(
    program: impl AsRef<OsStr>,
    args: impl IntoIterator<Item = &'a str>,
    extra_env: &[(String, String)],
) -> Command {
    let mut command = Command::new(program);
    command.args(args);
    command.env_clear();
    command.envs(shell::child_env(std::env::vars_os()));
    command.envs(extra_env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    command
}

/// The `Command` of a spec: program, arguments, raw tail (cmd.exe), environment and directory.
fn spec_command(spec: &ExecSpec) -> Command {
    let (program, args, raw_tail) = command_parts(spec);
    let mut command = base_command(&program, args.iter().map(String::as_str), &spec.extra_env);
    #[cfg(windows)]
    if let Some(tail) = raw_tail {
        // cmd.exe does not follow the MSVC quoting rules: the command goes verbatim.
        command.raw_arg(tail);
    }
    #[cfg(not(windows))]
    let _ = raw_tail;
    command.current_dir(&spec.cwd);
    command
}

/// `CREATE_NO_WINDOW` for process-wrap: the flags type comes from the `windows` crate, which is
/// not a direct dependency, so it is built from its `Default` and its public field.
#[cfg(windows)]
fn no_window() -> CreationFlags {
    let mut flags = CreationFlags(Default::default());
    flags.0.0 = CREATE_NO_WINDOW;
    flags
}

/// A spawned process tree, killed when dropped unless [`TreeGuard::disarm`] was called
/// (process-wrap's std frontend has no kill-on-drop).
struct TreeGuard {
    child: Box<dyn ChildWrapper>,
    armed: bool,
}

impl TreeGuard {
    /// Spawns `command` in its own Job Object (Windows; hidden console when `hidden`) or process
    /// group (Unix).
    fn spawn(command: Command, hidden: bool) -> io::Result<TreeGuard> {
        let mut wrap = CommandWrap::from(command);
        #[cfg(windows)]
        {
            wrap.wrap(JobObject);
            if hidden {
                wrap.wrap(no_window());
            }
        }
        #[cfg(unix)]
        {
            let _ = hidden;
            wrap.wrap(ProcessGroup::leader());
        }
        Ok(TreeGuard::new(wrap.spawn()?))
    }

    fn new(child: Box<dyn ChildWrapper>) -> TreeGuard {
        TreeGuard { child, armed: true }
    }

    /// Kills the whole tree (Job Object termination / `SIGKILL` to the process group).
    fn kill_tree(&mut self) {
        let _ = self.child.start_kill();
    }

    /// Polls for the exit status for at most `max`.
    fn reap(&mut self, max: Duration) -> Option<ExitStatus> {
        let started = Instant::now();
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) if started.elapsed() < max => thread::sleep(Duration::from_millis(10)),
                _ => return None,
            }
        }
    }

    /// The tree finished normally: do not kill what it may have left running on purpose.
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TreeGuard {
    fn drop(&mut self) {
        if self.armed {
            self.kill_tree();
            let _ = self.reap(Duration::from_secs(1));
        }
    }
}

/// Message from a reader thread.
enum Event {
    /// Decoded text and the number of raw bytes it came from.
    Data(Stream, String, usize),
    /// The pipe reached end of file (or failed).
    Closed,
}

/// Incremental decoder: turns raw pipe bytes into text without splitting a UTF-8 sequence. Valid
/// UTF-8 is emitted as soon as it is complete; when invalid bytes show up (legacy OEM output), the
/// text is decoded line by line through [`platform::decode_output`].
#[derive(Default)]
struct LineDecoder {
    pending: Vec<u8>,
}

impl LineDecoder {
    /// Adds bytes; returns the decodable text and how many raw bytes it consumed.
    fn push(&mut self, bytes: &[u8]) -> Option<(String, usize)> {
        self.pending.extend_from_slice(bytes);
        let take = match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.len(),
            // Only an incomplete sequence at the end: keep it for the next chunk.
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            Err(_) => {
                // Legacy code page somewhere: decode everything, except an incomplete UTF-8
                // sequence ending a last line that is otherwise valid UTF-8.
                let tail_start = self
                    .pending
                    .iter()
                    .rposition(|&b| b == b'\n')
                    .map_or(0, |i| i + 1);
                match std::str::from_utf8(&self.pending[tail_start..]) {
                    Err(e) if e.error_len().is_none() => tail_start + e.valid_up_to(),
                    _ => self.pending.len(),
                }
            }
        };
        if take == 0 {
            return None;
        }
        let chunk: Vec<u8> = self.pending.drain(..take).collect();
        Some((decode_lines(&chunk), take))
    }

    /// Flushes what is left at end of file (an incomplete sequence is decoded, never lost).
    fn finish(&mut self) -> Option<(String, usize)> {
        if self.pending.is_empty() {
            return None;
        }
        let chunk = std::mem::take(&mut self.pending);
        Some((platform::decode_output(&chunk), chunk.len()))
    }
}

/// Decodes line by line, so that one legacy-encoded line does not affect UTF-8 ones.
fn decode_lines(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }
    bytes
        .split_inclusive(|&b| b == b'\n')
        .map(platform::decode_output)
        .collect()
}

/// Reads one pipe in raw chunks until end of file, sending decoded text.
fn spawn_reader(
    stream: Stream,
    mut pipe: impl Read + Send + 'static,
    tx: SyncSender<Event>,
) -> io::Result<()> {
    let name = match stream {
        Stream::Stdout => "cmd_ai-stdout",
        Stream::Stderr => "cmd_ai-stderr",
    };
    thread::Builder::new()
        .name(name.to_string())
        .spawn(move || {
            let mut decoder = LineDecoder::default();
            let mut buf = vec![0u8; READ_CHUNK];
            loop {
                match pipe.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if let Some((text, raw)) = decoder.push(&buf[..n]) {
                            if tx.send(Event::Data(stream, text, raw)).is_err() {
                                return;
                            }
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            if let Some((text, raw)) = decoder.finish() {
                let _ = tx.send(Event::Data(stream, text, raw));
            }
            let _ = tx.send(Event::Closed);
        })?;
    Ok(())
}

/// Accumulates one stream up to its limit.
struct Capture {
    out: Captured,
    limit: usize,
}

impl Capture {
    fn new(limit: usize) -> Capture {
        Capture {
            out: Captured::default(),
            limit,
        }
    }

    fn push(&mut self, text: &str, raw: usize) {
        self.out.total_bytes += raw as u64;
        if self.out.truncated {
            return;
        }
        let room = self.limit.saturating_sub(self.out.text.len());
        if text.len() <= room {
            self.out.text.push_str(text);
        } else {
            let mut cut = room;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            self.out.text.push_str(&text[..cut]);
            self.out.truncated = true;
        }
    }
}

fn run_captured(
    spec: &ExecSpec,
    on_chunk: &mut dyn FnMut(Stream, &str),
    cancel: &AtomicBool,
) -> io::Result<ExecResult> {
    let started = Instant::now();
    let mut command = spec_command(spec);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut guard = TreeGuard::spawn(command, true)?;

    let (tx, rx) = mpsc::sync_channel(CHANNEL_DEPTH);
    let mut open_pipes = 0usize;
    if let Some(pipe) = guard.child.stdout().take() {
        spawn_reader(Stream::Stdout, pipe, tx.clone())?;
        open_pipes += 1;
    }
    if let Some(pipe) = guard.child.stderr().take() {
        spawn_reader(Stream::Stderr, pipe, tx.clone())?;
        open_pipes += 1;
    }
    drop(tx);

    let mut stdout = Capture::new(spec.capture_limit);
    let mut stderr = Capture::new(spec.capture_limit);
    let mut status: Option<ExitStatus> = None;
    let mut exited_at: Option<Instant> = None;
    let mut killed_at: Option<Instant> = None;
    let mut timed_out = false;
    let mut cancelled = false;
    loop {
        match rx.recv_timeout(POLL) {
            Ok(Event::Data(stream, text, raw)) => {
                match stream {
                    Stream::Stdout => stdout.push(&text, raw),
                    Stream::Stderr => stderr.push(&text, raw),
                }
                on_chunk(stream, &text);
            }
            Ok(Event::Closed) => open_pipes = open_pipes.saturating_sub(1),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => open_pipes = 0,
        }
        if status.is_none() {
            if let Some(s) = guard.child.try_wait()? {
                status = Some(s);
                exited_at = Some(Instant::now());
            }
        }
        if open_pipes == 0 && status.is_some() {
            break;
        }
        match killed_at {
            None => {
                let kill = if status.is_none() && cancel.load(Ordering::SeqCst) {
                    cancelled = true;
                    true
                } else if status.is_none() && started.elapsed() >= spec.timeout {
                    timed_out = true;
                    true
                } else {
                    // The shell is gone but background processes still hold the pipes.
                    exited_at.is_some_and(|t| t.elapsed() >= EXIT_GRACE)
                };
                if kill {
                    guard.kill_tree();
                    killed_at = Some(Instant::now());
                }
            }
            // Something outside the tree keeps a pipe open: stop waiting for it.
            Some(t) if t.elapsed() >= DRAIN_TIMEOUT => break,
            Some(_) => {}
        }
    }
    if status.is_none() {
        status = guard.reap(REAP_TIMEOUT);
    }
    if killed_at.is_none() {
        guard.disarm();
    }
    let code = if timed_out || cancelled {
        None
    } else {
        status.and_then(|s| s.code())
    };
    Ok(ExecResult {
        code,
        stdout: stdout.out,
        stderr: stderr.out,
        duration: started.elapsed(),
        timed_out,
        cancelled,
    })
}

/// Starts a GUI launcher outside any job (Windows: `CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS`;
/// Unix: its own process group), stdio null, and waits at most `max_wait` for the launcher process
/// itself. Code `None` if it is still running then; it is never killed.
fn run_detached(
    spec: &ExecSpec,
    cancel: &AtomicBool,
    max_wait: Duration,
) -> io::Result<ExecResult> {
    let started = Instant::now();
    let mut command = spec_command(spec);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn()?;
    let mut code = None;
    let mut cancelled = false;
    let mut finished = false;
    loop {
        if let Some(status) = child.try_wait()? {
            code = status.code();
            finished = true;
            break;
        }
        if cancel.load(Ordering::SeqCst) {
            cancelled = true;
            break;
        }
        if started.elapsed() >= max_wait {
            break;
        }
        thread::sleep(POLL);
    }
    if !finished {
        // Reap it whenever it exits (no zombie on Unix); a failed thread spawn only costs that.
        let _ = thread::Builder::new()
            .name("cmd_ai-detached".to_string())
            .spawn(move || {
                let _ = child.wait();
            });
    }
    Ok(ExecResult {
        code,
        stdout: Captured::default(),
        stderr: Captured::default(),
        duration: started.elapsed(),
        timed_out: false,
        cancelled,
    })
}

/// Runs an interactive step with the terminal handed over. Windows: inside a Job Object sharing
/// the console. Unix: no new process group (it must stay in the terminal's foreground group), so
/// only the shell itself is killed on timeout or cancel.
fn run_inherited(spec: &ExecSpec, cancel: &AtomicBool) -> io::Result<ExecResult> {
    let started = Instant::now();
    let mut command = spec_command(spec);
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    let mut guard = TreeGuard::spawn(command, false)?;
    #[cfg(not(windows))]
    let mut guard = TreeGuard::new(CommandWrap::from(command).spawn()?);
    let mut status = None;
    let mut timed_out = false;
    let mut cancelled = false;
    loop {
        if let Some(s) = guard.child.try_wait()? {
            status = Some(s);
            guard.disarm();
            break;
        }
        if cancel.load(Ordering::SeqCst) {
            cancelled = true;
        } else if started.elapsed() >= spec.timeout {
            timed_out = true;
        }
        if cancelled || timed_out {
            guard.kill_tree();
            let _ = guard.reap(REAP_TIMEOUT);
            break;
        }
        thread::sleep(POLL);
    }
    Ok(ExecResult {
        code: status.and_then(|s| s.code()),
        stdout: Captured::default(),
        stderr: Captured::default(),
        duration: started.elapsed(),
        timed_out,
        cancelled,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::{self, Shell};
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::time::Instant;

    fn spec(shell: ShellInfo, command: &str) -> ExecSpec {
        ExecSpec {
            shell,
            command: command.to_string(),
            cwd: std::env::current_dir().unwrap(),
            timeout: Duration::from_secs(60),
            mode: ExecMode::Captured,
            extra_env: Vec::new(),
            sudo: false,
            capture_limit: 1 << 20,
        }
    }

    fn run(spec: &ExecSpec) -> (ExecResult, Vec<(Stream, String)>) {
        let cancel = AtomicBool::new(false);
        let mut chunks = Vec::new();
        let r = SystemExecutor
            .run(spec, &mut |s, t| chunks.push((s, t.to_string())), &cancel)
            .unwrap();
        (r, chunks)
    }

    fn sandbox(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-sandbox")
            .join(format!("exec-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fake_shell(shell: Shell, path: &str) -> ShellInfo {
        ShellInfo {
            shell,
            path: PathBuf::from(path),
        }
    }

    #[test]
    fn command_parts_per_shell() {
        let s = spec(fake_shell(Shell::Bash, "/bin/bash"), "echo hi");
        let (prog, args, raw) = command_parts(&s);
        assert_eq!(prog, PathBuf::from("/bin/bash"));
        assert_eq!(args, vec!["-c".to_string(), "echo hi".to_string()]);
        assert_eq!(raw, None);

        let s = spec(
            fake_shell(Shell::Cmd, "C:\\Windows\\System32\\cmd.exe"),
            "echo hi",
        );
        let (prog, args, raw) = command_parts(&s);
        assert_eq!(prog, PathBuf::from("C:\\Windows\\System32\\cmd.exe"));
        assert_eq!(
            args,
            vec!["/d".to_string(), "/s".to_string(), "/c".to_string()]
        );
        assert_eq!(raw.as_deref(), Some("\"echo hi\""));
    }

    #[test]
    fn command_parts_sudo() {
        let mut s = spec(fake_shell(Shell::Bash, "/bin/bash"), "apt update");
        s.sudo = true;
        let (prog, args, raw) = command_parts(&s);
        if cfg!(unix) {
            assert_eq!(prog, PathBuf::from("sudo"));
            assert_eq!(
                args,
                vec![
                    "-n".to_string(),
                    "/bin/bash".to_string(),
                    "-c".to_string(),
                    "apt update".to_string()
                ]
            );
        } else {
            // sudo is ignored on Windows.
            assert_eq!(prog, PathBuf::from("/bin/bash"));
            assert_eq!(args, vec!["-c".to_string(), "apt update".to_string()]);
        }
        assert_eq!(raw, None);
        assert!(
            !args.iter().any(|a| a == "-S"),
            "sudo must never read a password from stdin"
        );
    }

    #[test]
    fn line_decoder_splits_at_newlines_and_keeps_partial_utf8() {
        let mut d = LineDecoder::default();
        let bytes = "ligne é\nsuite à".as_bytes();
        // Split inside the two-byte "é".
        let cut = bytes.iter().position(|&b| b == 0xc3).unwrap() + 1;
        let first = d.push(&bytes[..cut]);
        // "ligne " is valid and complete UTF-8 without newline: streamed, the lone lead byte kept.
        assert_eq!(first, Some(("ligne ".to_string(), 6)));
        let second = d.push(&bytes[cut..]);
        assert_eq!(second, Some(("é\nsuite à".to_string(), bytes.len() - 6)));
        assert_eq!(d.finish(), None);
    }

    #[test]
    fn line_decoder_counts_every_byte_and_flushes_on_finish() {
        let mut d = LineDecoder::default();
        let data = "aé€😀\nb".as_bytes();
        let mut text = String::new();
        let mut count = 0;
        for b in data {
            if let Some((t, n)) = d.push(std::slice::from_ref(b)) {
                text.push_str(&t);
                count += n;
            }
        }
        if let Some((t, n)) = d.finish() {
            text.push_str(&t);
            count += n;
        }
        assert_eq!(text, "aé€😀\nb");
        assert_eq!(count, data.len());
    }

    #[test]
    fn line_decoder_truncated_sequence_at_eof_is_not_lost() {
        let mut d = LineDecoder::default();
        assert_eq!(d.push(b"ok\xe2\x82"), Some(("ok".to_string(), 2)));
        let (t, n) = d.finish().unwrap();
        assert_eq!(n, 2);
        assert!(!t.is_empty());
    }

    #[test]
    fn line_decoder_invalid_bytes_are_decoded_not_held() {
        let mut d = LineDecoder::default();
        // 0x82 is "é" in OEM 850/437; invalid as UTF-8 lead byte.
        let (t, n) = d.push(b"caf\x82 sans fin de ligne").unwrap();
        assert_eq!(n, 22);
        assert!(t.starts_with("caf"));
        assert!(t.ends_with(" sans fin de ligne"));
        assert_eq!(d.finish(), None);
    }

    #[test]
    fn line_decoder_mixed_utf8_and_legacy_lines() {
        let mut d = LineDecoder::default();
        // First line UTF-8, second line legacy (0x82), third line starts a UTF-8 "é" cut in two.
        let (t, n) = d.push(b"d\xc3\xa9j\xc3\xa0\nl\x82gacy\nfin \xc3").unwrap();
        assert!(t.starts_with("déjà\nl"), "{t:?}");
        assert!(t.ends_with("gacy\nfin "), "{t:?}");
        assert_eq!(n, 18);
        assert_eq!(d.push(b"\xa9\n"), Some(("é\n".to_string(), 3)));
        assert_eq!(d.finish(), None);
        #[cfg(unix)]
        {
            let mut d = LineDecoder::default();
            assert_eq!(
                d.push(b"a\xffb\n").map(|(t, _)| t).as_deref(),
                Some("a\u{fffd}b\n")
            );
        }
    }

    #[test]
    fn capture_cuts_on_char_boundaries_and_counts_everything() {
        let mut c = Capture::new(5);
        c.push("éé", 4);
        assert_eq!((c.out.text.as_str(), c.out.truncated), ("éé", false));
        // One byte of room left, "à" needs two: nothing more is kept.
        c.push("àb", 3);
        assert_eq!(c.out.text, "éé");
        assert!(c.out.truncated);
        c.push("zzz", 3);
        assert_eq!(c.out.text, "éé");
        assert_eq!(c.out.total_bytes, 10);
        let mut c = Capture::new(0);
        c.push("x", 1);
        assert_eq!(
            (c.out.text.as_str(), c.out.truncated, c.out.total_bytes),
            ("", true, 1)
        );
        let mut c = Capture::new(3);
        c.push("abc", 3);
        assert!(!c.out.truncated, "exactly at the limit is not truncated");
    }

    #[test]
    fn extra_env_overrides_the_scrubbed_environment() {
        let mut s = spec(fake_shell(Shell::Sh, "sh"), "x");
        s.extra_env = vec![("PAGER".to_string(), "more".to_string())];
        let command = spec_command(&s);
        let envs: Vec<_> = command.get_envs().collect();
        let pager: Vec<_> = envs
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("PAGER"))
            .collect();
        assert_eq!(
            pager.last().and_then(|(_, v)| *v),
            Some(std::ffi::OsStr::new("more"))
        );
        assert!(
            !envs
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("OLLAMA_API_KEY"))
        );
        assert_eq!(command.get_current_dir(), Some(s.cwd.as_path()));
    }

    #[test]
    fn spawn_error_is_reported() {
        let missing = if cfg!(windows) {
            "C:\\nope\\missing-shell.exe"
        } else {
            "/nope/missing-shell"
        };
        let s = spec(fake_shell(Shell::Sh, missing), "echo x");
        let cancel = AtomicBool::new(false);
        assert!(SystemExecutor.run(&s, &mut |_, _| {}, &cancel).is_err());
        assert_eq!(probe_program(missing, &[], Duration::from_secs(1)), None);
        assert!(run_inherited_program(missing, &[]).is_err());
    }

    // ---------------------------------------------------------------- Windows

    #[cfg(windows)]
    mod windows {
        use super::*;

        fn cmd() -> ShellInfo {
            shell::find(Shell::Cmd).expect("cmd.exe")
        }

        #[test]
        fn cmd_accents_and_success() {
            let (r, chunks) = run(&spec(cmd(), "echo éàçù"));
            assert_eq!(r.code, Some(0));
            assert!(r.stdout.text.contains("éàçù"), "{:?}", r.stdout.text);
            assert!(!r.timed_out && !r.cancelled);
            let live: String = chunks
                .iter()
                .filter(|(s, _)| *s == Stream::Stdout)
                .map(|(_, t)| t.as_str())
                .collect();
            assert_eq!(live, r.stdout.text);
            assert!(r.stdout.total_bytes >= 6, "{}", r.stdout.total_bytes);
        }

        #[test]
        fn powershell_accents() {
            for shell in [Shell::PowerShell, Shell::Pwsh] {
                let Some(info) = shell::find(shell) else {
                    eprintln!("skip: {shell:?} not found");
                    continue;
                };
                let (r, _) = run(&spec(info, "Write-Output 'éàçù'"));
                assert_eq!(r.code, Some(0), "{shell:?} stderr: {}", r.stderr.text);
                assert_eq!(r.stdout.text.trim_end(), "éàçù", "{shell:?}");
                assert_eq!(r.stderr.text, "", "{shell:?}");
            }
        }

        #[test]
        fn exit_codes() {
            let (r, _) = run(&spec(cmd(), "exit /b 3"));
            assert_eq!(r.code, Some(3));
            for shell in [Shell::PowerShell, Shell::Pwsh] {
                if let Some(info) = shell::find(shell) {
                    let (r, _) = run(&spec(info, "exit 4"));
                    assert_eq!(r.code, Some(4), "{shell:?}");
                }
            }
        }

        #[test]
        fn stderr_is_captured_separately() {
            let (r, chunks) = run(&spec(cmd(), "echo sortie & echo erreur 1>&2"));
            assert!(r.stdout.text.contains("sortie"));
            assert!(!r.stdout.text.contains("erreur"));
            assert!(r.stderr.text.contains("erreur"), "{:?}", r.stderr);
            assert!(
                chunks
                    .iter()
                    .any(|(s, t)| *s == Stream::Stderr && t.contains("erreur"))
            );
        }

        #[test]
        fn cwd_is_used() {
            let dir = sandbox("cwd");
            let mut s = spec(cmd(), "cd");
            s.cwd = dir.clone();
            let (r, _) = run(&s);
            assert_eq!(
                r.stdout.text.trim_end().to_lowercase(),
                dir.display().to_string().to_lowercase()
            );
            let _ = std::fs::remove_dir_all(dir);
        }

        #[test]
        fn timeout_kills_the_whole_tree() {
            let dir = sandbox("timeout");
            let marker = dir.join("late.txt");
            // The grandchild (inner cmd) would write the marker after about 4 s if it survived.
            let command = format!(
                "cmd /d /c \"ping -n 5 127.0.0.1 >nul & echo late > \"{}\"\"",
                marker.display()
            );
            let mut s = spec(cmd(), &command);
            s.timeout = Duration::from_secs(1);
            let start = Instant::now();
            let (r, _) = run(&s);
            let elapsed = start.elapsed();
            assert!(r.timed_out);
            assert!(!r.cancelled);
            assert_eq!(r.code, None);
            assert!(elapsed < Duration::from_secs(4), "took {elapsed:?}");
            std::thread::sleep(Duration::from_secs(6));
            assert!(!marker.exists(), "grandchild survived the timeout");
            let _ = std::fs::remove_dir_all(dir);
        }

        #[test]
        fn background_child_holding_pipes_does_not_hang() {
            // cmd exits at once, ping keeps the pipes open: the call must still return promptly.
            let start = Instant::now();
            let (r, _) = run(&spec(cmd(), "start /b ping -n 30 127.0.0.1"));
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "took {:?}",
                start.elapsed()
            );
            assert!(!r.timed_out);
            assert_eq!(r.code, Some(0));
        }

        #[test]
        fn cancel_flag_kills() {
            let s = spec(cmd(), "ping -n 30 127.0.0.1");
            let cancel = Arc::new(AtomicBool::new(false));
            let c2 = Arc::clone(&cancel);
            let setter = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(700));
                c2.store(true, Ordering::SeqCst);
            });
            let start = Instant::now();
            let r = SystemExecutor.run(&s, &mut |_, _| {}, &cancel).unwrap();
            setter.join().unwrap();
            assert!(r.cancelled);
            assert!(!r.timed_out);
            assert_eq!(r.code, None);
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "took {:?}",
                start.elapsed()
            );
        }

        #[test]
        fn capture_limit_truncates_but_counts() {
            let mut s = spec(cmd(), "for /l %i in (1,1,3000) do @echo ligne numero %i");
            s.capture_limit = 1000;
            let (r, chunks) = run(&s);
            assert_eq!(r.code, Some(0));
            assert!(r.stdout.truncated);
            assert!(r.stdout.text.len() <= 1000);
            assert!(r.stdout.text.len() > 900);
            assert!(r.stdout.total_bytes > 30_000, "{}", r.stdout.total_bytes);
            assert!(!r.stderr.truncated);
            let live: usize = chunks.iter().map(|(_, t)| t.len()).sum();
            assert_eq!(live as u64, r.stdout.total_bytes);
        }

        #[test]
        fn stdin_is_closed() {
            let mut s = spec(cmd(), "set /p x=");
            s.timeout = Duration::from_secs(20);
            let start = Instant::now();
            let (r, _) = run(&s);
            assert!(!r.timed_out);
            assert!(start.elapsed() < Duration::from_secs(10));
        }

        #[test]
        fn extra_env_is_visible_and_secrets_are_not() {
            let mut s = spec(
                cmd(),
                "echo [%CMD_AI_TEST_VAR%] [%NO_COLOR%] [%PSModuleAnalysisCachePath%] [%PAGER%]",
            );
            s.extra_env = vec![("CMD_AI_TEST_VAR".to_string(), "valeur-é".to_string())];
            let (r, _) = run(&s);
            assert!(
                r.stdout.text.contains("[valeur-é] [1] [NUL] [cat]"),
                "{:?}",
                r.stdout.text
            );
        }

        #[test]
        fn timeout_kills_a_long_ping_promptly() {
            let mut s = spec(cmd(), "echo debut & ping -n 30 127.0.0.1");
            s.timeout = Duration::from_secs(1);
            let start = Instant::now();
            let (r, _) = run(&s);
            let elapsed = start.elapsed();
            assert!(r.timed_out && !r.cancelled);
            assert_eq!(r.code, None);
            assert!(elapsed >= Duration::from_secs(1), "took {elapsed:?}");
            assert!(elapsed < Duration::from_secs(3), "took {elapsed:?}");
            // What was printed before the kill is kept.
            assert!(r.stdout.text.contains("debut"), "{:?}", r.stdout.text);
        }

        #[test]
        fn powershell_tree_is_killed_on_timeout() {
            let Some(info) = shell::find(Shell::PowerShell).or_else(|| shell::find(Shell::Pwsh))
            else {
                eprintln!("skip: no PowerShell");
                return;
            };
            let mut s = spec(info, "ping -n 30 127.0.0.1 | Out-Null");
            s.timeout = Duration::from_secs(2);
            let start = Instant::now();
            let (r, _) = run(&s);
            assert!(r.timed_out);
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "took {:?}",
                start.elapsed()
            );
        }

        #[test]
        fn inherited_honours_timeout_and_cancel() {
            let mut s = spec(cmd(), "ping -n 30 127.0.0.1 >nul");
            s.mode = ExecMode::Inherited;
            s.timeout = Duration::from_millis(500);
            let start = Instant::now();
            let (r, _) = run(&s);
            assert!(r.timed_out && !r.cancelled);
            assert_eq!(r.code, None);
            assert!(start.elapsed() < Duration::from_secs(3));

            s.timeout = Duration::from_secs(60);
            let cancel = AtomicBool::new(true);
            let r = SystemExecutor.run(&s, &mut |_, _| {}, &cancel).unwrap();
            assert!(r.cancelled && !r.timed_out);
        }

        #[test]
        fn detached_cancel_stops_waiting_without_killing() {
            let mut s = spec(cmd(), "ping -n 3 127.0.0.1 >nul");
            s.mode = ExecMode::Detached;
            let cancel = AtomicBool::new(true);
            let start = Instant::now();
            let r = SystemExecutor.run(&s, &mut |_, _| {}, &cancel).unwrap();
            assert!(r.cancelled);
            assert_eq!(r.code, None);
            assert!(start.elapsed() < Duration::from_secs(2));
        }

        #[test]
        fn quoting_corpus_through_cmd() {
            // Command-line (not batch) rules: `^&` escapes, `%%` and a lone `%` stay literal,
            // `%VAR%` expands, quotes are kept by echo.
            let (r, _) = run(&spec(
                cmd(),
                "echo \"a b\" ^& 100%% 5% [%NO_COLOR%] $x ; 'q' é",
            ));
            assert_eq!(
                r.stdout.text.trim_end(),
                "\"a b\" & 100%% 5% [1] $x ; 'q' é"
            );
        }

        #[test]
        fn quoting_corpus_through_powershell() {
            let Some(info) = shell::find(Shell::PowerShell).or_else(|| shell::find(Shell::Pwsh))
            else {
                return;
            };
            let command = "$v = 'a \"b\" c'; Write-Output \"[$v] [%PATH%] [^&|;] [é à]\"";
            let (r, _) = run(&spec(info, command));
            assert_eq!(
                r.stdout.text.trim_end(),
                "[a \"b\" c] [%PATH%] [^&|;] [é à]",
                "{}",
                r.stderr.text
            );
        }

        #[test]
        fn kill_on_drop_when_the_caller_panics() {
            let dir = sandbox("drop");
            let marker = dir.join("survivor.txt");
            let command = format!(
                "echo start & ping -n 4 127.0.0.1 >nul & echo done > \"{}\"",
                marker.display()
            );
            let s = spec(cmd(), &command);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let cancel = AtomicBool::new(false);
                let _ = SystemExecutor.run(
                    &s,
                    &mut |_, t| assert!(!t.contains("start"), "boom"),
                    &cancel,
                );
            }));
            assert!(result.is_err());
            std::thread::sleep(Duration::from_secs(5));
            assert!(!marker.exists(), "child survived the drop guard");
            let _ = std::fs::remove_dir_all(dir);
        }

        #[test]
        fn detached_returns_launcher_code() {
            let mut s = spec(cmd(), "exit /b 7");
            s.mode = ExecMode::Detached;
            let start = Instant::now();
            let (r, chunks) = run(&s);
            assert_eq!(r.code, Some(7));
            assert!(chunks.is_empty());
            assert!(start.elapsed() < Duration::from_secs(5));
        }

        #[test]
        fn detached_does_not_wait_forever() {
            let mut s = spec(cmd(), "ping -n 4 127.0.0.1 >nul");
            s.mode = ExecMode::Detached;
            let cancel = AtomicBool::new(false);
            let start = Instant::now();
            let r = run_detached(&s, &cancel, Duration::from_millis(500)).unwrap();
            assert_eq!(r.code, None);
            assert!(start.elapsed() < Duration::from_secs(3));
        }

        #[test]
        fn inherited_returns_code() {
            let mut s = spec(cmd(), "exit /b 2");
            s.mode = ExecMode::Inherited;
            let (r, chunks) = run(&s);
            assert_eq!(r.code, Some(2));
            assert!(chunks.is_empty());
            assert_eq!(r.stdout, Captured::default());
        }

        #[test]
        fn helper_programs() {
            assert_eq!(
                run_inherited_program("cmd", &["/d", "/c", "exit 5"]).unwrap(),
                Some(5)
            );
            assert_eq!(
                probe_program("cmd", &["/d", "/c", "exit 0"], Duration::from_secs(10)),
                Some(0)
            );
            assert_eq!(
                probe_program("cmd", &["/d", "/c", "exit 9"], Duration::from_secs(10)),
                Some(9)
            );
            let start = Instant::now();
            assert_eq!(
                probe_program(
                    "ping",
                    &["-n", "30", "127.0.0.1"],
                    Duration::from_millis(500)
                ),
                None
            );
            assert!(start.elapsed() < Duration::from_secs(4));
        }
    }

    // ---------------------------------------------------------------- Unix

    #[cfg(unix)]
    mod unix {
        use super::*;

        fn sh() -> ShellInfo {
            shell::find(Shell::Sh).expect("/bin/sh")
        }

        #[test]
        fn accents_and_success() {
            let (r, chunks) = run(&spec(sh(), "printf 'éàçù\\n'"));
            assert_eq!(r.code, Some(0));
            assert_eq!(r.stdout.text, "éàçù\n");
            assert_eq!(r.stdout.total_bytes, "éàçù\n".len() as u64);
            let live: String = chunks.iter().map(|(_, t)| t.as_str()).collect();
            assert_eq!(live, "éàçù\n");
            if let Some(bash) = shell::find(Shell::Bash) {
                let (r, _) = run(&spec(bash, "echo \"éàçù $((1+2))\""));
                assert_eq!(r.stdout.text, "éàçù 3\n");
            }
        }

        #[test]
        fn exit_codes_and_stderr() {
            let (r, _) = run(&spec(sh(), "echo sortie; echo erreur >&2; exit 3"));
            assert_eq!(r.code, Some(3));
            assert_eq!(r.stdout.text, "sortie\n");
            assert_eq!(r.stderr.text, "erreur\n");
        }

        #[test]
        fn cwd_is_used() {
            let dir = sandbox("cwd");
            let mut s = spec(sh(), "pwd -P");
            s.cwd = dir.clone();
            let (r, _) = run(&s);
            assert_eq!(
                PathBuf::from(r.stdout.text.trim_end()),
                dir.canonicalize().unwrap()
            );
            let _ = std::fs::remove_dir_all(dir);
        }

        #[test]
        fn timeout_kills_the_whole_tree() {
            let dir = sandbox("timeout");
            let marker = dir.join("late.txt");
            let command = format!("sh -c 'sleep 3; echo late > \"{}\"'", marker.display());
            let mut s = spec(sh(), &command);
            s.timeout = Duration::from_secs(1);
            let start = Instant::now();
            let (r, _) = run(&s);
            assert!(r.timed_out);
            assert_eq!(r.code, None);
            assert!(
                start.elapsed() < Duration::from_secs(4),
                "took {:?}",
                start.elapsed()
            );
            std::thread::sleep(Duration::from_secs(4));
            assert!(!marker.exists(), "grandchild survived the timeout");
            let _ = std::fs::remove_dir_all(dir);
        }

        #[test]
        fn background_child_holding_pipes_does_not_hang() {
            let start = Instant::now();
            let (r, _) = run(&spec(sh(), "sleep 30 & echo lance"));
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "took {:?}",
                start.elapsed()
            );
            assert_eq!(r.code, Some(0));
            assert_eq!(r.stdout.text, "lance\n");
        }

        #[test]
        fn cancel_flag_kills() {
            let s = spec(sh(), "sleep 30");
            let cancel = Arc::new(AtomicBool::new(false));
            let c2 = Arc::clone(&cancel);
            let setter = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(500));
                c2.store(true, Ordering::SeqCst);
            });
            let start = Instant::now();
            let r = SystemExecutor.run(&s, &mut |_, _| {}, &cancel).unwrap();
            setter.join().unwrap();
            assert!(r.cancelled);
            assert_eq!(r.code, None);
            assert!(start.elapsed() < Duration::from_secs(5));
        }

        #[test]
        fn capture_limit_truncates_but_counts() {
            let mut s = spec(
                sh(),
                "i=0; while [ $i -lt 3000 ]; do echo ligne numero $i; i=$((i+1)); done",
            );
            s.capture_limit = 1000;
            let (r, _) = run(&s);
            assert_eq!(r.code, Some(0));
            assert!(r.stdout.truncated);
            assert!(r.stdout.text.len() <= 1000);
            assert!(r.stdout.total_bytes > 30_000);
        }

        #[test]
        fn stdin_is_closed() {
            let mut s = spec(sh(), "read x; echo \"rc=$?\"");
            s.timeout = Duration::from_secs(20);
            let (r, _) = run(&s);
            assert!(!r.timed_out);
            assert!(r.stdout.text.starts_with("rc="));
            assert_ne!(r.stdout.text, "rc=0\n");
        }

        #[test]
        fn extra_env_is_visible() {
            let mut s = spec(
                sh(),
                "echo \"[$CMD_AI_TEST_VAR] [$NO_COLOR] [$PAGER] [$PSModuleAnalysisCachePath]\"",
            );
            s.extra_env = vec![("CMD_AI_TEST_VAR".to_string(), "valeur-é".to_string())];
            let (r, _) = run(&s);
            assert_eq!(r.stdout.text, "[valeur-é] [1] [cat] [/dev/null]\n");
        }

        #[test]
        fn timeout_kills_a_long_sleep_promptly() {
            let mut s = spec(sh(), "echo debut; sleep 30");
            s.timeout = Duration::from_secs(1);
            let start = Instant::now();
            let (r, _) = run(&s);
            let elapsed = start.elapsed();
            assert!(r.timed_out && !r.cancelled);
            assert_eq!(r.code, None);
            assert!(elapsed < Duration::from_secs(3), "took {elapsed:?}");
            assert_eq!(r.stdout.text, "debut\n");
        }

        #[test]
        fn inherited_honours_timeout_and_cancel() {
            let mut s = spec(sh(), "sleep 30");
            s.mode = ExecMode::Inherited;
            s.timeout = Duration::from_millis(500);
            let start = Instant::now();
            let (r, _) = run(&s);
            assert!(r.timed_out);
            assert_eq!(r.code, None);
            assert!(start.elapsed() < Duration::from_secs(3));
            s.timeout = Duration::from_secs(60);
            let cancel = AtomicBool::new(true);
            let r = SystemExecutor.run(&s, &mut |_, _| {}, &cancel).unwrap();
            assert!(r.cancelled);
        }

        #[test]
        fn detached_cancel_stops_waiting() {
            let mut s = spec(sh(), "sleep 2");
            s.mode = ExecMode::Detached;
            let cancel = AtomicBool::new(true);
            let r = SystemExecutor.run(&s, &mut |_, _| {}, &cancel).unwrap();
            assert!(r.cancelled);
            assert_eq!(r.code, None);
        }

        #[test]
        fn kill_on_drop_when_the_caller_panics() {
            let dir = sandbox("drop");
            let marker = dir.join("survivor.txt");
            let command = format!("echo start; sleep 2; echo done > \"{}\"", marker.display());
            let s = spec(sh(), &command);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let cancel = AtomicBool::new(false);
                let _ = SystemExecutor.run(
                    &s,
                    &mut |_, t| assert!(!t.contains("start"), "boom"),
                    &cancel,
                );
            }));
            assert!(result.is_err());
            std::thread::sleep(Duration::from_secs(3));
            assert!(!marker.exists(), "child survived the drop guard");
            let _ = std::fs::remove_dir_all(dir);
        }

        #[test]
        fn detached_and_inherited() {
            let mut s = spec(sh(), "exit 7");
            s.mode = ExecMode::Detached;
            let (r, _) = run(&s);
            assert_eq!(r.code, Some(7));
            let mut s = spec(sh(), "sleep 3");
            s.mode = ExecMode::Detached;
            let cancel = AtomicBool::new(false);
            let r = run_detached(&s, &cancel, Duration::from_millis(300)).unwrap();
            assert_eq!(r.code, None);
            let mut s = spec(sh(), "exit 2");
            s.mode = ExecMode::Inherited;
            let (r, _) = run(&s);
            assert_eq!(r.code, Some(2));
        }

        #[test]
        fn helper_programs() {
            assert_eq!(
                run_inherited_program("sh", &["-c", "exit 5"]).unwrap(),
                Some(5)
            );
            assert_eq!(
                probe_program("sh", &["-c", "exit 0"], Duration::from_secs(10)),
                Some(0)
            );
            let start = Instant::now();
            assert_eq!(
                probe_program("sleep", &["30"], Duration::from_millis(300)),
                None
            );
            assert!(start.elapsed() < Duration::from_secs(4));
        }
    }
}
