//! "TUI ouvert": ratatui inline viewport (no alternate screen), so finished blocks stay in
//! the terminal scrollback (FR-062).
//!
//! The agent runs on a worker thread and talks to this UI thread through channels
//! ([`ChannelFrontend`]). The viewport shows the status bar (mode and privilege always
//! visible), the progress `n/M`, the live tail of the current output and the input line.

use std::io::{self, Stdout};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use ratatui::Terminal;
use ratatui::TerminalOptions;
use ratatui::Viewport;
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::crossterm::{cursor, execute, terminal};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::theme::{Role, Theme};
use super::{AgentEvent, Approval, ApprovalRequest, Frontend, PlanStep, StatusLine, TaskOutcome};
use crate::Session;
use crate::agent::{Agent, SystemElevator};
use crate::config::{Config, PermissionMode};
use crate::exec::{Stream, SystemExecutor};
use crate::text;

/// Height of the inline viewport.
const VIEWPORT_HEIGHT: u16 = 6;
/// Rows inserted into the scrollback per frame at most.
const MAX_INSERT_ROWS: usize = 400;
const GUTTER: &str = "  │ ";
const SPINNER: [&str; 4] = ["◐", "◓", "◑", "◒"];

/// Agent thread → UI thread.
pub enum UiMsg {
    Event(AgentEvent),
    Approve(ApprovalRequest),
    /// Give the terminal away (sudo -v, interactive step); the UI acks on `suspended`.
    Suspend,
    Resume,
    TaskDone(TaskOutcome),
}

/// UI thread → agent thread.
pub enum AgentCmd {
    Run(String),
    SetMode(PermissionMode),
    SetModel(String),
    Clear,
    Quit,
}

/// The `Frontend` the agent sees on its thread.
pub struct ChannelFrontend {
    to_ui: Sender<UiMsg>,
    approvals: Receiver<Approval>,
    suspended: Receiver<()>,
    cancel: Arc<AtomicBool>,
}

impl Frontend for ChannelFrontend {
    fn event(&mut self, ev: AgentEvent) {
        let _ = self.to_ui.send(UiMsg::Event(ev));
    }

    fn approve(&mut self, req: ApprovalRequest) -> Approval {
        if self.to_ui.send(UiMsg::Approve(req)).is_err() {
            return Approval::No;
        }
        self.approvals.recv().unwrap_or(Approval::No)
    }

    fn handover(&mut self, run: &mut dyn FnMut() -> Option<i32>) -> Option<i32> {
        if self.to_ui.send(UiMsg::Suspend).is_err() || self.suspended.recv().is_err() {
            return None;
        }
        let code = run();
        let _ = self.to_ui.send(UiMsg::Resume);
        code
    }

    fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel)
    }
}

/// What the UI thread does after a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyAction {
    None,
    Submit(String),
    Answer(Approval),
    Cancel,
    Quit,
}

/// Everything rendered (pure state, unit-tested).
pub struct UiState {
    pub theme: Theme,
    pub status: Option<StatusLine>,
    pub busy: bool,
    pub activity: String,
    pub progress: Option<(usize, usize)>,
    /// Current incomplete line (command output or streamed answer).
    pub partial: String,
    partial_role: Role,
    in_command: bool,
    answer_streamed: bool,
    pub pending: Vec<Line<'static>>,
    pub input: String,
    pub approval: Option<ApprovalRequest>,
    pub quit: bool,
    tick: usize,
}

impl UiState {
    pub fn new(theme: Theme) -> Self {
        UiState {
            theme,
            status: None,
            busy: false,
            activity: String::new(),
            progress: None,
            partial: String::new(),
            partial_role: Role::Text,
            in_command: false,
            answer_streamed: false,
            pending: Vec::new(),
            input: String::new(),
            approval: None,
            quit: false,
            tick: 0,
        }
    }

    fn style(&self, role: Role) -> Style {
        self.theme.style(role)
    }

    fn push(&mut self, role: Role, text: impl Into<String>) {
        let style = self.style(role);
        for l in text.into().split('\n') {
            self.pending
                .push(Line::from(Span::styled(l.to_string(), style)));
        }
    }

    fn push_spans(&mut self, spans: Vec<Span<'static>>) {
        self.pending.push(Line::from(spans));
    }

