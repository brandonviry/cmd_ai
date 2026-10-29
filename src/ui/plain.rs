//! "TUI fermé": the agent's trace as plain text (one-shot runs, redirected output).
//!
//! Every command is shown before it runs (shell, privilege, purpose, exact text), its output
//! is streamed with a `│` gutter, then its exit code and duration (constitution IV).
//! Approvals are read from stdin when it is a terminal; otherwise they are `Unavailable`
//! (plan mode then stops with exit code 3).

use std::io::{BufRead, Write};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::theme::{Role, Theme};
use super::{AgentEvent, Approval, ApprovalRequest, Frontend, PlanStep};
use crate::exec::Stream;
use crate::text;

const GUTTER: &str = "  │ ";

pub struct PlainFrontend {
    theme: Theme,
    interactive: bool,
    cancel: Arc<AtomicBool>,
    out: Box<dyn Write>,
    err: Box<dyn Write>,
    input: Box<dyn BufRead>,
    /// Inside a command output block, at the start of a line.
    line_start: bool,
    in_output: bool,
    /// Streaming answer text currently open (needs a newline before anything else).
    answer_open: bool,
    answer_streamed: bool,
    reasoning_open: bool,
    status_shown: bool,
}

impl PlainFrontend {
    pub fn new(
        theme: Theme,
        interactive: bool,
        cancel: Arc<AtomicBool>,
        out: Box<dyn Write>,
        err: Box<dyn Write>,
        input: Box<dyn BufRead>,
    ) -> Self {
        PlainFrontend {
            theme,
            interactive,
            cancel,
            out,
            err,
            input,
            line_start: true,
            in_output: false,
            answer_open: false,
            answer_streamed: false,
            reasoning_open: false,
            status_shown: false,
        }
    }

    /// stdout / stderr / stdin of the process.
    pub fn stdio(theme: Theme, interactive: bool, cancel: Arc<AtomicBool>) -> Self {
        PlainFrontend::new(
            theme,
            interactive,
            cancel,
            Box::new(std::io::stdout()),
            Box::new(std::io::stderr()),
            Box::new(std::io::BufReader::new(std::io::stdin())),
        )
    }

    fn close_open_text(&mut self) {
        if self.answer_open || self.reasoning_open {
            let _ = writeln!(self.out);
            self.answer_open = false;
            self.reasoning_open = false;
        }
        if self.in_output && !self.line_start {
            let _ = writeln!(self.out);
            self.line_start = true;
        }
    }

    fn step_line(&self, step: &PlanStep) -> String {
        let privilege = text::privilege_label(step.admin);
        let role = if step.admin { Role::Admin } else { Role::Dim };
        let mut s = format!("{} · {}", step.shell, self.theme.paint(role, privilege));
        if !step.purpose.is_empty() {
            s.push_str(" — ");
            s.push_str(&step.purpose);
        }
        s
    }

    fn command_text(&self, command: &str) -> String {
        command
            .lines()
            .enumerate()
            .map(|(i, l)| {
                let prefix = if i == 0 { "$ " } else { "  " };
                self.theme.paint(Role::Command, &format!("{prefix}{l}"))
            })
            .collect::<Vec<_>>()
            .join("\n     ")
    }

    fn print_plan(&mut self, steps: &[PlanStep]) {
        let _ = writeln!(
            self.out,
            "{}",
            self.theme.paint(Role::Plan, &text::plan_title(steps.len()))
        );
        for (i, step) in steps.iter().enumerate() {
            let head = self.step_line(step);
            let cmd = self.command_text(&step.command);
            let _ = writeln!(self.out, "  {}. {head}", i + 1);
            let _ = writeln!(self.out, "     {cmd}");
        }
        let _ = self.out.flush();
    }

    fn write_output(&mut self, stream: Stream, chunk: &str) {
        let role = match stream {
            Stream::Stdout => Role::Text,
            Stream::Stderr => Role::Warning,
        };
        let mut buf = String::new();
        for piece in chunk.split_inclusive('\n') {
            if self.line_start {
                buf.push_str(&self.theme.paint(Role::Dim, GUTTER));
            }
            let (body, newline) = match piece.strip_suffix('\n') {
                Some(b) => (b.strip_suffix('\r').unwrap_or(b), true),
                None => (piece, false),
            };
            buf.push_str(&self.theme.paint(role, body));
            if newline {
                buf.push('\n');
            }
            self.line_start = newline;
        }
        let _ = self.out.write_all(buf.as_bytes());
        let _ = self.out.flush();
    }

