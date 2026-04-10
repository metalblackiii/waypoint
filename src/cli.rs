use std::path::PathBuf;

use clap::{Parser, Subcommand};

const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), ")");

#[derive(Debug, Parser)]
#[command(
    name = "waypoint",
    version = VERSION,
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
    /// Display waypoint status for the current project
    Status {
        /// Show status across all sibling projects
        #[arg(long)]
        all: bool,
    },
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
    /// List files that import a symbol
    Callers {
        /// Symbol name to look up
        symbol: String,
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
pub enum HookCommand {
    /// PreToolUse:Read — inject file map context
    PreRead,
    /// `SessionStart` — auto-scan and record session start
    SessionStart,
}