    fn flush_partial(&mut self) {
        if self.partial.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.partial);
        if self.in_command {
            self.output_line(&text, self.partial_role);
        } else {
            self.push(self.partial_role, text);
        }
    }

    fn output_line(&mut self, line: &str, role: Role) {
        let spans = vec![
            Span::styled(GUTTER, self.style(Role::Dim)),
            Span::styled(line.trim_end_matches('\r').to_string(), self.style(role)),
        ];
        self.push_spans(spans);
    }

    fn stream_text(&mut self, text: &str, role: Role, command: bool) {
        if self.partial_role != role || self.in_command != command {
            self.flush_partial();
        }
        self.partial_role = role;
        self.in_command = command;
        let mut rest = text;
        while let Some(i) = rest.find('\n') {
            let piece = &rest[..i];
            let mut line = std::mem::take(&mut self.partial);
            line.push_str(piece);
            if command {
                self.output_line(&line, role);
            } else {
                self.push(role, line);
            }
            rest = &rest[i + 1..];
        }
        self.partial.push_str(rest);
    }

    fn step_spans(&self, prefix: String, step: &PlanStep) -> Vec<Span<'static>> {
        let mut spans = vec![
            Span::styled(prefix, self.style(Role::Accent)),
            Span::styled(format!("{} · ", step.shell), self.style(Role::Dim)),
            Span::styled(
                text::privilege_label(step.admin).to_string(),
                self.style(if step.admin { Role::Admin } else { Role::Dim }),
            ),
        ];
        if !step.purpose.is_empty() {
            spans.push(Span::styled(
                format!(" — {}", step.purpose),
                self.style(Role::Text),
            ));
        }
        spans
    }

    fn command_line(&mut self, indent: &str, command: &str) {
        for (i, l) in command.lines().enumerate() {
            let prefix = if i == 0 { "$ " } else { "  " };
            let spans = vec![
                Span::raw(indent.to_string()),
                Span::styled(format!("{prefix}{l}"), self.style(Role::Command)),
            ];
            self.push_spans(spans);
        }
    }

    /// Applies one agent event.
    pub fn apply(&mut self, ev: AgentEvent) {
        match ev {
            AgentEvent::Status(s) => self.status = Some(s),
            AgentEvent::Reasoning(t) => self.stream_text(&t, Role::Reasoning, false),
            AgentEvent::AssistantDelta(t) => {
                self.answer_streamed = true;
                self.activity = text::TUI_THINKING.to_string();
                self.stream_text(&t, Role::Text, false);
            }
            AgentEvent::AssistantDone(t) => {
                self.flush_partial();
                if !self.answer_streamed && !t.is_empty() {
                    self.push(Role::Text, t);
                }
                self.answer_streamed = false;
            }
            AgentEvent::Plan(steps) => {
                self.flush_partial();
                self.push(Role::Plan, text::plan_title(steps.len()));
                for (i, step) in steps.iter().enumerate() {
                    let spans = self.step_spans(format!("  {}. ", i + 1), step);
                    self.push_spans(spans);
                    self.command_line("     ", &step.command);
                }
            }
            AgentEvent::Progress { done, total } => self.progress = Some((done, total)),
            AgentEvent::CommandStart {
                index,
                total,
                step,
                hook,
            } => {
                self.flush_partial();
                self.answer_streamed = false;
                let tag = if hook {
                    format!("▶ [{}] ", text::HOOK_TAG)
                } else {
                    format!("▶ [{index}/{total}] ")
                };
                if !hook {
                    self.progress = Some((index.saturating_sub(1), total));
                }
                let spans = self.step_spans(tag, &step);
                self.push_spans(spans);
                self.command_line("  ", &step.command);
                self.activity = format!("{} : {}", text::TUI_RUNNING, step.command);
                self.in_command = true;
            }
            AgentEvent::CommandOutput { stream, text } => {
                let role = match stream {
                    Stream::Stdout => Role::Text,
                    Stream::Stderr => Role::Warning,
                };
                self.stream_text(&text, role, true);
            }
            AgentEvent::CommandEnd {
                code,
                duration,
                timed_out,
                cancelled,
                truncated,
            } => {
                self.flush_partial();
                self.in_command = false;
                let mut line = text::command_end(code, duration.as_secs_f64());
                for (flag, label) in [
                    (timed_out, text::OUTPUT_TIMED_OUT),
                    (cancelled, text::OUTPUT_CANCELLED),
                    (truncated, text::OUTPUT_TRUNCATED),
                ] {
                    if flag {
                        line.push_str(" · ");
                        line.push_str(label);
                    }
                }
                let (mark, role) = if code == Some(0) {
                    ("✓", Role::Success)
                } else {
                    ("✗", Role::Error)
                };
                self.push(role, format!("  {mark} {line}"));
                self.activity = text::TUI_THINKING.to_string();
            }
            AgentEvent::Info(t) => {
                self.flush_partial();
                self.push(Role::Dim, format!("ℹ {t}"));
            }
            AgentEvent::Warning(t) => {
                self.flush_partial();
                self.push(Role::Warning, format!("⚠ {t}"));
            }
            AgentEvent::Error(t) => {
                self.flush_partial();
                self.push(Role::Error, format!("✗ {t}"));
            }
            AgentEvent::TaskEnd(_) => {
                self.flush_partial();
            }
        }
    }

    /// An approval request arrives: show the details, wait for o/n.
    pub fn ask(&mut self, req: ApprovalRequest) {
        self.flush_partial();
        match &req {
            ApprovalRequest::Plan(_) => {}
            ApprovalRequest::Single { step, garde_fou } => {
                if let Some(p) = garde_fou {
                    self.push(
                        Role::Warning,
                        format!("⚠ {}", text::ask_garde_fou(p).trim_end()),
                    );
                } else {
                    self.push(Role::Warning, text::ASK_SINGLE.trim_end().to_string());
                }
                let spans = self.step_spans("  ".into(), step);
                self.push_spans(spans);
                self.command_line("  ", &step.command);
            }
            ApprovalRequest::Elevation { reason } => self.push(Role::Admin, reason.clone()),
        }
        self.approval = Some(req);
    }

    pub fn task_done(&mut self, outcome: TaskOutcome) {
        self.flush_partial();
        self.busy = false;
        self.approval = None;
        self.activity.clear();
        self.progress = None;
        self.push(Role::Dim, String::new());
        if outcome == TaskOutcome::Elevating {
            self.quit = true;
        }
    }

    /// Key handling (pure).
    pub fn key(&mut self, k: KeyEvent) -> KeyAction {
        if k.kind == KeyEventKind::Release {
            return KeyAction::None;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if self.approval.is_some() {
            let answer = match k.code {
                KeyCode::Char('o' | 'O' | 'y' | 'Y') if !ctrl => Some(Approval::Yes),
                KeyCode::Char('n' | 'N') if !ctrl => Some(Approval::No),
                KeyCode::Esc => Some(Approval::No),
                KeyCode::Char('c') if ctrl => Some(Approval::No),
                _ => None,
            };
            return match answer {
                Some(a) => {
                    self.approval = None;
                    KeyAction::Answer(a)
                }
                None => KeyAction::None,
            };
        }
        match k.code {
            KeyCode::Char('d') if ctrl => KeyAction::Quit,
            KeyCode::Char('c') if ctrl => {
                if self.busy {
                    KeyAction::Cancel
                } else if !self.input.is_empty() {
                    self.input.clear();
                    KeyAction::None
                } else {
                    KeyAction::Quit
                }
            }
            KeyCode::Esc => {
                if self.busy {
                    KeyAction::Cancel
                } else {
                    self.input.clear();
                    KeyAction::None
                }
            }
            KeyCode::Enter => {
                let line = self.input.trim().to_string();
                if line.is_empty() {
                    return KeyAction::None;
                }
                if self.busy && !line.starts_with('/') {
                    self.push(Role::Warning, text::TUI_BUSY);
                    return KeyAction::None;
                }
                self.input.clear();
                KeyAction::Submit(line)
            }
            KeyCode::Backspace => {
                self.input.pop();
                KeyAction::None
            }
            KeyCode::Char(c) if !ctrl => {
                self.input.push(c);
                KeyAction::None
            }
            _ => KeyAction::None,
        }
    }

    /// Renders the inline viewport.
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }
        let row = |i: u16| Rect::new(area.x, area.y + i, area.width, 1);
        // 0: status bar.
        if let Some(s) = &self.status {
            let (label, role) = match s.mode {
                PermissionMode::Plan => (text::MODE_PLAN, Role::Plan),
                PermissionMode::Bypass => (text::MODE_BYPASS, Role::Bypass),
            };
            let mut spans = vec![
                Span::styled(
                    format!(" {label} "),
                    self.style(role)
                        .add_modifier(Modifier::REVERSED | Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    text::privilege_label(s.admin).to_string(),
                    self.style(if s.admin { Role::Admin } else { Role::Dim }),
                ),
                Span::styled(
                    format!(" · {} · {}", s.model, s.shell),
                    self.style(Role::Dim),
                ),
            ];
            if s.ephemeral {
                spans.push(Span::styled(
                    format!(" · {}", text::EPHEMERAL),
                    self.style(Role::Warning),
                ));
            }
            Paragraph::new(Line::from(spans)).render(row(0), buf);
        }
        if area.height < 2 {
            return;
        }
        // 1: activity + progress.
        if self.busy {
            let mut spans = vec![Span::styled(
                format!("{} ", SPINNER[self.tick % SPINNER.len()]),
                self.style(Role::Accent),
            )];
            if let Some((done, total)) = self.progress {
                if total > 0 {
                    spans.push(Span::styled(
                        format!("[{}] ", gauge(done, total, 10)),
                        self.style(Role::Accent),
                    ));
                    spans.push(Span::styled(
                        format!("{}  ", text::progress(done, total)),
                        self.style(Role::Text),
                    ));
                }
            }
            let activity = if self.approval.is_some() {
                text::TUI_WAITING_APPROVAL.to_string()
            } else if self.activity.is_empty() {
                text::TUI_THINKING.to_string()
            } else {
                self.activity.clone()
            };
            spans.push(Span::styled(activity, self.style(Role::Dim)));
            Paragraph::new(Line::from(spans)).render(row(1), buf);
        }
        // 2-3: live tail.
        if area.height >= 4 && !self.partial.is_empty() {
            let tail: String = self
                .partial
                .chars()
                .rev()
                .take(area.width as usize * 2)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let prefix = if self.in_command { GUTTER } else { "" };
            Paragraph::new(Line::from(vec![
                Span::styled(prefix, self.style(Role::Dim)),
                Span::styled(tail, self.style(self.partial_role)),
            ]))
            .wrap(ratatui::widgets::Wrap { trim: false })
            .render(Rect::new(area.x, area.y + 2, area.width, 2), buf);
        }
        let input_row = area.height.saturating_sub(2).max(2);
        let hint_row = area.height.saturating_sub(1).max(3);
        // input or approval question.
        if input_row < area.height {
            let line = match &self.approval {
                Some(req) => {
                    let q = match req {
                        ApprovalRequest::Plan(_) => text::ASK_PLAN.to_string(),
                        ApprovalRequest::Single {
                            garde_fou: Some(p), ..
                        } => text::ask_garde_fou(p),
                        ApprovalRequest::Single { .. } => text::ASK_SINGLE.to_string(),
                        ApprovalRequest::Elevation { .. } => text::ASK_ELEVATION.to_string(),
                    };
                    Line::from(Span::styled(
                        q,
                        self.style(Role::Warning).add_modifier(Modifier::BOLD),
                    ))
                }
                None if self.input.is_empty() => Line::from(vec![
                    Span::styled("› ", self.style(Role::Accent)),
                    Span::styled(text::TUI_INPUT_PLACEHOLDER, self.style(Role::Dim)),
                ]),
                None => Line::from(vec![
                    Span::styled("› ", self.style(Role::Accent)),
                    Span::styled(self.input.clone(), self.style(Role::Text)),
                ]),
            };
            Paragraph::new(line).render(row(input_row), buf);
        }
        if hint_row < area.height && hint_row != input_row {
            let hint = if self.approval.is_some() {
                text::TUI_KEYS_APPROVAL
            } else if self.busy {
                text::TUI_KEYS_RUNNING
            } else {
                text::TUI_KEYS_IDLE
            };
            Paragraph::new(Line::from(Span::styled(hint, self.style(Role::Dim))))
                .render(row(hint_row), buf);
        }
    }

    /// Cursor position inside the viewport (end of the input line).
    fn cursor(&self, area: Rect) -> Option<(u16, u16)> {
        if self.approval.is_some() || area.height < 3 {
            return None;
        }
        let input_row = area.height.saturating_sub(2).max(2);
        let width = Line::from(self.input.as_str()).width() as u16;
        Some((
            area.x + (2 + width).min(area.width.saturating_sub(1)),
            area.y + input_row,
        ))
    }
}

