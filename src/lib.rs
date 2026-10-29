//! cmd_ai : agent TUI portable dont le seul tool est l'exécution de commandes shell.
//!
//! Voir `specs/001-cmd-ai-v1/contracts/rust-api.md` pour le contrat entre modules.

pub mod agent;
pub mod cli;
pub mod config;
pub mod exec;
pub mod home;
pub mod hooks;
pub mod llm;
pub mod log;
pub mod memory;
pub mod output;
pub mod platform;
pub mod risk;
pub mod secret;
pub mod session;
pub mod shell;
pub mod skills;
pub mod text;
pub mod tool;
pub mod ui;

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use agent::{Agent, AgentSettings, DEFAULT_RETRY_DELAYS, SystemElevator};
use cli::{Action, PromptSource};
use config::{Config, ConfigError, ThemeChoice};
use exec::SystemExecutor;
use home::Home;
use llm::client::{ClientOptions, OpenAiClient};
use log::SessionLog;
use memory::Memory;
use secret::Redactor;
use shell::{Shell, ShellInfo};
use skills::Skill;
use ui::theme::{self, Theme};
use ui::{AgentEvent, Frontend};

/// Everything a frontend needs to run the agent, owned (the TUI moves it to a worker thread).
pub struct Session {
    pub client: OpenAiClient,
    pub settings: AgentSettings,
    pub skills: Vec<Skill>,
    pub warnings: Vec<String>,
    pub memory: Memory,
    pub log: SessionLog,
    pub redactor: Redactor,
    pub home: Home,
    pub theme: Theme,
    pub config: Config,
}

/// Point d'entrée : renvoie le code de sortie du processus.
pub fn run() -> i32 {
    let args = match cli::parse_args(std::env::args_os().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    match args.action {
        Action::Help => {
            println!("{}", text::help());
            return 0;
        }
        Action::Version => {
            println!("{}", text::version_line());
            return 0;
        }
        Action::Run => {}
    }
    let stdin_tty = std::io::stdin().is_terminal();
    let stdout_tty = std::io::stdout().is_terminal();

    // FR-001: the prompt is read before anything else, and before any UI.
    let mut prompt = match &args.prompt {
        Some(PromptSource::Text(t)) => Some(t.clone()),
        Some(PromptSource::File(p)) => match cli::read_prompt_file(p) {
            Ok(t) => Some(t),
            Err(e) => {
                eprintln!("{e}");
                return 2;
            }
        },
        None => None,
    };

    let home = match Home::resolve(args.home.as_deref()) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("{}", text::home_error(&e));
            return 4;
        }
    };
    let env_key = std::env::var(config::ENV_API_KEY)
        .ok()
        .filter(|k| !k.trim().is_empty());
    let config = match Config::load(&home.config_path(), env_key.clone()) {
        Ok(c) => c,
        Err(ConfigError::Missing(_)) if stdin_tty && stdout_tty => {
            match first_run(&home, env_key.as_deref()) {
                Some(c) => c,
                None => return 4,
            }
        }
        Err(e) => {
            eprintln!("{}", text::config_error(&e));
            return 4;
        }
    };

    let mut mode = config.permissions;
    let mut resumed_model = None;
    let mut resume_cwd = None;
    if let Some(path) = &args.resume {
        match session::load_resume(path) {
            Ok(st) => {
                prompt = Some(st.prompt);
                mode = st.mode;
                resumed_model = Some(st.model);
                resume_cwd = Some(st.cwd);
            }
            Err(e) => {
                eprintln!("{e}");
                return 4;
            }
        }
    }
    if let Some(m) = args.mode {
        mode = m;
    }
    let model = match (resumed_model, args.model.as_deref()) {
        (Some(m), None) => m,
        (_, choice) => match config.resolve_model(choice) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("{}", text::config_error(&e));
                return 4;
            }
        },
    };

    let shells = available_shells();
    if shells.is_empty() {
        eprintln!("{}", text::SHELL_NOT_FOUND);
        return 4;
    }
    let tui = args.tui.unwrap_or(config.ui.tui) && stdin_tty && stdout_tty;
    if !tui && prompt.is_none() {
        eprintln!("{}", text::usage_error(text::NO_PROMPT_WITHOUT_TUI));
        return 2;
    }

    let redactor = Redactor::new(&[config.provider.api_key.expose()]);
    let mut log = if home.writable {
        SessionLog::open(
            &home,
            redactor.clone(),
            mib(config.limits.log_file_mib),
            mib(config.limits.log_total_mib),
        )
    } else {
        SessionLog::disabled()
    };
    let ca_file = config.network.ca_file.as_ref().map(|p| {
        let p = Path::new(p);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            home.root.join(p)
        }
    });
    let client = match OpenAiClient::new(ClientOptions {
        base_url: config.provider.base_url.clone(),
        api_key: config.provider.api_key.clone(),
        proxy: config.network.proxy.clone(),
        ca_file,
        connect_timeout: Duration::from_secs(config.network.connect_timeout_secs),
        request_timeout: Duration::from_secs(config.provider.timeout_secs),
    }) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", text::client_error(&redactor.redact(&e.to_string())));
            return 4;
        }
    };

    let (skills, warnings) = skills::load_catalog(&home.skills_dir());
    for w in &warnings {
        log.line("SKILL", w);
    }
    let memory = Memory::load(
        &home.memory_dir(),
        home.writable,
        config.limits.memory_max_notes as usize,
    );
    let cwd = resume_cwd
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let host = platform::host_info();
    let privilege = platform::current_privilege();

    let env = |k: &str| std::env::var(k).ok();
    let color_choice = args
        .color
        .clone()
        .unwrap_or_else(|| config.ui.color.as_str().to_string());
    let level = theme::detect_level(&color_choice, &env, stdout_tty);
    let theme_choice = match config.ui.theme {
        ThemeChoice::Auto => "auto",
        ThemeChoice::Dark => "dark",
        ThemeChoice::Light => "light",
    };
    let theme = Theme::new(level, theme::detect_background(theme_choice, &env));

    log.line(
        "SESSION",
        &format!(
            "{} · {} {} ({}) · {} · tui={tui}",
            text::version_line(),
            host.os,
            host.os_version,
            host.arch,
            shells[0].shell.name()
        ),
    );
    let settings = AgentSettings {
        mode,
        model,
        max_tokens: config.provider.max_tokens,
        limits: config.limits.clone(),
        garde_fous: config.garde_fous.clone(),
        hooks: config.hooks.clone(),
        cwd,
        host,
        shells,
        privilege,
        show_reasoning: config.ui.show_reasoning,
        ephemeral: !home.writable,
        base_url: config.provider.base_url.clone(),
        retry_delays: DEFAULT_RETRY_DELAYS.to_vec(),
    };
    let session = Session {
        client,
        settings,
        skills,
        warnings,
        memory,
        log,
        redactor,
        home,
        theme,
        config,
    };
    if tui {
        match ui::tui::probe() {
            Ok(()) => return ui::tui::run(session, prompt),
            Err(detail) => eprintln!("{}", text::tui_unavailable(&detail)),
        }
    }
    run_plain(session, prompt, stdin_tty)
}