    /// Text-mode loop (terminal without inline TUI support): prints the prompt marker and
    /// reads one request. `None` at end of input. Shares the input buffer with approvals.
    pub fn read_request(&mut self) -> Option<String> {
        self.close_open_text();
        let _ = write!(
            self.err,
            "{}",
            self.theme.paint(Role::Accent, text::REPL_PROMPT)
        );
        let _ = self.err.flush();
        let mut line = String::new();
        match self.input.read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim().to_string()),
        }
    }

    fn ask(&mut self, question: &str) -> Approval {
        if !self.interactive {
            return Approval::Unavailable;
        }
        let _ = write!(self.err, "{}", self.theme.paint(Role::Warning, question));
        let _ = self.err.flush();
        let mut line = String::new();
        match self.input.read_line(&mut line) {
            Ok(0) | Err(_) => {
                let _ = writeln!(self.err);
                Approval::No
            }
            Ok(_) if text::is_yes(&line) => Approval::Yes,
            Ok(_) => Approval::No,
        }
    }
}

impl Frontend for PlainFrontend {
    fn event(&mut self, ev: AgentEvent) {
        match ev {
            AgentEvent::AssistantDelta(t) => {
                if self.reasoning_open {
                    let _ = writeln!(self.out);
                    self.reasoning_open = false;
                }
                if !self.answer_open {
                    self.close_open_text();
                    self.in_output = false;
                    self.answer_open = true;
                }
                self.answer_streamed = true;
                let _ = write!(self.out, "{t}");
                let _ = self.out.flush();
                return;
            }
            AgentEvent::Reasoning(t) => {
                if !self.reasoning_open {
                    self.close_open_text();
                    self.reasoning_open = true;
                }
                let _ = write!(self.out, "{}", self.theme.paint(Role::Reasoning, &t));
                let _ = self.out.flush();
                return;
            }
            AgentEvent::CommandOutput { stream, text } => {
                self.write_output(stream, &text);
                return;
            }
            _ => {}
        }
        self.close_open_text();
        match ev {
            AgentEvent::Status(s) => {
                if !self.status_shown {
                    self.status_shown = true;
                    let line = text::status_line(
                        text::mode_label(s.mode),
                        text::privilege_label(s.admin),
                        &s.model,
                        &s.shell,
                        s.ephemeral,
                    );
                    let _ = writeln!(self.out, "{}", self.theme.paint(Role::Dim, &line));
                }
            }
            AgentEvent::AssistantDone(t) => {
                if !self.answer_streamed && !t.is_empty() {
                    let _ = writeln!(self.out, "{t}");
                }
                self.answer_streamed = false;
            }
            AgentEvent::Plan(steps) => self.print_plan(&steps),
            AgentEvent::Progress { .. } | AgentEvent::TaskEnd(_) => {}
            AgentEvent::CommandStart {
                index,
                total,
                step,
                hook,
            } => {
                let tag = if hook {
                    format!("[{}]", text::HOOK_TAG)
                } else {
                    format!("[{index}/{total}]")
                };
                let head = self.step_line(&step);
                let _ = writeln!(
                    self.out,
                    "{} {} {head}",
                    self.theme.paint(Role::Accent, "▶"),
                    self.theme.paint(Role::Accent, &tag)
                );
                let cmd = self.command_text(&step.command);
                let _ = writeln!(self.out, "  {cmd}");
                self.in_output = true;
                self.line_start = true;
            }
            AgentEvent::CommandEnd {
                code,
                duration,
                timed_out,
                cancelled,
                truncated,
            } => {
                let ok = code == Some(0);
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
                let (mark, role) = if ok {
                    ("✓", Role::Success)
                } else {
                    ("✗", Role::Error)
                };
                let _ = writeln!(
                    self.out,
                    "  {}",
                    self.theme.paint(role, &format!("{mark} {line}"))
                );
                self.in_output = false;
            }
            AgentEvent::Info(t) => {
                let _ = writeln!(
                    self.out,
                    "{}",
                    self.theme.paint(Role::Dim, &format!("ℹ {t}"))
                );
            }
            AgentEvent::Warning(t) => {
                let _ = writeln!(
                    self.err,
                    "{}",
                    self.theme.paint(Role::Warning, &format!("⚠ {t}"))
                );
            }
            AgentEvent::Error(t) => {
                let _ = writeln!(
                    self.err,
                    "{}",
                    self.theme.paint(Role::Error, &format!("✗ {t}"))
                );
            }
            AgentEvent::AssistantDelta(_)
            | AgentEvent::Reasoning(_)
            | AgentEvent::CommandOutput { .. } => {}
        }
        let _ = self.out.flush();
        let _ = self.err.flush();
    }