fn gauge(done: usize, total: usize, width: usize) -> String {
    let filled = (done * width).checked_div(total).unwrap_or(0).min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

/// Splits a styled line into rows of at most `width` display columns (char-based), so that
/// `insert_before` gets the exact row count.
pub fn wrap_line(line: &Line<'static>, width: u16) -> Vec<Line<'static>> {
    let width = width.max(1) as usize;
    if line.width() <= width {
        return vec![line.clone()];
    }
    let mut rows: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    for span in &line.spans {
        let mut chunk = String::new();
        for ch in span.content.chars() {
            let mut tmp = [0u8; 4];
            let w = Span::raw(&*ch.encode_utf8(&mut tmp)).width();
            if used + w > width && used > 0 {
                if !chunk.is_empty() {
                    current.push(Span::styled(std::mem::take(&mut chunk), span.style));
                }
                rows.push(Line::from(std::mem::take(&mut current)));
                used = 0;
            }
            chunk.push(ch);
            used += w;
        }
        if !chunk.is_empty() {
            current.push(Span::styled(chunk, span.style));
        }
    }
    if !current.is_empty() || rows.is_empty() {
        rows.push(Line::from(current));
    }
    rows
}

fn new_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    Terminal::with_options(
        CrosstermBackend::new(io::stdout()),
        TerminalOptions {
            viewport: Viewport::Inline(VIEWPORT_HEIGHT),
        },
    )
}

/// Checks that the terminal supports what the inline viewport needs (raw mode and the
/// cursor position query). Some consoles never answer the query: lib.rs then falls back to
/// the plain text mode instead of failing.
pub fn probe() -> Result<(), String> {
    terminal::enable_raw_mode().map_err(|e| e.to_string())?;
    let result = cursor::position().map(|_| ()).map_err(|e| e.to_string());
    let _ = terminal::disable_raw_mode();
    result
}

fn restore_terminal() {
    let _ = terminal::disable_raw_mode();
    let _ = execute!(io::stdout(), cursor::Show);
}

/// Inserts the pending lines above the viewport, at most [`MAX_INSERT_ROWS`] rows per call.
fn flush_pending<B: Backend>(term: &mut Terminal<B>, state: &mut UiState) -> io::Result<()> {
    if state.pending.is_empty() {
        return Ok(());
    }
    let width = term.size().map(|s| s.width).unwrap_or(80).max(1);
    let mut rows: Vec<Line<'static>> = Vec::new();
    let mut consumed = 0;
    for line in &state.pending {
        let wrapped = wrap_line(line, width);
        if !rows.is_empty() && rows.len() + wrapped.len() > MAX_INSERT_ROWS {
            break;
        }
        rows.extend(wrapped);
        consumed += 1;
    }
    state.pending.drain(..consumed);
    let height = rows.len() as u16;
    term.insert_before(height, |buf| {
        Paragraph::new(rows).render(buf.area, buf);
    })
    .map_err(|e| io::Error::other(e.to_string()))
}

/// Runs the interactive TUI until the user quits. Returns the process exit code.
pub fn run(session: Session, initial_prompt: Option<String>) -> i32 {
    let Session {
        client,
        settings,
        skills,
        warnings,
        mut memory,
        mut log,
        redactor,
        home,
        theme,
        config,
    } = session;

    let (to_ui, from_agent) = mpsc::channel::<UiMsg>();
    let (to_agent, agent_cmds) = mpsc::channel::<AgentCmd>();
    let (approval_tx, approval_rx) = mpsc::channel::<Approval>();
    let (suspended_tx, suspended_rx) = mpsc::channel::<()>();
    let cancel = Arc::new(AtomicBool::new(false));
    // Raw mode turns Ctrl+C into a key; while the terminal is handed over (sudo -v), the
    // signal must still cancel cleanly instead of killing cmd_ai.
    crate::platform::install_interrupt_handler(Arc::clone(&cancel));
    let ephemeral = !home.writable;

    let agent_cancel = Arc::clone(&cancel);
    let agent_to_ui = to_ui.clone();
    let worker = std::thread::Builder::new()
        .name("cmd_ai-agent".into())
        .spawn(move || {
            let mut exec = SystemExecutor;
            let mut elevator = SystemElevator { home };
            let mut agent = Agent::new(
                &client,
                &mut exec,
                &mut elevator,
                settings,
                skills,
                &mut memory,
                &mut log,
                redactor,
            );
            let mut fe = ChannelFrontend {
                to_ui: agent_to_ui.clone(),
                approvals: approval_rx,
                suspended: suspended_rx,
                cancel: agent_cancel,
            };
            fe.event(AgentEvent::Status(agent.status()));
            agent.session_start(&mut fe);
            while let Ok(cmd) = agent_cmds.recv() {
                match cmd {
                    AgentCmd::Run(p) => {
                        let outcome = agent.run_task(&p, &mut fe);
                        let _ = agent_to_ui.send(UiMsg::TaskDone(outcome));
                        if outcome == TaskOutcome::Elevating {
                            break;
                        }
                    }
                    AgentCmd::SetMode(m) => {
                        agent.set_mode(m);
                        fe.event(AgentEvent::Status(agent.status()));
                    }
                    AgentCmd::SetModel(m) => {
                        agent.set_model(m);
                        fe.event(AgentEvent::Status(agent.status()));
                    }
                    AgentCmd::Clear => agent.clear_history(),
                    AgentCmd::Quit => break,
                }
            }
        });
    let worker = match worker {
        Ok(w) => w,
        Err(e) => {
            eprintln!("{}", text::tool_spawn_failed(&e.to_string()));
            return 1;
        }
    };

    let mut state = UiState::new(theme);
    for w in warnings {
        state.push(Role::Warning, format!("⚠ {w}"));
    }
    if ephemeral {
        state.push(Role::Warning, format!("⚠ {}", text::EPHEMERAL_NOTICE));
    }
    if let Some(p) = initial_prompt {
        state.push(Role::Accent, format!("› {p}"));
        state.busy = true;
        state.activity = text::TUI_THINKING.to_string();
        let _ = to_agent.send(AgentCmd::Run(p));
    }

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous_hook(info);
    }));

    let code = match ui_loop(
        &mut state,
        &from_agent,
        &to_agent,
        &approval_tx,
        &suspended_tx,
        &cancel,
        &config,
    ) {
        Ok(()) => 0,
        Err(e) => {
            restore_terminal();
            eprintln!("{}", text::tui_error(&e.to_string()));
            1
        }
    };
    cancel.store(true, Ordering::SeqCst);
    let _ = to_agent.send(AgentCmd::Quit);
    drop(approval_tx);
    let _ = worker.join();
    code
}

