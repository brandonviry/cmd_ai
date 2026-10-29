# Contrat d'API interne (Rust)

Contrat entre modules. Les signatures publiques ci-dessous font foi ; les détails internes sont libres. Tout texte destiné à l'utilisateur vient de `text.rs`. Aucun `unwrap`/`expect`/`panic!` hors tests. Seul `exec.rs` (et `platform` pour `ShellExecuteExW`) crée des processus.

## secret.rs (squelette fourni)

```rust
pub struct Secret(String);              // Debug/Display => "••••" ; expose() => &str
impl Secret { pub fn new(s: impl Into<String>) -> Self; pub fn expose(&self) -> &str; pub fn is_empty(&self) -> bool; }
pub struct Redactor { /* secrets à masquer */ }
impl Redactor { pub fn new(secrets: &[&str]) -> Self; pub fn redact(&self, s: &str) -> String; } // remplace chaque secret (len >= 4) par "••••"
```

## home.rs

```rust
pub struct Home { pub root: PathBuf, pub writable: bool }
impl Home {
    pub fn resolve(override_dir: Option<&Path>) -> Result<Home, HomeError>; // --home > CMD_AI_HOME > canonicalize(current_exe).parent()
    pub fn config_path(&self) -> PathBuf;     // root/config.json
    pub fn example_config_path(&self) -> PathBuf; // root/config.example.json
    pub fn log_dir(&self) -> PathBuf; pub fn memory_dir(&self) -> PathBuf; pub fn skills_dir(&self) -> PathBuf;
    pub fn ephemeral(&self) -> bool;          // !writable
}
// writable = création + suppression d'un fichier sonde `root/.cmd_ai_write_test` réussies.
```

## config.rs

Voir [config.md](config.md). Structures `serde` avec `deny_unknown_fields`.

```rust
pub struct Config { pub version: u32, pub provider: Provider, pub permissions: PermissionMode, pub ui: UiConfig,
                    pub network: Network, pub limits: Limits, pub garde_fous: Vec<String>, pub hooks: HooksConfig }
pub struct Provider { pub base_url: String, pub api_key: Secret, pub model: String, pub roles: BTreeMap<String,String>,
                      pub max_tokens: u32, pub timeout_secs: u64 }
pub enum PermissionMode { Plan, Bypass }               // serde: "plan" | "bypass"
pub struct UiConfig { pub tui: bool, pub color: ColorChoice, pub theme: ThemeChoice, pub show_reasoning: bool }
pub enum ColorChoice { Auto, Truecolor, Ansi256, Ansi16, None }   // "auto"|"truecolor"|"256"|"16"|"none"
pub enum ThemeChoice { Auto, Dark, Light }
pub struct Network { pub proxy: Option<String>, pub ca_file: Option<String>, pub connect_timeout_secs: u64 }
pub struct Limits { pub command_timeout_secs: u64, pub max_steps: u32, pub max_fixes: u32, pub model_output_kib: u32,
                    pub capture_mib: u32, pub log_file_mib: u32, pub log_total_mib: u32, pub memory_max_notes: u32 }
pub struct HooksConfig { pub session_start: Vec<Hook>, pub pre_command: Vec<Hook>, pub post_command: Vec<Hook>,
                         pub file_changed: Vec<FileHook>, pub task_end: Vec<Hook> }
pub struct Hook { pub r#match: Option<String>, pub run: String, pub shell: Option<String>, pub timeout_secs: Option<u64> }
pub struct FileHook { pub paths: Vec<String>, pub run: String, pub shell: Option<String>, pub timeout_secs: Option<u64> }
pub enum ConfigError { Missing(PathBuf), Io(PathBuf, String), Parse(String /*ligne:col + msg*/), UnsupportedVersion(u32), Invalid(String) }
impl Config {
    pub fn load(path: &Path, env_api_key: Option<String>) -> Result<Config, ConfigError>; // OLLAMA_API_KEY non vide prioritaire
    pub fn from_json(json: &str, env_api_key: Option<String>) -> Result<Config, ConfigError>;
    pub fn resolve_model(&self, choice: Option<&str>) -> Result<String, ConfigError>; // None => model ; rôle connu => roles[rôle] ; sinon nom brut si contient ':' ou '/' ou existe ? => voir config.md
    pub fn default_garde_fous() -> Vec<String>;
}
// Validation : base_url http(s) ; http seulement vers localhost/127.0.0.1/[::1] ; model non vide ; limites > 0.
// Valeurs par défaut serde pour toutes les sections sauf provider.base_url/model (obligatoires).
```

