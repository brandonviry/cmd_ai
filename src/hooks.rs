//! Hooks: deterministic triggers declared by the human in `config.json` (IDEE.md §5).
//!
//! The model can neither trigger, create nor modify them. They run through `exec` like any
//! command, always visibly (constitution II and IV). `file_changed` uses a cheap
//! before/after snapshot (existence, size, mtime) around each command — no watcher thread.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::config::{FileHook, Hook, HooksConfig};
use crate::risk::glob_match;

/// Default timeout of a hook when `timeout_secs` is not set.
pub const DEFAULT_HOOK_TIMEOUT: Duration = Duration::from_secs(30);

/// A hook ready to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookRun {
    pub event: &'static str,
    pub run: String,
    pub shell: Option<String>,
    pub timeout: Duration,
    pub env: Vec<(String, String)>,
}

fn timeout_of(secs: Option<u64>) -> Duration {
    secs.filter(|s| *s > 0)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_HOOK_TIMEOUT)
}

fn hook_run(event: &'static str, hook: &Hook, extra: &[(&str, &str)]) -> HookRun {
    let mut env = vec![("CMD_AI_EVENT".to_string(), event.to_string())];
    env.extend(
        extra
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string())),
    );
    HookRun {
        event,
        run: hook.run.clone(),
        shell: hook.shell.clone(),
        timeout: timeout_of(hook.timeout_secs),
        env,
    }
}

