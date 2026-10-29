//! End-to-end tests of the one-shot mode (TUI fermé): the real binary against the fake
//! Ollama server (spec 001, US1, FR-001…FR-053).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{FakeOllama, Reply, TempHome, run_cmd_ai};
use serde_json::Value;

fn out(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Messages of the n-th request received by the fake server.
fn messages(server: &FakeOllama, n: usize) -> Vec<Value> {
    let reqs = server.requests();
    reqs[n].json()["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn tool_messages(server: &FakeOllama, n: usize) -> Vec<String> {
    messages(server, n)
        .into_iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["content"].as_str().unwrap_or("").to_string())
        .collect()
}

#[test]
fn version_and_help() {
    let home = TempHome::empty();
    let o = run_cmd_ai(&home, &["--version"], None, &[]);
    assert_eq!(o.status.code(), Some(0));
    assert!(out(&o).starts_with("cmd_ai "), "{}", out(&o));

    let o = run_cmd_ai(&home, &["--help"], None, &[]);
    assert_eq!(o.status.code(), Some(0));
    assert!(out(&o).contains("--prompt"));
}

#[test]
fn usage_errors_exit_with_2() {
    let home = TempHome::empty();
    let o = run_cmd_ai(&home, &["--nimporte"], None, &[]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("Erreur d'usage"));

    let o = run_cmd_ai(&home, &["--prompt", "a", "--prompt-file", "b"], None, &[]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn bypass_runs_the_command_and_streams_everything() {
    let server = FakeOllama::with_replies(vec![
        Reply::run_command("c1", "echo bonjour", "Dire bonjour"),
        Reply::text("C'est fait."),
    ]);
    let home = TempHome::new(&server.base_url(), "", "bypass");
    let o = run_cmd_ai(&home, &["--no-tui", "--prompt", "dis bonjour"], None, &[]);
    let stdout = out(&o);
    assert_eq!(
        o.status.code(),
        Some(0),
        "stdout: {stdout}\nstderr: {}",
        err(&o)
    );
    assert!(stdout.contains("▶ [1/1]"), "{stdout}");
    assert!(stdout.contains("Dire bonjour"));
    assert!(stdout.contains("$ echo bonjour"));
    assert!(stdout.contains("│ bonjour"), "{stdout}");
    assert!(stdout.contains("✓ code 0"));
    assert!(stdout.contains("C'est fait."));

    // The model received the framed output, and exactly one tool was declared.
    assert_eq!(server.request_count(), 2);
    let first = server.requests()[0].json();
    assert_eq!(first["tools"].as_array().map(Vec::len), Some(1));
    assert_eq!(first["stream"], true);
    let tools = tool_messages(&server, 1);
    assert_eq!(tools.len(), 1);
    assert!(tools[0].starts_with("code de sortie : 0"), "{}", tools[0]);
    assert!(tools[0].contains("<<<SORTIE NON FIABLE (stdout)>>>"));
    assert!(tools[0].contains("bonjour"));
    // No Authorization header without a key (local Ollama).
    assert!(server.requests()[0].header("authorization").is_none());

    // The session is logged in the cmd_ai folder.
    let logs = home.read_logs();
    assert!(logs.contains("echo bonjour"), "{logs}");
    assert!(logs.contains("dis bonjour"));
}

#[test]
fn plan_mode_without_terminal_executes_nothing() {
    let server = FakeOllama::with_replies(vec![Reply::run_command(
        "c1",
        "echo marqueur > plan-marker.txt",
        "Créer un fichier",
    )]);
    let home = TempHome::new(&server.base_url(), "", "plan");
    let o = run_cmd_ai(
        &home,
        &["--no-tui", "--prompt", "crée un fichier"],
        None,
        &[],
    );
    assert_eq!(o.status.code(), Some(3), "{}", err(&o));
    assert!(out(&o).contains("Plan (1 commande)"));
    assert!(err(&o).contains("rien n'est exécuté"));
    assert!(!home.path().join("plan-marker.txt").exists());
}

#[test]
fn prompt_file_is_read_before_anything() {
    let server = FakeOllama::with_replies(vec![Reply::text("Lu.")]);
    let home = TempHome::new(&server.base_url(), "", "plan");
    let file = home.path().join("demande.txt");
    std::fs::write(&file, "\u{feff}traduis « bonjour » en anglais\r\n").unwrap();
    let o = run_cmd_ai(
        &home,
        &["--no-tui", "--prompt-file", file.to_str().unwrap()],
        None,
        &[],
    );
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let msgs = messages(&server, 0);
    let user = msgs.iter().find(|m| m["role"] == "user").unwrap();
    assert_eq!(user["content"], "traduis « bonjour » en anglais");

    let o = run_cmd_ai(
        &home,
        &["--no-tui", "--prompt-file", "absent.txt"],
        None,
        &[],
    );
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn missing_config_exits_with_4_without_terminal() {
    let home = TempHome::empty();
    let o = run_cmd_ai(&home, &["--no-tui", "--prompt", "x"], None, &[]);
    assert_eq!(o.status.code(), Some(4));
    assert!(err(&o).contains("config.example.json"), "{}", err(&o));
    assert!(
        !home.config_path().exists(),
        "never created without consent"
    );
}

#[test]
fn no_prompt_without_tui_is_a_usage_error() {
    let server = FakeOllama::start();
    let home = TempHome::new(&server.base_url(), "", "plan");
    let o = run_cmd_ai(&home, &["--no-tui"], None, &[]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(server.request_count(), 0);
}

#[test]
fn malformed_tool_calls_are_never_executed() {
    let server = FakeOllama::with_replies(vec![
        Reply::tool_call("b1", "run_command", "{cassé"),
        Reply::tool_call("b2", "run_command", "{\"purpose\":\"sans commande\"}"),
        Reply::tool_call("b3", "autre_outil", "{}"),
    ]);
    let home = TempHome::new(&server.base_url(), "", "bypass");
    let o = run_cmd_ai(&home, &["--no-tui", "--prompt", "x"], None, &[]);
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    assert!(!out(&o).contains("▶ ["), "nothing ran: {}", out(&o));
    assert_eq!(server.request_count(), 3);
    assert!(tool_messages(&server, 1)[0].starts_with("ERREUR"));
}

#[test]
fn provider_errors_have_their_own_exit_code() {
    let server = FakeOllama::with_replies(vec![Reply::Status(
        401,
        "{\"error\":\"unauthorized\"}".into(),
        None,
    )]);
    let home = TempHome::new(&server.base_url(), "", "plan");
    let o = run_cmd_ai(&home, &["--no-tui", "--prompt", "x"], None, &[]);
    assert_eq!(o.status.code(), Some(5));
    assert!(err(&o).contains("Clé API refusée"), "{}", err(&o));

    let server = FakeOllama::with_replies(vec![Reply::Status(
        402,
        "{\"error\":\"no credits\"}".into(),
        None,
    )]);
    let home = TempHome::new(&server.base_url(), "", "plan");
    let o = run_cmd_ai(&home, &["--no-tui", "--prompt", "x"], None, &[]);
    assert_eq!(o.status.code(), Some(5));
    assert!(err(&o).contains("Crédits épuisés"));
}

#[test]
fn concurrency_429_is_retried() {
    let server = FakeOllama::with_replies(vec![
        Reply::Status(
            429,
            "{\"error\":\"timed out waiting for a concurrent request slot\"}".into(),
            Some("0".into()),
        ),
        Reply::text("Après la file d'attente."),
    ]);
    let home = TempHome::new(&server.base_url(), "", "plan");
    let o = run_cmd_ai(&home, &["--no-tui", "--prompt", "x"], None, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(server.request_count(), 2);
    assert!(out(&o).contains("Après la file d'attente."));
}

#[test]
fn self_commands_load_skills_without_a_shell() {
    let server = FakeOllama::with_replies(vec![
        Reply::run_command("s1", "cmd_ai skill projet-test", "Charger le playbook"),
        Reply::text("Playbook lu."),
    ]);
    let home = TempHome::new(&server.base_url(), "", "plan");
    home.write_skill(
        "projet-test",
        "---\nname: projet-test\ndescription: Playbook de test\n---\nÉtape 1 : lire le README.\n",
    );
    let o = run_cmd_ai(&home, &["--no-tui", "--prompt", "aide-moi"], None, &[]);
    assert_eq!(o.status.code(), Some(0), "{}\n{}", out(&o), err(&o));
    assert!(out(&o).contains("auto-commande : cmd_ai skill projet-test"));
    // Plan mode, no terminal, yet nothing was asked: self-commands need no approval.
    let sys = messages(&server, 0)[0]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(sys.contains("projet-test — Playbook de test"));
    assert!(tool_messages(&server, 1)[0].contains("Étape 1 : lire le README."));
}

#[test]
fn pre_command_hook_can_block() {
    let server = FakeOllama::with_replies(vec![
        Reply::run_command("c1", "echo interdit", "Test"),
        Reply::text("Bloqué par le hook."),
    ]);
    let home = TempHome::new(&server.base_url(), "", "bypass");
    let mut cfg: Value =
        serde_json::from_str(&std::fs::read_to_string(home.config_path()).unwrap()).unwrap();
    cfg["hooks"] = serde_json::json!({ "pre_command": [{ "match": "echo *", "run": "exit 2" }] });
    home.write_config(&cfg.to_string());
    let o = run_cmd_ai(&home, &["--no-tui", "--prompt", "x"], None, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let stdout = out(&o);
    assert!(stdout.contains("[hook]"), "{stdout}");
    assert!(
        !stdout.contains("│ interdit"),
        "the command did not run: {stdout}"
    );
    assert!(tool_messages(&server, 1)[0].starts_with("REFUSÉ"));
}

#[test]
fn garde_fou_without_terminal_is_refused_in_bypass() {
    let server = FakeOllama::with_replies(vec![
        Reply::run_command("c1", "rm -rf /", "Tout effacer"),
        Reply::text("Je ne l'ai pas fait."),
    ]);
    let home = TempHome::new(&server.base_url(), "", "bypass");
    let o = run_cmd_ai(&home, &["--no-tui", "--prompt", "x"], None, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(!out(&o).contains("▶ [1/"), "nothing ran");
    let t = tool_messages(&server, 1);
    assert!(
        t[0].starts_with("REFUSÉ") && t[0].contains("garde-fou"),
        "{t:?}"
    );
}

#[test]
fn failing_command_is_reported_and_exit_code_is_1() {
    let server = FakeOllama::with_replies(vec![
        Reply::run_command("c1", "exit 3", "Échouer"),
        Reply::text("La commande a échoué."),
    ]);
    let home = TempHome::new(&server.base_url(), "", "bypass");
    let o = run_cmd_ai(&home, &["--no-tui", "--prompt", "x"], None, &[]);
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    assert!(out(&o).contains("✗ code 3"));
    assert!(tool_messages(&server, 1)[0].starts_with("code de sortie : 3"));
}

#[test]
fn model_roles_are_resolved() {
    let server = FakeOllama::with_replies(vec![Reply::text("ok")]);
    let home = TempHome::new(&server.base_url(), "", "plan");
    let mut cfg: Value =
        serde_json::from_str(&std::fs::read_to_string(home.config_path()).unwrap()).unwrap();
    cfg["provider"]["roles"] = serde_json::json!({ "fast": "modele-rapide" });
    home.write_config(&cfg.to_string());
    let o = run_cmd_ai(
        &home,
        &["--no-tui", "--model", "fast", "--prompt", "x"],
        None,
        &[],
    );
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(server.requests()[0].json()["model"], "modele-rapide");

    let o = run_cmd_ai(
        &home,
        &["--no-tui", "--model", "inconnu", "--prompt", "x"],
        None,
        &[],
    );
    assert_eq!(o.status.code(), Some(4));
    assert!(err(&o).contains("fast"), "lists the roles: {}", err(&o));
}