## log.rs

```rust
pub struct SessionLog { /* None si éphémère */ }
impl SessionLog {
    pub fn open(home: &Home, redactor: Redactor, file_max: u64, total_max: u64) -> SessionLog; // log/AAAA-MM-JJ_HHMMSS.log, rotation (supprime les plus anciens au-delà de total_max), jamais d'erreur fatale
    pub fn disabled() -> SessionLog;
    pub fn line(&mut self, kind: &str, text: &str);   // "[2026-10-29T12:34:56Z] KIND text" (masqué, plafonné à file_max)
    pub fn path(&self) -> Option<&Path>;
}
pub fn utc_timestamp(t: SystemTime) -> String;      // "2026-10-29T12:34:56Z" (algorithme civil-from-days, sans dépendance)
pub fn file_stamp(t: SystemTime) -> String;         // "2026-10-29_123456"
```

## platform/

```rust
pub struct HostInfo { pub os: &'static str /*"windows"|"linux"|"macos"*/, pub arch: &'static str, pub os_version: String }
pub fn host_info() -> HostInfo;                      // os_version : best effort sans processus (Windows: RtlGetVersion ; Linux: /etc/os-release PRETTY_NAME ; macOS: /System/Library/CoreServices/SystemVersion.plist)
pub enum Privilege { User, Admin }
pub fn current_privilege() -> Privilege;             // Windows: TokenElevation ; Unix: geteuid()==0
pub fn decode_output(bytes: &[u8]) -> String;        // UTF-8 ; sinon (Windows) page OEM via MultiByteToWideChar(GetOEMCP) ; sinon lossy
#[cfg(windows)] pub fn relaunch_elevated(exe: &Path, args: &[String]) -> Result<(), String>; // ShellExecuteExW "runas"
```

## shell.rs

```rust
pub enum Shell { Pwsh, PowerShell, Cmd, Bash, Sh, Zsh }
pub struct ShellInfo { pub shell: Shell, pub path: PathBuf }
impl Shell { pub fn name(&self) -> &'static str; pub fn parse(s: &str) -> Option<Shell>; } // "pwsh","powershell","cmd","bash","sh","zsh"
pub fn detect_default() -> Option<ShellInfo>;       // ordre FR-031, par recherche dans PATH + emplacements connus, SANS lancer de processus
pub fn find(shell: Shell) -> Option<ShellInfo>;
pub fn build_args(shell: Shell, command: &str) -> ShellArgs; // arguments à passer (pwsh/powershell: -NoProfile -NonInteractive -NoLogo -Command <cmd> précédé du préambule UTF-8 ; cmd: /d /s /c "<cmd>" en raw_arg ; sh/bash/zsh: -c <cmd>)
pub struct ShellArgs { pub args: Vec<String>, pub raw_tail: Option<String> /* cmd.exe : passé via raw_arg */ }
pub fn child_env(base: impl Iterator<Item=(OsString,OsString)>) -> Vec<(OsString,OsString)>; // FR-032 : retire OLLAMA_API_KEY, CMD_AI_*, BASH_ENV, ENV, HISTFILE ; ajoute PSModuleAnalysisCachePath, POWERSHELL_TELEMETRY_OPTOUT=1, POWERSHELL_UPDATECHECK=Off, PAGER=cat, GIT_PAGER=cat, HISTFILE= ; NO_COLOR=1
```

## exec.rs (seule porte)

```rust
pub enum Stream { Stdout, Stderr }
pub enum ExecMode { Captured, Detached, Inherited }
pub struct ExecSpec { pub shell: ShellInfo, pub command: String, pub cwd: PathBuf, pub timeout: Duration,
                      pub mode: ExecMode, pub extra_env: Vec<(String,String)>, pub sudo: bool /* préfixe sudo -n (Unix) */,
                      pub capture_limit: usize }
pub struct Captured { pub text: String, pub total_bytes: u64, pub truncated: bool }
pub struct ExecResult { pub code: Option<i32>, pub stdout: Captured, pub stderr: Captured, pub duration: Duration,
                        pub timed_out: bool, pub cancelled: bool }
pub trait Executor { fn run(&mut self, spec: &ExecSpec, on_chunk: &mut dyn FnMut(Stream, &str), cancel: &AtomicBool) -> io::Result<ExecResult>; }
pub struct SystemExecutor;
impl Executor for SystemExecutor { … }   // process-wrap : JobObject (Windows) / ProcessGroup (Unix) ; stdin null en Captured ; kill de l'arbre si délai/annulation ; Detached : spawn détaché, attente ≤ 10 s du lanceur ; Inherited : stdio hérités
pub fn run_inherited_program(program: &str, args: &[&str]) -> io::Result<Option<i32>>; // pour `sudo -v` (Unix)
pub fn probe_program(program: &str, args: &[&str], timeout: Duration) -> Option<i32>;    // pour `sudo -n true` (stdin null, sortie ignorée)
```