    fn approve(&mut self, req: ApprovalRequest) -> Approval {
        self.close_open_text();
        match req {
            ApprovalRequest::Plan(_) => self.ask(text::ASK_PLAN),
            ApprovalRequest::Single { step, garde_fou } => {
                if self.interactive {
                    let head = self.step_line(&step);
                    let cmd = self.command_text(&step.command);
                    let _ = writeln!(self.out, "  {head}\n  {cmd}");
                    let _ = self.out.flush();
                }
                match garde_fou {
                    Some(p) => self.ask(&text::ask_garde_fou(&p)),
                    None => self.ask(text::ASK_SINGLE),
                }
            }
            ApprovalRequest::Elevation { reason } => {
                if self.interactive {
                    let _ = writeln!(self.err, "{}", self.theme.paint(Role::Admin, &reason));
                }
                self.ask(text::ASK_ELEVATION)
            }
        }
    }

    fn handover(&mut self, run: &mut dyn FnMut() -> Option<i32>) -> Option<i32> {
        self.close_open_text();
        let _ = self.out.flush();
        run()
    }

    fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PermissionMode;
    use crate::ui::StatusLine;
    use crate::ui::theme::{Background, ColorLevel};
    use std::sync::Mutex;
    use std::time::Duration;

    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Shared {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    fn fe(interactive: bool, input: &str) -> (PlainFrontend, Shared, Shared) {
        let out = Shared::default();
        let err = Shared::default();
        let f = PlainFrontend::new(
            Theme::new(ColorLevel::None, Background::Dark),
            interactive,
            Arc::new(AtomicBool::new(false)),
            Box::new(out.clone()),
            Box::new(err.clone()),
            Box::new(std::io::Cursor::new(input.as_bytes().to_vec())),
        );
        (f, out, err)
    }

    fn step(cmd: &str) -> PlanStep {
        PlanStep {
            command: cmd.into(),
            purpose: "Lister les fichiers".into(),
            shell: "pwsh".into(),
            admin: false,
        }
    }

    #[test]
    fn command_block_is_complete() {
        let (mut f, out, _) = fe(false, "");
        f.event(AgentEvent::CommandStart {
            index: 1,
            total: 2,
            step: step("Get-ChildItem"),
            hook: false,
        });
        f.event(AgentEvent::CommandOutput {
            stream: Stream::Stdout,
            text: "a.txt\nb".into(),
        });
        f.event(AgentEvent::CommandOutput {
            stream: Stream::Stdout,
            text: ".txt\n".into(),
        });
        f.event(AgentEvent::CommandEnd {
            code: Some(0),
            duration: Duration::from_millis(400),
            timed_out: false,
            cancelled: false,
            truncated: false,
        });
        let t = out.text();
        assert!(
            t.contains("▶ [1/2] pwsh · utilisateur — Lister les fichiers"),
            "{t}"
        );
        assert!(t.contains("$ Get-ChildItem"));
        assert!(t.contains("  │ a.txt\n  │ b.txt\n"), "{t}");
        assert!(t.contains("✓ code 0 · 0,4 s"));
    }

    #[test]
    fn failure_flags_and_hooks() {
        let (mut f, out, _) = fe(false, "");
        f.event(AgentEvent::CommandStart {
            index: 0,
            total: 0,
            step: step("check"),
            hook: true,
        });
        f.event(AgentEvent::CommandOutput {
            stream: Stream::Stderr,
            text: "boom".into(),
        });
        f.event(AgentEvent::CommandEnd {
            code: None,
            duration: Duration::from_secs(120),
            timed_out: true,
            cancelled: false,
            truncated: true,
        });
        let t = out.text();
        assert!(t.contains("[hook]"));
        assert!(t.contains("  │ boom\n"), "unterminated line is closed: {t}");
        assert!(
            t.contains("✗ sans code · 120,0 s · délai dépassé · sortie tronquée"),
            "{t}"
        );
    }

    #[test]
    fn plan_and_approval() {
        let (mut f, out, err) = fe(true, "o\n");
        let steps = vec![
            step("ls"),
            PlanStep {
                admin: true,
                ..step("apt install x")
            },
        ];
        f.event(AgentEvent::Plan(steps.clone()));
        assert_eq!(f.approve(ApprovalRequest::Plan(steps)), Approval::Yes);
        let t = out.text();
        assert!(t.contains("Plan (2 commandes) :"));
        assert!(t.contains("  2. pwsh · admin"));
        assert!(err.text().contains("Exécuter ce plan ?"));

        let (mut f, _, _) = fe(true, "n\n");
        assert_eq!(f.approve(ApprovalRequest::Plan(vec![])), Approval::No);
        let (mut f, _, _) = fe(true, "");
        assert_eq!(
            f.approve(ApprovalRequest::Plan(vec![])),
            Approval::No,
            "EOF means no"
        );
        let (mut f, _, _) = fe(false, "o\n");
        assert_eq!(
            f.approve(ApprovalRequest::Plan(vec![])),
            Approval::Unavailable
        );
    }

    #[test]
    fn garde_fou_question_names_the_pattern() {
        let (mut f, _, err) = fe(true, "oui\n");
        let a = f.approve(ApprovalRequest::Single {
            step: step("rm -rf /"),
            garde_fou: Some("*rm -rf /*".into()),
        });
        assert_eq!(a, Approval::Yes);
        assert!(err.text().contains("Garde-fou « *rm -rf /* »"));
    }

    #[test]
    fn streamed_answer_is_not_printed_twice() {
        let (mut f, out, _) = fe(false, "");
        f.event(AgentEvent::AssistantDelta("Bon".into()));
        f.event(AgentEvent::AssistantDelta("jour.".into()));
        f.event(AgentEvent::AssistantDone("Bonjour.".into()));
        assert_eq!(out.text().matches("Bonjour.").count(), 1);
        assert!(out.text().ends_with("Bonjour.\n"));

        let (mut f, out, _) = fe(false, "");
        f.event(AgentEvent::AssistantDone("Réponse non streamée".into()));
        assert!(out.text().contains("Réponse non streamée\n"));
    }

    #[test]
    fn status_is_shown_once_and_messages_go_to_the_right_stream() {
        let (mut f, out, err) = fe(false, "");
        let s = StatusLine {
            mode: PermissionMode::Plan,
            admin: false,
            model: "modele-test".into(),
            ephemeral: true,
            shell: "bash".into(),
        };
        f.event(AgentEvent::Status(s.clone()));
        f.event(AgentEvent::Status(s));
        f.event(AgentEvent::Info("info".into()));
        f.event(AgentEvent::Warning("attention".into()));
        f.event(AgentEvent::Error("erreur".into()));
        let t = out.text();
        assert_eq!(t.matches("cmd_ai · PLAN").count(), 1);
        assert!(t.contains("éphémère"));
        assert!(t.contains("ℹ info"));
        assert!(err.text().contains("⚠ attention"));
        assert!(err.text().contains("✗ erreur"));
    }

    #[test]
    fn handover_runs_the_closure() {
        let (mut f, _, _) = fe(false, "");
        let mut called = false;
        let code = f.handover(&mut || {
            called = true;
            Some(0)
        });
        assert!(called);
        assert_eq!(code, Some(0));
    }
}
