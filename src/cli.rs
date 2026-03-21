use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "waypoint",
    version,
    about = "Project intelligence for Claude Code"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Scan project files and generate map.md
    Scan {
        /// Check if map is stale (exit non-zero if outdated)
        #[arg(long)]
        check: bool,
    },
    /// Display token savings analytics
    Gain,
    /// Manage the bug fix trap log
    Trap {
        #[command(subcommand)]
        command: TrapCommand,
    },
    /// Manage the cross-session journal
    Journal {
        #[command(subcommand)]
        command: JournalCommand,
    },
    /// Display waypoint status for the current project
    Status,
    /// Hook subcommands invoked by Claude Code hooks
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum TrapCommand {
    /// Search traps by keyword
    Search {
        /// Search term
        term: String,
    },
    /// Log a new bug fix trap
    Log {
        /// Error message
        #[arg(long)]
        error: String,
        /// File path where the bug occurred
        #[arg(long)]
        file: String,
        /// Root cause of the bug
        #[arg(long)]
        cause: String,
        /// What was done to fix it
        #[arg(long)]
        fix: String,
        /// Comma-separated tags
        #[arg(long)]
        tags: String,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum JournalSection {
    Preferences,
    Learnings,
    DoNotRepeat,
}

#[derive(Debug, Subcommand)]
pub enum JournalCommand {
    /// Add an entry to the journal
    Add {
        /// Section to add to
        #[arg(long)]
        section: JournalSection,
        /// Entry text
        entry: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum HookCommand {
    /// PreToolUse:Read — inject file map context
    PreRead,
    /// `SessionStart` — inject journal context and auto-scan
    SessionStart,
    /// PreToolUse:Edit|Write — inject trap warnings
    PreWrite,
    /// PostToolUse:Edit|Write — update map entry
    PostWrite,
    /// PostToolUseFailure:Edit|Write — suggest trap search
    PostFailure,
}