/// Hooks of `pre_command` / `post_command` whose `match` glob matches the whole command
/// (no `match` = every command), in declaration order.
pub fn hooks_for_command(cfg: &HooksConfig, event: &'static str, command: &str) -> Vec<HookRun> {
    let list: &[Hook] = match event {
        "pre_command" => &cfg.pre_command,
        "post_command" => &cfg.post_command,
        _ => &[],
    };
    list.iter()
        .filter(|h| h.r#match.as_deref().is_none_or(|p| glob_match(p, command)))
        .map(|h| hook_run(event, h, &[("CMD_AI_COMMAND", command)]))
        .collect()
}

/// Hooks of `session_start` / `task_end`, in declaration order.
pub fn hooks_simple(cfg: &HooksConfig, event: &'static str) -> Vec<HookRun> {
    let list: &[Hook] = match event {
        "session_start" => &cfg.session_start,
        "task_end" => &cfg.task_end,
        _ => &[],
    };
    list.iter().map(|h| hook_run(event, h, &[])).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileState {
    exists: bool,
    len: u64,
    modified: Option<SystemTime>,
}

fn state_of(path: &Path) -> FileState {
    match std::fs::metadata(path) {
        Ok(m) => FileState {
            exists: true,
            len: m.len(),
            modified: m.modified().ok(),
        },
        Err(_) => FileState {
            exists: false,
            len: 0,
            modified: None,
        },
    }
}

fn resolve(cwd: &Path, p: &str) -> PathBuf {
    let path = Path::new(p);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// States of every watched path, taken before a command runs.
#[derive(Debug, Clone, Default)]
pub struct FileSnapshot {
    states: Vec<(PathBuf, FileState)>,
}

impl FileSnapshot {
    pub fn take(cfg: &HooksConfig, cwd: &Path) -> Self {
        let mut states = Vec::new();
        for hook in &cfg.file_changed {
            for p in &hook.paths {
                let path = resolve(cwd, p);
                if !states.iter().any(|(q, _)| q == &path) {
                    let st = state_of(&path);
                    states.push((path, st));
                }
            }
        }
        FileSnapshot { states }
    }

    /// `file_changed` hooks to run: one run per (hook, changed path), with `CMD_AI_FILE`.
    pub fn changed(&self, cfg: &HooksConfig, cwd: &Path) -> Vec<HookRun> {
        let mut out = Vec::new();
        for hook in &cfg.file_changed {
            for p in &hook.paths {
                let path = resolve(cwd, p);
                let before = self.states.iter().find(|(q, _)| q == &path).map(|(_, s)| s);
                let now = state_of(&path);
                if before.is_some_and(|b| *b != now) {
                    out.push(file_hook_run(hook, &path));
                }
            }
        }
        out
    }
}

fn file_hook_run(hook: &FileHook, path: &Path) -> HookRun {
    HookRun {
        event: "file_changed",
        run: hook.run.clone(),
        shell: hook.shell.clone(),
        timeout: timeout_of(hook.timeout_secs),
        env: vec![
            ("CMD_AI_EVENT".to_string(), "file_changed".to_string()),
            (
                "CMD_AI_FILE".to_string(),
                path.to_string_lossy().into_owned(),
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook(m: Option<&str>, run: &str) -> Hook {
        Hook {
            r#match: m.map(str::to_string),
            run: run.to_string(),
            shell: None,
            timeout_secs: None,
        }
    }

    fn cfg() -> HooksConfig {
        HooksConfig {
            session_start: vec![hook(None, "echo start")],
            pre_command: vec![
                hook(Some("git push*"), "git status"),
                hook(None, "echo any"),
            ],
            post_command: vec![hook(Some("npm *"), "echo npm done")],
            file_changed: vec![],
            task_end: vec![hook(None, "echo end")],
        }
    }

    #[test]
    fn pre_command_matching_and_order() {
        let runs = hooks_for_command(&cfg(), "pre_command", "git push origin main");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].run, "git status");
        assert_eq!(runs[1].run, "echo any");
        assert!(
            runs[0]
                .env
                .contains(&("CMD_AI_COMMAND".into(), "git push origin main".into()))
        );
        assert!(
            runs[0]
                .env
                .contains(&("CMD_AI_EVENT".into(), "pre_command".into()))
        );
        assert_eq!(runs[0].timeout, DEFAULT_HOOK_TIMEOUT);

        let runs = hooks_for_command(&cfg(), "pre_command", "ls -la");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run, "echo any");
    }

    #[test]
    fn post_command_and_unknown_events() {
        assert_eq!(
            hooks_for_command(&cfg(), "post_command", "npm install").len(),
            1
        );
        assert!(hooks_for_command(&cfg(), "post_command", "cargo build").is_empty());
        assert!(hooks_for_command(&cfg(), "session_start", "x").is_empty());
        assert!(hooks_simple(&cfg(), "pre_command").is_empty());
    }

    #[test]
    fn simple_hooks() {
        let s = hooks_simple(&cfg(), "session_start");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].event, "session_start");
        assert_eq!(hooks_simple(&cfg(), "task_end")[0].run, "echo end");
    }

    #[test]
    fn custom_timeout() {
        let mut c = cfg();
        c.task_end[0].timeout_secs = Some(5);
        assert_eq!(
            hooks_simple(&c, "task_end")[0].timeout,
            Duration::from_secs(5)
        );
    }

    #[test]
    fn file_snapshot_detects_changes() {
        // Inside target/, never in the host temp dir (constitution III also guides our tests).
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("unit-tmp")
            .join(format!("hooks_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("package.json");
        std::fs::write(&file, "{}").unwrap();
        let mut c = cfg();
        c.file_changed = vec![FileHook {
            paths: vec!["package.json".into(), "absent.txt".into()],
            run: "echo changed".into(),
            shell: None,
            timeout_secs: None,
        }];
        let snap = FileSnapshot::take(&c, &dir);
        assert!(snap.changed(&c, &dir).is_empty(), "nothing changed yet");

        std::fs::write(&file, "{\"name\":\"x\"}").unwrap();
        let runs = snap.changed(&c, &dir);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].event, "file_changed");
        assert!(
            runs[0]
                .env
                .iter()
                .any(|(k, v)| k == "CMD_AI_FILE" && v.ends_with("package.json"))
        );

        // Creation of a watched file also counts.
        let snap = FileSnapshot::take(&c, &dir);
        std::fs::write(dir.join("absent.txt"), "now here").unwrap();
        assert_eq!(snap.changed(&c, &dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
