//! Agent ↔ display interface (`Frontend`), theme, plain and TUI frontends.
//!
//! The agent never prints: it emits [`AgentEvent`]s and asks for [`Approval`]s through a
//! [`Frontend`]. `plain` renders them as text (TUI fermé), `tui` with ratatui (TUI ouvert).

pub mod plain;
pub mod theme;
pub mod tui;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::config::PermissionMode;
use crate::exec::Stream;

/// One command as shown to the user (plan list, command header).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanStep {
    pub command: String,
    pub purpose: String,
    pub shell: String,
    pub admin: bool,
}

/// What the status bar shows permanently (constitution V: mode and privilege always visible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub mode: PermissionMode,
    pub admin: bool,
    pub model: String,
    pub ephemeral: bool,
    pub shell: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEvent {
    Status(StatusLine),
    /// Reasoning tokens (shown dimmed only if `ui.show_reasoning`).
    Reasoning(String),
    /// Streaming answer text.
    AssistantDelta(String),
    /// The final answer of a turn (complete text).
    AssistantDone(String),
    /// A plan is about to be submitted for approval (or shown in bypass mode).
    Plan(Vec<PlanStep>),
    Progress {
        done: usize,
        total: usize,
    },
    CommandStart {
        index: usize,
        total: usize,
        step: PlanStep,
        hook: bool,
    },
    CommandOutput {
        stream: Stream,
        text: String,
    },
    CommandEnd {
        code: Option<i32>,
        duration: Duration,
        timed_out: bool,
        cancelled: bool,
        truncated: bool,
    },
    Info(String),
    Warning(String),
    Error(String),
    TaskEnd(TaskOutcome),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskOutcome {
    Done,
    Failed,
    PlanRejected,
    /// Plan mode without an interactive terminal (exit code 3).
    PlanNotApproved,
    Cancelled,
    StepLimit,
    ProviderError,
    /// The session is being relaunched elevated (Windows).
    Elevating,
}

impl TaskOutcome {
    /// Process exit code for one-shot runs (contracts/cli-and-tool.md).
    pub fn exit_code(self) -> i32 {
        match self {
            TaskOutcome::Done | TaskOutcome::Elevating => 0,
            TaskOutcome::Failed | TaskOutcome::StepLimit => 1,
            TaskOutcome::PlanRejected | TaskOutcome::PlanNotApproved => 3,
            TaskOutcome::ProviderError => 5,
            TaskOutcome::Cancelled => 130,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalRequest {
    /// The whole plan, before anything runs (plan mode).
    Plan(Vec<PlanStep>),
    /// A command outside the approved plan, or a garde-fou match in bypass mode.
    Single {
        step: PlanStep,
        garde_fou: Option<String>,
    },
    /// Relaunch the session as administrator (Windows) — the reason is shown.
    Elevation { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    Yes,
    No,
    /// No interactive terminal to ask.
    Unavailable,
}

pub trait Frontend {
    fn event(&mut self, ev: AgentEvent);
    fn approve(&mut self, req: ApprovalRequest) -> Approval;
    /// Gives the terminal to `program` (e.g. `sudo -v`, an interactive step); returns its exit code.
    fn handover(&mut self, program: &str, args: &[&str]) -> Option<i32>;
    /// Set by the frontend when the user cancels (Échap / Ctrl+C).
    fn cancel_flag(&self) -> Arc<AtomicBool>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_follow_the_contract() {
        assert_eq!(TaskOutcome::Done.exit_code(), 0);
        assert_eq!(TaskOutcome::Failed.exit_code(), 1);
        assert_eq!(TaskOutcome::StepLimit.exit_code(), 1);
        assert_eq!(TaskOutcome::PlanRejected.exit_code(), 3);
        assert_eq!(TaskOutcome::PlanNotApproved.exit_code(), 3);
        assert_eq!(TaskOutcome::ProviderError.exit_code(), 5);
        assert_eq!(TaskOutcome::Cancelled.exit_code(), 130);
    }
}
