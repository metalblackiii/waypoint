use std::path::PathBuf;

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
        #[arg(long, conflicts_with = "all")]
        check: bool,
        /// Scan all immediate child git repos in a directory
        #[arg(long)]
        all: bool,
        /// Directory to scan (with --all: parent dir; without: project root)
        path: Option<PathBuf>,
    },
    /// Display token savings analytics
    Gain {
        /// Show stats across all projects
        #[arg(long)]
        global: bool,
    },
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
    /// Manage contextual learnings
    Learning {
        #[command(subcommand)]
        command: LearningCommand,
    },
    /// Display waypoint status for the current project
    Status,
    /// Show structural overview of a symbol
    Sketch {
        /// Symbol name to look up
        symbol: String,
        /// Resolve project from this path instead of cwd
        #[arg(short = 'C', long = "context")]
        context: Option<String>,
    },
    /// Search symbols by name or intent
    Find {
        /// Search query
        query: String,
        /// Maximum results
        #[arg(long, default_value = "20")]
        limit: usize,
        /// Resolve project from this path instead of cwd
        #[arg(short = 'C', long = "context")]
        context: Option<String>,
    },
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
        /// Resolve project from this path instead of cwd
        #[arg(short = 'C', long = "context")]
        context: Option<String>,
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
    /// Remove trap entries older than a duration (e.g., 90d)
    Prune {
        /// Duration threshold, e.g. "90d" (days only)
        #[arg(long)]
        older_than: Option<String>,
        /// Prune across all sibling projects
        #[arg(long, conflicts_with = "context")]
        all: bool,
        /// Resolve project from this path instead of cwd
        #[arg(short = 'C', long = "context")]
        context: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum JournalSection {
    Preferences,
    DoNotRepeat,
}

impl JournalSection {
    /// Markdown header for this section.
    #[must_use]
    pub fn header(self) -> &'static str {
        match self {
            Self::Preferences => "## Preferences",
            Self::DoNotRepeat => "## Do-Not-Repeat",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum LearningCommand {
    /// Add a new learning
    Add {
        /// Learning text
        entry: String,
        /// Comma-separated file paths or topic tags (required)
        #[arg(long)]
        tags: String,
        /// Resolve project from this path instead of cwd
        #[arg(short = 'C', long = "context")]
        context: Option<String>,
    },
    /// Search learnings by keyword
    Search {
        /// Search term
        term: String,
        /// Resolve project from this path instead of cwd
        #[arg(short = 'C', long = "context")]
        context: Option<String>,
    },
    /// List all learnings
    List {
        /// Resolve project from this path instead of cwd
        #[arg(short = 'C', long = "context")]
        context: Option<String>,
    },
    /// Remove learning entries older than a duration (e.g., 90d)
    Prune {
        /// Duration threshold, e.g. "90d" (days only)
        #[arg(long)]
        older_than: Option<String>,
        /// Prune across all sibling projects
        #[arg(long, conflicts_with = "context")]
        all: bool,
        /// Resolve project from this path instead of cwd
        #[arg(short = 'C', long = "context")]
        context: Option<String>,
    },
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
        /// Resolve project from this path instead of cwd
        #[arg(short = 'C', long = "context")]
        context: Option<String>,
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
