pub mod cli;
pub mod hook;

use thiserror::Error;

use crate::cli::{Cli, Command, HookCommand};

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub fn run(cli: Cli) -> Result<(), AppError> {
    match cli.command {
        Command::Hook { command } => match command {
            HookCommand::PreRead { large } => hook::pre_read::run(large),
            HookCommand::SessionStart => hook::session_start::run(),
            HookCommand::PreWrite => hook::pre_write::run(),
            HookCommand::PostWrite => hook::post_write::run(),
            HookCommand::Stop => hook::stop::run(),
        },
    }
}