## output.rs

```rust
pub fn sanitize_for_display(s: &str) -> String;   // retire ESC/CSI/OSC/DCS sauf SGR (ESC[…m) ; garde \n \t ; remplace \r de progression (garde le dernier segment de la ligne)
pub fn strip_ansi(s: &str) -> String;              // retire TOUTES les séquences (pour le modèle et les logs)
pub fn reduce_for_model(s: &str, max_bytes: usize) -> (String, bool); // strip_ansi + collapse de lignes répétées ("[ligne répétée N fois]") + début 25 % / fin 75 % avec marqueur d'omission ; frontières UTF-8 respectées
pub fn frame_untrusted(label: &str, body: &str) -> String; // "<<<SORTIE NON FIABLE (label)>>>\n…\n<<<FIN SORTIE NON FIABLE>>>" (neutralise les marqueurs présents dans body)
```

## risk.rs

```rust
pub struct Assessment { pub admin: bool, pub garde_fou: Option<String> /* motif déclenché */, pub gui_launcher: bool, pub strip_sudo: Option<String> /* commande sans "sudo " initial */ }
pub fn assess(command: &str, os: &str, declared_admin: bool, garde_fous: &[String]) -> Assessment;
pub fn glob_match(pattern: &str, text: &str) -> bool;   // * et ?, insensible à la casse, espaces normalisés
pub fn looks_like_access_denied(code: Option<i32>, stdout: &str, stderr: &str, os: &str) -> bool;
```

## llm/

```rust
pub enum Role { System, User, Assistant, Tool }
pub struct ToolCall { pub id: String, pub name: String, pub arguments: String }
pub struct Message { pub role: Role, pub content: Option<String>, pub tool_calls: Vec<ToolCall>, pub tool_call_id: Option<String> }
impl Message { pub fn system(s) ; pub fn user(s) ; pub fn assistant(content, tool_calls) ; pub fn tool(id, content) }
pub struct ChatRequest { pub model: String, pub messages: Vec<Message>, pub tools: serde_json::Value, pub max_tokens: u32 }
pub enum StreamEvent { Content(String), Reasoning(String) }
pub struct Usage { pub prompt_tokens: u64, pub completion_tokens: u64 }
pub struct ChatResponse { pub content: String, pub reasoning: String, pub tool_calls: Vec<ToolCall>, pub usage: Option<Usage>, pub finish_reason: Option<String> }
pub enum LlmError { Auth(String), Credits(String), RateLimited { retry_after: Option<u64>, message: String }, Quota(String),
                    ModelNotFound(String), Server(u16, String), Network(String), Tls(String), BadResponse(String), Empty, Cancelled }
pub trait ChatBackend { fn chat(&self, req: &ChatRequest, on_event: &mut dyn FnMut(StreamEvent), cancel: &AtomicBool) -> Result<ChatResponse, LlmError>; }
// sse.rs
pub struct SseParser { … } impl SseParser { pub fn new() -> Self; pub fn push_line(&mut self, line: &str) -> Option<SseEvent>; }
pub enum SseEvent { Data(String), Done }
// client.rs
pub struct ClientOptions { pub base_url: String, pub api_key: Secret, pub proxy: Option<String>, pub ca_file: Option<PathBuf>, pub connect_timeout: Duration, pub request_timeout: Duration }
pub struct OpenAiClient { … }
impl OpenAiClient { pub fn new(opts: ClientOptions) -> Result<Self, LlmError>; }
impl ChatBackend for OpenAiClient { … } // POST {base_url}/chat/completions, stream=true, stream_options.include_usage ; assemble les deltas tool_calls par index ; reasoning|reasoning_content ; classifie les erreurs (FR-051) ; PAS de reprise ici (l'agent décide)
pub fn classify_http_error(status: u16, body: &str, retry_after: Option<&str>) -> LlmError;
```

## tool.rs