#[allow(clippy::too_many_arguments)]
fn ui_loop(
    state: &mut UiState,
    from_agent: &Receiver<UiMsg>,
    to_agent: &Sender<AgentCmd>,
    approval_tx: &Sender<Approval>,
    suspended_tx: &Sender<()>,
    cancel: &Arc<AtomicBool>,
    config: &Config,
) -> io::Result<()> {
    terminal::enable_raw_mode()?;
    let mut term = match new_terminal() {
        Ok(t) => t,
        Err(e) => {
            restore_terminal();
            return Err(e);
        }
    };
    let result = (|| -> io::Result<()> {
        loop {
            // Agent messages.
            loop {
                match from_agent.try_recv() {
                    Ok(UiMsg::Event(ev)) => state.apply(ev),
                    Ok(UiMsg::Approve(req)) => state.ask(req),
                    Ok(UiMsg::TaskDone(o)) => state.task_done(o),
                    Ok(UiMsg::Suspend) => {
                        flush_all(&mut term, state)?;
                        term.clear().map_err(|e| io::Error::other(e.to_string()))?;
                        restore_terminal();
                        let _ = suspended_tx.send(());
                        // Wait until the program that owns the terminal is done.
                        loop {
                            match from_agent.recv() {
                                Ok(UiMsg::Resume) => break,
                                Ok(UiMsg::Event(ev)) => state.apply(ev),
                                Ok(_) => {}
                                Err(_) => return Ok(()),
                            }
                        }
                        terminal::enable_raw_mode()?;
                        term = new_terminal()?;
                    }
                    Ok(UiMsg::Resume) => {}
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        state.quit = true;
                        break;
                    }
                }
            }
            flush_pending(&mut term, state)?;
            state.tick = state.tick.wrapping_add(1);
            term.draw(|f| {
                let area = f.area();
                state.render(area, f.buffer_mut());
                if let Some((x, y)) = state.cursor(area) {
                    f.set_cursor_position((x, y));
                }
            })
            .map_err(|e| io::Error::other(e.to_string()))?;
            if state.quit && state.pending.is_empty() {
                break;
            }
            if !event::poll(Duration::from_millis(60))? {
                continue;
            }
            match event::read()? {
                Event::Key(k) => match state.key(k) {
                    KeyAction::None => {}
                    KeyAction::Answer(a) => {
                        let _ = approval_tx.send(a);
                    }
                    KeyAction::Cancel => cancel.store(true, Ordering::SeqCst),
                    KeyAction::Quit => {
                        if state.busy {
                            cancel.store(true, Ordering::SeqCst);
                        }
                        state.push(Role::Dim, text::TUI_BYE);
                        state.quit = true;
                    }
                    KeyAction::Submit(line) => submit(state, &line, to_agent, config),
                },
                Event::Paste(p) if state.approval.is_none() => {
                    state.input.push_str(&p.replace(['\r', '\n'], " "));
                }
                _ => {}
            }
        }
        flush_all(&mut term, state)?;
        term.clear().map_err(|e| io::Error::other(e.to_string()))?;
        Ok(())
    })();
    restore_terminal();
    result
}