/// TUI fermé. With a prompt: one task, then exit with the task's code. Without (terminal
/// that cannot host the inline TUI): a line-by-line loop with the same slash commands.
fn run_plain(session: Session, prompt: Option<String>, interactive: bool) -> i32 {
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
    let cancel = Arc::new(AtomicBool::new(false));
    platform::install_interrupt_handler(Arc::clone(&cancel));
    let mut fe = ui::plain::PlainFrontend::stdio(theme, interactive, cancel);
    for w in warnings {
        fe.event(AgentEvent::Warning(w));
    }
    if !home.writable {
        fe.event(AgentEvent::Warning(text::EPHEMERAL_NOTICE.to_string()));
    }
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
    agent.session_start(&mut fe);
    if let Some(prompt) = prompt {
        return agent.run_task(&prompt, &mut fe).exit_code();
    }
    fe.event(AgentEvent::Status(agent.status()));
    while let Some(line) = fe.read_request() {
        if line.is_empty() {
            continue;
        }
        match line.strip_prefix('/') {
            Some(cmd) => {
                if !slash_command(cmd, &mut agent, &mut fe, &config) {
                    break;
                }
            }
            None => {
                agent.run_task(&line, &mut fe);
            }
        }
    }
    fe.event(AgentEvent::Info(text::TUI_BYE.to_string()));
    0
}