```rust
pub const TOOL_NAME: &str = "run_command";
pub fn schema() -> serde_json::Value;            // tableau "tools" OpenAI avec UN seul tool
pub struct CommandRequest { pub command: String, pub purpose: String, pub shell: Option<String>, pub needs_admin: bool, pub interactive: bool, pub plan: Vec<String> }
pub enum ToolParseError { WrongTool(String), BadJson(String), MissingField(&'static str), Empty }
pub fn parse_call(call: &ToolCall) -> Result<CommandRequest, ToolParseError>; // tolérant : arguments chaîne JSON ou JSON doublement encodé ; booléens "true"/"false" acceptés
pub enum SelfCommand { SkillList, SkillShow(String), MemoryList, MemoryAdd(String), MemoryForget(usize) }
pub fn parse_self_command(command: &str) -> Option<Result<SelfCommand, String>>; // None si la commande ne commence pas par "cmd_ai " (ou "./cmd_ai", "cmd_ai.exe")
```

## skills.rs / memory.rs / hooks.rs / session.rs

```rust
pub struct Skill { pub name: String, pub description: String, pub body: String }
pub fn valid_skill_name(name: &str) -> bool;                  // ^[a-z0-9]+(-[a-z0-9]+)*$, ≤ 64
pub fn load_catalog(dir: &Path) -> (Vec<Skill>, Vec<String> /*avertissements*/); // skills/<nom>/SKILL.md ; frontmatter name/description ; name == nom du dossier
pub fn parse_skill_md(text: &str) -> Result<(String, String, String), String>;

pub struct Memory { … }
impl Memory { pub fn load(dir: &Path, writable: bool, max_notes: usize) -> Memory; pub fn notes(&self) -> &[String];
              pub fn add(&mut self, note: &str) -> Result<usize, String>; pub fn forget(&mut self, n: usize) -> Result<String, String>;
              pub fn prompt_block(&self, max_bytes: usize) -> String; }

pub struct HookRun { pub event: &'static str, pub run: String, pub shell: Option<String>, pub timeout: Duration, pub env: Vec<(String,String)> }
pub fn hooks_for_command(cfg: &HooksConfig, event: &'static str /*"pre_command"|"post_command"*/, command: &str) -> Vec<HookRun>;
pub fn hooks_simple(cfg: &HooksConfig, event: &'static str /*"session_start"|"task_end"*/) -> Vec<HookRun>;
pub struct FileSnapshot { … } impl FileSnapshot { pub fn take(cfg: &HooksConfig, cwd: &Path) -> Self; pub fn changed(&self, cfg: &HooksConfig, cwd: &Path) -> Vec<HookRun>; }

pub struct ResumeState { pub prompt: String, pub mode: PermissionMode, pub model: String, pub cwd: PathBuf }
pub fn save_resume(home: &Home, st: &ResumeState) -> Result<PathBuf, String>; pub fn load_resume(path: &Path) -> Result<ResumeState, String>; // supprime le fichier après lecture
```

## ui/mod.rs — interface agent ↔ affichage

```rust
pub struct PlanStep { pub command: String, pub purpose: String, pub shell: String, pub admin: bool }
pub struct StatusLine { pub mode: PermissionMode, pub admin: bool, pub model: String, pub ephemeral: bool, pub shell: String }
pub enum AgentEvent {
    Status(StatusLine), Reasoning(String), AssistantDelta(String), AssistantDone(String),
    Plan(Vec<PlanStep>), Progress { done: usize, total: usize },
    CommandStart { index: usize, total: usize, step: PlanStep, hook: bool },
    CommandOutput { stream: Stream, text: String },
    CommandEnd { code: Option<i32>, duration: Duration, timed_out: bool, cancelled: bool, truncated: bool },
    Info(String), Warning(String), Error(String), TaskEnd(TaskOutcome),
}
pub enum TaskOutcome { Done, Failed, PlanRejected, PlanNotApproved, Cancelled, StepLimit, ProviderError, Elevating }
impl TaskOutcome { pub fn exit_code(self) -> i32; }   // 0 / 1 / 3 / 130 / 5 (cli-and-tool.md)
pub enum ApprovalRequest { Plan(Vec<PlanStep>), Single { step: PlanStep, garde_fou: Option<String> }, Elevation { reason: String } }
pub enum Approval { Yes, No, Unavailable }
pub trait Frontend {
    fn event(&mut self, ev: AgentEvent);
    fn approve(&mut self, req: ApprovalRequest) -> Approval;
    fn handover(&mut self, run: &mut dyn FnMut() -> Option<i32>) -> Option<i32>; // suspend the display, run (spawned by exec), resume
    fn cancel_flag(&self) -> Arc<AtomicBool>;
}
```

