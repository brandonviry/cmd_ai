//! The agent loop (contracts/rust-api.md, data-model.md "Cycle d'une tâche").
//!
//! Generic over the provider ([`ChatBackend`]), the spawn gate ([`Executor`]), privilege
//! elevation ([`Elevator`]) and the display ([`Frontend`]), so every rule of the
//! constitution (closed plan, garde-fous, step and fix caps, single tool, untrusted
//! outputs, redaction) is unit-tested without a network or a real shell.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::config::{HooksConfig, Limits, PermissionMode};
use crate::exec::{ExecMode, ExecResult, ExecSpec, Executor};
use crate::hooks::{self, FileSnapshot, HookRun};
use crate::home::Home;
use crate::llm::{ChatBackend, ChatRequest, ChatResponse, LlmError, Message, Role, StreamEvent, ToolCall};
use crate::log::SessionLog;
use crate::memory::{self, Memory};
use crate::output;
use crate::platform::{HostInfo, Privilege};
use crate::risk;
use crate::secret::Redactor;
use crate::session::ResumeState;
use crate::shell::{self, Shell, ShellInfo};
use crate::skills::{self, Skill};
use crate::text;
use crate::tool::{self, SelfCommand};
use crate::ui::{AgentEvent, Approval, ApprovalRequest, Frontend, PlanStep, StatusLine, TaskOutcome};

/// Consecutive invalid tool calls tolerated before the task stops (FR-012).
pub const MAX_INVALID_CALLS: u32 = 2;
/// Conversation size above which the oldest turns are dropped (bytes of text).
pub const HISTORY_BUDGET_BYTES: usize = 400 * 1024;
/// Default backoff between provider retries (FR-051).
pub const DEFAULT_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(8),
    Duration::from_secs(20),
];
/// Upper bound for a `Retry-After` wait.
const MAX_RETRY_AFTER: u64 = 60;

/// Everything the agent needs to know about the session.
#[derive(Debug, Clone)]
pub struct AgentSettings {
    pub mode: PermissionMode,
    pub model: String,
    pub max_tokens: u32,
    pub limits: Limits,
    pub garde_fous: Vec<String>,
    pub hooks: HooksConfig,
    pub cwd: PathBuf,
    pub host: HostInfo,
    /// Available shells; the first one is the default (FR-031). Never empty.
    pub shells: Vec<ShellInfo>,
    pub privilege: Privilege,
    pub show_reasoning: bool,
    pub ephemeral: bool,
    /// Provider URL, for error messages only.
    pub base_url: String,
    pub retry_delays: Vec<Duration>,
}

/// Privilege elevation, injectable for tests.
pub trait Elevator {
    /// Unix: `sudo -n` usable right now (cached credentials or NOPASSWD).
    fn sudo_ready(&mut self) -> bool;
    /// Unix: `sudo -v` with the terminal handed over (called inside [`Frontend::handover`]).
    fn sudo_validate(&mut self) -> Option<i32>;
    /// Windows: save the task in the cmd_ai folder and relaunch cmd_ai elevated.
    fn relaunch(&mut self, state: &ResumeState) -> Result<(), String>;
}

/// The real elevator.
pub struct SystemElevator {
    pub home: Home,
}

impl Elevator for SystemElevator {
    fn sudo_ready(&mut self) -> bool {
        crate::exec::probe_program("sudo", &["-n", "true"], Duration::from_secs(10)) == Some(0)
    }

    fn sudo_validate(&mut self) -> Option<i32> {
        crate::exec::run_inherited_program("sudo", &["-v"]).ok().flatten()
    }

    #[cfg(windows)]
    fn relaunch(&mut self, state: &ResumeState) -> Result<(), String> {
        let path = crate::session::save_resume(&self.home, state)?;
        let exe = std::env::current_exe().map_err(|e| text::elevation_failed(&e.to_string()))?;
        let args = vec![
            "--resume".to_string(),
            path.display().to_string(),
            "--home".to_string(),
            self.home.root.display().to_string(),
        ];
        let result = crate::platform::relaunch_elevated(&exe, &args);
        if result.is_err() {
            let _ = std::fs::remove_file(&path);
        }
        result
    }

    #[cfg(not(windows))]
    fn relaunch(&mut self, _state: &ResumeState) -> Result<(), String> {
        Err(text::TOOL_ELEVATION_REJECTED.to_string())
    }
}

/// Per-task state.
#[derive(Debug, Default)]
struct TaskState {
    /// Normalized commands of the approved plan (plan mode); `None` before approval.
    approved: Option<Vec<String>>,
    total: usize,
    done: usize,
    steps: u32,
    invalid_streak: u32,
    fail_streak: u32,
    last_failed: bool,
    bypass_plan_shown: bool,
}

enum Flow {
    /// Tool message for the model; the task goes on.
    Continue(String),
    /// Tool message for the model; the task stops with this outcome.
    Stop(TaskOutcome, String),
}

pub struct Agent<'a, B: ChatBackend, E: Executor> {
    backend: &'a B,
    exec: &'a mut E,
    elevator: &'a mut dyn Elevator,
    s: AgentSettings,
    skills: Vec<Skill>,
    memory: &'a mut Memory,
    log: &'a mut SessionLog,
    redactor: Redactor,
    history: Vec<Message>,
    current_prompt: String,
}