/// Slash commands of the text-mode loop (same as the TUI). Returns false to quit.
fn slash_command<B: llm::ChatBackend, E: exec::Executor>(
    cmd: &str,
    agent: &mut Agent<'_, B, E>,
    fe: &mut dyn Frontend,
    config: &Config,
) -> bool {
    let mut parts = cmd.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or("").to_lowercase();
    let arg = parts.next().map(str::trim).filter(|a| !a.is_empty());
    match name.as_str() {
        "aide" | "help" | "?" => fe.event(AgentEvent::Info(text::TUI_HELP.to_string())),
        "plan" => {
            agent.set_mode(config::PermissionMode::Plan);
            fe.event(AgentEvent::Info(text::tui_mode_set(text::MODE_PLAN)));
        }
        "bypass" => {
            agent.set_mode(config::PermissionMode::Bypass);
            fe.event(AgentEvent::Info(text::tui_mode_set(text::MODE_BYPASS)));
        }
        "model" | "modele" | "modèle" => match config.resolve_model(arg) {
            Ok(m) => {
                fe.event(AgentEvent::Info(text::tui_model_set(&m)));
                agent.set_model(m);
            }
            Err(e) => fe.event(AgentEvent::Error(text::config_error(&e))),
        },
        "effacer" | "clear" => {
            agent.clear_history();
            fe.event(AgentEvent::Info(text::TUI_CLEARED.to_string()));
        }
        "quitter" | "quit" | "exit" => return false,
        _ => fe.event(AgentEvent::Warning(text::tui_unknown_command(&format!(
            "/{cmd}"
        )))),
    }
    true
}

/// Default shell first (FR-031), then the other shells found on this host.
pub fn available_shells() -> Vec<ShellInfo> {
    let mut out: Vec<ShellInfo> = Vec::new();
    if let Some(d) = shell::detect_default() {
        out.push(d);
    }
    let candidates: &[Shell] = if cfg!(windows) {
        &[Shell::Pwsh, Shell::PowerShell, Shell::Cmd]
    } else {
        &[Shell::Bash, Shell::Sh, Shell::Zsh, Shell::Pwsh]
    };
    for s in candidates {
        if out.iter().any(|i| i.shell == *s) {
            continue;
        }
        if let Some(i) = shell::find(*s) {
            out.push(i);
        }
    }
    out
}

fn mib(n: u32) -> u64 {
    u64::from(n) * 1024 * 1024
}

/// First launch without `config.json`: offer to create it from `config.example.json` and to
/// type the API key (not echoed). Interactive terminals only.
fn first_run(home: &Home, env_key: Option<&str>) -> Option<Config> {
    eprintln!("{}", text::setup_config_missing(&home.config_path()));
    eprint!("{}", text::SETUP_COPY_QUESTION);
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer).is_err() || !text::is_yes(&answer) {
        eprintln!("{}", text::SETUP_DECLINED);
        return None;
    }
    let key = if env_key.is_some() {
        None
    } else {
        eprintln!("{}", text::SETUP_API_KEY_HINT);
        eprint!("{}", text::SETUP_API_KEY_QUESTION);
        let _ = std::io::stderr().flush();
        read_secret_line()
    };
    match config::create_from_example(home, key.as_deref()) {
        Ok(path) => {
            eprintln!("{}", text::setup_copied(&path));
            if key.as_deref().is_some_and(|k| !k.trim().is_empty()) {
                eprintln!("{}", text::setup_api_key_saved(&path));
            } else if env_key.is_none() {
                eprintln!("{}", text::SETUP_API_KEY_SKIPPED);
            }
        }
        Err(m) => {
            eprintln!("{m}");
            return None;
        }
    }
    match Config::load(&home.config_path(), env_key.map(str::to_string)) {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("{}", text::config_error(&e));
            None
        }
    }
}

/// Reads one line without echo (raw mode); `None` on Échap or error.
fn read_secret_line() -> Option<String> {
    use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use ratatui::crossterm::terminal;
    terminal::enable_raw_mode().ok()?;
    let mut s = String::new();
    let result = loop {
        match event::read() {
            Ok(Event::Key(k)) if k.kind != KeyEventKind::Release => match k.code {
                KeyCode::Enter => break Some(s.trim().to_string()),
                KeyCode::Esc => break None,
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => break None,
                KeyCode::Backspace => {
                    s.pop();
                }
                KeyCode::Char(c) => s.push(c),
                _ => {}
            },
            Ok(Event::Paste(p)) => s.push_str(&p),
            Ok(_) => {}
            Err(_) => break None,
        }
    };
    let _ = terminal::disable_raw_mode();
    eprintln!();
    result
}
