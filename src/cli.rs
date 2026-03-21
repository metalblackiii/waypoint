use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "waypoint", version, about = "Claude Code context hooks")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Hook subcommands invoked by Claude Code hooks
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum HookCommand {
    /// PreToolUse:Read — inject file context into Claude's view
    PreRead {
        /// Inject ~2K chars to test additionalContext size limits
        #[arg(long)]
        large: bool,
    },
    /// SessionStart — inject journal/preferences context
    SessionStart,
    /// PreToolUse:Edit|Write — inject trap warnings
    PreWrite,
    /// PostToolUse:Edit|Write — notify map updated
    PostWrite,
    /// Stop — log session end contract
    Stop,
}