/// Whitespace-normalized command, for plan comparison.
fn normalize(command: &str) -> String {
    command.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl<'a, B: ChatBackend, E: Executor> Agent<'a, B, E> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        backend: &'a B,
        exec: &'a mut E,
        elevator: &'a mut dyn Elevator,
        settings: AgentSettings,
        skills: Vec<Skill>,
        memory: &'a mut Memory,
        log: &'a mut SessionLog,
        redactor: Redactor,
    ) -> Self {
        Agent {
            backend,
            exec,
            elevator,
            s: settings,
            skills,
            memory,
            log,
            redactor,
            history: Vec::new(),
            current_prompt: String::new(),
        }
    }

    pub fn mode(&self) -> PermissionMode {
        self.s.mode
    }

    pub fn model(&self) -> &str {
        &self.s.model
    }

    pub fn set_mode(&mut self, m: PermissionMode) {
        self.s.mode = m;
    }

    pub fn set_model(&mut self, m: String) {
        self.s.model = m;
    }

    /// Forgets the conversation (TUI `/effacer`).
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    pub fn history(&self) -> &[Message] {
        &self.history
    }

    pub fn status(&self) -> StatusLine {
        StatusLine {
            mode: self.s.mode,
            admin: self.s.privilege == Privilege::Admin,
            model: self.s.model.clone(),
            ephemeral: self.s.ephemeral,
            shell: self.default_shell().shell.name().to_string(),
        }
    }

    fn default_shell(&self) -> &ShellInfo {
        // `shells` is never empty (checked by lib.rs before building the agent).
        &self.s.shells[0]
    }

    pub fn system_prompt(&self) -> String {
        let default = self.default_shell();
        let others: Vec<&str> = self.s.shells[1..].iter().map(|s| s.shell.name()).collect();
        let others = if others.is_empty() {
            text::SHELLS_NONE.to_string()
        } else {
            others.join(", ")
        };
        let skills = if self.skills.is_empty() {
            text::SKILLS_CATALOG_EMPTY.to_string()
        } else {
            self.skills
                .iter()
                .map(|s| format!("- {} — {}", s.name, s.description))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let block = self.memory.prompt_block(memory::PROMPT_MAX_BYTES);
        let memory = if block.trim().is_empty() {
            text::MEMORY_EMPTY_PROMPT.to_string()
        } else {
            output::frame_untrusted("mémoire", &self.redactor.redact(&block))
        };
        let cwd = self.s.cwd.display().to_string();
        let shell_path = default.path.display().to_string();
        text::system_prompt(&text::PromptContext {
            os: self.s.host.os,
            os_version: &self.s.host.os_version,
            arch: self.s.host.arch,
            shell: default.shell.name(),
            shell_path: &shell_path,
            other_shells: &others,
            admin: self.s.privilege == Privilege::Admin,
            bypass: self.s.mode == PermissionMode::Bypass,
            cwd: &cwd,
            max_fixes: self.s.limits.max_fixes,
            skills: &skills,
            memory: &memory,
        })
    }

    /// Runs the `session_start` hooks (called once by the frontend wiring).
    pub fn session_start(&mut self, ui: &mut dyn Frontend) {
        let cancel = ui.cancel_flag();
        for h in hooks::hooks_simple(&self.s.hooks, "session_start") {
            self.run_hook(&h, ui, &cancel);
        }
    }

    /// Runs one user request to completion. The conversation is kept between tasks.
    pub fn run_task(&mut self, prompt: &str, ui: &mut dyn Frontend) -> TaskOutcome {
        let cancel = ui.cancel_flag();
        cancel.store(false, Ordering::SeqCst);
        self.current_prompt = prompt.to_string();
        self.log.line("TASK", prompt);
        ui.event(AgentEvent::Status(self.status()));
        self.history.push(Message::user(prompt));
        let mut st = TaskState::default();
        let outcome = self.task_loop(&mut st, ui, &cancel);
        if outcome == TaskOutcome::Cancelled {
            ui.event(AgentEvent::Warning(text::TASK_CANCELLED.to_string()));
        }
        if outcome != TaskOutcome::Elevating {
            // task_end hooks run even after a cancel; give them a fresh cancel flag.
            let fresh = AtomicBool::new(false);
            for h in hooks::hooks_simple(&self.s.hooks, "task_end") {
                self.run_hook(&h, ui, &fresh);
            }
        }
        self.log.line("END", &format!("{outcome:?}"));
        ui.event(AgentEvent::TaskEnd(outcome));
        outcome
    }

    fn task_loop(
        &mut self,
        st: &mut TaskState,
        ui: &mut dyn Frontend,
        cancel: &AtomicBool,
    ) -> TaskOutcome {
        loop {
            if cancel.load(Ordering::SeqCst) {
                return TaskOutcome::Cancelled;
            }
            let resp = match self.chat(ui, cancel) {
                Ok(r) => r,
                Err(outcome) => return outcome,
            };
            if resp.tool_calls.is_empty() {
                let answer = self.redactor.redact(resp.content.trim());
                self.history.push(Message::assistant(answer.clone(), Vec::new()));
                self.log.line("ANSWER", &answer);
                ui.event(AgentEvent::AssistantDone(answer));
                return if st.last_failed {
                    TaskOutcome::Failed
                } else {
                    TaskOutcome::Done
                };
            }
            let calls = resp.tool_calls.clone();
            let content = self.redactor.redact(&resp.content);
            self.history.push(Message::assistant(content, calls.clone()));
            let mut stop: Option<TaskOutcome> = None;
            for call in &calls {
                if stop.is_some() {
                    self.history
                        .push(Message::tool(&call.id, text::TOOL_CANCELLED_UNANSWERED));
                    continue;
                }
                match self.handle_call(call, st, ui, cancel) {
                    Flow::Continue(msg) => self.history.push(Message::tool(&call.id, msg)),
                    Flow::Stop(outcome, msg) => {
                        self.history.push(Message::tool(&call.id, msg));
                        stop = Some(outcome);
                    }
                }
            }
            if let Some(outcome) = stop {
                return outcome;
            }
        }
    }

    /// Drops the oldest whole turns (user message up to the next one) above the budget.
    fn trim_history(&mut self) {
        let size = |m: &Message| {
            m.content.as_ref().map_or(0, String::len)
                + m.tool_calls.iter().map(|c| c.arguments.len() + c.name.len()).sum::<usize>()
        };
        loop {
            let total: usize = self.history.iter().map(size).sum();
            if total <= HISTORY_BUDGET_BYTES {
                return;
            }
            let next_user = self
                .history
                .iter()
                .enumerate()
                .skip(1)
                .find(|(_, m)| m.role == Role::User)
                .map(|(i, _)| i);
            match next_user {
                Some(i) => {
                    self.history.drain(..i);
                }
                None => return,
            }
        }
    }

    fn chat(
        &mut self,
        ui: &mut dyn Frontend,
        cancel: &AtomicBool,
    ) -> Result<ChatResponse, TaskOutcome> {
        self.trim_history();
        let mut messages = vec![Message::system(self.system_prompt())];
        messages.extend(self.history.iter().cloned());
        let req = ChatRequest {
            model: self.s.model.clone(),
            messages,
            tools: tool::schema(),
            max_tokens: self.s.max_tokens,
        };
        let max_retries = self.s.retry_delays.len() as u32;
        let mut attempt: u32 = 0;
        let mut empty_retried = false;
        loop {
            let result = {
                let show = self.s.show_reasoning;
                let red = &self.redactor;
                let mut on_event = |ev: StreamEvent| match ev {
                    StreamEvent::Content(t) => ui.event(AgentEvent::AssistantDelta(red.redact(&t))),
                    StreamEvent::Reasoning(t) => {
                        if show {
                            ui.event(AgentEvent::Reasoning(red.redact(&t)));
                        }
                    }
                };
                self.backend.chat(&req, &mut on_event, cancel)
            };
            match result {
                Ok(r) => return Ok(r),
                Err(LlmError::Cancelled) => return Err(TaskOutcome::Cancelled),
                Err(LlmError::Empty) if !empty_retried => {
                    empty_retried = true;
                    self.log.line("LLM", "empty answer, retrying");
                    ui.event(AgentEvent::Warning(text::EMPTY_ANSWER_RETRY.to_string()));
                }
                Err(e) if attempt < max_retries && retryable(&e) => {
                    let delay = match &e {
                        LlmError::RateLimited {
                            retry_after: Some(s),
                            ..
                        } => Duration::from_secs((*s).min(MAX_RETRY_AFTER)),
                        _ => self.s.retry_delays[attempt as usize],
                    };
                    attempt += 1;
                    let full = text::llm_error(&e, &self.s.base_url);
                    let reason = full.split(". ").next().unwrap_or(&full).to_string();
                    self.log.line("LLM", &self.redactor.redact(&e.to_string()));
                    ui.event(AgentEvent::Warning(text::retrying(
                        &reason,
                        delay.as_secs(),
                        attempt,
                        max_retries,
                    )));
                    if !sleep_cancellable(delay, cancel) {
                        return Err(TaskOutcome::Cancelled);
                    }
                }
                Err(e) => {
                    self.log.line("LLM", &self.redactor.redact(&e.to_string()));
                    let msg = self.redactor.redact(&text::llm_error(&e, &self.s.base_url));
                    ui.event(AgentEvent::Error(msg));
                    return Err(TaskOutcome::ProviderError);
                }
            }
        }
    }

    fn resolve_shell(&self, name: Option<&str>) -> Result<ShellInfo, String> {
        let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) else {
            return Ok(self.default_shell().clone());
        };
        let available = || {
            self.s
                .shells
                .iter()
                .map(|s| s.shell.name())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let Some(wanted) = Shell::parse(name) else {
            return Err(text::tool_unknown_shell(name, &available()));
        };
        if let Some(found) = self.s.shells.iter().find(|s| s.shell == wanted) {
            return Ok(found.clone());
        }
        shell::find(wanted).ok_or_else(|| text::tool_unknown_shell(name, &available()))
    }

    fn redact(&self, s: &str) -> String {
        self.redactor.redact(s)
    }

    fn self_command(&mut self, c: SelfCommand) -> String {
        match c {
            SelfCommand::SkillList => text::skill_list(&self.skills),
            SelfCommand::SkillShow(name) => {
                if !skills::valid_skill_name(&name) {
                    return text::tool_self_error(&text::skill_invalid_name(&name));
                }
                match skills::find_skill(&self.skills, &name) {
                    Some(s) => text::tool_skill_body(&s.name, &s.body),
                    None => text::tool_self_error(&text::skill_not_found(&name, &self.skills)),
                }
            }
            SelfCommand::MemoryList => text::memory_list(self.memory.notes()),
            SelfCommand::MemoryAdd(note) => {
                let note = self.redactor.redact(&note);
                match self.memory.add(&note) {
                    Ok(n) => text::memory_added(n, self.memory.persistent()),
                    Err(m) => text::tool_self_error(&m),
                }
            }
            SelfCommand::MemoryForget(n) => match self.memory.forget(n) {
                Ok(note) => text::memory_forgotten(n, &note),
                Err(m) => text::tool_self_error(&m),
            },
        }
    }

    /// Plan list for the first call: the model's `plan`, with the actual command inserted
    /// first when it is missing from it.
    fn plan_list(&self, plan: &[String], original: &str, command: &str) -> Vec<String> {
        let mut list: Vec<String> = plan
            .iter()
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();
        let wanted = [normalize(command), normalize(original)];
        if !list.iter().any(|c| wanted.contains(&normalize(c))) {
            list.insert(0, command.to_string());
        }
        list
    }

    /// Normalized forms under which a plan item counts as approved (raw and without sudo).
    fn approved_forms(&self, item: &str) -> Vec<String> {
        let mut forms = vec![normalize(item)];
        let a = risk::assess(item, self.s.host.os, false, &[]);
        if let Some(stripped) = a.strip_sudo {
            forms.push(normalize(&stripped));
        }
        forms
    }

    fn handle_call(
        &mut self,
        call: &ToolCall,
        st: &mut TaskState,
        ui: &mut dyn Frontend,
        cancel: &AtomicBool,
    ) -> Flow {
        let req = match tool::parse_call(call) {
            Ok(r) => {
                st.invalid_streak = 0;
                r
            }
            Err(e) => {
                st.invalid_streak += 1;
                let detail = self.redact(&e.to_string());
                self.log.line("INVALID", &detail);
                if st.invalid_streak > MAX_INVALID_CALLS {
                    ui.event(AgentEvent::Error(text::invalid_calls_exhausted(st.invalid_streak)));
                    return Flow::Stop(TaskOutcome::Failed, text::tool_invalid_call(&detail));
                }
                return Flow::Continue(text::tool_invalid_call(&detail));
            }
        };

        // Self-commands: intercepted, never sent to a shell, not counted in n/M.
        if let Some(sc) = tool::parse_self_command(&req.command) {
            let shown = self.redact(&req.command);
            ui.event(AgentEvent::Info(text::self_command_info(&shown)));
            self.log.line("SELF", &shown);
            let msg = match sc {
                Ok(c) => self.self_command(c),
                Err(m) => text::tool_self_error(&m),
            };
            return Flow::Continue(self.redact(&msg));
        }

        st.steps += 1;
        if st.steps > self.s.limits.max_steps {
            let m = text::step_limit_reached(self.s.limits.max_steps);
            ui.event(AgentEvent::Error(m.clone()));
            return Flow::Stop(TaskOutcome::StepLimit, format!("REFUSÉ : {m}"));
        }

        let shell = match self.resolve_shell(req.shell.as_deref()) {
            Ok(s) => s,
            Err(m) => return Flow::Continue(m),
        };
        let assessment = risk::assess(
            &req.command,
            self.s.host.os,
            req.needs_admin,
            &self.s.garde_fous,
        );
        let command = assessment
            .strip_sudo
            .clone()
            .unwrap_or_else(|| req.command.trim().to_string());
        let admin = assessment.admin;
        let step = PlanStep {
            command: self.redact(&command),
            purpose: self.redact(&req.purpose),
            shell: shell.shell.name().to_string(),
            admin,
        };

        // Consent (constitution V).
        match self.s.mode {
            PermissionMode::Plan => {
                if st.approved.is_none() {
                    let list = self.plan_list(&req.plan, &req.command, &command);
                    let steps = self.plan_steps(&list, &step, &command, &shell);
                    self.log.line("PLAN", &self.redact(&list.join(" | ")));
                    ui.event(AgentEvent::Plan(steps.clone()));
                    match ui.approve(ApprovalRequest::Plan(steps)) {
                        Approval::Yes => {
                            st.total = list.len();
                            st.approved =
                                Some(list.iter().flat_map(|c| self.approved_forms(c)).collect());
                        }
                        Approval::No => {
                            ui.event(AgentEvent::Info(text::PLAN_REJECTED.to_string()));
                            return Flow::Stop(
                                TaskOutcome::PlanRejected,
                                text::TOOL_PLAN_REJECTED.to_string(),
                            );
                        }
                        Approval::Unavailable => {
                            ui.event(AgentEvent::Warning(
                                text::PLAN_NOT_APPROVED_NO_TTY.to_string(),
                            ));
                            return Flow::Stop(
                                TaskOutcome::PlanNotApproved,
                                text::TOOL_PLAN_REJECTED.to_string(),
                            );
                        }
                    }
                } else {
                    let forms = [normalize(&command), normalize(&req.command)];
                    let in_plan = st
                        .approved
                        .as_ref()
                        .is_some_and(|a| a.iter().any(|c| forms.contains(c)));
                    if !in_plan {
                        match ui.approve(ApprovalRequest::Single {
                            step: step.clone(),
                            garde_fou: None,
                        }) {
                            Approval::Yes => {
                                if let Some(a) = st.approved.as_mut() {
                                    a.push(normalize(&command));
                                }
                            }
                            Approval::No => {
                                return Flow::Continue(text::TOOL_SINGLE_REJECTED.to_string());
                            }
                            Approval::Unavailable => {
                                return Flow::Stop(
                                    TaskOutcome::PlanNotApproved,
                                    text::TOOL_SINGLE_REJECTED.to_string(),
                                );
                            }
                        }
                    }
                }
            }
            PermissionMode::Bypass => {
                if !st.bypass_plan_shown {
                    st.bypass_plan_shown = true;
                    let list = self.plan_list(&req.plan, &req.command, &command);
                    st.total = list.len();
                    if list.len() > 1 {
                        let steps = self.plan_steps(&list, &step, &command, &shell);
                        ui.event(AgentEvent::Plan(steps));
                    }
                }
                if let Some(pattern) = assessment.garde_fou.clone() {
                    let answer = ui.approve(ApprovalRequest::Single {
                        step: step.clone(),
                        garde_fou: Some(pattern.clone()),
                    });
                    if answer != Approval::Yes {
                        self.log.line("GARDE-FOU", &format!("{pattern} :: refusé"));
                        return Flow::Continue(text::tool_garde_fou_rejected(&pattern));
                    }
                }
            }
        }
        st.total = st.total.max(st.done + 1);

        // Elevation: always through the OS consent (constitution V).
        let mut use_sudo = false;
        if admin && self.s.privilege == Privilege::User {
            if self.s.host.os == "windows" {
                let reason = text::elevation_reason(&step.command);
                match ui.approve(ApprovalRequest::Elevation { reason }) {
                    Approval::Yes => {
                        let state = ResumeState {
                            prompt: self.current_prompt.clone(),
                            mode: self.s.mode,
                            model: self.s.model.clone(),
                            cwd: self.s.cwd.clone(),
                        };
                        match self.elevator.relaunch(&state) {
                            Ok(()) => {
                                ui.event(AgentEvent::Info(text::ELEVATION_STARTED.to_string()));
                                return Flow::Stop(
                                    TaskOutcome::Elevating,
                                    text::TOOL_CANCELLED_UNANSWERED.to_string(),
                                );
                            }
                            Err(m) => {
                                ui.event(AgentEvent::Error(text::elevation_failed(&m)));
                                return Flow::Continue(text::TOOL_ELEVATION_REJECTED.to_string());
                            }
                        }
                    }
                    _ => return Flow::Continue(text::TOOL_ELEVATION_REJECTED.to_string()),
                }
            } else {
                if !self.elevator.sudo_ready() {
                    ui.event(AgentEvent::Info(text::SUDO_HANDOVER.to_string()));
                    let elevator = &mut *self.elevator;
                    let code = ui.handover(&mut || elevator.sudo_validate());
                    if code != Some(0) {
                        ui.event(AgentEvent::Error(text::SUDO_FAILED.to_string()));
                        return Flow::Stop(
                            TaskOutcome::Failed,
                            text::TOOL_ELEVATION_REJECTED.to_string(),
                        );
                    }
                }
                use_sudo = true;
            }
        }

        // pre_command hooks: exit code 2 blocks the command.
        for h in hooks::hooks_for_command(&self.s.hooks, "pre_command", &command) {
            if self.run_hook(&h, ui, cancel) == Some(2) {
                let blocker = self.redact(&h.run);
                ui.event(AgentEvent::Warning(text::tool_hook_blocked(&blocker)));
                return Flow::Continue(text::tool_hook_blocked(&blocker));
            }
        }
        if cancel.load(Ordering::SeqCst) {
            return Flow::Stop(
                TaskOutcome::Cancelled,
                text::TOOL_CANCELLED_UNANSWERED.to_string(),
            );
        }

        let snapshot = FileSnapshot::take(&self.s.hooks, &self.s.cwd);
        st.done += 1;
        let mode = if req.interactive {
            ExecMode::Inherited
        } else if assessment.gui_launcher {
            ExecMode::Detached
        } else {
            ExecMode::Captured
        };
        let spec = ExecSpec {
            shell: shell.clone(),
            command: command.clone(),
            cwd: self.s.cwd.clone(),
            timeout: Duration::from_secs(self.s.limits.command_timeout_secs),
            mode,
            extra_env: Vec::new(),
            sudo: use_sudo,
            capture_limit: self.capture_limit(),
        };
        ui.event(AgentEvent::CommandStart {
            index: st.done,
            total: st.total,
            step: step.clone(),
            hook: false,
        });
        self.log.line(
            "CMD",
            &format!(
                "[{}/{}] {} · {} :: {}",
                st.done,
                st.total,
                step.shell,
                text::privilege_label(admin),
                step.command
            ),
        );

        let result = self.execute(&spec, ui, cancel);
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                let detail = self.redact(&e.to_string());
                ui.event(AgentEvent::Error(text::tool_spawn_failed(&detail)));
                st.last_failed = true;
                return Flow::Continue(text::tool_spawn_failed(&detail));
            }
        };
        self.end_command(&result, ui, st);

        for h in hooks::hooks_for_command(&self.s.hooks, "post_command", &command) {
            self.run_hook(&h, ui, cancel);
        }
        for h in snapshot.changed(&self.s.hooks, &self.s.cwd) {
            self.run_hook(&h, ui, cancel);
        }

        let failed = match mode {
            ExecMode::Detached => result.code.is_some_and(|c| c != 0),
            _ => result.code != Some(0),
        };
        let denied = failed
            && !admin
            && risk::looks_like_access_denied(
                result.code,
                &result.stdout.text,
                &result.stderr.text,
                self.s.host.os,
            );
        if denied {
            ui.event(AgentEvent::Warning(text::ACCESS_DENIED_WARNING.to_string()));
        }
        let msg = self.tool_result(&result, mode, denied);
        if result.cancelled {
            return Flow::Stop(TaskOutcome::Cancelled, msg);
        }
        if failed {
            st.last_failed = true;
            st.fail_streak += 1;
            if st.fail_streak > self.s.limits.max_fixes {
                ui.event(AgentEvent::Error(text::fixes_exhausted(self.s.limits.max_fixes)));
                return Flow::Stop(TaskOutcome::Failed, msg);
            }
        } else {
            st.last_failed = false;
            st.fail_streak = 0;
        }
        Flow::Continue(msg)
    }

    fn plan_steps(
        &self,
        list: &[String],
        step: &PlanStep,
        command: &str,
        shell: &ShellInfo,
    ) -> Vec<PlanStep> {
        let target = normalize(command);
        list.iter()
            .map(|item| {
                let a = risk::assess(item, self.s.host.os, false, &self.s.garde_fous);
                let cmd = a.strip_sudo.clone().unwrap_or_else(|| item.clone());
                if normalize(&cmd) == target || normalize(item) == target {
                    step.clone()
                } else {
                    PlanStep {
                        command: self.redact(&cmd),
                        purpose: String::new(),
                        shell: shell.shell.name().to_string(),
                        admin: a.admin,
                    }
                }
            })
            .collect()
    }

    fn capture_limit(&self) -> usize {
        (self.s.limits.capture_mib as usize).saturating_mul(1024 * 1024)
    }

    fn execute(
        &mut self,
        spec: &ExecSpec,
        ui: &mut dyn Frontend,
        cancel: &AtomicBool,
    ) -> std::io::Result<ExecResult> {
        if spec.mode == ExecMode::Inherited {
            let exec = &mut *self.exec;
            let mut out: Option<std::io::Result<ExecResult>> = None;
            ui.handover(&mut || {
                let r = exec.run(spec, &mut |_, _| {}, cancel);
                let code = r.as_ref().ok().and_then(|x| x.code);
                out = Some(r);
                code
            });
            out.unwrap_or_else(|| Err(std::io::Error::other("handover")))
        } else {
            let red = &self.redactor;
            self.exec.run(
                spec,
                &mut |stream, chunk| {
                    ui.event(AgentEvent::CommandOutput {
                        stream,
                        text: red.redact(&output::sanitize_for_display(chunk)),
                    });
                },
                cancel,
            )
        }
    }

    fn end_command(&mut self, r: &ExecResult, ui: &mut dyn Frontend, st: &TaskState) {
        let truncated = r.stdout.truncated || r.stderr.truncated;
        ui.event(AgentEvent::CommandEnd {
            code: r.code,
            duration: r.duration,
            timed_out: r.timed_out,
            cancelled: r.cancelled,
            truncated,
        });
        ui.event(AgentEvent::Progress {
            done: st.done,
            total: st.total,
        });
        if !r.stdout.text.is_empty() {
            self.log.line("OUT", &output::strip_ansi(&r.stdout.text));
        }
        if !r.stderr.text.is_empty() {
            self.log.line("ERR", &output::strip_ansi(&r.stderr.text));
        }
        self.log.line(
            "EXIT",
            &text::tool_result_header(
                r.code,
                r.duration.as_secs_f64(),
                r.timed_out,
                r.cancelled,
                truncated,
            ),
        );
    }

    /// The tool message: header, then each non-empty stream reduced, masked and framed as
    /// untrusted data (constitution VI).
    fn tool_result(&self, r: &ExecResult, mode: ExecMode, denied: bool) -> String {
        let budget = (self.s.limits.model_output_kib as usize).saturating_mul(1024);
        let out = self.redact(&r.stdout.text);
        let err = self.redact(&r.stderr.text);
        let (out_budget, err_budget) = match (out.trim().is_empty(), err.trim().is_empty()) {
            (_, true) => (budget, 0),
            (true, false) => (0, budget),
            (false, false) => (budget * 3 / 4, budget / 4),
        };
        let (out, t1) = output::reduce_for_model(&out, out_budget);
        let (err, t2) = output::reduce_for_model(&err, err_budget);
        let truncated = r.stdout.truncated || r.stderr.truncated || t1 || t2;
        let mut msg = text::tool_result_header(
            r.code,
            r.duration.as_secs_f64(),
            r.timed_out,
            r.cancelled,
            truncated,
        );
        match mode {
            ExecMode::Detached => {
                msg.push('\n');
                msg.push_str(text::TOOL_GUI_DETACHED);
            }
            ExecMode::Inherited => {
                msg.push('\n');
                msg.push_str(text::TOOL_INTERACTIVE_DONE);
            }
            ExecMode::Captured => {}
        }
        if !out.trim().is_empty() {
            msg.push('\n');
            msg.push_str(&output::frame_untrusted("stdout", &out));
        }
        if !err.trim().is_empty() {
            msg.push('\n');
            msg.push_str(&output::frame_untrusted("stderr", &err));
        }
        if denied {
            msg.push('\n');
            msg.push_str(text::TOOL_ACCESS_DENIED_NOTE);
        }
        msg
    }

    /// Runs one hook through the spawn gate, visibly. Returns its exit code.
    fn run_hook(&mut self, h: &HookRun, ui: &mut dyn Frontend, cancel: &AtomicBool) -> Option<i32> {
        let shell = match self.resolve_shell(h.shell.as_deref()) {
            Ok(s) => s,
            Err(m) => {
                ui.event(AgentEvent::Warning(m));
                return None;
            }
        };
        let step = PlanStep {
            command: self.redact(&h.run),
            purpose: text::hook_label(h.event),
            shell: shell.shell.name().to_string(),
            admin: false,
        };
        ui.event(AgentEvent::CommandStart {
            index: 0,
            total: 0,
            step: step.clone(),
            hook: true,
        });
        self.log.line("HOOK", &format!("{} :: {}", h.event, step.command));
        let spec = ExecSpec {
            shell,
            command: h.run.clone(),
            cwd: self.s.cwd.clone(),
            timeout: h.timeout,
            mode: ExecMode::Captured,
            extra_env: h.env.clone(),
            sudo: false,
            capture_limit: self.capture_limit(),
        };
        let red = &self.redactor;
        let result = self.exec.run(
            &spec,
            &mut |stream, chunk| {
                ui.event(AgentEvent::CommandOutput {
                    stream,
                    text: red.redact(&output::sanitize_for_display(chunk)),
                });
            },
            cancel,
        );
        match result {
            Ok(r) => {
                ui.event(AgentEvent::CommandEnd {
                    code: r.code,
                    duration: r.duration,
                    timed_out: r.timed_out,
                    cancelled: r.cancelled,
                    truncated: r.stdout.truncated || r.stderr.truncated,
                });
                self.log.line(
                    "HOOK-EXIT",
                    &text::tool_result_header(r.code, r.duration.as_secs_f64(), r.timed_out, r.cancelled, false),
                );
                r.code
            }
            Err(e) => {
                let detail = self.redact(&e.to_string());
                ui.event(AgentEvent::Warning(text::tool_spawn_failed(&detail)));
                None
            }
        }
    }
}

