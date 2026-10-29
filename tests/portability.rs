//! Portability and secrets (spec 001, US3; constitution III and VI, both NON NÉGOCIABLE).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{FakeOllama, Reply, TempHome, run_cmd_ai};

fn out(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// A command printing a file, for the default shell of this OS.
fn print_file(name: &str) -> String {
    if cfg!(windows) {
        format!("Get-Content {name}")
    } else {
        format!("cat {name}")
    }
}

/// A command printing an environment variable (empty when absent).
fn print_env(var: &str) -> String {
    if cfg!(windows) {
        format!("Write-Output \"[$env:{var}]\"")
    } else {
        format!("echo \"[${var}]\"")
    }
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(files_under(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}

#[test]
fn the_api_key_never_leaks() {
    const KEY: &str = "sk-canary-7f3a9c2e1b";
    let server = FakeOllama::with_replies(vec![
        Reply::run_command(
            "c1",
            &print_file("config.json"),
            "Afficher la configuration",
        ),
        Reply::run_command("c2", &print_env("OLLAMA_API_KEY"), "Lire la variable"),
        Reply::text(&format!("La clé est {KEY}.")),
    ]);
    let home = TempHome::new(&server.base_url(), KEY, "bypass");
    let o = run_cmd_ai(
        &home,
        &["--no-tui", "--prompt", "montre la config"],
        None,
        &[],
    );
    assert_eq!(o.status.code(), Some(0), "{}\n{}", out(&o), err(&o));

    // Sent only as the Authorization header to the configured base URL.
    let reqs = server.requests();
    assert_eq!(
        reqs[0].header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    for r in &reqs {
        assert!(!r.body.contains(KEY), "request body leaked the key");
    }
    // Never on screen nor in the logs (the file itself is the only place).
    assert!(!out(&o).contains(KEY), "{}", out(&o));
    assert!(!err(&o).contains(KEY));
    assert!(!home.read_logs().contains(KEY));
    let leaking: Vec<_> = home
        .files_containing(KEY)
        .into_iter()
        .filter(|p| p.file_name().is_none_or(|n| n != "config.json"))
        .collect();
    assert!(leaking.is_empty(), "{leaking:?}");
    // Children never inherit the key: config key here, so the variable is empty.
    assert!(out(&o).contains("[]"), "{}", out(&o));
}

#[test]
fn env_key_overrides_and_is_scrubbed_from_children() {
    const ENV_KEY: &str = "sk-env-override-55aa";
    let server = FakeOllama::with_replies(vec![
        Reply::run_command("c1", &print_env("OLLAMA_API_KEY"), "Lire la variable"),
        Reply::text("ok"),
    ]);
    let home = TempHome::new(&server.base_url(), "", "bypass");
    let o = run_cmd_ai(
        &home,
        &["--no-tui", "--prompt", "x"],
        None,
        &[("OLLAMA_API_KEY", ENV_KEY)],
    );
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(
        server.requests()[0].header("authorization"),
        Some(format!("Bearer {ENV_KEY}").as_str())
    );
    let stdout = out(&o);
    assert!(stdout.contains("[]"), "the child saw no key: {stdout}");
    assert!(!stdout.contains(ENV_KEY));
}

#[test]
fn nothing_is_written_on_the_host() {
    let server = FakeOllama::with_replies(vec![
        Reply::run_command("c1", "echo portable", "Écrire sur la sortie"),
        Reply::text("ok"),
    ]);
    let home = TempHome::new(&server.base_url(), "", "bypass");
    let host = home.path().join("sentinelles");
    let vars = [
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
        "TMP",
        "TEMP",
        "TMPDIR",
    ];
    let mut env: Vec<(String, String)> = Vec::new();
    for v in vars {
        let dir = host.join(v);
        fs::create_dir_all(&dir).unwrap();
        env.push((v.to_string(), dir.display().to_string()));
    }
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let o = run_cmd_ai(&home, &["--no-tui", "--prompt", "x"], None, &env_refs);
    assert_eq!(o.status.code(), Some(0), "{}\n{}", out(&o), err(&o));
    assert!(out(&o).contains("│ portable"));
    // Only exception imposed by the shell (documented in doc/portabilite.md): PowerShell's
    // startup JIT profile, which no option can disable.
    let written: Vec<_> = files_under(&host)
        .into_iter()
        .filter(|p| {
            !p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("StartupProfileData-"))
        })
        .collect();
    assert!(
        written.is_empty(),
        "host directories must stay empty: {written:?}"
    );
    assert!(
        !home.log_files().is_empty(),
        "the log lives in the cmd_ai folder"
    );
}

#[test]
fn config_is_read_next_to_the_executable_not_the_cwd() {
    let server = FakeOllama::with_replies(vec![Reply::text("Depuis le dossier de l'exécutable.")]);
    let portable = TempHome::new(&server.base_url(), "", "plan");
    let exe_name = if cfg!(windows) {
        "cmd_ai.exe"
    } else {
        "cmd_ai"
    };
    let exe = portable.path().join(exe_name);
    fs::copy(common::cmd_ai_bin(), &exe).unwrap();
    // Another directory with a decoy config.json as the current directory.
    let elsewhere = TempHome::new("http://127.0.0.1:9/v1", "", "plan");
    let mut cmd = common::command(&exe);
    cmd.args(["--no-tui", "--prompt", "x"])
        .current_dir(elsewhere.path())
        .env_remove("OLLAMA_API_KEY")
        .env_remove("CMD_AI_HOME")
        .env("NO_COLOR", "1");
    let o = cmd.output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(out(&o).contains("Depuis le dossier de l'exécutable."));
    assert_eq!(server.request_count(), 1);
    assert!(!portable.log_files().is_empty());
    assert!(elevated_logs_absent(&elsewhere));
}

fn elevated_logs_absent(home: &TempHome) -> bool {
    home.log_files().is_empty()
}

#[test]
fn cmd_ai_home_env_is_honoured() {
    let server = FakeOllama::with_replies(vec![Reply::text("ok")]);
    let home = TempHome::new(&server.base_url(), "", "plan");
    let mut cmd = common::command(common::cmd_ai_bin());
    cmd.args(["--no-tui", "--prompt", "x"])
        .env_remove("OLLAMA_API_KEY")
        .env("CMD_AI_HOME", home.path())
        .env("NO_COLOR", "1");
    let o = cmd.output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(server.request_count(), 1);
}

#[cfg(unix)]
#[test]
fn read_only_folder_runs_in_ephemeral_mode() {
    use std::os::unix::fs::PermissionsExt;
    let server = FakeOllama::with_replies(vec![Reply::text("ok")]);
    let home = TempHome::new(&server.base_url(), "", "plan");
    for sub in ["log", "memory", "skills"] {
        fs::remove_dir_all(home.path().join(sub)).unwrap();
    }
    fs::set_permissions(home.path(), fs::Permissions::from_mode(0o555)).unwrap();
    let o = run_cmd_ai(&home, &["--no-tui", "--prompt", "x"], None, &[]);
    fs::set_permissions(home.path(), fs::Permissions::from_mode(0o755)).unwrap();
    if unsafe_is_root() {
        return; // root ignores permissions; nothing to assert.
    }
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(format!("{}{}", out(&o), err(&o)).contains("éphémère"));
    assert!(!home.path().join("log").exists());
}

#[cfg(unix)]
fn unsafe_is_root() -> bool {
    std::env::var("USER").is_ok_and(|u| u == "root")
}