Frontends : `ui::plain::PlainFrontend` (TUI fermé ; `read_request()` pour la boucle texte de repli) et `ui::tui` (`probe()` vérifie raw mode + requête de position du curseur ; `run(Session, Option<String>) -> i32` : agent sur un thread de travail, `ChannelFrontend` + canaux `UiMsg` / `AgentCmd`). Si `probe()` échoue, `lib.rs` bascule en TUI fermé (boucle ligne à ligne avec les mêmes commandes `/`).

## agent.rs

```rust
pub struct AgentSettings { pub mode: PermissionMode, pub model: String, pub max_tokens: u32, pub limits: Limits, pub garde_fous: Vec<String>,
                           pub hooks: HooksConfig, pub cwd: PathBuf, pub host: HostInfo, pub shells: Vec<ShellInfo> /* [0] = défaut, jamais vide */,
                           pub privilege: Privilege, pub show_reasoning: bool, pub ephemeral: bool, pub base_url: String, pub retry_delays: Vec<Duration> }
pub trait Elevator { fn sudo_ready(&mut self) -> bool; fn sudo_validate(&mut self) -> Option<i32>; fn relaunch(&mut self, state: &ResumeState) -> Result<(), String>; }
pub struct SystemElevator { pub home: Home }      // sudo -n true / sudo -v / save_resume + relaunch_elevated
pub struct Agent<'a, B: ChatBackend, E: Executor> { … }
impl Agent {
    pub fn new(backend: &'a B, exec: &'a mut E, elevator: &'a mut dyn Elevator, settings: AgentSettings, skills: Vec<Skill>,
               memory: &'a mut Memory, log: &'a mut SessionLog, redactor: Redactor) -> Self;
    pub fn system_prompt(&self) -> String; pub fn status(&self) -> StatusLine;
    pub fn session_start(&mut self, ui: &mut dyn Frontend);              // hooks session_start
    pub fn run_task(&mut self, prompt: &str, ui: &mut dyn Frontend) -> TaskOutcome; // historique conservé entre tâches
    pub fn set_mode(&mut self, m: PermissionMode); pub fn set_model(&mut self, m: String); pub fn clear_history(&mut self);
}
```

Boucle `run_task` : message user → `chat` (reprises : 429 et 5xx jusqu'à 3 fois, backoff 2 s / 8 s / 20 s ou `Retry-After` ≤ 60 s ; erreur réseau 1 seule reprise ; réponse vide 1 relance ; le reste → `ProviderError`) → sans `tool_calls` : `AssistantDone`, fin `Done` (ou `Failed` si la dernière commande a échoué) → sinon, pour chaque appel (chaque `tool_call_id` reçoit une réponse, même après un arrêt) : parse (erreur → `ERREUR :`, > 2 de suite → `Failed`) → auto-commande (interceptée, sans validation, hors `n/M`) → plafond d'étapes → shell → `risk::assess` (préfixe sudo retiré) → mode plan : 1er appel = validation du plan entier (`plan` ou `[command]`), ensuite seules les commandes hors plan sont redemandées (`Single`) ; bypass : `Single` seulement pour un garde-fou → admin et session non élevée : Windows `Elevation` → `relaunch` → `Elevating` ; Unix `sudo_ready` sinon `handover(sudo -v)` puis `sudo=true` → hooks `pre_command` (code 2 = bloqué) → `FileSnapshot` → `exec.run` (Captured / Detached pour les lanceurs GUI / Inherited dans `handover` si `interactive`) → hooks `post_command` + `file_changed` → résultat réduit (16 Kio, 3/4 stdout, 1/4 stderr), masqué, encadré → message `tool`. Échecs consécutifs > `max_fixes` → `Failed`. Annulation → `Cancelled`. Hooks `task_end` à la fin (sauf `Elevating`).

## lib.rs

`pub struct Session { client, settings, skills, warnings, memory, log, redactor, home, theme, config }` ; `pub fn run() -> i32` ; `pub fn available_shells() -> Vec<ShellInfo>`. Ordre : arguments → prompt (`--prompt-file` lu avant toute interface) → dossier → configuration (assistant de première configuration si terminal interactif) → `--resume` → mode / modèle → shells → TUI ou TUI fermé.
