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

/// Point d'entrée : renvoie le code de sortie du processus.
pub fn run() -> i32 {
    0
}