fn retryable(e: &LlmError) -> bool {
    match e {
        LlmError::RateLimited { .. } | LlmError::Network(_) => true,
        LlmError::Server(code, _) => *code >= 500,
        _ => false,
    }
}

/// Sleeps `d`, waking every 50 ms to check `cancel`. Returns false when cancelled.
fn sleep_cancellable(d: Duration, cancel: &AtomicBool) -> bool {
    let start = Instant::now();
    while start.elapsed() < d {
        if cancel.load(Ordering::SeqCst) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50).min(d.saturating_sub(start.elapsed())));
    }
    !cancel.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Hook, HooksConfig};
    use crate::exec::{Captured, Stream};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::Arc;

    // ---------- fakes ----------

    struct FakeBackend {
        replies: RefCell<VecDeque<Result<ChatResponse, LlmError>>>,
        requests: RefCell<Vec<ChatRequest>>,
    }

    impl FakeBackend {
        fn new(replies: Vec<Result<ChatResponse, LlmError>>) -> Self {
            FakeBackend {
                replies: RefCell::new(replies.into()),
                requests: RefCell::new(Vec::new()),
            }
        }
    }

    impl ChatBackend for FakeBackend {
        fn chat(
            &self,
            req: &ChatRequest,
            on_event: &mut dyn FnMut(StreamEvent),
            _cancel: &AtomicBool,
        ) -> Result<ChatResponse, LlmError> {
            self.requests.borrow_mut().push(req.clone());
            let reply = self
                .replies
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| Ok(answer("Terminé.")));
            if let Ok(r) = &reply {
                if !r.content.is_empty() {
                    on_event(StreamEvent::Content(r.content.clone()));
                }
            }
            reply
        }
    }

    fn answer(text: &str) -> ChatResponse {
        ChatResponse {
            content: text.to_string(),
            reasoning: String::new(),
            tool_calls: Vec::new(),
            usage: None,
            finish_reason: Some("stop".into()),
        }
    }

    fn call_args(id: &str, args: serde_json::Value) -> ChatResponse {
        ChatResponse {
            content: String::new(),
            reasoning: String::new(),
            tool_calls: vec![ToolCall {
                id: id.into(),
                name: tool::TOOL_NAME.into(),
                arguments: args.to_string(),
            }],
            usage: None,
            finish_reason: Some("tool_calls".into()),
        }
    }

    fn cmd(id: &str, command: &str) -> ChatResponse {
        call_args(id, serde_json::json!({"command": command, "purpose": "test"}))
    }

    #[derive(Default)]
    struct FakeExecutor {
        specs: Vec<ExecSpec>,
        /// (code, stdout, stderr) per call; default (0, "ok", "").
        results: VecDeque<(Option<i32>, String, String)>,
        cancel_on_run: bool,
    }

    impl Executor for FakeExecutor {
        fn run(
            &mut self,
            spec: &ExecSpec,
            on_chunk: &mut dyn FnMut(Stream, &str),
            _cancel: &AtomicBool,
        ) -> std::io::Result<ExecResult> {
            self.specs.push(spec.clone());
            let (code, out, err) = self
                .results
                .pop_front()
                .unwrap_or((Some(0), "ok\n".into(), String::new()));
            on_chunk(Stream::Stdout, &out);
            if !err.is_empty() {
                on_chunk(Stream::Stderr, &err);
            }
            let cap = |t: &str| Captured {
                text: t.to_string(),
                total_bytes: t.len() as u64,
                truncated: false,
            };
            Ok(ExecResult {
                code: if self.cancel_on_run { None } else { code },
                stdout: cap(&out),
                stderr: cap(&err),
                duration: Duration::from_millis(5),
                timed_out: false,
                cancelled: self.cancel_on_run,
            })
        }
    }

    #[derive(Default)]
    struct FakeElevator {
        ready: bool,
        validate_code: Option<i32>,
        validated: u32,
        relaunched: Vec<ResumeState>,
        relaunch_ok: bool,
    }

    impl Elevator for FakeElevator {
        fn sudo_ready(&mut self) -> bool {
            self.ready
        }
        fn sudo_validate(&mut self) -> Option<i32> {
            self.validated += 1;
            self.validate_code
        }
        fn relaunch(&mut self, state: &ResumeState) -> Result<(), String> {
            self.relaunched.push(state.clone());
            if self.relaunch_ok { Ok(()) } else { Err("refusé".into()) }
        }
    }

    struct FakeUi {
        events: Vec<AgentEvent>,
        approvals: VecDeque<Approval>,
        asked: Vec<ApprovalRequest>,
        handovers: u32,
        cancel: Arc<AtomicBool>,
    }

    impl FakeUi {
        fn new(approvals: Vec<Approval>) -> Self {
            FakeUi {
                events: Vec::new(),
                approvals: approvals.into(),
                asked: Vec::new(),
                handovers: 0,
                cancel: Arc::new(AtomicBool::new(false)),
            }
        }
        fn texts(&self) -> String {
            self.events.iter().map(|e| format!("{e:?}")).collect::<Vec<_>>().join("\n")
        }
    }

    impl Frontend for FakeUi {
        fn event(&mut self, ev: AgentEvent) {
            self.events.push(ev);
        }
        fn approve(&mut self, req: ApprovalRequest) -> Approval {
            self.asked.push(req);
            self.approvals.pop_front().unwrap_or(Approval::Unavailable)
        }
        fn handover(&mut self, run: &mut dyn FnMut() -> Option<i32>) -> Option<i32> {
            self.handovers += 1;
            run()
        }
        fn cancel_flag(&self) -> Arc<AtomicBool> {
            Arc::clone(&self.cancel)
        }
    }

    fn settings(mode: PermissionMode, os: &'static str) -> AgentSettings {
        let shell = if os == "windows" { Shell::Pwsh } else { Shell::Bash };
        AgentSettings {
            mode,
            model: "modele-test".into(),
            max_tokens: 1024,
            limits: Limits::default(),
            garde_fous: crate::config::Config::default_garde_fous(),
            hooks: HooksConfig::default(),
            cwd: PathBuf::from("."),
            host: HostInfo {
                os,
                arch: "x86_64",
                os_version: "test".into(),
            },
            shells: vec![ShellInfo {
                shell,
                path: PathBuf::from("/bin/fake"),
            }],
            privilege: Privilege::User,
            show_reasoning: false,
            ephemeral: true,
            base_url: "http://127.0.0.1:1/v1".into(),
            retry_delays: vec![Duration::ZERO; 3],
        }
    }

    struct World {
        exec: FakeExecutor,
        elevator: FakeElevator,
        memory: Memory,
        log: SessionLog,
    }

    impl World {
        fn new() -> Self {
            World {
                exec: FakeExecutor::default(),
                elevator: FakeElevator::default(),
                memory: Memory::load(Path::new("target/unit-tmp/agent-memory-none"), false, 200),
                log: SessionLog::disabled(),
            }
        }
    }

    fn run(
        backend: &FakeBackend,
        w: &mut World,
        s: AgentSettings,
        skills: Vec<Skill>,
        ui: &mut FakeUi,
        prompt: &str,
    ) -> TaskOutcome {
        let mut agent = Agent::new(
            backend,
            &mut w.exec,
            &mut w.elevator,
            s,
            skills,
            &mut w.memory,
            &mut w.log,
            Redactor::new(&["sk-canary-1234"]),
        );
        agent.run_task(prompt, ui)
    }

    fn last_tool_message(backend: &FakeBackend) -> String {
        let reqs = backend.requests.borrow();
        let last = reqs.last().expect("a request");
        last.messages
            .iter()
            .rev()
            .find(|m| m.role == Role::Tool)
            .and_then(|m| m.content.clone())
            .unwrap_or_default()
    }

    // ---------- tests ----------

    #[test]
    fn bypass_runs_without_asking_and_frames_output() {
        let backend = FakeBackend::new(vec![Ok(cmd("c1", "echo bonjour")), Ok(answer("Fait."))]);
        let mut w = World::new();
        let mut ui = FakeUi::new(vec![]);
        let out = run(&backend, &mut w, settings(PermissionMode::Bypass, "linux"), vec![], &mut ui, "dis bonjour");
        assert_eq!(out, TaskOutcome::Done);
        assert!(ui.asked.is_empty());
        assert_eq!(w.exec.specs.len(), 1);
        assert_eq!(w.exec.specs[0].command, "echo bonjour");
        assert_eq!(w.exec.specs[0].mode, ExecMode::Captured);
        let msg = last_tool_message(&backend);
        assert!(msg.starts_with("code de sortie : 0"), "{msg}");
        assert!(msg.contains(output::UNTRUSTED_OPEN));
        assert!(ui.events.iter().any(|e| matches!(e, AgentEvent::AssistantDone(t) if t == "Fait.")));
        assert!(ui.events.iter().any(|e| matches!(e, AgentEvent::Progress { done: 1, total: 1 })));
    }

    #[test]
    fn exactly_one_tool_is_declared() {
        let backend = FakeBackend::new(vec![Ok(answer("Bonjour."))]);
        let mut w = World::new();
        let mut ui = FakeUi::new(vec![]);
        run(&backend, &mut w, settings(PermissionMode::Plan, "linux"), vec![], &mut ui, "salut");
        let reqs = backend.requests.borrow();
        let tools = reqs[0].tools.as_array().expect("tools array");
        assert_eq!(tools.len(), 1);
        assert_eq!(reqs[0].messages[0].role, Role::System);
    }

    #[test]
    fn plan_is_approved_once_and_closed() {
        let backend = FakeBackend::new(vec![
            Ok(call_args("c1", serde_json::json!({"command": "ls", "purpose": "lister", "plan": ["ls", "pwd"]}))),
            Ok(cmd("c2", "pwd")),
            Ok(cmd("c3", "whoami")),
            Ok(answer("Fini.")),
        ]);
        let mut w = World::new();
        // Plan approved, then the out-of-plan command refused.
        let mut ui = FakeUi::new(vec![Approval::Yes, Approval::No]);
        let out = run(&backend, &mut w, settings(PermissionMode::Plan, "linux"), vec![], &mut ui, "x");
        assert_eq!(out, TaskOutcome::Done);
        assert_eq!(ui.asked.len(), 2);
        assert!(matches!(&ui.asked[0], ApprovalRequest::Plan(steps) if steps.len() == 2));
        assert!(matches!(&ui.asked[1], ApprovalRequest::Single { step, garde_fou: None } if step.command == "whoami"));
        let ran: Vec<_> = w.exec.specs.iter().map(|s| s.command.as_str()).collect();
        assert_eq!(ran, vec!["ls", "pwd"]);
        assert!(ui.events.iter().any(|e| matches!(e, AgentEvent::CommandStart { index: 2, total: 2, .. })));
        // The refusal reached the model.
        let reqs = backend.requests.borrow();
        let tool_msgs: Vec<String> = reqs[3]
            .messages
            .iter()
            .filter(|m| m.role == Role::Tool)
            .filter_map(|m| m.content.clone())
            .collect();
        assert!(tool_msgs.iter().any(|m| m.starts_with("REFUSÉ")));
    }

    #[test]
    fn rejected_plan_executes_nothing() {
        let backend = FakeBackend::new(vec![Ok(cmd("c1", "ls"))]);
        let mut w = World::new();
        let mut ui = FakeUi::new(vec![Approval::No]);
        let out = run(&backend, &mut w, settings(PermissionMode::Plan, "linux"), vec![], &mut ui, "x");
        assert_eq!(out, TaskOutcome::PlanRejected);
        assert!(w.exec.specs.is_empty());
        assert_eq!(out.exit_code(), 3);
    }

    #[test]
    fn plan_without_terminal_is_not_approved() {
        let backend = FakeBackend::new(vec![Ok(cmd("c1", "ls"))]);
        let mut w = World::new();
        let mut ui = FakeUi::new(vec![Approval::Unavailable]);
        let out = run(&backend, &mut w, settings(PermissionMode::Plan, "linux"), vec![], &mut ui, "x");
        assert_eq!(out, TaskOutcome::PlanNotApproved);
        assert!(w.exec.specs.is_empty());
        assert!(ui.events.iter().any(|e| matches!(e, AgentEvent::Plan(_))));
    }

    #[test]
    fn malformed_calls_are_never_executed_and_capped() {
        let bad = || {
            Ok(ChatResponse {
                content: String::new(),
                reasoning: String::new(),
                tool_calls: vec![ToolCall { id: "b".into(), name: "run_command".into(), arguments: "{not json".into() }],
                usage: None,
                finish_reason: None,
            })
        };
        // Two invalid then a valid one: executed.
        let backend = FakeBackend::new(vec![bad(), bad(), Ok(cmd("c", "echo ok")), Ok(answer("ok"))]);
        let mut w = World::new();
        let mut ui = FakeUi::new(vec![]);
        let out = run(&backend, &mut w, settings(PermissionMode::Bypass, "linux"), vec![], &mut ui, "x");
        assert_eq!(out, TaskOutcome::Done);
        assert_eq!(w.exec.specs.len(), 1);

        // Three invalid in a row: the task stops.
        let backend = FakeBackend::new(vec![bad(), bad(), bad()]);
        let mut w = World::new();
        let mut ui = FakeUi::new(vec![]);
        let out = run(&backend, &mut w, settings(PermissionMode::Bypass, "linux"), vec![], &mut ui, "x");
        assert_eq!(out, TaskOutcome::Failed);
        assert!(w.exec.specs.is_empty());
    }

    #[test]
    fn step_limit_stops_the_task() {
        let mut s = settings(PermissionMode::Bypass, "linux");
        s.limits.max_steps = 2;
        let backend = FakeBackend::new(vec![Ok(cmd("a", "echo 1")), Ok(cmd("b", "echo 2")), Ok(cmd("c", "echo 3"))]);
        let mut w = World::new();
        let mut ui = FakeUi::new(vec![]);
        let out = run(&backend, &mut w, s, vec![], &mut ui, "x");
        assert_eq!(out, TaskOutcome::StepLimit);
        assert_eq!(w.exec.specs.len(), 2);
    }

    #[test]
    fn automatic_fixes_are_capped() {
        let backend = FakeBackend::new(vec![
            Ok(cmd("a", "false1")),
            Ok(cmd("b", "false2")),
            Ok(cmd("c", "false3")),
            Ok(cmd("d", "never")),
        ]);
        let mut w = World::new();
        for _ in 0..3 {
            w.exec.results.push_back((Some(1), String::new(), "erreur\n".into()));
        }
        let mut ui = FakeUi::new(vec![]);
        let out = run(&backend, &mut w, settings(PermissionMode::Bypass, "linux"), vec![], &mut ui, "x");
        assert_eq!(out, TaskOutcome::Failed);
        assert_eq!(w.exec.specs.len(), 3, "original + 2 corrections");
    }

    #[test]
    fn a_failure_then_a_success_resets_the_streak() {
        let backend = FakeBackend::new(vec![Ok(cmd("a", "bad")), Ok(cmd("b", "good")), Ok(answer("ok"))]);
        let mut w = World::new();
        w.exec.results.push_back((Some(1), String::new(), "x".into()));
        let mut ui = FakeUi::new(vec![]);
        let out = run(&backend, &mut w, settings(PermissionMode::Bypass, "linux"), vec![], &mut ui, "x");
        assert_eq!(out, TaskOutcome::Done);
    }

    #[test]
    fn garde_fou_asks_even_in_bypass() {
        let backend = FakeBackend::new(vec![Ok(cmd("a", "rm -rf /")), Ok(answer("Je ne l'ai pas fait."))]);
        let mut w = World::new();
        let mut ui = FakeUi::new(vec![Approval::No]);
        let out = run(&backend, &mut w, settings(PermissionMode::Bypass, "linux"), vec![], &mut ui, "x");
        assert_eq!(out, TaskOutcome::Done);
        assert!(w.exec.specs.is_empty());
        assert!(matches!(&ui.asked[0], ApprovalRequest::Single { garde_fou: Some(_), .. }));
        assert!(last_tool_message(&backend).starts_with("REFUSÉ"));
    }

    #[test]
    fn self_commands_never_reach_a_shell() {
        let skill = Skill {
            name: "projet-npm".into(),
            description: "Projet npm".into(),
            body: "Lance npm install.".into(),
        };
        let backend = FakeBackend::new(vec![
            Ok(cmd("a", "cmd_ai skill projet-npm")),
            Ok(cmd("b", "cmd_ai skill ../config")),
            Ok(cmd("c", "cmd_ai memory add préfère pwsh")),
            Ok(answer("ok")),
        ]);
        let mut w = World::new();
        let mut ui = FakeUi::new(vec![]);
        let out = run(&backend, &mut w, settings(PermissionMode::Plan, "linux"), vec![skill], &mut ui, "x");
        assert_eq!(out, TaskOutcome::Done);
        assert!(w.exec.specs.is_empty());
        assert!(ui.asked.is_empty(), "self-commands need no approval");
        let reqs = backend.requests.borrow();
        let tools: Vec<String> = reqs[3].messages.iter().filter(|m| m.role == Role::Tool).filter_map(|m| m.content.clone()).collect();
        assert!(tools[0].contains("Lance npm install."));
        assert!(tools[1].starts_with("ERREUR"));
        assert_eq!(w.memory.notes().len(), 1);
        // The skill catalog is in the system prompt.
        assert!(reqs[0].messages[0].content.as_deref().unwrap_or("").contains("projet-npm — Projet npm"));
    }

    #[test]
    fn provider_retries_and_errors() {
        // 503 then success: retried.
        let backend = FakeBackend::new(vec![Err(LlmError::Server(503, "x".into())), Ok(answer("ok"))]);
        let mut w = World::new();
        let mut ui = FakeUi::new(vec![]);
        assert_eq!(run(&backend, &mut w, settings(PermissionMode::Plan, "linux"), vec![], &mut ui, "x"), TaskOutcome::Done);
        assert_eq!(backend.requests.borrow().len(), 2);

        // Auth error: no retry, provider error (exit code 5).
        let backend = FakeBackend::new(vec![Err(LlmError::Auth("bad".into()))]);
        let mut ui = FakeUi::new(vec![]);
        let out = run(&backend, &mut w, settings(PermissionMode::Plan, "linux"), vec![], &mut ui, "x");
        assert_eq!(out, TaskOutcome::ProviderError);
        assert_eq!(out.exit_code(), 5);
        assert_eq!(backend.requests.borrow().len(), 1);
        assert!(ui.texts().contains("Clé API refusée"));

        // Empty answer: retried exactly once.
        let backend = FakeBackend::new(vec![Err(LlmError::Empty), Err(LlmError::Empty)]);
        let mut ui = FakeUi::new(vec![]);
        let out = run(&backend, &mut w, settings(PermissionMode::Plan, "linux"), vec![], &mut ui, "x");
        assert_eq!(out, TaskOutcome::ProviderError);
        assert_eq!(backend.requests.borrow().len(), 2);

        // 400 is not retried.
        let backend = FakeBackend::new(vec![Err(LlmError::Server(400, "bad".into()))]);
        let mut ui = FakeUi::new(vec![]);
        run(&backend, &mut w, settings(PermissionMode::Plan, "linux"), vec![], &mut ui, "x");
        assert_eq!(backend.requests.borrow().len(), 1);
    }

    #[test]
    fn cancelled_command_stops_the_task() {
        let backend = FakeBackend::new(vec![Ok(cmd("a", "sleep 100")), Ok(cmd("b", "never"))]);
        let mut w = World::new();
        w.exec.cancel_on_run = true;
        let mut ui = FakeUi::new(vec![]);
        let out = run(&backend, &mut w, settings(PermissionMode::Bypass, "linux"), vec![], &mut ui, "x");
        assert_eq!(out, TaskOutcome::Cancelled);
        assert_eq!(out.exit_code(), 130);
        assert_eq!(w.exec.specs.len(), 1);
    }

    #[test]
    fn secrets_are_masked_everywhere() {
        let backend = FakeBackend::new(vec![Ok(cmd("a", "cat config.json")), Ok(answer("La clé est sk-canary-1234"))]);
        let mut w = World::new();
        w.exec.results.push_back((Some(0), "{\"api_key\": \"sk-canary-1234\"}".into(), String::new()));
        let mut ui = FakeUi::new(vec![]);
        run(&backend, &mut w, settings(PermissionMode::Bypass, "linux"), vec![], &mut ui, "x");
        assert!(!ui.texts().contains("sk-canary-1234"), "{}", ui.texts());
        let reqs = backend.requests.borrow();
        let all: String = reqs.iter().flat_map(|r| r.messages.iter()).filter_map(|m| m.content.clone()).collect();
        assert!(!all.contains("sk-canary-1234"));
    }

    #[test]
    fn unix_admin_uses_sudo_after_handover() {
        let backend = FakeBackend::new(vec![
            Ok(call_args("a", serde_json::json!({"command": "apt install htop", "purpose": "installer", "needs_admin": true}))),
            Ok(answer("ok")),
        ]);
        let mut w = World::new();
        w.elevator.validate_code = Some(0);
        let mut ui = FakeUi::new(vec![]);
        let out = run(&backend, &mut w, settings(PermissionMode::Bypass, "linux"), vec![], &mut ui, "x");
        assert_eq!(out, TaskOutcome::Done);
        assert_eq!(ui.handovers, 1);
        assert_eq!(w.elevator.validated, 1);
        assert!(w.exec.specs[0].sudo);

        // sudo prefix written by the model is stripped and handled by cmd_ai.
        let backend = FakeBackend::new(vec![Ok(cmd("a", "sudo systemctl restart nginx")), Ok(answer("ok"))]);
        let mut w = World::new();
        w.elevator.ready = true;
        let mut ui = FakeUi::new(vec![]);
        run(&backend, &mut w, settings(PermissionMode::Bypass, "linux"), vec![], &mut ui, "x");
        assert_eq!(ui.handovers, 0, "credentials already cached");
        assert_eq!(w.exec.specs[0].command, "systemctl restart nginx");
        assert!(w.exec.specs[0].sudo);

        // Failed authentication stops without running anything.
        let backend = FakeBackend::new(vec![Ok(cmd("a", "sudo apt update"))]);
        let mut w = World::new();
        w.elevator.validate_code = Some(1);
        let mut ui = FakeUi::new(vec![]);
        let out = run(&backend, &mut w, settings(PermissionMode::Bypass, "linux"), vec![], &mut ui, "x");
        assert_eq!(out, TaskOutcome::Failed);
        assert!(w.exec.specs.is_empty());
    }

    #[test]
    fn windows_admin_relaunches_the_session() {
        let backend = FakeBackend::new(vec![Ok(call_args(
            "a",
            serde_json::json!({"command": "Stop-Service Spooler", "purpose": "arrêter", "needs_admin": true}),
        ))]);
        let mut w = World::new();
        w.elevator.relaunch_ok = true;
        let mut ui = FakeUi::new(vec![Approval::Yes]);
        let out = run(&backend, &mut w, settings(PermissionMode::Bypass, "windows"), vec![], &mut ui, "arrête le spooler");
        assert_eq!(out, TaskOutcome::Elevating);
        assert!(w.exec.specs.is_empty());
        assert_eq!(w.elevator.relaunched.len(), 1);
        assert_eq!(w.elevator.relaunched[0].prompt, "arrête le spooler");

        // Refused elevation: the model is told, nothing runs.
        let backend = FakeBackend::new(vec![
            Ok(call_args("a", serde_json::json!({"command": "Stop-Service Spooler", "purpose": "p", "needs_admin": true}))),
            Ok(answer("ok")),
        ]);
        let mut w = World::new();
        let mut ui = FakeUi::new(vec![Approval::No]);
        let out = run(&backend, &mut w, settings(PermissionMode::Bypass, "windows"), vec![], &mut ui, "x");
        assert_eq!(out, TaskOutcome::Done);
        assert!(w.exec.specs.is_empty());
        assert!(w.elevator.relaunched.is_empty());
    }

    #[test]
    fn pre_command_hook_exit_2_blocks() {
        let mut s = settings(PermissionMode::Bypass, "linux");
        s.hooks.pre_command = vec![Hook {
            r#match: Some("git push*".into()),
            run: "check".into(),
            shell: None,
            timeout_secs: None,
        }];
        let backend = FakeBackend::new(vec![Ok(cmd("a", "git push origin main")), Ok(answer("bloqué"))]);
        let mut w = World::new();
        w.exec.results.push_back((Some(2), String::new(), String::new()));
        let mut ui = FakeUi::new(vec![]);
        run(&backend, &mut w, s, vec![], &mut ui, "x");
        assert_eq!(w.exec.specs.len(), 1, "only the hook ran");
        assert!(w.exec.specs[0].extra_env.iter().any(|(k, _)| k == "CMD_AI_EVENT"));
        assert!(last_tool_message(&backend).starts_with("REFUSÉ"));
        assert!(ui.events.iter().any(|e| matches!(e, AgentEvent::CommandStart { hook: true, .. })));
    }

    #[test]
    fn gui_launchers_are_detached() {
        let backend = FakeBackend::new(vec![Ok(cmd("a", "xdg-open https://www.youtube.com")), Ok(answer("ok"))]);
        let mut w = World::new();
        let mut ui = FakeUi::new(vec![]);
        run(&backend, &mut w, settings(PermissionMode::Bypass, "linux"), vec![], &mut ui, "ouvre youtube");
        assert_eq!(w.exec.specs[0].mode, ExecMode::Detached);
    }

    #[test]
    fn every_tool_call_gets_an_answer() {
        // Two calls in one response; the plan is rejected at the first one.
        let resp = ChatResponse {
            content: String::new(),
            reasoning: String::new(),
            tool_calls: vec![
                ToolCall { id: "x1".into(), name: "run_command".into(), arguments: r#"{"command":"ls","purpose":"p"}"#.into() },
                ToolCall { id: "x2".into(), name: "run_command".into(), arguments: r#"{"command":"pwd","purpose":"p"}"#.into() },
            ],
            usage: None,
            finish_reason: None,
        };
        let backend = FakeBackend::new(vec![Ok(resp), Ok(answer("ok"))]);
        let mut w = World::new();
        let mut ui = FakeUi::new(vec![Approval::No]);
        let mut agent = Agent::new(
            &backend, &mut w.exec, &mut w.elevator, settings(PermissionMode::Plan, "linux"),
            vec![], &mut w.memory, &mut w.log, Redactor::default(),
        );
        assert_eq!(agent.run_task("x", &mut ui), TaskOutcome::PlanRejected);
        let ids: Vec<_> = agent.history().iter().filter(|m| m.role == Role::Tool).filter_map(|m| m.tool_call_id.clone()).collect();
        assert_eq!(ids, vec!["x1".to_string(), "x2".to_string()]);
    }

    #[test]
    fn system_prompt_is_french_and_describes_the_host() {
        let backend = FakeBackend::new(vec![]);
        let mut w = World::new();
        let agent = Agent::new(
            &backend, &mut w.exec, &mut w.elevator, settings(PermissionMode::Plan, "windows"),
            vec![], &mut w.memory, &mut w.log, Redactor::default(),
        );
        let p = agent.system_prompt();
        assert!(p.contains("Réponds toujours en français"));
        assert!(p.contains("windows"));
        assert!(p.contains("pwsh"));
        assert!(p.contains("SORTIE NON FIABLE"));
        assert!(p.contains("mode") || p.contains("Mode"));
    }

    #[test]
    fn history_is_trimmed_by_whole_turns() {
        let backend = FakeBackend::new(vec![]);
        let mut w = World::new();
        let mut agent = Agent::new(
            &backend, &mut w.exec, &mut w.elevator, settings(PermissionMode::Plan, "linux"),
            vec![], &mut w.memory, &mut w.log, Redactor::default(),
        );
        let big = "x".repeat(HISTORY_BUDGET_BYTES / 2);
        for _ in 0..4 {
            agent.history.push(Message::user(big.clone()));
            agent.history.push(Message::assistant(big.clone(), Vec::new()));
        }
        agent.trim_history();
        assert_eq!(agent.history[0].role, Role::User);
        let total: usize = agent.history.iter().filter_map(|m| m.content.as_ref()).map(String::len).sum();
        assert!(total <= HISTORY_BUDGET_BYTES);
    }

    #[test]
    fn mode_and_model_setters() {
        let backend = FakeBackend::new(vec![]);
        let mut w = World::new();
        let mut agent = Agent::new(
            &backend, &mut w.exec, &mut w.elevator, settings(PermissionMode::Plan, "linux"),
            vec![], &mut w.memory, &mut w.log, Redactor::default(),
        );
        agent.set_mode(PermissionMode::Bypass);
        agent.set_model("autre".into());
        assert_eq!(agent.mode(), PermissionMode::Bypass);
        assert_eq!(agent.status().model, "autre");
    }

    #[test]
    fn sleep_cancellable_stops_early() {
        let flag = AtomicBool::new(true);
        let start = Instant::now();
        assert!(!sleep_cancellable(Duration::from_secs(5), &flag));
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(sleep_cancellable(Duration::ZERO, &AtomicBool::new(false)));
    }
}