fn flush_all<B: Backend>(term: &mut Terminal<B>, state: &mut UiState) -> io::Result<()> {
    while !state.pending.is_empty() {
        flush_pending(term, state)?;
    }
    Ok(())
}

/// A submitted line: slash command or a new task.
fn submit(state: &mut UiState, line: &str, to_agent: &Sender<AgentCmd>, config: &Config) {
    if let Some(rest) = line.strip_prefix('/') {
        let mut parts = rest.splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or("").to_lowercase();
        let arg = parts.next().map(str::trim).filter(|a| !a.is_empty());
        match cmd.as_str() {
            "aide" | "help" | "?" => state.push(Role::Dim, text::TUI_HELP),
            "plan" => {
                let _ = to_agent.send(AgentCmd::SetMode(PermissionMode::Plan));
                state.push(Role::Plan, text::tui_mode_set(text::MODE_PLAN));
            }
            "bypass" => {
                let _ = to_agent.send(AgentCmd::SetMode(PermissionMode::Bypass));
                state.push(Role::Bypass, text::tui_mode_set(text::MODE_BYPASS));
            }
            "model" | "modele" | "modèle" => match config.resolve_model(arg) {
                Ok(m) => {
                    state.push(Role::Dim, text::tui_model_set(&m));
                    let _ = to_agent.send(AgentCmd::SetModel(m));
                }
                Err(e) => state.push(Role::Error, format!("✗ {}", text::config_error(&e))),
            },
            "effacer" | "clear" => {
                let _ = to_agent.send(AgentCmd::Clear);
                state.push(Role::Dim, text::TUI_CLEARED);
            }
            "quitter" | "quit" | "exit" => {
                state.push(Role::Dim, text::TUI_BYE);
                state.quit = true;
            }
            _ => state.push(Role::Warning, text::tui_unknown_command(line)),
        }
        return;
    }
    state.push(Role::Accent, format!("› {line}"));
    state.busy = true;
    state.activity = text::TUI_THINKING.to_string();
    state.progress = None;
    let _ = to_agent.send(AgentCmd::Run(line.to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::{Background, ColorLevel};
    use ratatui::backend::TestBackend;

    fn state() -> UiState {
        UiState::new(Theme::new(ColorLevel::None, Background::Dark))
    }

    fn lines(s: &UiState) -> Vec<String> {
        s.pending
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|sp| sp.content.to_string())
                    .collect::<String>()
            })
            .collect()
    }

    fn screen(s: &UiState, w: u16, h: u16) -> String {
        let backend = TestBackend::new(w, h);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| s.render(f.area(), f.buffer_mut())).unwrap();
        let buf = term.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn key(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    fn step(cmd: &str) -> PlanStep {
        PlanStep {
            command: cmd.into(),
            purpose: "Lister".into(),
            shell: "bash".into(),
            admin: false,
        }
    }

    #[test]
    fn command_block_and_progress() {
        let mut s = state();
        s.busy = true;
        s.apply(AgentEvent::CommandStart {
            index: 2,
            total: 4,
            step: step("ls -la"),
            hook: false,
        });
        s.apply(AgentEvent::CommandOutput {
            stream: Stream::Stdout,
            text: "a\nb".into(),
        });
        assert_eq!(s.partial, "b");
        s.apply(AgentEvent::CommandEnd {
            code: Some(0),
            duration: Duration::from_millis(300),
            timed_out: false,
            cancelled: false,
            truncated: false,
        });
        s.apply(AgentEvent::Progress { done: 2, total: 4 });
        let l = lines(&s);
        assert!(
            l[0].contains("▶ [2/4] bash · utilisateur — Lister"),
            "{l:?}"
        );
        assert!(l[1].contains("$ ls -la"));
        assert_eq!(l[2], "  │ a");
        assert_eq!(l[3], "  │ b");
        assert!(l[4].contains("✓ code 0 · 0,3 s"));
        let sc = screen(&s, 60, VIEWPORT_HEIGHT);
        assert!(sc.contains("2/4 · 50 %"), "{sc}");
    }

    #[test]
    fn status_bar_always_shows_mode_and_privilege() {
        let mut s = state();
        s.apply(AgentEvent::Status(StatusLine {
            mode: PermissionMode::Bypass,
            admin: true,
            model: "modele-test".into(),
            ephemeral: true,
            shell: "pwsh".into(),
        }));
        let sc = screen(&s, 80, VIEWPORT_HEIGHT);
        assert!(sc.contains("BYPASS"));
        assert!(sc.contains("admin"));
        assert!(sc.contains("modele-test"));
        assert!(sc.contains("éphémère"));
        assert!(sc.contains("Votre demande"), "placeholder when idle: {sc}");
    }

    #[test]
    fn plan_approval_flow() {
        let mut s = state();
        s.busy = true;
        s.apply(AgentEvent::Plan(vec![step("ls"), step("pwd")]));
        s.ask(ApprovalRequest::Plan(vec![step("ls"), step("pwd")]));
        let sc = screen(&s, 60, VIEWPORT_HEIGHT);
        assert!(sc.contains("Exécuter ce plan"), "{sc}");
        assert!(sc.contains("[o] exécuter"));
        assert_eq!(
            s.key(key(KeyCode::Char('x'))),
            KeyAction::None,
            "other keys ignored"
        );
        assert_eq!(
            s.key(key(KeyCode::Char('o'))),
            KeyAction::Answer(Approval::Yes)
        );
        assert!(s.approval.is_none());
        s.ask(ApprovalRequest::Plan(vec![]));
        assert_eq!(s.key(key(KeyCode::Esc)), KeyAction::Answer(Approval::No));
        let l = lines(&s);
        assert!(l[0].contains("Plan (2 commandes)"));
    }

    #[test]
    fn input_editing_and_keys() {
        let mut s = state();
        for c in "salut".chars() {
            s.key(key(KeyCode::Char(c)));
        }
        s.key(key(KeyCode::Backspace));
        assert_eq!(s.input, "salu");
        assert_eq!(s.key(key(KeyCode::Enter)), KeyAction::Submit("salu".into()));
        assert!(s.input.is_empty());
        assert_eq!(s.key(key(KeyCode::Enter)), KeyAction::None, "empty input");
        // Ctrl+C: quit when idle and empty, cancel when busy.
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(s.key(ctrl_c), KeyAction::Quit);
        s.busy = true;
        assert_eq!(s.key(ctrl_c), KeyAction::Cancel);
        assert_eq!(s.key(key(KeyCode::Esc)), KeyAction::Cancel);
        // A new task while busy is refused, slash commands are allowed.
        s.input = "autre".into();
        assert_eq!(s.key(key(KeyCode::Enter)), KeyAction::None);
        s.input = "/aide".into();
        assert_eq!(
            s.key(key(KeyCode::Enter)),
            KeyAction::Submit("/aide".into())
        );
        let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert_eq!(s.key(ctrl_d), KeyAction::Quit);
        let release = KeyEvent {
            kind: KeyEventKind::Release,
            ..key(KeyCode::Char('z'))
        };
        assert_eq!(s.key(release), KeyAction::None);
    }

    #[test]
    fn streamed_answer_and_messages() {
        let mut s = state();
        s.apply(AgentEvent::AssistantDelta("Voici ".into()));
        s.apply(AgentEvent::AssistantDelta("la réponse.\nFin".into()));
        s.apply(AgentEvent::AssistantDone("Voici la réponse.\nFin".into()));
        s.apply(AgentEvent::Warning("attention".into()));
        let l = lines(&s);
        assert_eq!(l[0], "Voici la réponse.");
        assert_eq!(l[1], "Fin");
        assert_eq!(l[2], "⚠ attention");
        assert_eq!(l.len(), 3, "answer not duplicated: {l:?}");
    }

    #[test]
    fn garde_fou_and_elevation_requests_are_explained() {
        let mut s = state();
        s.ask(ApprovalRequest::Single {
            step: step("rm -rf /"),
            garde_fou: Some("*rm -rf /*".into()),
        });
        assert!(
            lines(&s)
                .iter()
                .any(|l| l.contains("Garde-fou « *rm -rf /* »"))
        );
        s.key(key(KeyCode::Char('n')));
        s.ask(ApprovalRequest::Elevation {
            reason: "raison".into(),
        });
        assert!(lines(&s).iter().any(|l| l == "raison"));
        let sc = screen(&s, 60, VIEWPORT_HEIGHT);
        assert!(sc.contains("administrateur"), "{sc}");
    }

    #[test]
    fn task_done_resets_and_elevating_quits() {
        let mut s = state();
        s.busy = true;
        s.progress = Some((1, 2));
        s.task_done(TaskOutcome::Done);
        assert!(!s.busy && s.progress.is_none() && !s.quit);
        s.task_done(TaskOutcome::Elevating);
        assert!(s.quit);
    }

    #[test]
    fn wrap_line_counts_rows_exactly() {
        let line = Line::from(vec![Span::raw("abcdef"), Span::raw("ghij")]);
        let rows = wrap_line(&line, 4);
        let texts: Vec<String> = rows
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.to_string()).collect())
            .collect();
        assert_eq!(texts, vec!["abcd", "efgh", "ij"]);
        assert_eq!(wrap_line(&Line::from("court"), 80).len(), 1);
        assert_eq!(wrap_line(&Line::from(""), 10).len(), 1);
        // Wide characters count double.
        assert_eq!(wrap_line(&Line::from("日本語"), 4).len(), 2);
    }

    #[test]
    fn gauge_shape() {
        assert_eq!(gauge(0, 4, 4), "░░░░");
        assert_eq!(gauge(2, 4, 4), "██░░");
        assert_eq!(gauge(9, 4, 4), "████");
        assert_eq!(gauge(1, 0, 4), "░░░░");
    }

    #[test]
    fn slash_commands() {
        let cfg = crate::config::Config::from_json(
            r#"{"version":1,"provider":{"base_url":"http://127.0.0.1:1/v1","model":"modele-a","roles":{"fast":"modele-b"}}}"#,
            None,
        )
        .unwrap();
        let (tx, rx) = mpsc::channel();
        let mut s = state();
        submit(&mut s, "/bypass", &tx, &cfg);
        assert!(matches!(
            rx.try_recv(),
            Ok(AgentCmd::SetMode(PermissionMode::Bypass))
        ));
        submit(&mut s, "/model fast", &tx, &cfg);
        assert!(matches!(rx.try_recv(), Ok(AgentCmd::SetModel(m)) if m == "modele-b"));
        submit(&mut s, "/model", &tx, &cfg);
        assert!(matches!(rx.try_recv(), Ok(AgentCmd::SetModel(m)) if m == "modele-a"));
        submit(&mut s, "/model inconnu", &tx, &cfg);
        assert!(rx.try_recv().is_err());
        submit(&mut s, "/effacer", &tx, &cfg);
        assert!(matches!(rx.try_recv(), Ok(AgentCmd::Clear)));
        submit(&mut s, "/nimporte", &tx, &cfg);
        assert!(lines(&s).iter().any(|l| l.contains("Commande inconnue")));
        submit(&mut s, "liste les fichiers", &tx, &cfg);
        assert!(matches!(rx.try_recv(), Ok(AgentCmd::Run(p)) if p == "liste les fichiers"));
        assert!(s.busy);
        submit(&mut s, "/quitter", &tx, &cfg);
        assert!(s.quit);
    }
}
